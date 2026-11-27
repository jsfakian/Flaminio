package service

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"math/big"
	"math/rand/v2"
	"net"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	clientv3 "go.etcd.io/etcd/client/v3"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/proto"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/kv/kvtest"
	"github.com/flaminio/flaminio/pkg/captoken"
	"github.com/flaminio/flaminio/pkg/pki"
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

// headerHistory observes applied header revisions below transaction retries. Mem has no
// internal retries, so its direct application result is definitive; etcd's native watch
// history also includes writes whose reply was lost inside Etcd.TxnRev.
type headerHistory struct {
	kv.KV
	key       string
	client    *clientv3.Client
	mu        sync.Mutex
	revisions map[int64]bool
}

func headerHistoryBackends(t *testing.T, key string, f func(*testing.T, *headerHistory)) {
	t.Run("mem", func(t *testing.T) {
		f(t, &headerHistory{KV: kv.NewMem(nil), key: key, revisions: map[int64]bool{}})
	})
	t.Run("etcd", func(t *testing.T) {
		cl := kvtest.Start(t)
		defer cl.KV.Close()
		f(t, &headerHistory{KV: cl.KV, key: key, client: cl.Client})
	})
}

func (h *headerHistory) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := h.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (h *headerHistory) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	rev, ok, err := h.KV.TxnRev(ctx, cmps, ops)
	if h.client == nil && ok && err == nil {
		for _, op := range ops {
			if op.Key == h.key && !op.Delete {
				h.mu.Lock()
				h.revisions[rev] = true
				h.mu.Unlock()
			}
		}
	}
	return rev, ok, err
}

func (h *headerHistory) revision(t *testing.T) int64 {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	e, ok, err := h.KV.Get(ctx, h.key)
	if err != nil || !ok {
		t.Fatalf("header history revision: found=%v err=%v", ok, err)
	}
	return e.Rev
}

func (h *headerHistory) appliedAfter(t *testing.T, after int64) map[int64]bool {
	t.Helper()
	revisions := map[int64]bool{}
	if h.client == nil {
		h.mu.Lock()
		defer h.mu.Unlock()
		for rev := range h.revisions {
			if rev > after {
				revisions[rev] = true
			}
		}
		return revisions
	}
	// Writers have finished. Replay from the initial revision through the final header
	// revision; replay avoids relying on watch registration timing or API responses.
	final := h.revision(t)
	if final == after {
		return revisions
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	watch := h.client.Watch(ctx, h.key, clientv3.WithRev(after+1))
	for {
		select {
		case response, ok := <-watch:
			if !ok || response.Err() != nil {
				t.Fatalf("header history watch: open=%v err=%v", ok, response.Err())
			}
			for _, event := range response.Events {
				if event.Type != clientv3.EventTypePut {
					t.Fatalf("unexpected header history event: %v", event)
				}
				revisions[event.Kv.ModRevision] = true
				if event.Kv.ModRevision == final {
					return revisions
				}
			}
		case <-ctx.Done():
			t.Fatalf("header history stopped before revision %d: %v", final, ctx.Err())
		}
	}
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
		// A volume starts at its first epoch, the revision of its claim (docs/issues/30).
		e := v.Epoch
		if len(v.Chunks) != 3 || e == 0 {
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
		if err != nil || a.Volume.Epoch != e+1 || a.Volume.Target != "a" {
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
		if r := change(e+1, 0, c0.Replicas[0]); !r.Applied || r.Chunks[0].IsrVer != 1 || len(r.Chunks[0].Isr) != 1 {
			t.Fatalf("shrink: %+v", r)
		}
		// A retry of the same change is refused, and shows the state it produced.
		if r := change(e+1, 0, c0.Replicas[0]); r.Applied || r.Chunks[0].IsrVer != 1 {
			t.Fatalf("retry: %+v", r)
		}
		if r := change(e+1, 1, c0.Replicas...); !r.Applied || len(r.Chunks[0].Isr) != 2 {
			t.Fatalf("expand: %+v", r)
		}
		// A new attach deposes the old target: its changes are refused with the new epoch.
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b"}); err != nil {
			t.Fatal(err)
		}
		if r := change(e+1, 2, c0.Replicas[0]); r.Applied || r.Epoch != e+2 {
			t.Fatalf("old epoch: %+v", r)
		}
		// unless_target: a repeated publish to the target changes nothing and issues no token;
		// to another node it attaches as usual.
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", UnlessTarget: true}); err != nil || r.Volume.Epoch != e+2 || r.Token != nil {
			t.Fatalf("unless_target, already the target: %v %v", r, err)
		}
		// only_if_target: the target restarts under a new epoch; another node is refused.
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a", OnlyIfTarget: true}); code(err) != codes.FailedPrecondition {
			t.Fatalf("only_if_target, not the target: %v", err)
		}
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", OnlyIfTarget: true}); err != nil || r.Volume.Epoch != e+3 || r.Volume.Target != "b" {
			t.Fatalf("only_if_target: %v %v", r, err)
		}
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b", OnlyIfTarget: true, UnlessTarget: true}); code(err) != codes.InvalidArgument {
			t.Fatalf("both modes: %v", err)
		}
		if r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a", UnlessTarget: true}); err != nil || r.Volume.Epoch != e+4 || r.Volume.Target != "a" {
			t.Fatalf("unless_target, another node: %v %v", r, err)
		}
		if _, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: e + 4, Changes: []*metav1.IsrChange{{Chunk: 9}}}); code(err) != codes.InvalidArgument {
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
		// Created again, it starts above every epoch the earlier volume reached.
		if g, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); err != nil || len(g.Volume.Chunks) != 1 || g.Volume.Epoch <= e+4 {
			t.Fatalf("recreated: %v %v", g, err)
		}
	})
}

