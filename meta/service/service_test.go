package service

import (
	"context"
	"encoding/json"
	"fmt"
	"math/rand/v2"
	"net"
	"slices"
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
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/kv/kvtest"
)

const mib = 1 << 20

// stores runs a test against the in-memory store and against a real etcd.
func stores(t *testing.T, f func(t *testing.T, s *Service)) {
	backends(t, func(t *testing.T, store kv.KV) { f(t, New(store, Options{})) })
}

// backends runs a test against the in-memory store and against a real etcd.
func backends(t *testing.T, f func(t *testing.T, store kv.KV)) {
	t.Run("mem", func(t *testing.T) { f(t, kv.NewMem(nil)) })
	t.Run("etcd", func(t *testing.T) {
		e := kvtest.Etcd(t)
		defer e.Close()
		f(t, e)
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
		// unless_target: a repeated publish to the target changes nothing and issues no token;
		// to another node it attaches as usual.
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", UnlessTarget: true}); err != nil || r.Volume.Epoch != 2 || r.Token != nil {
			t.Fatalf("unless_target, already the target: %v %v", r, err)
		}
		// only_if_target: the target restarts under a new epoch; another node is refused.
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a", OnlyIfTarget: true}); code(err) != codes.FailedPrecondition {
			t.Fatalf("only_if_target, not the target: %v", err)
		}
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", OnlyIfTarget: true}); err != nil || r.Volume.Epoch != 3 || r.Volume.Target != "b" {
			t.Fatalf("only_if_target: %v %v", r, err)
		}
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", OnlyIfTarget: true, UnlessTarget: true}); code(err) != codes.InvalidArgument {
			t.Fatalf("both modes: %v", err)
		}
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a", UnlessTarget: true}); err != nil || r.Volume.Epoch != 4 || r.Volume.Target != "a" {
			t.Fatalf("unless_target, another node: %v %v", r, err)
		}
		if _, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: 4, Changes: []*metav1.IsrChange{{Chunk: 9}}}); code(err) != codes.InvalidArgument {
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
		// The id cannot be reused until every former replica node confirms deletion.
		if _, err := create(s, "v", 1, 3, 2); code(err) != codes.AlreadyExists {
			t.Fatalf("recreate with tombstone: %v", err)
		}
		for _, n := range []string{"a", "b", "c"} {
			list, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: n})
			if err != nil {
				t.Fatal(err)
			}
			if len(list.Tombstones) == 0 {
				continue
			}
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: n}); err != nil {
				t.Fatal(err)
			}
		}
		if v, err := create(s, "v", 1, 3, 2); err != nil || len(v.Chunks) != 1 {
			t.Fatalf("recreate: %v %v", v, err)
		}
		if g, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); err != nil || len(g.Volume.Chunks) != 1 || g.Volume.Epoch != 0 {
			t.Fatalf("recreated: %v %v", g, err)
		}
	})
}

