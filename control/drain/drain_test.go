package drain

import (
	"context"
	"net"
	"slices"
	"sync/atomic"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
)

func client(t *testing.T) (metav1.MetadataServiceClient, *service.Service) {
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

// A volume deleted after a step listed it, before the step's move of its chunk: the step skips
// it and goes on to the next volume, instead of failing (docs/8).
func TestDrainSkipsAVolumeDeletedMidStep(t *testing.T) {
	ctx := context.Background()
	svc := service.New(kv.NewMem(nil), service.Options{})
	var deleted atomic.Bool
	// Deletes the volume just before the step's first move of it reaches the service.
	intercept := func(ctx context.Context, req any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		if m, ok := req.(*metav1.MoveReplicaRequest); ok && m.GetVolume() == "v1" && !deleted.Swap(true) {
			if _, err := svc.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v1"}); err != nil {
				t.Error(err)
			}
		}
		return h(ctx, req)
	}
	srv := grpc.NewServer(grpc.UnaryInterceptor(intercept))
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
	c := metav1.NewMetadataServiceClient(conn)
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
	p, err := Step(ctx, c, "b")
	if err != nil {
		t.Fatalf("step with a volume deleted mid-step: %v", err)
	}
	// v2's four chunks were still moved off b.
	g, err := c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v2"})
	if err != nil {
		t.Fatal(err)
	}
	for i, ch := range g.GetVolume().GetChunks() {
		if ch.GetMovingFrom() != "b" || ch.GetMovingTo() != "d" {
			t.Fatalf("v2 chunk %d: move %q -> %q (progress %+v)", i, ch.GetMovingFrom(), ch.GetMovingTo(), p)
		}
	}
}