func TestChangeIsrBatchBoundary(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b")
		limit := domain.MaxChangesPerRequest
		for _, tc := range []struct {
			name    string
			count   int
			alter   func(*metav1.ChangeIsrRequest)
			code    codes.Code
			applied bool
		}{
			{name: "boundary", count: limit, code: codes.OK, applied: true},
			{name: "oversized", count: limit + 1, code: codes.InvalidArgument},
			{name: "stale-epoch", count: limit, code: codes.OK, alter: func(r *metav1.ChangeIsrRequest) {
				r.Epoch--
			}},
			{name: "wrong-version", count: limit, code: codes.OK, alter: func(r *metav1.ChangeIsrRequest) {
				r.Changes[len(r.Changes)-1].ExpectedVer++
			}},
			{name: "duplicate-chunk", count: limit, code: codes.OK, alter: func(r *metav1.ChangeIsrRequest) {
				r.Changes[len(r.Changes)-1] = proto.Clone(r.Changes[0]).(*metav1.IsrChange)
			}},
			{name: "below-minimum", count: limit, code: codes.OK, alter: func(r *metav1.ChangeIsrRequest) {
				r.Changes[len(r.Changes)-1].NewIsr = nil
			}},
		} {
			t.Run(tc.name, func(t *testing.T) {
				id := "batch-" + tc.name
				if _, err := create(s, id, uint64(limit+1), 2, 1); err != nil {
					t.Fatal(err)
				}
				att, err := s.Attach(ctx, &metav1.AttachRequest{Volume: id, Node: "a"})
				if err != nil {
					t.Fatal(err)
				}
				// Give chunk zero a distinct version, so comparing the reply to request
				// order also detects a service that sorts the requested chunks.
				first := att.Volume.Chunks[0]
				seed, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{
					Volume: id, Epoch: att.Volume.Epoch,
					Changes: []*metav1.IsrChange{{Chunk: 0, ExpectedVer: first.IsrVer, NewIsr: first.Replicas[:1]}},
				})
				if err != nil || !seed.GetApplied() {
					t.Fatalf("seed chunk version: %v %v", seed, err)
				}
				before, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: id})
				if err != nil {
					t.Fatal(err)
				}
				entries, err := s.kv.List(ctx, chunkPrefix(id))
				if err != nil {
					t.Fatal(err)
				}
				req := &metav1.ChangeIsrRequest{Volume: id, Epoch: before.Volume.Epoch}
				for i := tc.count - 1; i >= 0; i-- {
					c := before.Volume.Chunks[i]
					req.Changes = append(req.Changes, &metav1.IsrChange{
						Chunk: uint32(i), ExpectedVer: c.IsrVer, NewIsr: c.Replicas[:1],
					})
				}
				if tc.alter != nil {
					tc.alter(req)
				}
				reply, err := s.ChangeIsr(ctx, req)
				if code(err) != tc.code || reply.GetApplied() != tc.applied {
					t.Fatalf("ChangeIsr: %v %v, want code %v applied %v", reply, err, tc.code, tc.applied)
				}
				after, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: id})
				if err != nil {
					t.Fatal(err)
				}
				want := proto.Clone(before.Volume).(*metav1.Volume)
				if tc.applied {
					for _, change := range req.Changes {
						want.Chunks[change.Chunk].Isr = slices.Clone(change.NewIsr)
						want.Chunks[change.Chunk].IsrVer++
					}
				}
				if !proto.Equal(after.Volume, want) {
					t.Fatalf("batch changed the wrong chunks: got %v, want %v", after.Volume, want)
				}
				if !tc.applied {
					unchanged, err := s.kv.List(ctx, chunkPrefix(id))
					if err != nil {
						t.Fatal(err)
					}
					if !slices.EqualFunc(entries, unchanged, func(a, b kv.Entry) bool {
						return a.Key == b.Key && a.Rev == b.Rev && bytes.Equal(a.Value, b.Value)
					}) {
						t.Fatal("refused batch wrote chunk metadata")
					}
				}
				if tc.code == codes.OK {
					if reply.Epoch != before.Volume.Epoch || len(reply.Chunks) != len(req.Changes) {
						t.Fatalf("reply epoch/count: %v", reply)
					}
					for i, change := range req.Changes {
						if !proto.Equal(reply.Chunks[i], want.Chunks[change.Chunk]) {
							t.Fatalf("reply item %d does not describe requested chunk %d: %v", i, change.Chunk, reply.Chunks[i])
						}
					}
				}
			})
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
	headerHistoryBackends(t, volumeKey("v"), func(t *testing.T, backend *headerHistory) {
		store := &scheduledKV{KV: backend}
		s := New(store, Options{})
		ctx := context.Background()
		addNodes(t, s, "a", "b", "c", "d")
		v, err := create(s, "v", 4, 3, 2)
		if err != nil {
			t.Fatal(err)
		}
		var applied atomic.Int64
		initialRevision := backend.revision(t)
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
							continue // A committed Attach may have been superseded before its response.
						}
						if err != nil {
							t.Error(err)
							return
						}
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
		commits := backend.appliedAfter(t, initialRevision)
		if g.Volume.Epoch != v.Epoch+uint64(len(commits)) {
			t.Fatalf("epoch %d after %d committed attaches from %d", g.Volume.Epoch, len(commits), v.Epoch)
		}
		for i, c := range g.Volume.Chunks {
			if c.IsrVer != uint64(perChunk[i].Load()) {
				t.Fatalf("chunk %d: version %d after %d applied changes", i, c.IsrVer, perChunk[i].Load())
			}
			if len(c.Isr) < 2 {
				t.Fatalf("chunk %d: ISR %v below the minimum", i, c.Isr)
			}
		}
		t.Logf("%d backend-applied attaches, %d ISR changes applied", len(commits), applied.Load())
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
		a, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil {
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
			r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: a.Volume.Epoch, Changes: []*metav1.IsrChange{ch}})
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

// interleaveKV runs a hook once: after the first Get of `key`, or List of `prefix`, it sees, before
// returning that read, or after the first write of `txnKey` that applied. The hook can make
// another write land between a read and the write that follows it, or between two writes. It
// counts the writes of `key` made outside the hook.
// scheduledKV places deterministic interleavings at real storage operation boundaries.
// Unset operations always reach the backend; hooks call KV directly to avoid recursion.
type scheduledKV struct {
	kv.KV
	get     func(context.Context, string) (kv.Entry, bool, error)
	list    func(context.Context, string) ([]kv.Entry, error)
	readSet func(context.Context, []string) ([]kv.ReadResult, error)
	txn     func(context.Context, []kv.Cmp, []kv.Op) (bool, error)
	txnRev  func(context.Context, []kv.Cmp, []kv.Op) (int64, bool, error)
}

func (k *scheduledKV) Get(ctx context.Context, key string) (kv.Entry, bool, error) {
	if k.get != nil {
		return k.get(ctx, key)
	}
	return k.KV.Get(ctx, key)
}

func (k *scheduledKV) List(ctx context.Context, prefix string) ([]kv.Entry, error) {
	if k.list != nil {
		return k.list(ctx, prefix)
	}
	return k.KV.List(ctx, prefix)
}

func (k *scheduledKV) ReadSet(ctx context.Context, keys []string) ([]kv.ReadResult, error) {
	if k.readSet != nil {
		return k.readSet(ctx, keys)
	}
	return k.KV.ReadSet(ctx, keys)
}

func (k *scheduledKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	if k.txn != nil {
		return k.txn(ctx, cmps, ops)
	}
	return k.KV.Txn(ctx, cmps, ops)
}

func (k *scheduledKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	if k.txnRev != nil {
		return k.txnRev(ctx, cmps, ops)
	}
	return k.KV.TxnRev(ctx, cmps, ops)
}