// TestTombstones: DeleteVolume writes a tombstone; partial confirms leave it; full confirms remove it.
func TestTombstones(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b", "c")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		nodes := map[string]struct{}{}
		for _, c := range v.Chunks {
			for _, n := range c.Replicas {
				nodes[n] = struct{}{}
			}
		}
		if len(nodes) < 2 {
			t.Fatalf("want replicas on ≥2 nodes, got %v", nodes)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); code(err) != codes.NotFound {
			t.Fatalf("volume still visible: %v", err)
		}
		listed := map[string]bool{}
		for n := range nodes {
			r, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: n})
			if err != nil || len(r.Tombstones) != 1 || r.Tombstones[0].Volume != "v" {
				t.Fatalf("list for %s: %v %v", n, r, err)
			}
			if len(r.Tombstones[0].Pending) != len(nodes) {
				t.Fatalf("pending before any confirm: %v", r.Tombstones[0].Pending)
			}
			listed[n] = true
		}
		// A node that never held a replica sees nothing.
		for _, id := range []string{"a", "b", "c"} {
			if listed[id] {
				continue
			}
			r, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: id})
			if err != nil || len(r.Tombstones) != 0 {
				t.Fatalf("unrelated node %s: %v %v", id, r, err)
			}
		}
		// Confirm one node: tombstone remains for the others.
		var first string
		for n := range nodes {
			first = n
			break
		}
		if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: first}); err != nil {
			t.Fatal(err)
		}
		r, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: first})
		if err != nil || len(r.Tombstones) != 0 {
			t.Fatalf("after own confirm: %v %v", r, err)
		}
		remaining := 0
		for n := range nodes {
			if n == first {
				continue
			}
			r, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: n})
			if err != nil || len(r.Tombstones) != 1 {
				t.Fatalf("partial confirm, %s: %v %v", n, r, err)
			}
			remaining++
		}
		if remaining == 0 {
			t.Fatal("expected other nodes still pending")
		}
		// Confirm the rest: tombstone gone for everyone; confirm is idempotent.
		for n := range nodes {
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: n}); err != nil {
				t.Fatal(err)
			}
		}
		for n := range nodes {
			r, err := s.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: n})
			if err != nil || len(r.Tombstones) != 0 {
				t.Fatalf("after all confirms, %s: %v %v", n, r, err)
			}
		}
		if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: first}); err != nil {
			t.Fatalf("idempotent confirm after tombstone gone: %v", err)
		}
		// While a tombstone exists, a node that never held a replica may not confirm.
		if _, err := create(s, "w", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "w"}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "w", Node: "ghost"}); code(err) != codes.InvalidArgument {
			t.Fatalf("unnamed node on live tombstone: %v", err)
		}
		// Id reuse is refused until the tombstone is gone.
		if _, err := create(s, "w", 1, 1, 1); code(err) != codes.AlreadyExists {
			t.Fatalf("create while tombstoned: %v", err)
		}
	})
}

