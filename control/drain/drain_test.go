package drain

import (
	"context"
	"net"
	"slices"
	"sync/atomic"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
)

func client(t *testing.T, opts ...grpc.ServerOption) (metav1.MetadataServiceClient, *service.Service) {
	t.Helper()
	svc := service.New(kv.NewMem(nil), service.Options{})
	srv := grpc.NewServer(opts...)
	metav1.RegisterMetadataServiceServer(srv, svc)
	lis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	go func() { _ = srv.Serve(lis) }()
	t.Cleanup(srv.Stop)
	conn, err := grpc.NewClient(lis.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { conn.Close() })
	return metav1.NewMetadataServiceClient(conn), svc
}

func addNode(t *testing.T, c metav1.MetadataServiceClient, id string, labels map[string]string) {
	t.Helper()
	ctx := context.Background()
	if _, err := c.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: id, Address: id + ":7411", Labels: labels}}); err != nil {
		t.Fatal(err)
	}
	if _, err := c.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: id}); err != nil {
		t.Fatal(err)
	}
}

// completeMoves does what the targets would: add each destination, join it, drop the source.
func completeMoves(t *testing.T, c metav1.MetadataServiceClient, vol string) {
	t.Helper()
	ctx := context.Background()
	g, err := c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: vol})
	if err != nil {
		t.Fatal(err)
	}
	for i, ch := range g.GetVolume().GetChunks() {
		from, to := ch.GetMovingFrom(), ch.GetMovingTo()
		if from == "" {
			continue
		}
		added := append(slices.Clone(ch.GetReplicas()), to)
		isr := append(slices.Clone(ch.GetIsr()), to)
		dropped := slices.DeleteFunc(slices.Clone(added), func(n string) bool { return n == from })
		final := slices.DeleteFunc(slices.Clone(isr), func(n string) bool { return n == from })
		ver := ch.GetIsrVer()
		for _, step := range []*metav1.IsrChange{
			{ExpectedVer: ver, NewIsr: ch.GetIsr(), ChangeReplicas: true, NewReplicas: added},
			{ExpectedVer: ver + 1, NewIsr: isr},
			{ExpectedVer: ver + 2, NewIsr: final, ChangeReplicas: true, NewReplicas: dropped},
		} {
			step.Chunk = uint32(i)
			r, err := c.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: vol, Epoch: g.GetVolume().GetEpoch(), Changes: []*metav1.IsrChange{step}})
			if err != nil || !r.GetApplied() {
				t.Fatalf("chunk %d step %+v: %v %v", i, step, r, err)
			}
		}
	}
}

func TestDrain(t *testing.T) {
	c, _ := client(t)
	ctx := context.Background()
	for _, n := range []string{"a", "b"} {
		addNode(t, c, n, nil)
	}
	addNode(t, c, "w", map[string]string{WitnessLabel: ""})
	for _, v := range []string{"v1", "v2"} {
		if _, err := c.CreateVolume(ctx, &metav1.CreateVolumeRequest{Id: v, SizeBytes: 4 << 20, ChunkBytes: 1 << 20, Replicas: 2, MinIsr: 1}); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := c.Attach(ctx, &metav1.AttachRequest{Volume: "v1", Node: "a"}); err != nil {
		t.Fatal(err)
	}

	// Nowhere to go: the witness takes no data.
	if _, err := Step(ctx, c, "b"); err == nil {
		t.Fatal("a drain with no destination did not fail")
	}
	addNode(t, c, "c", nil)
	if _, err := c.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "b", Draining: true}); err != nil {
		t.Fatal(err)
	}
	p, err := Step(ctx, c, "b")
	if err != nil {
		t.Fatal(err)
	}
	if p.Remaining != 8 || p.Moving != 8 || !slices.Equal(p.Unattached, []string{"v2"}) {
		t.Fatalf("first step: %+v", p)
	}
	// Each chunk has one move pending, to c: a second step requests nothing more.
	if p2, err := Step(ctx, c, "b"); err != nil || p2.Moving != 8 {
		t.Fatalf("second step: %+v %v", p2, err)
	}
	g, _ := c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v1"})
	for _, ch := range g.GetVolume().GetChunks() {
		if ch.GetMovingFrom() != "b" || ch.GetMovingTo() != "c" {
			t.Fatalf("move %s -> %s", ch.GetMovingFrom(), ch.GetMovingTo())
		}
	}
	completeMoves(t, c, "v1")
	if p, _ := Step(ctx, c, "b"); p.Remaining != 4 || !slices.Equal(p.Unattached, []string{"v2"}) {
		t.Fatalf("after v1 moved: %+v", p)
	}
	if _, err := c.Attach(ctx, &metav1.AttachRequest{Volume: "v2", Node: "c"}); err != nil {
		t.Fatal(err)
	}
	completeMoves(t, c, "v2")
	if p, _ := Step(ctx, c, "b"); p.Remaining != 0 {
		t.Fatalf("after v2 moved: %+v", p)
	}
}