type interleaveKV struct {
	kv.KV
	key    string
	prefix string
	txnKey string
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

func (k *interleaveKV) GetBounded(ctx context.Context, key string, maxBytes int) (kv.Entry, bool, error) {
	e, ok, err := k.KV.GetBounded(ctx, key, maxBytes)
	if key == k.key {
		k.runHook()
	}
	return e, ok, err
}

func (k *interleaveKV) ReadSet(ctx context.Context, keys []string) ([]kv.ReadResult, error) {
	rows, err := k.KV.ReadSet(ctx, keys)
	if slices.Contains(keys, k.key) {
		k.runHook()
	}
	return rows, err
}

func (k *interleaveKV) List(ctx context.Context, prefix string) ([]kv.Entry, error) {
	es, err := k.KV.List(ctx, prefix)
	if k.prefix != "" && prefix == k.prefix {
		k.runHook()
	}
	return es, err
}

func (k *interleaveKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := k.TxnRev(ctx, cmps, ops)
	return ok, err
}

// TxnRev counts the writes of `key`, and runs the hook after the first write of `txnKey` that
// applied.
func (k *interleaveKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	wrote := false
	for _, op := range ops {
		if op.Key == k.key && !k.inner.Load() {
			k.txns.Add(1)
		}
		wrote = wrote || (k.txnKey != "" && op.Key == k.txnKey && !op.Delete)
	}
	rev, ok, err := k.KV.TxnRev(ctx, cmps, ops)
	if ok && wrote {
		k.runHook()
	}
	return rev, ok, err
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
// both their labels (docs/issues/7): with no record yet, and with one.
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
// label (docs/issues/7).
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
// node plugin cannot put back an old one; a node not registered yet is refused (docs/issues/7).
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
// lists, so the move's destination is on the tombstone (docs/issues/8).
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
// chunks it lists stay current (docs/issues/8).
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
// first refused on the header (docs/issues/8).
func TestMoveReplicaAfterAnAttachReadsAgain(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := moveFixture(t, store)
		a, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil {
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
		if v.GetVolume().GetEpoch() != a.Volume.Epoch+1 || v.GetVolume().GetTarget() != "b" || v.GetVolume().GetChunks()[0].GetMovingTo() != "c" {
			t.Fatalf("volume %v", v.GetVolume())
		}
	})
}

// A volume deleted between a move's read and its write gets no move (docs/issues/8).
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
// DeleteVolume, which finishes it (docs/issues/8).
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
		// Create refuses a Deleting header (docs/issues/76); it does not run the public deleter.
		if _, err := create(s, "v", 2, 2, 1); code(err) != codes.Aborted {
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

// A deletion during a creation stops the creation: it adds no chunk after the deletion fences
// the claim, and does not bring the volume back (docs/issues/8). For a never-published Creating
// claim, DeleteVolume uses proof-based header removal without a new tombstone (docs/issues/76 S8);
// orphan public chunk keys are paged away under header absence.
func TestDeleteVolumeDuringItsCreation(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		store.txnKey = chunkKey("v", 0)
		hook := func() {
			if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
				t.Error(err)
			}
		}
		store.hook.Store(&hook)
		if _, err := create(s, "v", 130, 2, 1); code(err) != codes.Aborted {
			t.Fatalf("a creation deleted midway: %v, want Aborted", err)
		}
		if tombstoned(t, s, "a", "v") || tombstoned(t, s, "b", "v") {
			t.Fatal("unpublished create rollback must not create a tombstone")
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
// operation on a deleted volume, not a missing chunk (docs/issues/8).
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

// A volume whose header does not parse can still be deleted: no move or ISR change can commit
// against it, so it needs no mark (docs/issues/8).
func TestDeleteVolumeWithADamagedHeader(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		if _, err := create(s, "v", 2, 2, 1); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("v"), Value: []byte("not json")}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
			t.Fatal(err)
		}
		if _, ok, _ := store.Get(ctx, volumeKey("v")); ok {
			t.Fatal("the damaged header is still there")
		}
		if !tombstoned(t, s, "a", "v") || !tombstoned(t, s, "b", "v") {
			t.Fatal("the tombstone does not name the replicas")
		}
	})
}

// A deletion whose chunk records were left behind (deleteChunks failed after its commit) has them
// cleared by its retry, which then answers NotFound (docs/issues/8).
func TestDeleteVolumeRetryClearsLeftoverChunks(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: chunkKey("v", 0), Value: []byte("{}")}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "v"}); code(err) != codes.NotFound {
			t.Fatalf("retry of a finished deletion: %v", err)
		}
		if l, err := store.List(ctx, chunkPrefix("v")); err != nil || len(l) != 0 {
			t.Fatalf("chunk records left: %d %v", len(l), err)
		}
	})
}

// A volume deleted between GetVolume's read of its header and of its chunks: NotFound, as for any
// deleted volume, not a chunk-count mismatch (docs/issues/8).
func TestGetVolumeOfAVolumeDeletedMidCallIsNotFound(t *testing.T) {
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
		if _, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"}); code(err) != codes.NotFound {
			t.Fatalf("get of a volume deleted mid-call: %v", err)
		}
	})
}

// captureLog sends slog's default logger to a buffer for the test, and returns what it logged
// as JSON records. The package's tests do not run in parallel, so no other test logs meanwhile.
func captureLog(t *testing.T) func() []map[string]any {
	t.Helper()
	var mu sync.Mutex
	var buf bytes.Buffer
	prev := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(lockedWriter{&mu, &buf}, nil)))
	t.Cleanup(func() { slog.SetDefault(prev) })
	return func() []map[string]any {
		mu.Lock()
		defer mu.Unlock()
		var out []map[string]any
		for _, line := range bytes.Split(bytes.TrimSpace(buf.Bytes()), []byte("\n")) {
			if len(line) == 0 {
				continue
			}
			var rec map[string]any
			if err := json.Unmarshal(line, &rec); err != nil {
				t.Fatalf("log line %q: %v", line, err)
			}
			out = append(out, rec)
		}
		return out
	}
}

type lockedWriter struct {
	mu *sync.Mutex
	w  io.Writer
}

func (l lockedWriter) Write(b []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.w.Write(b)
}

// confirmations returns the log's "deletion confirmed" records.
func confirmations(records []map[string]any) []map[string]any {
	var out []map[string]any
	for _, r := range records {
		if r["msg"] == "deletion confirmed" {
			out = append(out, r)
		}
	}
	return out
}

// tombstone3 is a volume v, one chunk on nodes a, b and c (RF 3), deleted: its tombstone names
// all three.
func tombstone3(t *testing.T, s *Service) {
	t.Helper()
	addNodes(t, s, "a", "b", "c")
	if _, err := create(s, "v", 1, 3, 1); err != nil {
		t.Fatal(err)
	}
	if _, err := s.DeleteVolume(context.Background(), &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
		t.Fatal(err)
	}
}