func TestLiveness(t *testing.T) {
	now := time.Unix(1000, 0)
	s := New(kv.NewMem(func() time.Time { return now }), Options{TTL: 10 * time.Second})
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
	s := New(kv.NewMem(nil), Options{})
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

func TestFailureDomainPlacement(t *testing.T) {
	s := New(kv.NewMem(nil), Options{})
	ctx := context.Background()
	for _, n := range []*metav1.Node{
		{Id: "a1", Address: "a1:1", Labels: map[string]string{"topology.kubernetes.io/zone": "z1"}},
		{Id: "a2", Address: "a2:1", Labels: map[string]string{"topology.kubernetes.io/zone": "z1"}},
		{Id: "b1", Address: "b1:1", Labels: map[string]string{"topology.kubernetes.io/zone": "z2"}},
		{Id: "b2", Address: "b2:1", Labels: map[string]string{"topology.kubernetes.io/zone": "z2"}},
	} {
		if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: n}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: n.Id}); err != nil {
			t.Fatal(err)
		}
	}
	// Strict zone with only two zones refuses three replicas.
	if _, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
		Id: "v3", SizeBytes: mib, ChunkBytes: mib, Replicas: 3, MinIsr: 1, FailureDomain: "zone",
	}); code(err) != codes.FailedPrecondition {
		t.Fatalf("strict zone RF=3: %v", err)
	}
	v, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
		Id: "v", SizeBytes: 4 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, FailureDomain: "zone",
	})
	if err != nil {
		t.Fatal(err)
	}
	zoneOf := map[string]string{"a1": "z1", "a2": "z1", "b1": "z2", "b2": "z2"}
	for _, c := range v.GetVolume().Chunks {
		if zoneOf[c.Replicas[0]] == zoneOf[c.Replicas[1]] {
			t.Fatalf("replicas share a zone: %v", c.Replicas)
		}
	}
	// Address-only re-register keeps topology labels (data plane must not wipe them).
	if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: "a1", Address: "a1:2"}}); err != nil {
		t.Fatal(err)
	}
	nodes, err := s.ListNodes(ctx, &metav1.ListNodesRequest{})
	if err != nil {
		t.Fatal(err)
	}
	for _, n := range nodes.Nodes {
		if n.Id == "a1" && (n.Address != "a1:2" || n.Labels["topology.kubernetes.io/zone"] != "z1") {
			t.Fatalf("label merge lost: %+v", n)
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
	metav1.RegisterMetadataServiceServer(srv, New(kv.NewMem(nil), Options{}))
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

// TestMoveAndDrain requests a move, carries it out as the target would (add, join, drop),
// and checks that a draining node gets no new volumes.
func TestMoveAndDrain(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b")
		if _, err := create(s, "v", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		addNodes(t, s, "c")
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"}); err != nil {
			t.Fatal(err)
		}
		if l, err := s.ListVolumes(ctx, &metav1.ListVolumesRequest{}); err != nil || len(l.Ids) != 1 || l.Ids[0] != "v" {
			t.Fatalf("volumes: %v %v", l, err)
		}
		move := func(from, to string) bool {
			t.Helper()
			r, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", Chunk: 0, From: from, To: to})
			if err != nil {
				t.Fatal(err)
			}
			return r.Applied
		}
		if move("c", "b") || move("a", "b") {
			t.Fatal("a move from a node without a replica, or to one with a replica, was applied")
		}
		if _, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", From: "b", To: "nope"}); code(err) != codes.NotFound {
			t.Fatalf("a move to an unregistered node: %v", err)
		}
		if !move("b", "c") || move("a", "c") {
			t.Fatal("one move, then no second one while it is pending")
		}
		g, _ := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		c0 := g.Volume.Chunks[0]
		if c0.MovingFrom != "b" || c0.MovingTo != "c" || c0.IsrVer != 0 {
			t.Fatalf("after the request: %+v", c0)
		}
		step := func(ver uint64, isr []string, replicas []string) bool {
			t.Helper()
			ch := &metav1.IsrChange{Chunk: 0, ExpectedVer: ver, NewIsr: isr}
			if replicas != nil {
				ch.ChangeReplicas, ch.NewReplicas = true, replicas
			}
			r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: 1, Changes: []*metav1.IsrChange{ch}})
			if err != nil {
				t.Fatal(err)
			}
			return r.Applied
		}
		rep := c0.Replicas // placement order
		if step(0, []string{"a"}, []string{"a"}) {
			t.Fatal("the source dropped before the destination joined")
		}
		if !step(0, c0.Isr, append(append([]string{}, rep...), "c")) {
			t.Fatal("add the destination")
		}
		if !step(1, []string{"a", "b", "c"}, nil) {
			t.Fatal("the destination joins")
		}
		without := []string{}
		for _, n := range append(append([]string{}, rep...), "c") {
			if n != "b" {
				without = append(without, n)
			}
		}
		if !step(2, []string{"a", "c"}, without) {
			t.Fatal("drop the source")
		}
		g, _ = s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		if c := g.Volume.Chunks[0]; c.MovingFrom != "" || len(c.Replicas) != 2 || c.IsrVer != 3 {
			t.Fatalf("after the move: %+v", c)
		}

		// A draining node gets no new volumes, and stays draining when it registers again.
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true}); err != nil {
			t.Fatal(err)
		}
		addNodes(t, s, "c")
		if _, err := create(s, "w", 4, 3, 1); code(err) != codes.FailedPrecondition {
			t.Fatalf("3 replicas with a draining node among 3: %v", err)
		}
		n, _ := s.ListNodes(ctx, &metav1.ListNodesRequest{})
		for _, x := range n.Nodes {
			if x.Draining != (x.Id == "c") {
				t.Fatalf("draining flags: %v", n.Nodes)
			}
		}
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: false}); err != nil {
			t.Fatal(err)
		}
		if _, err := create(s, "w", 4, 3, 1); err != nil {
			t.Fatalf("after undrain: %v", err)
		}
	})
}

