package csi

import (
	"context"
	"net"
	"path/filepath"
	"testing"

	"github.com/container-storage-interface/spec/lib/go/csi"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
)

// metaFixture runs an in-memory metadata service, without authentication (its rules are
// tested in meta/service), with live nodes a and b. The first endpoint given to the client
// refuses connections, so every call also exercises the move to the next instance.
func metaFixture(t *testing.T) *Meta {
	t.Helper()
	svc := service.New(kv.NewMem(nil), service.Options{})
	srv := grpc.NewServer()
	metav1.RegisterMetadataServiceServer(srv, svc)
	lis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	go func() { _ = srv.Serve(lis) }()
	t.Cleanup(srv.Stop)
	m, err := dialMeta([]string{"127.0.0.1:1", lis.Addr().String()}, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(m.Close)
	ctx := context.Background()
	for _, n := range []string{"a", "b"} {
		if _, err := svc.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: n, Address: n + ":7411"}}); err != nil {
			t.Fatal(err)
		}
		if _, err := svc.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: n}); err != nil {
			t.Fatal(err)
		}
	}
	return m
}

func mountCap(fs string) *csi.VolumeCapability {
	return &csi.VolumeCapability{
		AccessMode: &csi.VolumeCapability_AccessMode{Mode: csi.VolumeCapability_AccessMode_SINGLE_NODE_WRITER},
		AccessType: &csi.VolumeCapability_Mount{Mount: &csi.VolumeCapability_MountVolume{FsType: fs}},
	}
}

func blockCap() *csi.VolumeCapability {
	return &csi.VolumeCapability{
		AccessMode: &csi.VolumeCapability_AccessMode{Mode: csi.VolumeCapability_AccessMode_SINGLE_NODE_WRITER},
		AccessType: &csi.VolumeCapability_Block{Block: &csi.VolumeCapability_BlockVolume{}},
	}
}

func wantCode(t *testing.T, what string, err error, want codes.Code) {
	t.Helper()
	if status.Code(err) != want {
		t.Fatalf("%s: got %v, want %v", what, err, want)
	}
}