// Each confirmation that newly confirms a node logs one line, for audit (docs/issues/14, S4):
// the caller's identity, its address and certificate serial, the volume, the node, the nodes
// still pending and the write's revision. A repeat, or a missing tombstone, logs nothing.
func TestConfirmDeletionLogsOnce(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		tombstone3(t, s)
		logged := captureLog(t)
		cert := &x509.Certificate{SerialNumber: big.NewInt(0xbeef)}
		ctx := peer.NewContext(context.Background(), &peer.Peer{
			Addr:     &net.TCPAddr{IP: net.IPv4(10, 77, 0, 4), Port: 40000},
			AuthInfo: credentials.TLSInfo{State: tls.ConnectionState{PeerCertificates: []*x509.Certificate{cert}}},
		})
		ctx = context.WithValue(ctx, identityKey{}, pki.Identity{TrustDomain: td, Node: "c", Component: "agent"})
		for range 2 {
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: "c"}); err != nil {
				t.Fatal(err)
			}
		}
		got := confirmations(logged())
		if len(got) != 1 {
			t.Fatalf("%d lines for a confirmation and its repeat, want 1: %v", len(got), got)
		}
		l := got[0]
		if l["caller"] != pki.NodeID(td, "c", "agent") || l["peer"] != "10.77.0.4:40000" || l["serial"] != "beef" ||
			l["volume"] != "v" || l["node"] != "c" || fmt.Sprint(l["pending"]) != "[a b]" {
			t.Fatalf("log line %v", l)
		}
		if rev, ok := l["revision"].(float64); !ok || rev <= 0 {
			t.Fatalf("revision %v", l["revision"])
		}

		// Without authentication, the caller is "unauthenticated"; the last confirmation, which
		// removes the tombstone, logs too, with nothing pending.
		for _, n := range []string{"a", "b"} {
			if _, err := s.ConfirmDeletion(context.Background(), &metav1.ConfirmDeletionRequest{Volume: "v", Node: n}); err != nil {
				t.Fatal(err)
			}
		}
		got = confirmations(logged())
		if len(got) != 3 || got[1]["caller"] != "unauthenticated" || got[1]["peer"] != "" || got[1]["serial"] != "" ||
			got[2]["pending"] != nil || got[2]["revision"].(float64) <= got[1]["revision"].(float64) {
			t.Fatalf("log lines %v", got)
		}
		// The tombstone is gone: a confirmation succeeds, and logs nothing.
		if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: "c"}); err != nil {
			t.Fatal(err)
		}
		if got := confirmations(logged()); len(got) != 3 {
			t.Fatalf("a confirmation of a missing tombstone logged: %v", got[3:])
		}
	})
}

// Two confirmations interleaved, the second between the first's read and its write: both count,
// the first is written again after its read, and each logs once.
func TestConfirmDeletionInterleavedLogsEachOnce(t *testing.T) {
	interleaveStores(t, func(t *testing.T, store *interleaveKV) {
		ctx := context.Background()
		s := New(store, Options{})
		tombstone3(t, s)
		logged := captureLog(t)
		store.key = tombstoneKey("v")
		hook := func() {
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: "b"}); err != nil {
				t.Error(err)
			}
		}
		store.txns.Store(0)
		store.hook.Store(&hook)
		if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "v", Node: "a"}); err != nil {
			t.Fatal(err)
		}
		if got := store.txns.Load(); got != 2 {
			t.Fatalf("%d writes of the tombstone by the first confirmation, want 2: the first refused", got)
		}
		if tombstoned(t, s, "a", "v") || tombstoned(t, s, "b", "v") || !tombstoned(t, s, "c", "v") {
			t.Fatal("both confirmations should count, and c stay pending")
		}
		got := confirmations(logged())
		if len(got) != 2 || got[0]["node"] != "b" || fmt.Sprint(got[0]["pending"]) != "[a c]" ||
			got[1]["node"] != "a" || fmt.Sprint(got[1]["pending"]) != "[c]" {
			t.Fatalf("log lines %v", got)
		}
	})
}

// header reads a volume's stored header, first epoch included (no RPC returns it).
func header(t *testing.T, store kv.KV, id string) domain.Header {
	t.Helper()
	e, ok, err := store.Get(context.Background(), volumeKey(id))
	if err != nil || !ok {
		t.Fatalf("header of %s: %v %v", id, ok, err)
	}
	var h domain.Header
	if err := json.Unmarshal(e.Value, &h); err != nil {
		t.Fatal(err)
	}
	return h
}

// deleteAndConfirm deletes a volume, and confirms its tombstone for every node, so its id can be
// created again.
func deleteAndConfirm(t *testing.T, s *Service, id string, nodes ...string) {
	t.Helper()
	ctx := context.Background()
	if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: id}); err != nil {
		t.Fatal(err)
	}
	for _, n := range nodes {
		if tombstoned(t, s, n, id) {
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: id, Node: n}); err != nil {
				t.Fatal(err)
			}
		}
	}
}

// attachTimes attaches a volume n times, alternating its target, and returns the last epoch.
func attachTimes(t *testing.T, s *Service, id string, n int) uint64 {
	t.Helper()
	var epoch uint64
	for i := range n {
		r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: id, Node: "ab"[i%2 : i%2+1]})
		if err != nil {
			t.Fatal(err)
		}
		epoch = r.Volume.Epoch
	}
	return epoch
}

// A volume starts at its first epoch, the revision of its claim; the first attach takes the next
// one, and its token carries the first epoch (docs/issues/30, S1 and S4).
func TestCreateStartsAtTheFirstEpoch(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		pub, key, _ := ed25519.GenerateKey(nil)
		s := New(store, Options{TokenKey: key})
		addNodes(t, s, "a", "b")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		h := header(t, store, "v")
		if h.FirstEpoch == 0 || h.Epoch != h.FirstEpoch || v.Epoch != h.FirstEpoch {
			t.Fatalf("created at epoch %d, header %+v", v.Epoch, h)
		}
		a, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil || a.Volume.Epoch != h.FirstEpoch+1 {
			t.Fatalf("first attach: %v %v, want epoch %d", a, err, h.FirstEpoch+1)
		}
		if h2 := header(t, store, "v"); h2.FirstEpoch != h.FirstEpoch {
			t.Fatalf("the attach changed the first epoch: %+v", h2)
		}
		c, err := captoken.Verify(pub, a.Token)
		if err != nil || c.FirstEpoch != h.FirstEpoch || c.Epoch != a.Volume.Epoch {
			t.Fatalf("token %+v %v, want first epoch %d", c, err, h.FirstEpoch)
		}
	})
}

// A volume deleted after 7 attaches and created again starts above every epoch it reached
// (docs/issues/30, S1).
func TestCreateAgainStartsAboveTheDeletedVolume(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		addNodes(t, s, "a", "b")
		if _, err := create(s, "v", 2, 2, 1); err != nil {
			t.Fatal(err)
		}
		old := attachTimes(t, s, "v", 7)
		deleteAndConfirm(t, s, "v", "a", "b")
		v, err := create(s, "v", 3, 2, 1)
		if err != nil || v.Epoch <= old {
			t.Fatalf("created again at epoch %d (%v), want above %d", v.GetEpoch(), err, old)
		}
	})
}

// A volume whose damaged header was deleted: the next one of its name starts above every epoch
// it reached, which needs no read of it (docs/issues/30, S3).
func TestCreateAfterADamagedHeaderStartsAboveIt(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		if _, err := create(s, "v", 2, 2, 1); err != nil {
			t.Fatal(err)
		}
		old := attachTimes(t, s, "v", 7)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("v"), Value: []byte("not json")}}); err != nil {
			t.Fatal(err)
		}
		deleteAndConfirm(t, s, "v", "a", "b")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil || v.Epoch <= old {
			t.Fatalf("created again at epoch %d (%v), want above %d", v.GetEpoch(), err, old)
		}
	})
}