// interleaveKV runs a hook once, after the first Get of `key`, or List of `prefix`, it sees and
// before returning that read: the hook can make another write land between a read and the write
// that follows it. It counts the Txn calls on `key` made outside the hook.
type interleaveKV struct {
	kv.KV
	key    string
	prefix string
	hook   atomic.Pointer[func()]
	inner  atomic.Bool
	txns   atomic.Int32
}

// runHook runs the hook, swapped out before it runs, so that its own reads do not run it again.
func (k *interleaveKV) runHook() {
	if h := k.hook.Swap(nil); h != nil {
		k.inner.Store(true)
		(*h)()
		k.inner.Store(false)
	}
}

func (k *interleaveKV) Get(ctx context.Context, key string) (kv.Entry, bool, error) {
	e, ok, err := k.KV.Get(ctx, key)
	if key == k.key {
		k.runHook()
	}
	return e, ok, err
}

func (k *interleaveKV) List(ctx context.Context, prefix string) ([]kv.Entry, error) {
	es, err := k.KV.List(ctx, prefix)
	if k.prefix != "" && prefix == k.prefix {
		k.runHook()
	}
	return es, err
}

func (k *interleaveKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	for _, op := range ops {
		if op.Key == k.key && !k.inner.Load() {
			k.txns.Add(1)
		}
	}
	return k.KV.Txn(ctx, cmps, ops)
}

func nodeLabels(t *testing.T, s *Service, id string) (string, map[string]string) {
	t.Helper()
	r, err := s.ListNodes(context.Background(), &metav1.ListNodesRequest{})
	if err != nil {
		t.Fatal(err)
	}
	for _, n := range r.GetNodes() {
		if n.GetId() == id {
			return n.GetAddress(), n.GetLabels()
		}
	}
	t.Fatalf("node %q not listed", id)
	return "", nil
}

func register(ctx context.Context, s *Service, id, addr string, labels map[string]string) error {
	_, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: id, Address: addr, Labels: labels}})
	return err
}

// Two registrations of a node that interleave, one between the other's read and write, keep
// both their labels (docs/7): with no record yet, and with one.
func TestRegisterNodeInterleavedKeepsLabels(t *testing.T) {
	for _, tc := range []struct {
		name     string
		existing bool
	}{{"no record", false}, {"existing record", true}} {
		existing := tc.existing
		t.Run(tc.name, func(t *testing.T) {
			ctx := context.Background()
			store := &interleaveKV{KV: kv.NewMem(nil), key: nodeKey("n1")}
			s := New(store, Options{})
			if existing {
				if err := register(ctx, s, "n1", "n1:7410", map[string]string{"old": "1"}); err != nil {
					t.Fatal(err)
				}
			}
			hook := func() {
				if err := register(ctx, s, "n1", "n1:7410", map[string]string{"zone": "z1"}); err != nil {
					t.Error(err)
				}
			}
			store.txns.Store(0)
			store.hook.Store(&hook)
			if err := register(ctx, s, "n1", "n1:7410", map[string]string{"rack": "r1"}); err != nil {
				t.Fatal(err)
			}
			// The first write lost its compare to the hook's, and the second applied.
			if got := store.txns.Load(); got != 2 {
				t.Fatalf("%d writes by the first registration, want 2", got)
			}
			_, labels := nodeLabels(t, s, "n1")
			if labels["zone"] != "z1" || labels["rack"] != "r1" || (existing && labels["old"] != "1") {
				t.Fatalf("labels %v", labels)
			}
		})
	}
}

// Twenty registrations of a node at once, each with its own label, all succeed and leave every
// label (docs/7).
func TestRegisterNodeConcurrentKeepsLabels(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		const n = 20
		var wg sync.WaitGroup
		errs := make(chan error, n)
		for i := range n {
			wg.Go(func() {
				errs <- register(ctx, s, "n1", "n1:7410", map[string]string{fmt.Sprintf("l%02d", i): "x"})
			})
		}
		wg.Wait()
		close(errs)
		for err := range errs {
			if err != nil {
				t.Fatal(err)
			}
		}
		if _, labels := nodeLabels(t, s, "n1"); len(labels) != n {
			t.Fatalf("%d labels, want %d: %v", len(labels), n, labels)
		}
	})
}