func TestController(t *testing.T) {
	m := metaFixture(t)
	c := &Controller{Meta: m}
	ctx := context.Background()
	create := func(name string, size int64, params map[string]string, caps ...*csi.VolumeCapability) (*csi.CreateVolumeResponse, error) {
		if caps == nil {
			caps = []*csi.VolumeCapability{mountCap("")}
		}
		return c.CreateVolume(ctx, &csi.CreateVolumeRequest{
			Name: name, CapacityRange: &csi.CapacityRange{RequiredBytes: size}, Parameters: params, VolumeCapabilities: caps,
		})
	}

	// Defaults, and the size aligned up to 1 MiB.
	r, err := create("pvc-1", 1<<30+1, nil)
	wantCode(t, "create", err, codes.OK)
	if r.Volume.CapacityBytes != 1<<30+1<<20 {
		t.Fatalf("capacity %d", r.Volume.CapacityBytes)
	}
	v, err := m.GetVolume(ctx, "pvc-1")
	if err != nil || v.Replicas != 2 || v.MinIsr != 1 || v.ChunkBytes != 256<<20 || len(v.Chunks) != 5 {
		t.Fatalf("volume %+v %v", v, err)
	}
	// A retry succeeds; the same name with other properties does not.
	_, err = create("pvc-1", 1<<30+1, nil)
	wantCode(t, "retry", err, codes.OK)
	_, err = create("pvc-1", 2<<30, nil)
	wantCode(t, "same name, other size", err, codes.AlreadyExists)

	_, err = create("pvc-2", 1<<30, map[string]string{"replica": "2"})
	wantCode(t, "misspelt parameter", err, codes.InvalidArgument)
	_, err = create("pvc-2", 1<<30, map[string]string{"replicas": "3"})
	wantCode(t, "more replicas than nodes", err, codes.ResourceExhausted)
	_, err = create("pvc-2", 1<<30, nil, &csi.VolumeCapability{
		AccessMode: &csi.VolumeCapability_AccessMode{Mode: csi.VolumeCapability_AccessMode_MULTI_NODE_MULTI_WRITER},
		AccessType: &csi.VolumeCapability_Block{Block: &csi.VolumeCapability_BlockVolume{}},
	})
	wantCode(t, "multi-node access", err, codes.InvalidArgument)
	_, err = create("pvc-2", 1<<30, map[string]string{"replicas": "1", "chunkSize": "64Mi", "csi.storage.k8s.io/pvc/name": "x"}, blockCap())
	wantCode(t, "block, 1 replica, 64 MiB chunks", err, codes.OK)

	// Publishing moves the target; publishing again to it changes nothing.
	publish := func(node string) uint64 {
		t.Helper()
		r, err := c.ControllerPublishVolume(ctx, &csi.ControllerPublishVolumeRequest{VolumeId: "pvc-1", NodeId: node, VolumeCapability: mountCap("")})
		if err != nil {
			t.Fatalf("publish to %s: %v", node, err)
		}
		v, _ := m.GetVolume(ctx, "pvc-1")
		if v.Target != node || r.PublishContext["epoch"] == "" {
			t.Fatalf("publish to %s: target %s, context %v", node, v.Target, r.PublishContext)
		}
		return v.Epoch
	}
	if e := publish("a"); e != 1 {
		t.Fatalf("first publish: epoch %d", e)
	}
	if e := publish("a"); e != 1 {
		t.Fatalf("repeated publish: epoch %d", e)
	}
	if e := publish("b"); e != 2 {
		t.Fatalf("publish elsewhere: epoch %d", e)
	}
	_, err = c.ControllerUnpublishVolume(ctx, &csi.ControllerUnpublishVolumeRequest{VolumeId: "pvc-1", NodeId: "b"})
	wantCode(t, "unpublish", err, codes.OK)

	vr, err := c.ValidateVolumeCapabilities(ctx, &csi.ValidateVolumeCapabilitiesRequest{VolumeId: "pvc-1", VolumeCapabilities: []*csi.VolumeCapability{blockCap()}})
	if err != nil || vr.Confirmed == nil {
		t.Fatalf("validate: %v %v", vr, err)
	}
	_, err = c.ValidateVolumeCapabilities(ctx, &csi.ValidateVolumeCapabilitiesRequest{VolumeId: "nope", VolumeCapabilities: []*csi.VolumeCapability{blockCap()}})
	wantCode(t, "validate a missing volume", err, codes.NotFound)

	for range 2 {
		_, err = c.DeleteVolume(ctx, &csi.DeleteVolumeRequest{VolumeId: "pvc-1"})
		wantCode(t, "delete, twice", err, codes.OK)
	}
}

type fakeDataplane struct {
	volume, device string
	tokens         int
	detaches       int
}

func (f *fakeDataplane) Status(context.Context) (string, string, error) {
	return f.volume, f.device, nil
}

// Attach counts attaches: the in-memory service signs no tokens, so there is none to check.
func (f *fakeDataplane) Attach(_ context.Context, volume string, _ []byte) (string, error) {
	f.volume, f.device = volume, "/dev/ublk-"+volume
	f.tokens++
	return f.device, nil
}

func (f *fakeDataplane) Detach(_ context.Context, volume string) error {
	if f.volume == volume {
		f.volume, f.device = "", ""
		f.detaches++
	}
	return nil
}

type fakeMounter struct {
	fs      map[string]string // device -> signature
	mounts  map[string]string // target -> source
	formats int
}

func (f *fakeMounter) FSType(_ context.Context, dev string) (string, error) { return f.fs[dev], nil }

func (f *fakeMounter) Format(_ context.Context, dev, fs string) error {
	f.fs[dev] = fs
	f.formats++
	return nil
}

func (f *fakeMounter) Mount(_ context.Context, source, target, _ string, _ []string) error {
	f.mounts[target] = source
	return nil
}

func (f *fakeMounter) Unmount(_ context.Context, target string) error {
	delete(f.mounts, target)
	return nil
}

func (f *fakeMounter) IsMountPoint(target string) (bool, error) {
	_, ok := f.mounts[target]
	return ok, nil
}