// A volume created before the first epoch was recorded has none: it keeps its epoch and gets
// version 1 tokens. Deleted and created again, the next one starts above it (docs/issues/30, S3).
func TestALegacyVolume(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		pub, key, _ := ed25519.GenerateKey(nil)
		s := New(store, Options{TokenKey: key})
		addNodes(t, s, "a", "b")
		if _, err := create(s, "v", 2, 2, 1); err != nil {
			t.Fatal(err)
		}
		// As written before the change: no first_epoch, at epoch 7.
		h := header(t, store, "v")
		h.FirstEpoch, h.Epoch = 0, 7
		b, _ := json.Marshal(h)
		if strings.Contains(string(b), "first_epoch") {
			t.Fatalf("a header without a first epoch writes one: %s", b)
		}
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("v"), Value: b}}); err != nil {
			t.Fatal(err)
		}
		a, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil || a.Volume.Epoch != 8 {
			t.Fatalf("attach: %v %v, want epoch 8", a, err)
		}
		if h := header(t, store, "v"); h.FirstEpoch != 0 {
			t.Fatalf("the attach gave a first epoch: %+v", h)
		}
		c, err := captoken.Verify(pub, a.Token)
		if err != nil || c.FirstEpoch != 0 || c.Epoch != 8 || a.Token[4] != 1 {
			t.Fatalf("token %+v %v (version %d), want version 1", c, err, a.Token[4])
		}
		deleteAndConfirm(t, s, "v", "a", "b")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil || v.Epoch <= 8 {
			t.Fatalf("created again at epoch %d (%v), want above 8", v.GetEpoch(), err)
		}
	})
}

// An interrupted creation (its claim only) deleted, then created again: the new volume starts
// above the claim (docs/issues/30, S1).
func TestCreateAfterAnInterruptedCreationStartsAboveItsClaim(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		b, _ := json.Marshal(domain.Header{ID: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1, Creating: true})
		claim, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("v"), Rev: 0}}, []kv.Op{{Key: volumeKey("v"), Value: b}})
		if err != nil || !ok {
			t.Fatalf("claim: %v %v", ok, err)
		}
		deleteAndConfirm(t, s, "v", "a", "b")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil || v.Epoch <= uint64(claim) {
			t.Fatalf("created at epoch %d (%v), want above the interrupted claim %d", v.GetEpoch(), err, claim)
		}
	})
}

func coherentFixture(t *testing.T, backend kv.KV, opts Options) (*Service, *scheduledKV, *metav1.Volume) {
	t.Helper()
	entries, err := backend.List(context.Background(), prefix)
	if err != nil {
		t.Fatal(err)
	}
	var ops []kv.Op
	for _, e := range entries {
		ops = append(ops, kv.Op{Key: e.Key, Delete: true})
	}
	if len(ops) > 0 {
		writeTestOps(t, backend, ops...)
	}
	store := &scheduledKV{KV: backend}
	s := New(store, opts)
	addNodes(t, s, "a", "b")
	v, err := create(s, "v", 2, 2, 1)
	if err != nil {
		t.Fatal(err)
	}
	addNodes(t, s, "c")
	return s, store, v
}

func writeTestOps(t *testing.T, backend kv.KV, ops ...kv.Op) {
	t.Helper()
	if ok, err := backend.Txn(context.Background(), nil, ops); err != nil || !ok {
		t.Fatalf("fixture transaction: %v %v", ok, err)
	}
}

func writeTestHeader(t *testing.T, backend kv.KV, h domain.Header) {
	t.Helper()
	b, err := json.Marshal(h)
	if err != nil {
		t.Fatal(err)
	}
	writeTestOps(t, backend, kv.Op{Key: volumeKey(h.ID), Value: b})
}

func replaceTestVolume(t *testing.T, s *Service) *metav1.Volume {
	t.Helper()
	deleteAndConfirm(t, s, "v", "a", "b", "c")
	r, err := s.CreateVolume(context.Background(), &metav1.CreateVolumeRequest{
		Id: "v", SizeBytes: 4 * mib, ChunkBytes: 2 * mib, Replicas: 3, MinIsr: 2,
	})
	if err != nil {
		t.Fatal(err)
	}
	return r.Volume
}

// The audit F02 schedule keeps the chunk count equal while replacing every authority field.
func TestGetVolumeSameCountReplacementIsCoherent(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		s, store, old := coherentFixture(t, backend, Options{})
		oldFirst := header(t, backend, "v").FirstEpoch
		var replacement *metav1.Volume
		store.get = func(ctx context.Context, key string) (kv.Entry, bool, error) {
			e, ok, err := backend.Get(ctx, key)
			if key == volumeKey("v") {
				store.get = nil
				replacement = replaceTestVolume(t, s)
			}
			return e, ok, err
		}
		got, err := s.GetVolume(context.Background(), &metav1.GetVolumeRequest{Id: "v"})
		if err != nil || replacement == nil || !proto.Equal(got.GetVolume(), replacement) {
			t.Fatalf("mixed replacement: old=%v replacement=%v got=%v err=%v", old, replacement, got, err)
		}
		v, _, err := s.readVolumeView(context.Background(), "v", 0)
		if err != nil || v.FirstEpoch <= oldFirst || v.FirstEpoch != replacement.Epoch || v.ChunkCount() != len(old.Chunks) {
			t.Fatalf("incarnation/geometry: %+v %v, old first epoch %d", v, err, oldFirst)
		}
	})
}

func TestGetVolumeClassifiesChunksOnlyAfterHeaderValidation(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, damage := range []string{"json", "missing", "noncanonical"} {
			for _, replacing := range []bool{false, true} {
				t.Run(fmt.Sprintf("%s/replacing=%t", damage, replacing), func(t *testing.T) {
					s, store, _ := coherentFixture(t, backend, Options{})
					e, _, err := backend.Get(context.Background(), chunkKey("v", 1))
					if err != nil {
						t.Fatal(err)
					}
					switch damage {
					case "json":
						writeTestOps(t, backend, kv.Op{Key: e.Key, Value: []byte("{")})
					case "missing":
						writeTestOps(t, backend, kv.Op{Key: e.Key, Delete: true})
					case "noncanonical":
						writeTestOps(t, backend, kv.Op{Key: e.Key, Delete: true}, kv.Op{Key: chunkPrefix("v") + "1", Value: e.Value})
					}
					var replacement *metav1.Volume
					if replacing {
						store.list = func(ctx context.Context, p string) ([]kv.Entry, error) {
							entries, err := backend.List(ctx, p)
							if p == chunkPrefix("v") {
								store.list = nil
								// Restore the damaged old prefix so deletion can inspect it, after
								// the reader already acquired its invalid candidate.
								writeTestOps(t, backend, kv.Op{Key: e.Key, Value: e.Value}, kv.Op{Key: chunkPrefix("v") + "1", Delete: true})
								replacement = replaceTestVolume(t, s)
							}
							return entries, err
						}
					}
					got, err := s.GetVolume(context.Background(), &metav1.GetVolumeRequest{Id: "v"})
					if replacing {
						if err != nil || !proto.Equal(got.GetVolume(), replacement) {
							t.Fatalf("changing header classified as corruption: %v %v", got, err)
						}
					} else if got != nil || code(err) != codes.Internal {
						t.Fatalf("stable corruption returned partial/success: %v %v", got, err)
					}
				})
			}
		}
	})
}