// A registration's label overrides a stored one of the same name; other stored labels stay, and
// the address is replaced.
func TestRegisterNodeOverwritesLabelsAndAddress(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		if err := register(ctx, s, "n1", "n1:1", map[string]string{"zone": "z1", "rack": "r1"}); err != nil {
			t.Fatal(err)
		}
		if err := register(ctx, s, "n1", "n1:2", map[string]string{"zone": "z2"}); err != nil {
			t.Fatal(err)
		}
		addr, labels := nodeLabels(t, s, "n1")
		if addr != "n1:2" || labels["zone"] != "z2" || labels["rack"] != "r1" {
			t.Fatalf("address %q, labels %v", addr, labels)
		}
	})
}

// A node record that does not parse is overwritten by the next registration, which makes
// ListNodes work again.
func TestRegisterNodeOverwritesAnUnparseableRecord(t *testing.T) {
	ctx := context.Background()
	m := kv.NewMem(nil)
	if _, err := m.Txn(ctx, nil, []kv.Op{{Key: nodeKey("n1"), Value: []byte("not json")}}); err != nil {
		t.Fatal(err)
	}
	s := New(m, Options{})
	if err := register(ctx, s, "n1", "n1:7410", map[string]string{"zone": "z1"}); err != nil {
		t.Fatal(err)
	}
	if addr, labels := nodeLabels(t, s, "n1"); addr != "n1:7410" || labels["zone"] != "z1" {
		t.Fatalf("address %q, labels %v", addr, labels)
	}
}

// A registration without an address adds its labels and keeps the stored address, so the CSI
// node plugin cannot put back an old one; a node not registered yet is refused (docs/7).
func TestRegisterNodeWithoutAnAddressKeepsTheStoredOne(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		if err := register(ctx, s, "n1", "", map[string]string{"zone": "z1"}); code(err) != codes.NotFound {
			t.Fatalf("labels for an unregistered node: %v", err)
		}
		if err := register(ctx, s, "n1", "n1:1", nil); err != nil {
			t.Fatal(err)
		}
		if err := register(ctx, s, "n1", "n1:2", nil); err != nil {
			t.Fatal(err)
		}
		if err := register(ctx, s, "n1", "", map[string]string{"zone": "z1"}); err != nil {
			t.Fatal(err)
		}
		if addr, labels := nodeLabels(t, s, "n1"); addr != "n1:2" || labels["zone"] != "z1" {
			t.Fatalf("address %q, labels %v", addr, labels)
		}
	})
}

// A registration that changes nothing writes nothing: other registrations in flight need not
// retry.
func TestRegisterNodeThatChangesNothingDoesNotWrite(t *testing.T) {
	ctx := context.Background()
	store := &interleaveKV{KV: kv.NewMem(nil), key: nodeKey("n1")}
	s := New(store, Options{})
	labels := map[string]string{"zone": "z1"}
	if err := register(ctx, s, "n1", "n1:7410", labels); err != nil {
		t.Fatal(err)
	}
	store.txns.Store(0)
	if err := register(ctx, s, "n1", "n1:7410", labels); err != nil {
		t.Fatal(err)
	}
	if err := register(ctx, s, "n1", "", nil); err != nil {
		t.Fatal(err)
	}
	if got := store.txns.Load(); got != 0 {
		t.Fatalf("%d writes for registrations that change nothing", got)
	}
}

// interleaveStores runs a test against a wrapped in-memory store and a wrapped real etcd.
func interleaveStores(t *testing.T, f func(t *testing.T, store *interleaveKV)) {
	backends(t, func(t *testing.T, store kv.KV) { f(t, &interleaveKV{KV: store}) })
}

