package csi

import (
	"context"
	"net"
	"path/filepath"
	"slices"
	"testing"

	"github.com/container-storage-interface/spec/lib/go/csi"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
)

// serveMeta serves `svc` on a local port and dials it. The first endpoint given to the client
// refuses connections, so every call also exercises the move to the next instance.
func serveMeta(t *testing.T, svc metav1.MetadataServiceServer) *Meta {
	t.Helper()
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
	return m
}

// metaFixture runs an in-memory metadata service, without authentication (its rules are
// tested in meta/service), with live nodes a and b.
func metaFixture(t *testing.T) *Meta {
	t.Helper()
	svc := service.New(kv.NewMem(nil), service.Options{})
	m := serveMeta(t, svc)
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

	// Defaults, and capacity rounded up to a full 256 MiB chunk.
	r, err := create("pvc-1", 1<<30+1, nil)
	wantCode(t, "create", err, codes.OK)
	if r.Volume.CapacityBytes != 1<<30+256<<20 {
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

	// The node of the volume's first pod holds a replica of every chunk.
	if _, err := c.CreateVolume(ctx, &csi.CreateVolumeRequest{
		Name: "pvc-local", CapacityRange: &csi.CapacityRange{RequiredBytes: 4 << 30},
		VolumeCapabilities: []*csi.VolumeCapability{mountCap("")}, Parameters: map[string]string{"replicas": "1"},
		AccessibilityRequirements: &csi.TopologyRequirement{Preferred: []*csi.Topology{{Segments: map[string]string{TopologyKey: "b"}}}},
	}); err != nil {
		t.Fatal(err)
	}
	if v, _ := m.GetVolume(ctx, "pvc-local"); len(v.Chunks) != 16 || slices.ContainsFunc(v.Chunks, func(c *metav1.ChunkMeta) bool { return c.Replicas[0] != "b" }) {
		t.Fatalf("chunks of pvc-local not all on b: %v", v.Chunks)
	}
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

	// Publishing moves the target; publishing again to it changes nothing. The volume's epochs
	// start at its first epoch (docs/issues/30).
	v0, _ := m.GetVolume(ctx, "pvc-1")
	first := v0.Epoch
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
	if e := publish("a"); e != first+1 {
		t.Fatalf("first publish: epoch %d, first %d", e, first)
	}
	if e := publish("a"); e != first+1 {
		t.Fatalf("repeated publish: epoch %d, first %d", e, first)
	}
	if e := publish("b"); e != first+2 {
		t.Fatalf("publish elsewhere: epoch %d, first %d", e, first)
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

// docs/issues/76 S9: full-identity conflicts stay AlreadyExists; identical resumes succeed.
func TestControllerCreateIdentityConflicts(t *testing.T) {
	m := metaFixture(t)
	c := &Controller{Meta: m}
	ctx := context.Background()
	create := func(name string, params map[string]string) error {
		_, err := c.CreateVolume(ctx, &csi.CreateVolumeRequest{
			Name: name, CapacityRange: &csi.CapacityRange{RequiredBytes: 1 << 20},
			Parameters: params, VolumeCapabilities: []*csi.VolumeCapability{blockCap()},
		})
		return err
	}
	wantCode(t, "create zone", create("pvc-id", map[string]string{"replicas": "1", "failureDomain": "zone"}), codes.OK)
	wantCode(t, "identical retry", create("pvc-id", map[string]string{"replicas": "1", "failureDomain": "zone"}), codes.OK)
	// Policy-only conflict: metadata returns AlreadyExists; CSI must not turn a scalar match into success.
	wantCode(t, "failureDomain conflict", create("pvc-id", map[string]string{"replicas": "1", "failureDomain": "rack"}), codes.AlreadyExists)
	wantCode(t, "failureDomainPolicy conflict", create("pvc-id", map[string]string{"replicas": "1", "failureDomain": "zone", "failureDomainPolicy": "bestEffort"}), codes.AlreadyExists)
	wantCode(t, "replicas conflict", create("pvc-id", map[string]string{"replicas": "2", "failureDomain": "zone"}), codes.AlreadyExists)
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

func (f *fakeDataplane) Delete(context.Context, string) error { return nil }

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
	first := map[string]uint64{} // each volume's first epoch (docs/issues/30)
	for _, name := range []string{"pvc-1", "pvc-2"} {
		if _, err := c.CreateVolume(ctx, &csi.CreateVolumeRequest{Name: name, VolumeCapabilities: []*csi.VolumeCapability{mountCap("")}}); err != nil {
			t.Fatal(err)
		}
		v, _ := m.GetVolume(ctx, name)
		first[name] = v.Epoch
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
	if dp.volume != "pvc-1" || epoch("pvc-1") != first["pvc-1"]+2 || mnt.formats != 1 || mnt.mounts[staging] != "/dev/ublk-pvc-1" {
		t.Fatalf("after stage: dataplane %+v epoch %d mounter %+v", dp, epoch("pvc-1"), mnt)
	}
	// Staging again (a kubelet retry) changes nothing.
	wantCode(t, "stage again", stage("pvc-1"), codes.OK)
	if dp.tokens != 1 || epoch("pvc-1") != first["pvc-1"]+2 || mnt.formats != 1 {
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
	if epoch("pvc-2") != first["pvc-2"]+2 {
		t.Fatalf("pvc-2 epoch %d, first %d", epoch("pvc-2"), first["pvc-2"])
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

// A fake wire service records the complete desired identity and rejects unsafe Get fallback.
type geometryMetaFake struct {
	metav1.UnimplementedMetadataServiceServer
	creates, checks, gets int
	created               *metav1.CreateVolumeRequest
	checked               *metav1.CheckCreateVolumeRequest
	createCode, checkCode codes.Code
	actual                uint64
}

func (f *geometryMetaFake) CreateVolume(_ context.Context, r *metav1.CreateVolumeRequest) (*metav1.CreateVolumeResponse, error) {
	f.creates++
	f.created = proto.Clone(r).(*metav1.CreateVolumeRequest)
	if f.createCode != codes.OK {
		return nil, status.Error(f.createCode, "fake create")
	}
	return &metav1.CreateVolumeResponse{Volume: &metav1.Volume{Id: r.Id, SizeBytes: r.SizeBytes, ChunkBytes: r.ChunkBytes, Replicas: r.Replicas, MinIsr: r.MinIsr}}, nil
}
func (f *geometryMetaFake) CheckCreateVolume(_ context.Context, r *metav1.CheckCreateVolumeRequest) (*metav1.CheckCreateVolumeResponse, error) {
	f.checks++
	f.checked = proto.Clone(r).(*metav1.CheckCreateVolumeRequest)
	if f.checkCode != codes.OK {
		return nil, status.Error(f.checkCode, "fake check")
	}
	d := r.Desired
	return &metav1.CheckCreateVolumeResponse{Volume: &metav1.Volume{Id: d.Id, SizeBytes: f.actual, ChunkBytes: d.ChunkBytes, Replicas: d.Replicas, MinIsr: d.MinIsr}}, nil
}
func (f *geometryMetaFake) GetVolume(context.Context, *metav1.GetVolumeRequest) (*metav1.GetVolumeResponse, error) {
	f.gets++
	return nil, status.Error(codes.Internal, "unsafe scalar fallback")
}

func TestGeometryCapacityRanges(t *testing.T) {
	const m = int64(1 << 20)
	for _, x := range []struct {
		name            string
		required, limit int64
		chunk           string
		rf              string
		want            int64
		code            codes.Code
	}{
		{"default", 0, 0, "1Mi", "1", 1 << 30, codes.OK}, {"nil-default", 0, 0, "256Mi", "2", 1 << 30, codes.OK},
		{"one-byte", 1, 0, "1Mi", "1", m, codes.OK}, {"aligned", m, 0, "1Mi", "1", m, codes.OK},
		{"round-up", m + 1, 0, "1Mi", "1", 2 * m, codes.OK}, {"round-at-limit", m + 1, 2 * m, "1Mi", "1", 2 * m, codes.OK},
		{"limit-only", 0, 3*m + 4096, "1Mi", "1", 3 * m, codes.OK}, {"limit-default", 0, 2 << 30, "1Mi", "1", 1 << 30, codes.OK},
		{"negative-required", -1, 0, "1Mi", "1", 0, codes.InvalidArgument}, {"negative-limit", 0, -1, "1Mi", "1", 0, codes.InvalidArgument},
		{"both-negative", -1, -1, "1Mi", "1", 0, codes.InvalidArgument}, {"contradiction", 2 * m, m, "1Mi", "1", 0, codes.InvalidArgument},
		{"round-exceeds-limit", m + 1, m + 4096, "1Mi", "1", 0, codes.OutOfRange}, {"limit-less-chunk", 0, m - 1, "1Mi", "1", 0, codes.OutOfRange},
		{"max-i64", 1<<63 - 1, 0, "1Mi", "1", 0, codes.OutOfRange}, {"minimum-i64", -1 << 63, 0, "1Mi", "1", 0, codes.InvalidArgument},
		{"budget", 5138 * m, 0, "1Mi", "1", 0, codes.OutOfRange}, {"budget-max", 5137 * m, 0, "1Mi", "1", 5137 * m, codes.OK},
		{"invalid-chunk", m, 0, "4Ki", "1", 0, codes.InvalidArgument}, {"huge-chunk", m, 0, "2Gi", "1", 0, codes.InvalidArgument},
		{"quantity-wrap", m, 0, "17592186044417Mi", "1", 0, codes.InvalidArgument}, {"rf-zero", m, 0, "1Mi", "0", 0, codes.InvalidArgument},
	} {
		t.Run(x.name, func(t *testing.T) {
			f := &geometryMetaFake{}
			c := &Controller{Meta: serveMeta(t, f)}
			req := &csi.CreateVolumeRequest{Name: "v", VolumeCapabilities: []*csi.VolumeCapability{blockCap()}, CapacityRange: &csi.CapacityRange{RequiredBytes: x.required, LimitBytes: x.limit}, Parameters: map[string]string{ParamChunkSize: x.chunk, ParamReplicas: x.rf}}
			if x.name == "nil-default" {
				req.CapacityRange = nil
			}
			got, e := c.CreateVolume(context.Background(), req)
			wantCode(t, x.name, e, x.code)
			if x.code != codes.OK {
				if f.creates+f.checks+f.gets != 0 {
					t.Fatalf("invalid request sent RPCs: %+v", f)
				}
				return
			}
			if got.Volume.CapacityBytes != x.want || f.created.SizeBytes != uint64(x.want) {
				t.Fatalf("selected=%v desired=%v", got, f.created)
			}
			if got.Volume.CapacityBytes < x.required || x.limit > 0 && got.Volume.CapacityBytes > x.limit {
				t.Fatal("capacity escaped range")
			}
		})
	}
}

func TestGeometryAuthoritativeRetry(t *testing.T) {
	for _, x := range []struct {
		name   string
		check  codes.Code
		actual uint64
		want   codes.Code
	}{
		{"larger-current", codes.OK, 4 << 20, codes.OK}, {"identity-conflict", codes.AlreadyExists, 0, codes.AlreadyExists},
		{"missing-rpc", codes.Unimplemented, 0, codes.FailedPrecondition}, {"in-progress", codes.Aborted, 0, codes.Aborted},
		{"corrupt-identity", codes.DataLoss, 0, codes.DataLoss}, {"bad-returned-range", codes.OK, 8 << 20, codes.Internal},
		{"bad-returned-signed", codes.OK, ^uint64(0), codes.Internal},
	} {
		t.Run(x.name, func(t *testing.T) {
			f := &geometryMetaFake{createCode: codes.AlreadyExists, checkCode: x.check, actual: x.actual}
			c := &Controller{Meta: serveMeta(t, f)}
			req := &csi.CreateVolumeRequest{Name: "v", VolumeCapabilities: []*csi.VolumeCapability{blockCap()}, CapacityRange: &csi.CapacityRange{RequiredBytes: (1 << 20) + 1, LimitBytes: 5 << 20}, Parameters: map[string]string{ParamChunkSize: "1Mi", ParamReplicas: "3", ParamMinISR: "2", ParamFailureDomain: "zone", ParamFailureDomainPolicy: "bestEffort"}, AccessibilityRequirements: &csi.TopologyRequirement{Preferred: []*csi.Topology{{Segments: map[string]string{TopologyKey: "preferred"}}}}}
			got, e := c.CreateVolume(context.Background(), req)
			wantCode(t, x.name, e, x.want)
			want := &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 << 20, ChunkBytes: 1 << 20, Replicas: 3, MinIsr: 2, PreferNode: "preferred", FailureDomain: "zone", FailureDomainPolicy: "bestEffort"}
			if f.creates != 1 || f.checks != 1 || f.gets != 0 || !proto.Equal(f.created, want) || !proto.Equal(f.checked.Desired, want) || f.checked.RequiredBytes != req.CapacityRange.RequiredBytes || f.checked.LimitBytes != req.CapacityRange.LimitBytes {
				t.Fatalf("unsafe/incomplete retry %+v", f)
			}
			if e == nil && got.Volume.CapacityBytes != int64(x.actual) {
				t.Fatal("response reported initial candidate rather than actual capacity")
			}
		})
	}
}

func TestGeometryQuantityGrammar(t *testing.T) {
	for _, x := range []struct {
		s    string
		want uint64
	}{{"1Ki", 1024}, {"1Mi", 1 << 20}, {"1Gi", 1 << 30}, {"1Ti", 1 << 40}, {"18446744073709551615", ^uint64(0)}, {"17592186044415Mi", 18446744073708503040}} {
		n, e := parseQuantity(x.s)
		if e != nil || n != x.want {
			t.Fatalf("%s = %d,%v", x.s, n, e)
		}
	}
	for _, s := range []string{"17592186044416Mi", "17592186044417Mi", "18014398509481984Ki", "17179869184Gi", "16777216Ti", "1M", "1mi", "-1", "1.5Mi", ""} {
		if _, e := parseQuantity(s); e == nil {
			t.Errorf("accepted %q", s)
		}
	}
}

func TestGeometryInvalidParametersMakeNoRPC(t *testing.T) {
	for _, p := range []map[string]string{
		{ParamChunkSize: "0"}, {ParamChunkSize: "3Mi"}, {ParamReplicas: "6"}, {ParamReplicas: "4294967296"},
		{ParamMinISR: "0"}, {ParamReplicas: "2", ParamMinISR: "3"}, {ParamFailureDomain: "future"}, {ParamFailureDomainPolicy: "future"},
		{ParamChunkSize: "18014398509481984Ki"}, {ParamChunkSize: "17179869184Gi"}, {ParamChunkSize: "16777216Ti"},
	} {
		f := &geometryMetaFake{}
		c := &Controller{Meta: serveMeta(t, f)}
		_, e := c.CreateVolume(context.Background(), &csi.CreateVolumeRequest{Name: "v", Parameters: p, VolumeCapabilities: []*csi.VolumeCapability{blockCap()}})
		if status.Code(e) != codes.InvalidArgument || f.creates+f.checks+f.gets != 0 {
			t.Fatalf("invalid parameters %v reached metadata: %v %+v", p, e, f)
		}
	}
}