func TestGetVolumeHeaderChurnIsBounded(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, ending := range []string{"budget", "cancel", "deadline"} {
			t.Run(ending, func(t *testing.T) {
				s, store, _ := coherentFixture(t, backend, Options{})
				ctx, cancel := context.WithCancel(context.Background())
				defer cancel()
				if ending == "deadline" {
					var stop context.CancelFunc
					ctx, stop = context.WithTimeout(context.Background(), 100*time.Millisecond)
					defer stop()
				}
				reads := 0
				store.list = func(ctx context.Context, p string) ([]kv.Entry, error) {
					es, err := backend.List(ctx, p)
					if p == chunkPrefix("v") {
						reads++
						// Equal header values with a new revision still invalidate the candidate.
						writeTestHeader(t, backend, header(t, backend, "v"))
						if ending == "cancel" {
							cancel()
						}
					}
					return es, err
				}
				got, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
				want := map[string]codes.Code{"budget": codes.Aborted, "cancel": codes.Canceled, "deadline": codes.DeadlineExceeded}[ending]
				if got != nil || code(err) != want || reads == 0 || reads > retries || ending == "budget" && reads != retries || ending == "cancel" && reads != 1 {
					t.Fatalf("churn: response=%v err=%v reads=%d", got, err, reads)
				}
			})
		}
	})
}

func requireCommittedError(t *testing.T, err error, want codes.Code, epoch uint64, rev int64) {
	t.Helper()
	context := fmt.Sprintf("attach committed volume=v epoch=%d revision=%d; response unavailable:", epoch, rev)
	if code(err) != want || !strings.Contains(fmt.Sprint(err), context) {
		t.Fatalf("want %s with exact known-commit context %q, got %v", want, context, err)
	}
}

func TestAttachResponsePinnedToCommitRevision(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, phase := range []string{"commit", "first-header", "prefix", "final-header"} {
			for _, action := range []string{"attach", "delete", "replace"} {
				t.Run(phase+"/"+action, func(t *testing.T) {
					pub, key, _ := ed25519.GenerateKey(nil)
					s, store, old := coherentFixture(t, backend, Options{TokenKey: key})
					oldFirst := header(t, backend, "v").FirstEpoch
					var attachRev int64
					triggered := false
					trigger := func() {
						if triggered {
							return
						}
						triggered = true
						store.get, store.list, store.txnRev = nil, nil, nil
						switch action {
						case "attach":
							if _, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "b"}); err != nil {
								t.Fatal(err)
							}
						case "delete":
							if _, err := s.DeleteVolume(context.Background(), &metav1.DeleteVolumeRequest{Id: "v"}); err != nil {
								t.Fatal(err)
							}
						case "replace":
							replaceTestVolume(t, s)
						}
					}
					store.txnRev = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
						rev, ok, err := backend.TxnRev(ctx, cmps, ops)
						if ok && err == nil {
							attachRev = rev
							if phase == "commit" {
								trigger()
							}
						}
						return rev, ok, err
					}
					reads := 0
					store.get = func(ctx context.Context, k string) (kv.Entry, bool, error) {
						e, ok, err := backend.Get(ctx, k)
						if k == volumeKey("v") {
							reads++
							if phase == "first-header" && reads == 2 || phase == "final-header" && reads == 3 {
								trigger()
							}
						}
						return e, ok, err
					}
					store.list = func(ctx context.Context, p string) ([]kv.Entry, error) {
						es, err := backend.List(ctx, p)
						if p == chunkPrefix("v") && phase == "prefix" {
							trigger()
						}
						return es, err
					}
					r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"})
					if !triggered || attachRev == 0 {
						t.Fatalf("schedule did not run: triggered=%t rev=%d", triggered, attachRev)
					}
					if phase != "final-header" {
						if r != nil {
							t.Fatalf("superseded attach returned token/view: %v", r)
						}
						requireCommittedError(t, err, codes.Aborted, old.Epoch+1, attachRev)
					} else {
						// Supersession after the final snapshot may leave a valid older response;
						// ordinary epoch fencing handles it. Its token and view must still agree.
						if err != nil {
							t.Fatal(err)
						}
						want := proto.Clone(old).(*metav1.Volume)
						want.Epoch++
						want.Target = "a"
						claims, verifyErr := captoken.Verify(pub, r.Token)
						if !proto.Equal(r.Volume, want) || verifyErr != nil || claims.Epoch != want.Epoch || claims.FirstEpoch != oldFirst || claims.Node != want.Target || claims.Volume != "v" {
							t.Fatalf("unpinned token/view: %v claims=%+v err=%v", r, claims, verifyErr)
						}
					}
					if action == "attach" && header(t, backend, "v").Epoch != old.Epoch+2 {
						t.Fatal("committed epochs were rolled back")
					}
				})
			}
		}
	})
}

func TestAttachKnownCommitThenReadFailureKeepsEpoch(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, phase := range []string{"first-header", "final-header"} {
			for _, failure := range []string{"missing", "creating", "deleting", "backend", "list-backend", "corrupt-header", "corrupt-chunk", "cancel", "deadline"} {
				t.Run(phase+"/"+failure, func(t *testing.T) {
					s, store, old := coherentFixture(t, backend, Options{})
					ctx, cancel := context.WithCancel(context.Background())
					defer cancel()
					var attachRev int64
					var committed domain.Header
					store.txnRev = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
						rev, ok, err := backend.TxnRev(ctx, cmps, ops)
						if ok && err == nil {
							attachRev = rev
							committed = header(t, backend, "v")
						}
						return rev, ok, err
					}
					reads := 0
					store.get = func(ctx context.Context, k string) (kv.Entry, bool, error) {
						if k == volumeKey("v") {
							reads++
							at := 2
							if phase == "final-header" {
								at = 3
							}
							if reads == at {
								switch failure {
								case "missing":
									writeTestOps(t, backend, kv.Op{Key: k, Delete: true})
								case "creating", "deleting":
									h := committed
									h.Creating = failure == "creating"
									h.Deleting = failure == "deleting"
									writeTestHeader(t, backend, h)
								case "backend":
									return kv.Entry{}, false, status.Error(codes.Unavailable, "injected read failure")
								case "corrupt-header":
									writeTestOps(t, backend, kv.Op{Key: k, Value: []byte("{")})
								case "cancel":
									cancel()
									return kv.Entry{}, false, ctx.Err()
								case "deadline":
									return kv.Entry{}, false, context.DeadlineExceeded
								}
							}
						}
						return backend.Get(ctx, k)
					}
					if failure == "corrupt-chunk" {
						store.list = func(ctx context.Context, p string) ([]kv.Entry, error) {
							writeTestOps(t, backend, kv.Op{Key: chunkKey("v", 0), Value: []byte("{")})
							return backend.List(ctx, p)
						}
					}
					if failure == "list-backend" {
						store.list = func(context.Context, string) ([]kv.Entry, error) {
							return nil, status.Error(codes.Unavailable, "injected prefix failure")
						}
					}
					r, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
					want := codes.Aborted
					switch failure {
					case "backend", "list-backend", "corrupt-header":
						want = codes.Unavailable
					case "corrupt-chunk":
						want = codes.Internal
					case "cancel":
						want = codes.Canceled
					case "deadline":
						want = codes.DeadlineExceeded
					}
					if r != nil || attachRev == 0 || committed.Epoch != old.Epoch+1 {
						t.Fatalf("lost commit evidence: response=%v rev=%d header=%+v", r, attachRev, committed)
					}
					requireCommittedError(t, err, want, old.Epoch+1, attachRev)
					// Restore only deliberately removed/damaged publication, never its epoch.
					if failure == "missing" || failure == "creating" || failure == "deleting" || failure == "corrupt-header" {
						writeTestHeader(t, backend, committed)
					}
					if h := header(t, backend, "v"); h.Epoch != committed.Epoch || h.Target != "a" {
						t.Fatalf("postcommit error rolled back authority: %+v", h)
					}
				})
			}
		}
	})
}