// moveFixture is a volume v of 2 chunks on nodes a and b (RF 2), with node c registered after
// it, so that c holds no replica: a move of chunk 0 from a to c is always accepted.
func moveFixture(t *testing.T, store *interleaveKV) *Service {
	t.Helper()
	s := New(store, Options{})
	addNodes(t, s, "a", "b")
	if _, err := create(s, "v", 2, 2, 1); err != nil {
		t.Fatal(err)
	}
	addNodes(t, s, "c")
	return s
}

func move(ctx context.Context, s *Service, from, to string) (*metav1.MoveReplicaResponse, error) {
	return s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", Chunk: 0, From: from, To: to})
}

func tombstoned(t *testing.T, s *Service, node, volume string) bool {
	t.Helper()
	r, err := s.ListTombstones(context.Background(), &metav1.ListTombstonesRequest{Node: node})
	if err != nil {
		t.Fatal(err)
	}
	return slices.ContainsFunc(r.GetTombstones(), func(tb *metav1.Tombstone) bool { return tb.GetVolume() == volume })
}

// A move recorded after DeleteVolume read the header, before it marked it, is in the chunks it
// lists, so the move's destination is on the tombstone (docs/8).
func TestDeleteVolumeSeesAMoveBeforeItMarks(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		store.key = volumeKey("v")
		hook := func() {
			if r, err := move(ctx, s, "a", "c"); err != nil || !r.GetApplied() {
				t.Errorf("move: %v %v", r, err)
			}
		}
		store.hook.Store(&hook)
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if !tombstoned(t, s, "c", "v") {
			t.Fatal("the move's destination is not on the tombstone")
		}
	})
}

// Once DeleteVolume has marked the header, a move is refused: the volume is gone for it, so the
// chunks it lists stay current (docs/8).
func TestDeleteVolumeFencesMoves(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		store.prefix = chunkPrefix("v")
		hook := func() {
			if _, err := move(ctx, s, "a", "c"); code(err) != codes.NotFound {
				t.Errorf("move of a volume being deleted: %v", err)
			}
		}
		store.hook.Store(&hook)
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if !tombstoned(t, s, "a", "v") || !tombstoned(t, s, "b", "v") || tombstoned(t, s, "c", "v") {
			t.Fatal("the tombstone should list a and b, and not c, whose move was refused")
		}
	})
}

// A volume deleted while a chunk is moving has the move's destination on its tombstone.
func TestDeleteVolumeDuringAMove(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		if r, err := move(ctx, s, "a", "c"); err != nil || !r.GetApplied() {
			t.Fatalf("move: %v %v", r, err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if !tombstoned(t, s, "c", "v") {
			t.Fatal("the move's destination is not on the tombstone")
		}
	})
}

// An attach between a move's read and its write makes the move read again: two writes, the
// first refused on the header (docs/8).
func TestMoveReplicaAfterAnAttachReadsAgain(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"}); err != nil {
			t.Fatal(err)
		}
		store.key = chunkKey("v", 0)
		hook := func() {
			if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b"}); err != nil {
				t.Error(err)
			}
		}
		store.txns.Store(0)
		store.hook.Store(&hook)
		if r, err := move(ctx, s, "a", "c"); err != nil || !r.GetApplied() {
			t.Fatalf("move: %v %v", r, err)
		}
		if got := store.txns.Load(); got != 2 {
			t.Fatalf("%d writes of the chunk, want 2: the first refused on the header", got)
		}
		v, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		if err != nil {
			t.Fatal(err)
		}
		if v.GetVolume().GetEpoch() != 2 || v.GetVolume().GetTarget() != "b" || v.GetVolume().GetChunks()[0].GetMovingTo() != "c" {
			t.Fatalf("volume %v", v.GetVolume())
		}
	})
}

