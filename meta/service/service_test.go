package service

import (
	"context"
	"math/rand/v2"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/kv/kvtest"
)

const mib = 1 << 20

// stores runs a test against the in-memory store and against a real etcd.
func stores(t *testing.T, f func(t *testing.T, s *Service)) {
	t.Run("mem", func(t *testing.T) { f(t, New(kv.NewMem(nil), 10*time.Second)) })
	t.Run("etcd", func(t *testing.T) {
		e := kvtest.Etcd(t)
		defer e.Close()
		f(t, New(e, 10*time.Second))
	})
}

func code(err error) codes.Code { return status.Code(err) }

func addNodes(t *testing.T, s *Service, ids ...string) {
	ctx := context.Background()
	for _, id := range ids {
		if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: id, Address: id + ":7410"}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: id}); err != nil {
			t.Fatal(err)
		}
	}
}

func create(s *Service, id string, chunks uint64, rf, min uint32) (*metav1.Volume, error) {
	r, err := s.CreateVolume(context.Background(), &metav1.CreateVolumeRequest{
		Id: id, SizeBytes: chunks * mib, ChunkBytes: mib, Replicas: rf, MinIsr: min,
	})
	return r.GetVolume(), err
}

func TestVolumeLifecycle(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "ghost"}); code(err) != codes.NotFound {
			t.Fatalf("heartbeat of an unregistered node: %v", err)
		}
		addNodes(t, s, "a", "b", "c")
		if _, err := create(s, "v", 3, 4, 2); code(err) != codes.FailedPrecondition {
			t.Fatalf("4 replicas on 3 nodes: %v", err)
		}
		v, err := create(s, "v", 3, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		if len(v.Chunks) != 3 || v.Epoch != 0 {
			t.Fatalf("created %+v", v)
		}
		used := map[string]int{}
		for _, c := range v.Chunks {
			if len(c.Isr) != 2 || c.Replicas[0] == c.Replicas[1] {
				t.Fatalf("chunk %+v", c)
			}
			for _, n := range c.Replicas {
				used[n]++
			}
		}
		if len(used) != 3 {
			t.Fatalf("placement does not use every node: %v", used)
		}
		if _, err := create(s, "v", 1, 1, 1); code(err) != codes.AlreadyExists {
			t.Fatalf("duplicate: %v", err)
		}
		g, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		if err != nil || len(g.Volume.Chunks) != 3 {
			t.Fatalf("get: %v %v", g, err)
		}

		// Attach, then change an ISR under the right epoch and version.
		a, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil || a.Volume.Epoch != 1 || a.Volume.Target != "a" {
			t.Fatalf("attach: %v %v", a, err)
		}
		c0 := a.Volume.Chunks[0]
		change := func(epoch, ver uint64, isr ...string) *metav1.ChangeIsrResponse {
			r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: epoch, Changes: []*metav1.IsrChange{{Chunk: 0, ExpectedVer: ver, NewIsr: isr}}})
			if err != nil {
				t.Fatal(err)
			}
			return r
		}
		if r := change(1, 0, c0.Replicas[0]); !r.Applied || r.Chunks[0].IsrVer != 1 || len(r.Chunks[0].Isr) != 1 {
			t.Fatalf("shrink: %+v", r)
		}
		// A retry of the same change is refused, and shows the state it produced.
		if r := change(1, 0, c0.Replicas[0]); r.Applied || r.Chunks[0].IsrVer != 1 {
			t.Fatalf("retry: %+v", r)
		}
		if r := change(1, 1, c0.Replicas...); !r.Applied || len(r.Chunks[0].Isr) != 2 {
			t.Fatalf("expand: %+v", r)
		}
		// A new attach deposes the old target: its changes are refused with the new epoch.
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b"}); err != nil {
			t.Fatal(err)
		}
		if r := change(1, 2, c0.Replicas[0]); r.Applied || r.Epoch != 2 {
			t.Fatalf("old epoch: %+v", r)
		}
		if _, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: 2, Changes: []*metav1.IsrChange{{Chunk: 9}}}); code(err) != codes.InvalidArgument {
			t.Fatalf("chunk out of range: %v", err)
		}

		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); code(err) != codes.NotFound {
			t.Fatalf("after delete: %v", err)
		}
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"}); code(err) != codes.NotFound {
			t.Fatalf("attach after delete: %v", err)
		}
		// The id can be reused, and the new volume has no leftovers.
		if v, err := create(s, "v", 1, 3, 2); err != nil || len(v.Chunks) != 1 {
			t.Fatalf("recreate: %v %v", v, err)
		}
		if g, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); err != nil || len(g.Volume.Chunks) != 1 || g.Volume.Epoch != 0 {
			t.Fatalf("recreated: %v %v", g, err)
		}
	})
}

func TestLiveness(t *testing.T) {
	now := time.Unix(1000, 0)
	s := New(kv.NewMem(func() time.Time { return now }), 10*time.Second)
	addNodes(t, s, "a", "b")
	ctx := context.Background()
	now = now.Add(6 * time.Second)
	if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "a"}); err != nil {
		t.Fatal(err)
	}
	now = now.Add(6 * time.Second)
	l, err := s.ListNodes(ctx, &metav1.ListNodesRequest{})
	if err != nil || len(l.Nodes) != 2 || !l.Nodes[0].Alive || l.Nodes[1].Alive {
		t.Fatalf("a alive, b expired: %v %v", l, err)
	}
	// Placement uses live nodes only.
	if _, err := create(s, "v", 1, 2, 1); code(err) != codes.FailedPrecondition {
		t.Fatalf("placed on a dead node: %v", err)
	}
}