func TestAttachLostCommitReplyDoesNotRollBack(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		s, store, old := coherentFixture(t, backend, Options{})
		var applied []int64
		store.txnRev = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
			rev, ok, err := backend.TxnRev(ctx, cmps, ops)
			if ok && err == nil {
				applied = append(applied, rev)
				store.txnRev = nil
				return 0, false, status.Error(codes.Unavailable, "commit reply lost")
			}
			return rev, ok, err
		}
		if r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"}); r != nil || code(err) != codes.Unavailable {
			t.Fatalf("ambiguous response: %v %v", r, err)
		}
		if len(applied) != 1 || header(t, backend, "v").Epoch != old.Epoch+1 {
			t.Fatalf("actual application evidence: %v header=%+v", applied, header(t, backend, "v"))
		}
		r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "b"})
		if err != nil || r.Volume.Epoch != old.Epoch+2 {
			t.Fatalf("fresh recovery attach: %v %v", r, err)
		}
	})
}

func TestChangeIsrRejectedViewIsCoherent(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, mutation := range []string{"replacement", "multi-chunk"} {
			for _, refusal := range []string{"wrong-target", "stale-epoch", "invalid-change"} {
				t.Run(mutation+"/"+refusal, func(t *testing.T) {
					s, store, _ := coherentFixture(t, backend, Options{})
					a, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"})
					if err != nil {
						t.Fatal(err)
					}
					before := a.Volume
					ctx := context.Background()
					if refusal == "wrong-target" {
						ctx = context.WithValue(ctx, identityKey{}, pki.Identity{Node: "c", Component: "io"})
					}
					req := &metav1.ChangeIsrRequest{Volume: "v", Epoch: before.Epoch, Changes: []*metav1.IsrChange{
						{Chunk: 1, ExpectedVer: 0, NewIsr: []string{"a"}}, {Chunk: 0, ExpectedVer: 0, NewIsr: []string{"a"}},
					}}
					if refusal == "stale-epoch" {
						req.Epoch--
					}
					if refusal == "invalid-change" {
						req.Changes[0].ExpectedVer++
					}
					triggered := false
					var postMutation []kv.Entry
					trigger := func() {
						if triggered {
							return
						}
						triggered = true
						store.get, store.readSet = nil, nil
						if mutation == "replacement" {
							replaceTestVolume(t, s)
						} else {
							var ops []kv.Op
							for i, c := range before.Chunks {
								changed := domain.Chunk{Replicas: c.Replicas, ISR: []string{"b"}, ISRVer: 7}
								b, _ := json.Marshal(changed)
								ops = append(ops, kv.Op{Key: chunkKey("v", uint32(i)), Value: b})
							}
							writeTestOps(t, backend, ops...)
						}
						postMutation, err = backend.List(context.Background(), prefix)
						if err != nil {
							t.Fatal(err)
						}
					}
					store.readSet = func(ctx context.Context, keys []string) ([]kv.ReadResult, error) {
						rows, err := backend.ReadSet(ctx, keys)
						trigger()
						return rows, err
					}
					// This also reproduces the old sequential-reader bug: replacement after
					// its header, or a multi-key update after the first requested chunk.
					store.get = func(ctx context.Context, k string) (kv.Entry, bool, error) {
						e, ok, err := backend.Get(ctx, k)
						if mutation == "replacement" && k == volumeKey("v") || mutation == "multi-chunk" && k == chunkKey("v", 1) {
							trigger()
						}
						return e, ok, err
					}
					r, err := s.ChangeIsr(ctx, req)
					if !triggered || err != nil || r.GetApplied() || r.GetEpoch() != before.Epoch || len(r.GetChunks()) != 2 || !proto.Equal(r.Chunks[0], before.Chunks[1]) || !proto.Equal(r.Chunks[1], before.Chunks[0]) {
						t.Fatalf("torn refusal: %v %v, before=%v", r, err, before)
					}
					after, err := backend.List(context.Background(), prefix)
					if err != nil || !equalTestEntries(postMutation, after) {
						t.Fatalf("refusal changed storage: %v", err)
					}
				})
			}
		}
	})
}

func equalTestEntries(a, b []kv.Entry) bool {
	return slices.EqualFunc(a, b, func(a, b kv.Entry) bool { return a.Key == b.Key && a.Rev == b.Rev && bytes.Equal(a.Value, b.Value) })
}

func TestChangeIsrDuplicatePositionsAndRawCap(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		s, store, _ := coherentFixture(t, backend, Options{})
		a, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil {
			t.Fatal(err)
		}
		before, err := backend.List(context.Background(), prefix)
		if err != nil {
			t.Fatal(err)
		}
		reads := 0
		store.readSet = func(ctx context.Context, keys []string) ([]kv.ReadResult, error) {
			reads++
			if !slices.Equal(keys, []string{volumeKey("v"), chunkKey("v", 0), chunkKey("v", 0)}) {
				t.Fatalf("deduplicated/reordered request positions: %v", keys)
			}
			return backend.ReadSet(ctx, keys)
		}
		change := &metav1.IsrChange{Chunk: 0, NewIsr: []string{"a"}}
		r, err := s.ChangeIsr(context.Background(), &metav1.ChangeIsrRequest{Volume: "v", Epoch: a.Volume.Epoch, Changes: []*metav1.IsrChange{change, change}})
		if err != nil || r.GetApplied() || r.GetEpoch() != a.Volume.Epoch || len(r.GetChunks()) != 2 || !proto.Equal(r.Chunks[0], a.Volume.Chunks[0]) || !proto.Equal(r.Chunks[1], a.Volume.Chunks[0]) || reads != 1 {
			t.Fatalf("duplicate refusal: %v %v reads=%d", r, err, reads)
		}
		after, err := backend.List(context.Background(), prefix)
		if err != nil || !equalTestEntries(before, after) {
			t.Fatal("duplicate refusal wrote metadata")
		}
		tooMany := make([]*metav1.IsrChange, 65)
		for i := range tooMany {
			tooMany[i] = change
		}
		if r, err := s.ChangeIsr(context.Background(), &metav1.ChangeIsrRequest{Volume: "v", Epoch: a.Volume.Epoch, Changes: tooMany}); r != nil || code(err) != codes.InvalidArgument || reads != 1 {
			t.Fatalf("raw cap did not precede ReadSet: %v %v reads=%d", r, err, reads)
		}
	})
}