func TestNode(t *testing.T) {
	m := metaFixture(t)
	ctx := context.Background()
	c := &Controller{Meta: m}
	for _, name := range []string{"pvc-1", "pvc-2"} {
		if _, err := c.CreateVolume(ctx, &csi.CreateVolumeRequest{Name: name, VolumeCapabilities: []*csi.VolumeCapability{mountCap("")}}); err != nil {
			t.Fatal(err)
		}
	}
	dp := &fakeDataplane{}
	mnt := &fakeMounter{fs: map[string]string{}, mounts: map[string]string{}}
	n := &Node{NodeID: "a", Meta: m, Dataplane: dp, Mounter: mnt}
	dir := t.TempDir()
	staging, target := filepath.Join(dir, "staging"), filepath.Join(dir, "target")
	stage := func(volume string) error {
		_, err := n.NodeStageVolume(ctx, &csi.NodeStageVolumeRequest{VolumeId: volume, StagingTargetPath: staging, VolumeCapability: mountCap("ext4")})
		return err
	}
	epoch := func(volume string) uint64 {
		v, _ := m.GetVolume(ctx, volume)
		return v.Epoch
	}

	// Not published to this node: the agent may not start the target.
	wantCode(t, "stage unpublished", stage("pvc-1"), codes.FailedPrecondition)
	if _, err := c.ControllerPublishVolume(ctx, &csi.ControllerPublishVolumeRequest{VolumeId: "pvc-1", NodeId: "a", VolumeCapability: mountCap("")}); err != nil {
		t.Fatal(err)
	}
	// Staging starts the target under a new epoch, formats the empty device and mounts it.
	wantCode(t, "stage", stage("pvc-1"), codes.OK)
	if dp.volume != "pvc-1" || epoch("pvc-1") != 2 || mnt.formats != 1 || mnt.mounts[staging] != "/dev/ublk-pvc-1" {
		t.Fatalf("after stage: dataplane %+v epoch %d mounter %+v", dp, epoch("pvc-1"), mnt)
	}
	// Staging again (a kubelet retry) changes nothing.
	wantCode(t, "stage again", stage("pvc-1"), codes.OK)
	if dp.tokens != 1 || epoch("pvc-1") != 2 || mnt.formats != 1 {
		t.Fatalf("restage: dataplane %+v epoch %d formats %d", dp, epoch("pvc-1"), mnt.formats)
	}
	// One volume per node.
	if _, err := c.ControllerPublishVolume(ctx, &csi.ControllerPublishVolumeRequest{VolumeId: "pvc-2", NodeId: "a", VolumeCapability: mountCap("")}); err != nil {
		t.Fatal(err)
	}
	wantCode(t, "a second volume", stage("pvc-2"), codes.ResourceExhausted)

	_, err := n.NodePublishVolume(ctx, &csi.NodePublishVolumeRequest{VolumeId: "pvc-1", StagingTargetPath: staging, TargetPath: target, VolumeCapability: mountCap("ext4")})
	wantCode(t, "publish", err, codes.OK)
	if mnt.mounts[target] != staging {
		t.Fatalf("publish: mounts %v", mnt.mounts)
	}
	_, err = n.NodeUnpublishVolume(ctx, &csi.NodeUnpublishVolumeRequest{VolumeId: "pvc-1", TargetPath: target})
	wantCode(t, "unpublish", err, codes.OK)
	_, err = n.NodeUnstageVolume(ctx, &csi.NodeUnstageVolumeRequest{VolumeId: "pvc-1", StagingTargetPath: staging})
	wantCode(t, "unstage", err, codes.OK)
	if len(mnt.mounts) != 0 || dp.volume != "" || dp.detaches != 1 {
		t.Fatalf("after unstage: mounts %v dataplane %+v", mnt.mounts, dp)
	}

	// After a restart of the data plane, staging again starts the target under yet another
	// epoch: never the one it had.
	wantCode(t, "stage after a restart", stage("pvc-2"), codes.OK)
	if epoch("pvc-2") != 2 {
		t.Fatalf("pvc-2 epoch %d", epoch("pvc-2"))
	}

	// A device with something else on it is never formatted.
	_, _ = n.NodeUnstageVolume(ctx, &csi.NodeUnstageVolumeRequest{VolumeId: "pvc-2", StagingTargetPath: staging})
	mnt.fs["/dev/ublk-pvc-2"] = "xfs"
	wantCode(t, "stage over another filesystem", stage("pvc-2"), codes.FailedPrecondition)
	if mnt.formats != 2 {
		t.Fatalf("formats %d", mnt.formats)
	}
}

func TestUnescapeMountinfo(t *testing.T) {
	if got := unescapeMountinfo(`/var/lib/kubelet/a\040b\134c`); got != `/var/lib/kubelet/a b\c` {
		t.Fatalf("got %q", got)
	}
}