func TestWitnessHoldsNoData(t *testing.T) {
	s := New(kv.NewMem(nil), 10*time.Second)
	ctx := context.Background()
	addNodes(t, s, "a", "b")
	w := &metav1.Node{Id: "w", Address: "w:7410", Labels: map[string]string{WitnessLabel: ""}}
	if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: w}); err != nil {
		t.Fatal(err)
	}
	if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "w"}); err != nil {
		t.Fatal(err)
	}
	if _, err := create(s, "v3", 1, 3, 1); code(err) != codes.FailedPrecondition {
		t.Fatalf("3 replicas with 2 data nodes and a witness: %v", err)
	}
	v, err := create(s, "v", 8, 2, 1)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range v.Chunks {
		for _, n := range c.Replicas {
			if n == "w" {
				t.Fatalf("replica on the witness: %v", c.Replicas)
			}
		}
	}
}

// TestConcurrentTargets races attaches and ISR changes from many goroutines, as targets of
// different epochs would, and checks the invariants afterwards: every applied change bumped
// its chunk's version by exactly one, every attach bumped the epoch, and no ISR ever fell
// below the minimum.
func TestConcurrentTargets(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b", "c", "d")
		v, err := create(s, "v", 4, 3, 2)
		if err != nil {
			t.Fatal(err)
		}
		var attaches, applied atomic.Int64
		perChunk := make([]atomic.Int64, len(v.Chunks))
		var wg sync.WaitGroup
		for w := range 8 {
			wg.Add(1)
			go func() {
				defer wg.Done()
				rng := rand.New(rand.NewPCG(uint64(w), 1))
				var view *metav1.Volume
				for range 40 {
					if view == nil || rng.IntN(5) == 0 {
						r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "abcd"[w%4 : w%4+1]})
						if code(err) == codes.Aborted {
							continue // contention: nothing was committed
						}
						if err != nil {
							t.Error(err)
							return
						}
						attaches.Add(1)
						view = r.Volume
						continue
					}
					i := rng.IntN(len(view.Chunks))
					c := view.Chunks[i]
					isr := []string{}
					for _, n := range c.Replicas {
						if rng.IntN(3) > 0 {
							isr = append(isr, n)
						}
					}
					r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: view.Epoch, Changes: []*metav1.IsrChange{{Chunk: uint32(i), ExpectedVer: c.IsrVer, NewIsr: isr}}})
					if code(err) == codes.Aborted {
						continue
					}
					if err != nil {
						t.Error(err)
						return
					}
					if r.Applied {
						applied.Add(1)
						perChunk[i].Add(1)
					}
					if r.Epoch > view.Epoch {
						view = nil // deposed: attach again
						continue
					}
					view.Chunks[i] = r.Chunks[0]
				}
			}()
		}
		wg.Wait()
		g, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		if err != nil {
			t.Fatal(err)
		}
		if g.Volume.Epoch != uint64(attaches.Load()) {
			t.Fatalf("epoch %d after %d attaches", g.Volume.Epoch, attaches.Load())
		}
		for i, c := range g.Volume.Chunks {
			if c.IsrVer != uint64(perChunk[i].Load()) {
				t.Fatalf("chunk %d: version %d after %d applied changes", i, c.IsrVer, perChunk[i].Load())
			}
			if len(c.Isr) < 2 {
				t.Fatalf("chunk %d: ISR %v below the minimum", i, c.Isr)
			}
		}
		t.Logf("%d attaches, %d ISR changes applied", attaches.Load(), applied.Load())
	})
}

// TestGRPC checks the service end to end over a gRPC connection.
func TestGRPC(t *testing.T) {
	lis := bufconn.Listen(1 << 20)
	srv := grpc.NewServer()
	metav1.RegisterMetadataServiceServer(srv, New(kv.NewMem(nil), 10*time.Second))
	go func() { _ = srv.Serve(lis) }()
	defer srv.Stop()
	conn, err := grpc.NewClient("passthrough:///bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) { return lis.DialContext(ctx) }),
		grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	c := metav1.NewMetadataServiceClient(conn)
	ctx := context.Background()
	if _, err := c.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: "a", Address: "a:1"}}); err != nil {
		t.Fatal(err)
	}
	if _, err := c.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "a"}); err != nil {
		t.Fatal(err)
	}
	r, err := c.CreateVolume(ctx, &metav1.CreateVolumeRequest{Id: "v", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinIsr: 1})
	if err != nil || r.Volume.Chunks[0].Replicas[0] != "a" {
		t.Fatalf("%v %v", r, err)
	}
	if _, err := c.CreateVolume(ctx, &metav1.CreateVolumeRequest{Id: "../x", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinIsr: 1}); code(err) != codes.InvalidArgument {
		t.Fatalf("bad id: %v", err)
	}
}