func TestChangeIsrCASReadRetriesAndCommitPoint(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, schedule := range []string{"failed-cas", "commit-point", "cancel", "bounded"} {
			t.Run(schedule, func(t *testing.T) {
				s, store, v := coherentFixture(t, backend, Options{})
				ctx, cancel := context.WithCancel(context.Background())
				defer cancel()
				reads, writes := 0, 0
				store.readSet = func(ctx context.Context, keys []string) ([]kv.ReadResult, error) {
					reads++
					return backend.ReadSet(ctx, keys)
				}
				store.txn = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
					writes++
					if schedule == "cancel" {
						cancel()
						return false, nil
					}
					if schedule == "bounded" {
						return false, nil
					}
					if schedule == "failed-cas" && writes == 1 {
						changed := domain.Chunk{Replicas: v.Chunks[0].Replicas, ISR: []string{"b"}, ISRVer: 1}
						b, _ := json.Marshal(changed)
						writeTestOps(t, backend, kv.Op{Key: chunkKey("v", 0), Value: b})
					}
					ok, err := backend.Txn(ctx, cmps, ops)
					if schedule == "commit-point" && ok {
						// A newer authority immediately after CAS cannot rewrite its reply.
						h := header(t, backend, "v")
						h.Attach("b")
						writeTestHeader(t, backend, h)
					}
					return ok, err
				}
				r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: v.Epoch, Changes: []*metav1.IsrChange{{Chunk: 0, NewIsr: []string{"a"}}}})
				switch schedule {
				case "failed-cas":
					if err != nil || r.GetApplied() || reads != 2 || writes != 1 || r.Chunks[0].IsrVer != 1 || !slices.Equal(r.Chunks[0].Isr, []string{"b"}) {
						t.Fatalf("failed CAS did not acquire fresh snapshot: %v %v reads=%d writes=%d", r, err, reads, writes)
					}
				case "commit-point":
					if err != nil || !r.GetApplied() || r.Epoch != v.Epoch || r.Chunks[0].IsrVer != 1 || !slices.Equal(r.Chunks[0].Isr, []string{"a"}) || reads != 1 || header(t, backend, "v").Epoch != v.Epoch+1 {
						t.Fatalf("CAS reply lost its commit point: %v %v reads=%d", r, err, reads)
					}
				case "cancel":
					if r != nil || code(err) != codes.Canceled || reads != 1 || writes != 1 {
						t.Fatalf("CAS cancellation: %v %v reads=%d writes=%d", r, err, reads, writes)
					}
				case "bounded":
					if r != nil || code(err) != codes.Aborted || reads != retries || writes != retries {
						t.Fatalf("CAS retry budget: %v %v reads=%d writes=%d", r, err, reads, writes)
					}
				}
			})
		}
	})
}

func TestAttachSupersededCommitAccounting(t *testing.T) {
	headerHistoryBackends(t, volumeKey("v"), func(t *testing.T, backend *headerHistory) {
		s, store, old := coherentFixture(t, backend, Options{})
		initialRevision := backend.revision(t)
		successes := 0
		var firstRevision int64
		store.txnRev = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
			rev, ok, err := backend.TxnRev(ctx, cmps, ops)
			if ok && err == nil {
				if firstRevision == 0 {
					firstRevision = rev
					if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "b"}); err != nil {
						t.Fatal(err)
					}
					successes++
				}
			}
			return rev, ok, err
		}
		r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"})
		if r != nil {
			successes++
		}
		requireCommittedError(t, err, codes.Aborted, old.Epoch+1, firstRevision)
		commits := backend.appliedAfter(t, initialRevision)
		if len(commits) != 2 || successes != 1 || header(t, backend, "v").Epoch != old.Epoch+2 || header(t, backend, "v").Target != "b" {
			t.Fatalf("commits=%v successes=%d header=%+v", commits, successes, header(t, backend, "v"))
		}
	})
}

func TestAttachAmbiguousBackendRetryCountsEveryAppliedEpoch(t *testing.T) {
	headerHistoryBackends(t, volumeKey("v"), func(t *testing.T, backend *headerHistory) {
		s, store, old := coherentFixture(t, backend, Options{})
		initialRevision := backend.revision(t)
		var lostRevision int64
		store.txnRev = func(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
			rev, ok, err := backend.TxnRev(ctx, cmps, ops)
			if ok && err == nil && lostRevision == 0 {
				lostRevision = rev
				// The backend applies the write, loses its reply, and retries the same CAS.
				// Its outer result reports a failed compare despite that earlier application.
				rev, ok, err = backend.TxnRev(ctx, cmps, ops)
				if ok || err != nil {
					t.Fatalf("retry of applied CAS: applied=%v err=%v", ok, err)
				}
			}
			return rev, ok, err
		}
		r, err := s.Attach(context.Background(), &metav1.AttachRequest{Volume: "v", Node: "a"})
		commits := backend.appliedAfter(t, initialRevision)
		if err != nil || len(commits) != 2 || !commits[lostRevision] || r.GetVolume().GetEpoch() != old.Epoch+2 || header(t, backend, "v").Epoch != old.Epoch+uint64(len(commits)) {
			t.Fatalf("ambiguous retry lost epoch: %v %v applied=%v", r, err, commits)
		}
	})
}

func TestChangeIsrSnapshotErrors(t *testing.T) {
	backends(t, func(t *testing.T, backend kv.KV) {
		for _, failure := range []string{"missing-header", "creating", "deleting", "missing-chunk", "bad-chunk", "bad-header", "out-of-range", "backend", "cancel", "deadline"} {
			t.Run(failure, func(t *testing.T) {
				s, store, v := coherentFixture(t, backend, Options{})
				req := &metav1.ChangeIsrRequest{Volume: "v", Epoch: v.Epoch, Changes: []*metav1.IsrChange{{Chunk: 0, NewIsr: []string{"a"}}}}
				want := codes.Internal
				switch failure {
				case "missing-header":
					writeTestOps(t, backend, kv.Op{Key: volumeKey("v"), Delete: true})
					want = codes.NotFound
				case "creating", "deleting":
					h := header(t, backend, "v")
					h.Creating = failure == "creating"
					h.Deleting = failure == "deleting"
					writeTestHeader(t, backend, h)
					// Publication must be decoded before diagnosing the named chunk.
					writeTestOps(t, backend, kv.Op{Key: chunkKey("v", 0), Value: []byte("{")})
					want = codes.NotFound
				case "missing-chunk":
					writeTestOps(t, backend, kv.Op{Key: chunkKey("v", 0), Delete: true})
				case "bad-chunk":
					writeTestOps(t, backend, kv.Op{Key: chunkKey("v", 0), Value: []byte("{")})
				case "bad-header":
					writeTestOps(t, backend, kv.Op{Key: volumeKey("v"), Value: []byte("{")})
					want = codes.Unavailable
				case "out-of-range":
					req.Changes[0].Chunk = 2
					want = codes.InvalidArgument
				case "backend", "cancel", "deadline":
					err := status.Error(codes.Unavailable, "injected snapshot failure")
					want = codes.Unavailable
					if failure == "cancel" {
						err = context.Canceled
						want = codes.Canceled
					}
					if failure == "deadline" {
						err = context.DeadlineExceeded
						want = codes.DeadlineExceeded
					}
					store.readSet = func(context.Context, []string) ([]kv.ReadResult, error) { return nil, err }
				}
				before, err := backend.List(context.Background(), prefix)
				if err != nil {
					t.Fatal(err)
				}
				r, err := s.ChangeIsr(context.Background(), req)
				if r != nil || code(err) != want {
					t.Fatalf("snapshot error: %v %v, want %s", r, err, want)
				}
				after, err := backend.List(context.Background(), prefix)
				if err != nil || !equalTestEntries(before, after) {
					t.Fatal("failed snapshot changed storage")
				}
			})
		}
	})
}