// volumesOnABC is v1 and v2, 4 chunks each on a, b and c, attached on a, with d added after them
// as the only node to move to.
func volumesOnABC(t *testing.T, c metav1.MetadataServiceClient) {
	t.Helper()
	ctx := context.Background()
	for _, n := range []string{"a", "b", "c"} {
		addNode(t, c, n, nil)
	}
	for _, v := range []string{"v1", "v2"} {
		if _, err := c.CreateVolume(ctx, &metav1.CreateVolumeRequest{Id: v, SizeBytes: 4 << 20, ChunkBytes: 1 << 20, Replicas: 3, MinIsr: 1}); err != nil {
			t.Fatal(err)
		}
		if _, err := c.Attach(ctx, &metav1.AttachRequest{Volume: v, Node: "a"}); err != nil {
			t.Fatal(err)
		}
	}
	addNode(t, c, "d", nil)
}

// deleteBefore is a server interceptor that deletes v1 just before the first request that
// `match` picks reaches the service.
func deleteBefore(svc **service.Service, t *testing.T, match func(req any) bool) grpc.ServerOption {
	var done atomic.Bool
	return grpc.UnaryInterceptor(func(ctx context.Context, req any, _ *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		if match(req) && !done.Swap(true) {
			if _, err := (*svc).DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v1"}); err != nil {
				t.Error(err)
			}
		}
		return h(ctx, req)
	})
}

// v2Moved checks that the step moved v2's four chunks off b, and counted only v2.
func v2Moved(t *testing.T, c metav1.MetadataServiceClient, p Progress) {
	t.Helper()
	g, err := c.GetVolume(context.Background(), &metav1.GetVolumeRequest{Id: "v2"})
	if err != nil {
		t.Fatal(err)
	}
	for i, ch := range g.GetVolume().GetChunks() {
		if ch.GetMovingFrom() != "b" || ch.GetMovingTo() != "d" {
			t.Fatalf("v2 chunk %d: move %q -> %q", i, ch.GetMovingFrom(), ch.GetMovingTo())
		}
	}
	if p.Remaining != 4 || p.Moving != 4 || len(p.Unattached) != 0 {
		t.Fatalf("progress counts the deleted volume: %+v", p)
	}
}

// A volume deleted after a step listed it, before the step's move of its chunk: the step skips
// it, without counting it, and goes on to the next volume, instead of failing (docs/8).
func TestDrainSkipsAVolumeDeletedBeforeItsMove(t *testing.T) {
	var svc *service.Service
	c, s := client(t, deleteBefore(&svc, t, func(req any) bool {
		r, ok := req.(*metav1.MoveReplicaRequest)
		return ok && r.GetVolume() == "v1"
	}))
	svc = s
	volumesOnABC(t, c)
	p, err := Step(context.Background(), c, "b")
	if err != nil {
		t.Fatalf("step with a volume deleted mid-step: %v", err)
	}
	v2Moved(t, c, p)
}

// The same, with the volume deleted between the step's listing and its read of the volume.
func TestDrainSkipsAVolumeDeletedBeforeItsRead(t *testing.T) {
	var svc *service.Service
	c, s := client(t, deleteBefore(&svc, t, func(req any) bool {
		r, ok := req.(*metav1.GetVolumeRequest)
		return ok && r.GetId() == "v1"
	}))
	svc = s
	volumesOnABC(t, c)
	p, err := Step(context.Background(), c, "b")
	if err != nil {
		t.Fatalf("step with a volume deleted mid-step: %v", err)
	}
	v2Moved(t, c, p)
}

// A move refused with NotFound for another reason than a deleted volume (the destination gone)
// fails the step, rather than skip the volume silently.
func TestDrainFailsOnAnotherNotFound(t *testing.T) {
	refuse := grpc.UnaryInterceptor(func(ctx context.Context, req any, _ *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		if _, ok := req.(*metav1.MoveReplicaRequest); ok {
			return nil, status.Error(codes.NotFound, `not found: node "d" is not registered`)
		}
		return h(ctx, req)
	})
	c, _ := client(t, refuse)
	volumesOnABC(t, c)
	if _, err := Step(context.Background(), c, "b"); status.Code(err) != codes.NotFound {
		t.Fatalf("step with the destination gone: %v", err)
	}
}