// A volume deleted between a move's read and its write gets no move (docs/8).
func TestMoveReplicaAfterADeletionFails(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		store.key = chunkKey("v", 0)
		// As a DeleteVolume that has deleted the header, before it deletes the chunk records.
		hook := func() {
			if _, err := store.KV.Txn(ctx, nil, []kv.Op{{Key: volumeKey("v"), Delete: true}}); err != nil {
				t.Error(err)
			}
		}
		store.hook.Store(&hook)
		if _, err := move(ctx, s, "a", "c"); code(err) != codes.NotFound {
			t.Fatalf("move of a deleted volume: %v", err)
		}
		e, _, err := store.KV.Get(ctx, chunkKey("v", 0))
		if err != nil {
			t.Fatal(err)
		}
		var c domain.Chunk
		if err := json.Unmarshal(e.Value, &c); err != nil || c.Moving != nil {
			t.Fatalf("chunk 0 after a refused move: %+v %v", c, err)
		}
	})
}

// A deletion interrupted after marking the header leaves a volume that no operation sees but
// DeleteVolume, which finishes it (docs/8).
func TestDeleteVolumeFinishesAnInterruptedDeletion(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		// As a DeleteVolume that stopped after its first transaction.
		e, _, err := store.KV.Get(ctx, volumeKey("v"))
		if err != nil {
			t.Fatal(err)
		}
		var h domain.Header
		if err := json.Unmarshal(e.Value, &h); err != nil {
			t.Fatal(err)
		}
		h.Deleting = true
		b, _ := json.Marshal(h)
		if _, err := store.KV.Txn(ctx, nil, []kv.Op{{Key: volumeKey("v"), Value: b}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); code(err) != codes.NotFound {
			t.Fatalf("get of a volume being deleted: %v", err)
		}
		if l, err := s.ListVolumes(ctx, &metav1.ListVolumesRequest{}); err != nil || len(l.GetIds()) != 0 {
			t.Fatalf("list: %v %v", l, err)
		}
		if _, err := move(ctx, s, "a", "c"); code(err) != codes.NotFound {
			t.Fatalf("move of a volume being deleted: %v", err)
		}
		if _, err := create(s, "v", 2, 2, 1); code(err) != codes.FailedPrecondition {
			t.Fatalf("create over a volume being deleted: %v", err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if _, ok, _ := store.KV.Get(ctx, volumeKey("v")); ok || !tombstoned(t, s, "a", "v") || !tombstoned(t, s, "b", "v") {
			t.Fatal("the deletion was not finished: header left, or tombstone missing")
		}
	})
}

// A deletion during a creation stops the creation: it adds no chunk after the deletion's listing,
// and does not bring the volume back by clearing the deletion's mark (docs/8).
func TestDeleteVolumeDuringItsCreation(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		// createVolume reads its claimed header once, before it writes the chunks.
		store.key = volumeKey("v")
		hook := func() {
			if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
				t.Error(err)
			}
		}
		store.hook.Store(&hook)
		if _, err := create(s, "v", 2, 2, 1); err == nil {
			t.Fatal("a creation deleted midway succeeded")
		}
		if _, ok, _ := store.KV.Get(ctx, volumeKey("v")); ok {
			t.Fatal("the creation brought the deleted volume back")
		}
		if chunks, err := store.KV.List(ctx, chunkPrefix("v")); err != nil || len(chunks) != 0 {
			t.Fatalf("chunk records after the deletion: %d %v", len(chunks), err)
		}
	})
}

// A volume deleted between a move's read of its header and of the chunk: NotFound, as for any
// operation on a deleted volume, not a missing chunk (docs/8).
func TestMoveReplicaOfAVolumeDeletedMidCallIsNotFound(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		store.key = volumeKey("v")
		hook := func() {
			if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
				t.Error(err)
			}
		}
		store.hook.Store(&hook)
		if _, err := move(ctx, s, "a", "c"); code(err) != codes.NotFound {
			t.Fatalf("move of a volume deleted mid-call: %v", err)
		}
	})
}
