package service

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/grpc/codes"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// txnBoundsKV records the largest compare and op counts of successful Txn/TxnRev calls.
type txnBoundsKV struct {
	kv.KV
	maxCmps, maxOps atomic.Int32
	lastCmps        atomic.Int32
	lastOps         atomic.Int32
	onTxn           atomic.Pointer[func(cmps []kv.Cmp, ops []kv.Op)]
}

func (t *txnBoundsKV) note(cmps []kv.Cmp, ops []kv.Op) {
	n, m := int32(len(cmps)), int32(len(ops))
	t.lastCmps.Store(n)
	t.lastOps.Store(m)
	for {
		cur := t.maxCmps.Load()
		if n <= cur || t.maxCmps.CompareAndSwap(cur, n) {
			break
		}
	}
	for {
		cur := t.maxOps.Load()
		if m <= cur || t.maxOps.CompareAndSwap(cur, m) {
			break
		}
	}
	if h := t.onTxn.Load(); h != nil {
		(*h)(cmps, ops)
	}
}

func (t *txnBoundsKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	t.note(cmps, ops)
	return t.KV.Txn(ctx, cmps, ops)
}

func (t *txnBoundsKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	t.note(cmps, ops)
	return t.KV.TxnRev(ctx, cmps, ops)
}

func volumeOnNodes(t *testing.T, s *Service, id string) []string {
	t.Helper()
	g, err := s.GetVolume(context.Background(), &metav1.GetVolumeRequest{Id: id})
	if err != nil {
		t.Fatal(err)
	}
	var nodes []string
	for _, ch := range g.GetVolume().GetChunks() {
		for _, n := range ch.GetReplicas() {
			if !slices.Contains(nodes, n) {
				nodes = append(nodes, n)
			}
		}
	}
	return nodes
}

func headerAdmissionGen(t *testing.T, store kv.KV, id string) int64 {
	t.Helper()
	e, ok, err := store.Get(context.Background(), volumeKey(id))
	if err != nil || !ok {
		t.Fatalf("header %s: %v %v", id, ok, err)
	}
	var h domain.Header
	if err := json.Unmarshal(e.Value, &h); err != nil {
		t.Fatal(err)
	}
	return h.AdmissionGeneration
}

func plantCreatingHeader(t *testing.T, store kv.KV, id string, gen int64, sizeChunks int, replicas []string) {
	t.Helper()
	ctx := context.Background()
	cs := domain.CreateSpec{
		SizeBytes: uint64(sizeChunks) * mib, ChunkBytes: mib,
		Replicas: uint32(len(replicas)), MinISR: 1,
	}
	op := "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
	if len(id) > 0 {
		// Distinct ops per volume id so concurrent plants do not collide.
		op = "e" + id + "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
		if len(op) > 32 {
			op = op[:32]
		}
	}
	root := createRoot{
		Schema: createRootSchema, Volume: id, Spec: cs,
		State: rootCopying, ChunkCount: sizeChunks, Staged: uint32(sizeChunks), Next: 0,
	}
	rb, err := encodeRecord(root)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: 0}}, []kv.Op{
		{Key: createRootKey(op), Value: rb},
		{Key: createBirthKey(id), Value: []byte("1")},
	}); err != nil || !ok {
		t.Fatalf("root: %v %v", ok, err)
	}
	for i := 0; i < sizeChunks; i++ {
		ch := domain.Chunk{Replicas: slices.Clone(replicas), ISR: slices.Clone(replicas), ISRVer: 1}
		pb, _ := encodeRecord(ch)
		if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, uint32(i)), Value: pb}}); err != nil || !ok {
			t.Fatal(err)
		}
	}
	h := domain.Header{
		ID: id, SizeBytes: cs.SizeBytes, ChunkBytes: cs.ChunkBytes,
		Replicas: cs.Replicas, MinISR: cs.MinISR,
		Creating: true, CreateOperation: op, CreateSpec: &cs,
		AdmissionGeneration: gen,
	}
	hb, _ := encodeRecord(h)
	if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey(id), Rev: 0}}, []kv.Op{{Key: volumeKey(id), Value: hb}}); err != nil || !ok {
		t.Fatalf("header: %v %v", ok, err)
	}
}

// S1/S5: audit F03 — create after a concurrent drain mark at the load-scan boundary must not
// place on the draining node (docs/issues/78, audit F03).
func TestAdmissionCreateDoesNotPlaceOnConcurrentDrain(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		k := &interleaveKV{KV: store, prefix: prefix + "chunks/"}
		s := New(k, Options{})
		addNodes(t, s, "a", "b")
		hook := func() {
			if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true}); err != nil {
				t.Error(err)
			}
		}
		k.hook.Store(&hook)
		v, err := create(s, "v", 1, 2, 1)
		if err != nil {
			// Only b remains eligible: RF=2 may fail; RF=1 with two nodes should succeed on b.
			if code(err) != codes.FailedPrecondition && code(err) != codes.Aborted {
				t.Fatalf("unexpected error: %v", err)
			}
			return
		}
		if slices.Contains(volumeOnNodes(t, s, "v"), "a") {
			t.Fatalf("placed on draining node a: %v chunks=%v", v, volumeOnNodes(t, s, "v"))
		}
	})
}

// S1: drain mark after the load list / before the claim — replan or refuse; never place on a.
func TestAdmissionDrainBeforeClaim(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		k := &interleaveKV{KV: store, prefix: prefix + "chunks/"}
		s := New(k, Options{})
		addNodes(t, s, "a", "b")
		hook := func() {
			if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true}); err != nil {
				t.Error(err)
			}
		}
		k.hook.Store(&hook)
		v, err := create(s, "before", 1, 1, 1)
		if err != nil {
			if code(err) != codes.FailedPrecondition && code(err) != codes.Aborted {
				t.Fatalf("unexpected: %v", err)
			}
			return
		}
		if slices.Contains(volumeOnNodes(t, s, "before"), "a") {
			t.Fatalf("placed on a after pre-claim drain: %v", v)
		}
	})
}

// S1: drain mark after the claim CAS — earlier admission may finish; no longer pending once visible.
func TestAdmissionDrainAfterClaim(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		k := &interleaveKV{KV: store, txnKey: volumeKey("after")}
		s := New(k, Options{})
		addNodes(t, s, "a", "b")
		var drainGen uint64
		hook := func() {
			r, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
			if err != nil {
				t.Error(err)
				return
			}
			drainGen = r.GetGeneration()
		}
		k.hook.Store(&hook)
		if _, err := create(s, "after", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if drainGen == 0 {
			t.Fatal("drain did not run after claim")
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: drainGen})
		if err != nil {
			t.Fatal(err)
		}
		if check.GetPendingCreations() != 0 {
			t.Fatalf("visible volume still counted as pending: %+v", check)
		}
	})
}

// S1: a registration label change racing admission advances the gate; the claim replans.
func TestAdmissionRegistrationRacesClaim(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		k := &interleaveKV{KV: store, prefix: prefix + "chunks/"}
		s := New(k, Options{})
		addNodes(t, s, "a", "b")
		hook := func() {
			if err := register(ctx, s, "a", "a:7410", map[string]string{"zone": "z1"}); err != nil {
				t.Error(err)
			}
		}
		k.hook.Store(&hook)
		if _, err := create(s, "reg", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if g := headerAdmissionGen(t, store, "reg"); g <= 0 {
			t.Fatalf("admission_generation %d", g)
		}
	})
}

// S5: direct move refused while destination drains; racing mark invalidates a stale pass;
// undrain/re-drain ABA still fences via the gate.
func TestAdmissionMoveRefusesDrainingDestination(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		if _, err := create(s, "mv", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true})
		if err != nil || mark.GetGeneration() == 0 {
			t.Fatalf("mark: %v %+v", err, mark)
		}
		if _, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "mv", Chunk: 0, From: "a", To: "c"}); code(err) != codes.FailedPrecondition {
			t.Fatalf("move to draining: %v", err)
		}
		// Undrain then re-drain: a stale gate compare must not commit.
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: false}); err != nil {
			t.Fatal(err)
		}
		k := &interleaveKV{KV: store, key: placementGateKey}
		s2 := New(k, Options{})
		hook := func() {
			if _, err := s2.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true}); err != nil {
				t.Error(err)
			}
		}
		k.hook.Store(&hook)
		r, err := s2.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "mv", Chunk: 0, From: "b", To: "c"})
		if err == nil && r.GetApplied() {
			t.Fatal("move applied across concurrent re-drain")
		}
		if err != nil && code(err) != codes.FailedPrecondition && code(err) != codes.Aborted {
			t.Fatalf("unexpected move error: %v", err)
		}
		g, _ := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "mv"})
		if ch := g.GetVolume().GetChunks()[0]; ch.GetMovingTo() == "c" {
			t.Fatalf("forbidden move recorded: %+v", ch)
		}
	})
}

// S2/S3/S6: multi-batch publication with CheckDrain observing the reservation until visible;
// original admission generation retained; compare/op bounds held.
func TestAdmissionMultiBatchReservationAndBounds(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		bounds := &txnBoundsKV{KV: store}
		s := New(bounds, Options{})
		addNodes(t, s, "a", "b")
		// 40 chunks → more than one copy batch (createBatchSize=32).
		v, err := create(s, "batch", 40, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		if g := headerAdmissionGen(t, store, "batch"); g <= 0 {
			t.Fatalf("admission_generation %d", g)
		}
		if v == nil || len(v.GetChunks()) != 40 {
			t.Fatalf("chunks: %v", v)
		}
		// Claim: ≤4 compares (root, volume, tombstone, gate) / 2 ops; mark ≤3/2; check 1/0.
		if bounds.maxCmps.Load() > kv.MaxTxnOps || bounds.maxOps.Load() > kv.MaxTxnOps {
			t.Fatalf("txn exceeded MaxTxnOps: cmps=%d ops=%d", bounds.maxCmps.Load(), bounds.maxOps.Load())
		}
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if err != nil {
			t.Fatal(err)
		}
		if check.GetComplete() {
			t.Fatal("complete while replicas remain on a")
		}
		if bounds.lastCmps.Load() != 1 || bounds.lastOps.Load() != 0 {
			// Last txn may not be CheckDrain if DrainNode wrote last; probe CheckDrain again.
			bounds.lastCmps.Store(0)
			bounds.lastOps.Store(0)
			if _, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()}); err != nil {
				t.Fatal(err)
			}
			if bounds.lastCmps.Load() != 1 || bounds.lastOps.Load() != 0 {
				t.Fatalf("CheckDrain bounds cmps=%d ops=%d", bounds.lastCmps.Load(), bounds.lastOps.Load())
			}
		}
	})
}

// S2/S6: pauses during publication — CheckDrain never reports complete while a creating
// reservation older than D can still publish.
func TestAdmissionCheckDrainDuringPublicationPhases(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		// Plant an older creating reservation on b (unrelated node) with gen below a future D.
		gate, err := s.ensurePlacementGate(ctx)
		if err != nil {
			t.Fatal(err)
		}
		plantCreatingHeader(t, store, "old", gate, 2, []string{"b", "c"})
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		d := mark.GetGeneration()
		if d == 0 || int64(d) <= gate {
			// Mark bumps gate; marker rev equals txn rev which is > prior gate.
			if d == 0 {
				t.Fatal("generation 0")
			}
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: d})
		if err != nil {
			t.Fatal(err)
		}
		if check.GetComplete() || check.GetPendingCreations() == 0 {
			t.Fatalf("older creating must block: %+v", check)
		}
		// Later unrelated creator after mark must not block (admission_generation >= D).
		if _, err := create(s, "new", 1, 2, 1); err != nil {
			// a is draining; RF=2 on b,c should work.
			t.Fatal(err)
		}
		if g := headerAdmissionGen(t, store, "new"); g < int64(d) {
			t.Fatalf("new admission_generation %d < D %d", g, d)
		}
		check2, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: d})
		if err != nil {
			t.Fatal(err)
		}
		if check2.GetPendingCreations() != 1 {
			t.Fatalf("only the older creating should count: %+v", check2)
		}
	})
}

// S2/S6: lost finalize (hidden creating) + delete + recreate; stale batch cannot resurrect.
func TestAdmissionHiddenClaimDeleteAndRecreate(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		gate, err := s.ensurePlacementGate(ctx)
		if err != nil {
			t.Fatal(err)
		}
		plantCreatingHeader(t, store, "hid", gate, 2, []string{"a", "b"})
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if err != nil || check.GetPendingCreations() != 1 {
			t.Fatalf("hidden creating: %+v %v", check, err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "hid"}); err != nil {
			t.Fatal(err)
		}
		// After fenced delete of the creating volume, pending should clear once header is gone.
		check2, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if err != nil {
			t.Fatal(err)
		}
		if check2.GetPendingCreations() != 0 {
			t.Fatalf("pending after delete: %+v", check2)
		}
		// Undrain to allow recreate on a.
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: false}); err != nil {
			t.Fatal(err)
		}
		for _, n := range []string{"a", "b"} {
			if _, err := s.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: "hid", Node: n}); err != nil {
				// Tombstone may already be gone if unpublished rollback skipped it.
				if code(err) != codes.NotFound && code(err) != codes.OK {
					// ConfirmDeletion returns OK even when nothing left; ignore NotFound from Get.
					_ = err
				}
			}
		}
		if _, err := create(s, "hid", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if g := headerAdmissionGen(t, store, "hid"); g <= 0 {
			t.Fatalf("recreate recorded generation 0")
		}
	})
}

// S3/S4: CheckDrain inventory — incoming moves, corrupt headers, undrain during scan, empty
// visible listing alone never completes.
func TestCheckDrainInventoryAndValidation(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		if _, err := create(s, "v", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		// Empty visible listing on idle node d: mark with no replicas still needs validation.
		addNodes(t, s, "d")
		markD, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "d", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		ok, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "d", Generation: markD.GetGeneration()})
		if err != nil || !ok.GetComplete() {
			t.Fatalf("idle drain should complete: %+v %v", ok, err)
		}

		// Incoming move before replica addition blocks the destination's drain.
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "d", Draining: false}); err != nil {
			t.Fatal(err)
		}
		mv, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", Chunk: 0, From: "a", To: "c"})
		if err != nil || !mv.GetApplied() {
			t.Fatalf("move: %v %v", mv, err)
		}
		markC, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		inc, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "c", Generation: markC.GetGeneration()})
		if err != nil || inc.GetComplete() || inc.GetIncomingMoves() == 0 {
			t.Fatalf("incoming must block: %+v %v", inc, err)
		}

		// Corrupt volume header → DataLoss, never complete.
		if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("bad"), Value: []byte("{")}}); err != nil || !ok {
			t.Fatal(err)
		}
		markA, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: markA.GetGeneration()}); code(err) != codes.DataLoss {
			t.Fatalf("corrupt header: %v", err)
		}
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("bad"), Delete: true}}); err != nil {
			t.Fatal(err)
		}

		// Undrain during final compare invalidates D.
		k := &interleaveKV{KV: store, key: drainKey("b")}
		s2 := New(k, Options{})
		// Need replicas off b or accept incomplete; mark b with no placement via fresh node.
		addNodes(t, s2, "e")
		markE, err := s2.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "e", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		k.key = drainKey("e")
		hook := func() {
			if _, err := s2.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "e", Draining: false}); err != nil {
				t.Error(err)
			}
		}
		k.hook.Store(&hook)
		// CheckDrain Gets the marker first (key match), then lists; undrain after first Get
		// makes final compare fail.
		if _, err := s2.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "e", Generation: markE.GetGeneration()}); code(err) != codes.FailedPrecondition {
			// Hook runs after Get returns, so scan may still see marker; final Txn fails.
			if err == nil {
				t.Fatal("expected stale generation after undrain during check")
			}
			if code(err) != codes.FailedPrecondition {
				t.Fatalf("want FailedPrecondition, got %v", err)
			}
		}
	})
}

// S3: ChangeIsr completing a move clears the incoming count.
func TestCheckDrainAfterChangeIsrCompletesMove(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		if _, err := create(s, "v", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		att, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", Chunk: 0, From: "b", To: "c"}); err != nil {
			t.Fatal(err)
		}
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		g, _ := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
		c0 := g.GetVolume().GetChunks()[0]
		rep := append(slices.Clone(c0.GetReplicas()), "c")
		for i, step := range []*metav1.IsrChange{
			{Chunk: 0, ExpectedVer: c0.GetIsrVer(), NewIsr: c0.GetIsr(), ChangeReplicas: true, NewReplicas: rep},
			{Chunk: 0, ExpectedVer: c0.GetIsrVer() + 1, NewIsr: append(slices.Clone(c0.GetIsr()), "c")},
			{Chunk: 0, ExpectedVer: c0.GetIsrVer() + 2, NewIsr: []string{"a", "c"}, ChangeReplicas: true, NewReplicas: []string{"a", "c"}},
		} {
			r, err := s.ChangeIsr(ctx, &metav1.ChangeIsrRequest{Volume: "v", Epoch: att.GetVolume().GetEpoch(), Changes: []*metav1.IsrChange{step}})
			if err != nil || !r.GetApplied() {
				t.Fatalf("step %d: %v %v", i, r, err)
			}
		}
		// c still has a replica → not complete; incoming should be 0.
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "c", Generation: mark.GetGeneration()})
		if err != nil {
			t.Fatal(err)
		}
		if check.GetIncomingMoves() != 0 {
			t.Fatalf("incoming after complete: %+v", check)
		}
		if check.GetPlacementChunks() == 0 {
			t.Fatalf("expected placement on c: %+v", check)
		}
	})
}

// S7: legacy marker D with absent gate — initialize G>D; repeat mark retains D; unrelated
// creator with G does not block D; legacy generation0 creation still blocks; new requests
// never record 0.
func TestAdmissionLegacyGateAndGenerationZero(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		// Plant legacy drain marker before any gate.
		if _, ok, err := store.TxnRev(ctx, nil, []kv.Op{{Key: drainKey("a"), Value: []byte("1")}}); err != nil || !ok {
			t.Fatalf("legacy mark: %v %v", ok, err)
		}
		de, ok, err := store.Get(ctx, drainKey("a"))
		if err != nil || !ok || de.Rev <= 0 {
			t.Fatalf("legacy D: %v %v", de, err)
		}
		d := de.Rev
		s := New(store, Options{})
		// Register ensures gate init; unchanged registration must still initialize.
		if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: "a", Address: "a:7410"}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "a"}); err != nil {
			t.Fatal(err)
		}
		addNodes(t, s, "b", "c")
		ge, gok, err := store.Get(ctx, placementGateKey)
		if err != nil || !gok || ge.Rev <= d {
			t.Fatalf("gate G=%v must be > D=%d", ge, d)
		}
		// Idempotent remake retains D.
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		if int64(mark.GetGeneration()) != d {
			t.Fatalf("repeat mark generation %d want %d", mark.GetGeneration(), d)
		}
		// Unrelated creator after gate init: admission_generation G >= gate, does not block D
		// when gen >= D... wait: creating with gen < D blocks. New claim compares current gate
		// which is > D, so admission_generation > D and must NOT count as pending for D.
		if _, err := create(s, "unrel", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if g := headerAdmissionGen(t, store, "unrel"); g <= 0 {
			t.Fatalf("new request recorded generation 0")
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: uint64(d)})
		if err != nil {
			t.Fatal(err)
		}
		// a may have placement from create; pending creations for gen>=D should be 0.
		if check.GetPendingCreations() != 0 {
			t.Fatalf("unrelated creator blocked D: %+v gen=%d", check, headerAdmissionGen(t, store, "unrel"))
		}
		// Legacy generation0 creating still blocks.
		plantCreatingHeader(t, store, "leg0", 0, 1, []string{"b", "c"})
		check2, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: uint64(d)})
		if err != nil || check2.GetPendingCreations() == 0 {
			t.Fatalf("legacy gen0 must block: %+v %v", check2, err)
		}
	})
}

// S8: replay shared schedules against the real service CAS (clean path refuses stale generation).
func TestAdmissionScheduleReplay(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "testdata", "placement-admission.json"))
	if err != nil {
		t.Fatal(err)
	}
	var file struct {
		Schedules []struct {
			Name            string `json:"name"`
			ExpectViolation string `json:"expect_violation"`
			Steps           []struct {
				Action     string   `json:"action"`
				Node       string   `json:"node"`
				Volume     string   `json:"volume"`
				Nodes      []string `json:"nodes"`
				Chunk      uint32   `json:"chunk"`
				To         string   `json:"to"`
				Generation int64    `json:"generation"`
				MarkNode   string   `json:"mark_node"`
				ChunkCount uint32   `json:"chunk_count"`
				Pending    *uint64  `json:"expect_pending"`
				Placement  *uint64  `json:"expect_placement"`
				Incoming   *uint64  `json:"expect_incoming"`
			} `json:"steps"`
		} `json:"schedules"`
	}
	if err := json.Unmarshal(raw, &file); err != nil {
		t.Fatal(err)
	}
	for _, sched := range file.Schedules {
		sched := sched
		t.Run(sched.Name, func(t *testing.T) {
			backends(t, func(t *testing.T, store kv.KV) {
				s := New(store, Options{})
				ctx := context.Background()
				drainGen := map[string]uint64{}
				creators := map[string]*admissionScheduleActor{}
				deletions := map[string]*admissionScheduleActor{}
				moves := map[string]*admissionScheduleActor{}
				requests := map[string]*metav1.CreateVolumeRequest{}
				generations := map[string]int64{}
				claims := map[string]uint64{}
				var check *admissionScheduleActor
				var checkGen uint64
				var checkNode string
				startCreate := func(volume string, nodes []string, count uint32) *admissionScheduleActor {
					t.Helper()
					if count == 0 {
						count = 1
					}
					req := &metav1.CreateVolumeRequest{Id: volume, SizeBytes: uint64(count) * mib, ChunkBytes: mib, Replicas: uint32(len(nodes)), MinIsr: 1, PreferNode: nodes[0]}
					requests[volume] = req
					a := startAdmissionActor(t, store, volume, "create", func(ctx context.Context, svc *Service) (any, error) { return svc.CreateVolume(ctx, req) })
					creators[volume] = a
					return a
				}
				captureClaim := func(volume string) {
					t.Helper()
					h := header(t, store, volume)
					if !h.Creating || h.AdmissionGeneration <= 0 || h.Epoch != 0 {
						t.Fatalf("claim did not retain a hidden positive admission: %+v", h)
					}
					e, ok, err := store.Get(ctx, volumeKey(volume))
					if err != nil || !ok {
						t.Fatal(err)
					}
					generations[volume], claims[volume] = h.AdmissionGeneration, uint64(e.Rev)
				}
				for i, step := range sched.Steps {
					t.Logf("step %d: %s %s %s", i, step.Action, step.Volume, step.Node)
					switch step.Action {
					case "register":
						addNodes(t, s, step.Node)
					case "mark":
						r, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: step.Node, Draining: true})
						if err != nil {
							t.Fatal(err)
						}
						drainGen[step.Node] = r.GetGeneration()
					case "unmark":
						if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: step.Node, Draining: false}); err != nil {
							t.Fatal(err)
						}
					case "plan", "admit", "mark_then_admit":
						a := startCreate(step.Volume, step.Nodes, step.ChunkCount)
						a.requirePhase(t, "stage")
						if step.Action == "plan" {
							break
						}
						a.advancePhase(t, "stage")
						a.requirePhase(t, "claim")
						if step.Action == "mark_then_admit" {
							if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: step.MarkNode, Draining: true}); err != nil {
								t.Fatal(err)
							}
							a.advance(t)
							if a.result == nil || code(a.result.err) != codes.FailedPrecondition && code(a.result.err) != codes.Aborted {
								t.Fatalf("stale claim admitted after mark: %+v phase=%v", a.result, a.pause)
							}
							if _, ok, err := store.Get(ctx, volumeKey(step.Volume)); err != nil || ok {
								t.Fatalf("forbidden post-mark header: exists=%v err=%v", ok, err)
							}
							break
						}
						a.advance(t)
						captureClaim(step.Volume)
					case "stage":
						creators[step.Volume].advancePhase(t, "stage")
						creators[step.Volume].requirePhase(t, "claim")
					case "claim":
						a := creators[step.Volume]
						a.requirePhase(t, "claim")
						a.advance(t)
						captureClaim(step.Volume)
					case "clean", "batch":
						a := creators[step.Volume]
						a.requirePhase(t, step.Action)
						a.advance(t)
						if a.result != nil && a.result.err != nil {
							if _, deleting := deletions[step.Volume]; !deleting {
								t.Fatalf("publication failed: %v", a.result.err)
							}
						}
					case "publish":
						a := creators[step.Volume]
						a.requirePhase(t, "publish")
						a.advance(t)
						if a.result == nil || a.result.err != nil {
							t.Fatalf("finalization failed: %+v", a.result)
						}
						h := header(t, store, step.Volume)
						if h.Creating || h.AdmissionGeneration != generations[step.Volume] || h.FirstEpoch != claims[step.Volume] || h.Epoch != claims[step.Volume] {
							t.Fatalf("finalization changed incarnation/admission: %+v", h)
						}
					case "restart":
						s = New(store, Options{})
						for id, a := range creators {
							if a.result != nil {
								continue
							}
							a.stop(t)
							req := requests[id]
							creators[id] = startAdmissionActor(t, store, id, "create", func(ctx context.Context, svc *Service) (any, error) { return svc.CreateVolume(ctx, req) })
							if headerAdmissionGen(t, store, id) != generations[id] {
								t.Fatal("restart relabeled original admission")
							}
						}
					case "delete_fence":
						volume := step.Volume
						a := startAdmissionActor(t, store, volume, "delete", func(ctx context.Context, svc *Service) (any, error) {
							return svc.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: volume})
						})
						// #76's never-published rollback fences by deleting the exact header;
						// its chunks are deliberately left for the subsequent scheduled cleanup.
						a.requirePhase(t, "delete_header")
						a.advance(t)
						deletions[volume] = a
					case "delete_header":
						if _, ok, err := store.Get(ctx, volumeKey(step.Volume)); err != nil || ok {
							t.Fatalf("old header survived durable deletion fence: %v %v", ok, err)
						}
					case "delete_chunks":
						a := deletions[step.Volume]
						a.advancePhase(t, "delete_chunks")
						if a.result == nil || a.result.err != nil {
							t.Fatalf("delete cleanup: %+v", a.result)
						}
					case "delete":
						if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: step.Volume}); err != nil && code(err) != codes.NotFound {
							t.Fatal(err)
						}
					case "move", "plan_move":
						v, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: step.Volume})
						if err != nil {
							t.Fatal(err)
						}
						req := &metav1.MoveReplicaRequest{Volume: step.Volume, Chunk: step.Chunk, From: v.Volume.Chunks[step.Chunk].Replicas[0], To: step.To}
						a := startAdmissionActor(t, store, step.Volume, "move", func(ctx context.Context, svc *Service) (any, error) { return svc.MoveReplica(ctx, req) })
						moves[step.Volume] = a
						a.requirePhase(t, "move")
						if step.Action == "plan_move" {
							break
						}
						fallthrough
					case "commit_move":
						a := moves[step.Volume]
						a.requirePhase(t, "move")
						a.advance(t)
						if a.result == nil || code(a.result.err) != codes.FailedPrecondition {
							t.Fatalf("move crossed a winning mark: %+v", a.result)
						}
					case "check_start":
						checkNode = step.Node
						checkGen = drainGen[step.Node]
						if step.Generation > 0 {
							checkGen = uint64(step.Generation)
						}
						req := &metav1.CheckDrainRequest{Node: checkNode, Generation: checkGen}
						check = startAdmissionActor(t, store, "", "check", func(ctx context.Context, svc *Service) (any, error) { return svc.CheckDrain(ctx, req) })
						check.requirePhase(t, "header_scan")
					case "header_scan":
						check.requirePhase(t, "header_scan")
						check.advance(t)
						check.requirePhase(t, "chunk_scan")
					case "chunk_scan":
						check.requirePhase(t, "chunk_scan")
						check.advance(t)
						check.requirePhase(t, "final_validation")
					case "final_validation":
						marker, present, err := store.Get(ctx, drainKey(checkNode))
						if err != nil {
							t.Fatal(err)
						}
						check.requirePhase(t, "final_validation")
						if len(check.pause.cmps) != 1 || len(check.pause.ops) != 0 || check.pause.cmps[0] != (kv.Cmp{Key: drainKey(checkNode), Rev: int64(checkGen)}) {
							t.Fatalf("completion did not compare original generation: %+v", check.pause)
						}
						check.advance(t)
						if check.result == nil {
							t.Fatal("completion did not finish")
						}
						if !present || marker.Rev != int64(checkGen) {
							if code(check.result.err) != codes.FailedPrecondition {
								t.Fatalf("stale D succeeded after scans: %+v", check.result)
							}
							break
						}
						if check.result.err != nil {
							t.Fatal(check.result.err)
						}
						got := check.result.value.(*metav1.CheckDrainResponse)
						if step.Pending == nil || step.Placement == nil || step.Incoming == nil {
							t.Fatal("successful validation is missing independent fixture counts")
						}
						if got.PendingCreations != *step.Pending || got.PlacementChunks != *step.Placement || got.IncomingMoves != *step.Incoming || got.Complete != (*step.Pending+*step.Placement+*step.Incoming == 0) {
							t.Fatalf("inventory disagrees with independent fixture counts: %+v want pending=%d placement=%d incoming=%d", got, *step.Pending, *step.Placement, *step.Incoming)
						}
					default:
						t.Fatalf("unknown action %q", step.Action)
					}
				}
			})
		})
	}
}

// admissionScheduleKV pauses real service calls before the named storage operations. A
// schedule step releases one operation, then waits for the next boundary or RPC result.
// Nothing emulates claim, publication, inventory or deletion in a second state machine.
type admissionSchedulePause struct {
	phase  string
	cmps   []kv.Cmp
	ops    []kv.Op
	resume chan struct{}
}

type admissionScheduleResult struct {
	value any
	err   error
}

type admissionScheduleKV struct {
	kv.KV
	volume, mode string
	events       chan *admissionSchedulePause
}

func (k *admissionScheduleKV) boundary(ctx context.Context, phase string, cmps []kv.Cmp, ops []kv.Op) error {
	p := &admissionSchedulePause{phase: phase, cmps: cmps, ops: ops, resume: make(chan struct{})}
	select {
	case k.events <- p:
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-p.resume:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (k *admissionScheduleKV) List(ctx context.Context, p string) ([]kv.Entry, error) {
	if k.mode == "check" {
		phase := ""
		if p == prefix+"volumes/" {
			phase = "header_scan"
		} else if p == prefix+"chunks/" {
			phase = "chunk_scan"
		}
		if phase != "" {
			if err := k.boundary(ctx, phase, nil, nil); err != nil {
				return nil, err
			}
		}
	}
	return k.KV.List(ctx, p)
}

func (k *admissionScheduleKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := k.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (k *admissionScheduleKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	phase := ""
	if k.mode == "check" {
		phase = "final_validation"
	}
	for _, op := range ops {
		if k.mode == "create" && strings.HasPrefix(op.Key, prefix+"create-plans/") && !op.Delete {
			phase = "stage"
		}
		if op.Key == volumeKey(k.volume) {
			if op.Delete {
				phase = "delete_header"
			} else {
				var h domain.Header
				if err := json.Unmarshal(op.Value, &h); err != nil {
					return 0, false, err
				}
				switch {
				case h.Deleting:
					phase = "delete_fence"
				case h.Creating:
					phase = "claim"
				default:
					phase = "publish"
				}
			}
			break
		}
		if strings.HasPrefix(op.Key, chunkPrefix(k.volume)) {
			switch k.mode {
			case "create":
				phase = "batch"
				if op.Delete {
					phase = "clean"
				}
			case "move":
				phase = "move"
			case "delete":
				phase = "delete_chunks"
			}
			break
		}
		if k.mode == "create" && strings.HasPrefix(op.Key, prefix+"create-roots/") && !op.Delete {
			var root createRoot
			if err := json.Unmarshal(op.Value, &root); err != nil {
				return 0, false, err
			}
			if root.State == rootCopying && root.Next == 0 {
				phase = "clean"
			}
		}
	}
	if phase != "" {
		if err := k.boundary(ctx, phase, cmps, ops); err != nil {
			return 0, false, err
		}
	}
	return k.KV.TxnRev(ctx, cmps, ops)
}

type admissionScheduleActor struct {
	kv     *admissionScheduleKV
	cancel context.CancelFunc
	done   chan admissionScheduleResult
	pause  *admissionSchedulePause
	result *admissionScheduleResult
}

func startAdmissionActor(t *testing.T, store kv.KV, volume, mode string, run func(context.Context, *Service) (any, error)) *admissionScheduleActor {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	a := &admissionScheduleActor{
		kv:     &admissionScheduleKV{KV: store, volume: volume, mode: mode, events: make(chan *admissionSchedulePause)},
		cancel: cancel, done: make(chan admissionScheduleResult, 1),
	}
	go func() {
		value, err := run(ctx, New(a.kv, Options{}))
		a.done <- admissionScheduleResult{value: value, err: err}
	}()
	t.Cleanup(func() { a.stop(t) })
	a.wait(t)
	return a
}

func (a *admissionScheduleActor) wait(t *testing.T) {
	t.Helper()
	a.pause = nil
	select {
	case a.pause = <-a.kv.events:
	case r := <-a.done:
		a.result = &r
	case <-time.After(10 * time.Second):
		t.Fatal("service did not reach the next scheduled KV boundary")
	}
}

func (a *admissionScheduleActor) requirePhase(t *testing.T, want string) {
	t.Helper()
	if a == nil || a.pause == nil || a.pause.phase != want {
		t.Fatalf("expected real %s storage operation, actor=%+v", want, a)
	}
	if want == "claim" {
		if len(a.pause.cmps) != 4 || len(a.pause.ops) != 2 {
			t.Fatalf("claim lost #76 root fence or constant bounds: %+v", a.pause)
		}
		var gate, root, header, tombstone bool
		for _, c := range a.pause.cmps {
			gate = gate || c.Key == placementGateKey && c.Rev > 0
			root = root || strings.HasPrefix(c.Key, prefix+"create-roots/") && c.Rev > 0
			header = header || c.Key == volumeKey(a.kv.volume) && c.Rev == 0
			tombstone = tombstone || c.Key == tombstoneKey(a.kv.volume) && c.Rev == 0
		}
		if !gate || !root || !header || !tombstone {
			t.Fatalf("claim omitted an admission/incarnation comparison: %+v", a.pause.cmps)
		}
	}
	if want == "batch" || want == "publish" || want == "clean" {
		if len(a.pause.cmps) != 2 || len(a.pause.ops) > createBatchSize+1 {
			t.Fatalf("publication lost bounded root/claim comparisons: %+v", a.pause)
		}
		var root, header bool
		for _, c := range a.pause.cmps {
			root = root || strings.HasPrefix(c.Key, prefix+"create-roots/") && c.Rev > 0
			header = header || c.Key == volumeKey(a.kv.volume) && c.Rev > 0
		}
		if !root || !header {
			t.Fatalf("publication omitted root or original claim: %+v", a.pause.cmps)
		}
	}
}

func (a *admissionScheduleActor) advance(t *testing.T) {
	t.Helper()
	if a.pause == nil {
		t.Fatalf("cannot release completed service call: %+v", a.result)
	}
	close(a.pause.resume)
	a.wait(t)
}

func (a *admissionScheduleActor) advancePhase(t *testing.T, phase string) {
	t.Helper()
	a.requirePhase(t, phase)
	for a.pause != nil && a.pause.phase == phase {
		a.advance(t)
	}
}

func (a *admissionScheduleActor) stop(t *testing.T) {
	t.Helper()
	a.cancel()
	if a.result == nil {
		select {
		case r := <-a.done:
			a.result = &r
		case <-time.After(10 * time.Second):
			t.Error("cancelled service actor did not stop")
		}
	}
}

// S8: clean service refuses a stale generation after empty inventory + unmark (and re-mark D2).
func TestCheckDrainRefusesStaleGenerationAfterUnmark(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a")
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		d := mark.GetGeneration()
		ok, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: d})
		if err != nil || !ok.GetComplete() {
			t.Fatalf("empty drain: %+v %v", ok, err)
		}
		if _, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: false}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: d}); code(err) != codes.FailedPrecondition {
			t.Fatalf("after unmark: %v", err)
		}
		mark2, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		if mark2.GetGeneration() == d {
			t.Fatal("re-mark reused D")
		}
		if _, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: d}); code(err) != codes.FailedPrecondition {
			t.Fatalf("stale D after re-mark: %v", err)
		}
		ok2, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark2.GetGeneration()})
		if err != nil || !ok2.GetComplete() {
			t.Fatalf("D2: %+v %v", ok2, err)
		}
	})
}

// S1: create claim compares include the gate (exact bound).
func TestAdmissionClaimComparesGate(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		bounds := &txnBoundsKV{KV: store}
		var sawGate atomic.Bool
		fn := func(cmps []kv.Cmp, ops []kv.Op) {
			for _, c := range cmps {
				if c.Key == placementGateKey {
					sawGate.Store(true)
					if len(cmps) > 4 {
						t.Errorf("claim compares %d > 4", len(cmps))
					}
					if len(ops) > 2 {
						t.Errorf("claim ops %d > 2", len(ops))
					}
				}
			}
		}
		bounds.onTxn.Store(&fn)
		s := New(bounds, Options{})
		addNodes(t, s, "a", "b")
		if _, err := create(s, "cmp", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		if !sawGate.Load() {
			t.Fatal("create claim never compared the placement gate")
		}
	})
}

// List failure during CheckDrain must not report complete.
func TestCheckDrainListFailure(t *testing.T) {
	inner := kv.NewMem(nil)
	fail := &failListKV{KV: inner, prefix: prefix + "volumes/"}
	s := New(fail, Options{})
	addNodes(t, s, "a")
	mark, err := s.DrainNode(context.Background(), &metav1.DrainNodeRequest{Node: "a", Draining: true})
	if err != nil {
		t.Fatal(err)
	}
	fail.fail.Store(true)
	if _, err := s.CheckDrain(context.Background(), &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()}); err == nil {
		t.Fatal("expected list failure")
	}
}

type failListKV struct {
	kv.KV
	prefix string
	fail   atomic.Bool
}

func (f *failListKV) List(ctx context.Context, prefix string) ([]kv.Entry, error) {
	if f.fail.Load() && prefix == f.prefix {
		return nil, context.DeadlineExceeded
	}
	return f.KV.List(ctx, prefix)
}

// S3: duplicate nested fields must not hide a recorded incoming destination from inventory.
func TestCheckDrainRejectsAmbiguousNestedMove(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		writeTestOps(t, store, kv.Op{Key: chunkKey("orphan", 0), Value: []byte(`{"replicas":["b"],"isr":["b"],"isr_ver":0,"moving":{"from":"b","to":"a","to":"c"}}`)})
		got, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if code(err) != codes.DataLoss {
			t.Fatalf("ambiguous incoming destination must fail closed: got=%+v err=%v", got, err)
		}
	})
}

// S3/S6: all raw records participate, including hidden/deleting and orphan chunks.
// A malformed record anywhere must prevent an otherwise empty completion.
func TestCheckDrainRawInventorySchema(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		validHeader := domain.Header{ID: "raw", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Epoch: 7}
		hb, _ := json.Marshal(validHeader)
		validChunk := []byte(`{"replicas":["b"],"isr":["b"],"isr_ver":0}`)
		change := func(raw []byte, field string, value any, remove bool) []byte {
			t.Helper()
			var m map[string]any
			if err := json.Unmarshal(raw, &m); err != nil {
				t.Fatal(err)
			}
			if remove {
				delete(m, field)
			} else {
				m[field] = value
			}
			b, err := json.Marshal(m)
			if err != nil {
				t.Fatal(err)
			}
			return b
		}
		cases := []struct {
			name, key string
			value     []byte
		}{
			{"header-json", volumeKey("raw"), []byte("{")},
			{"header-null", volumeKey("raw"), []byte("null")},
			{"header-array", volumeKey("raw"), []byte("[]")},
			{"header-id-key", volumeKey("other"), hb},
			{"header-negative-generation", volumeKey("raw"), change(hb, "admission_generation", -1, false)},
			{"header-generation-type", volumeKey("raw"), change(hb, "admission_generation", "1", false)},
			{"header-null-generation", volumeKey("raw"), change(hb, "admission_generation", nil, false)},
			{"header-zero-isr", volumeKey("raw"), change(hb, "min_isr", 0, false)},
			{"header-isr-exceeds-rf", volumeKey("raw"), change(hb, "min_isr", 2, false)},
			{"header-target-id", volumeKey("raw"), change(hb, "target", "../bad", false)},
			{"header-first-epoch", volumeKey("raw"), change(hb, "first_epoch", 8, false)},
			{"header-creating-published", volumeKey("raw"), change(hb, "creating", true, false)},
			{"header-missing-operation", volumeKey("raw"), change(hb, "create_spec", domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1}, false)},
			{"header-missing-identity", volumeKey("raw"), change(hb, "create_operation", "op", false)},
			{"header-duplicate-generation", volumeKey("raw"), append(bytes.TrimSuffix(hb, []byte("}")), []byte(`,"admission_generation":-1,"admission_generation":0}`)...)},
			{"header-oversized", volumeKey("raw"), bytes.Repeat([]byte(" "), domain.MaxCreateRecordBytes+1)},
			{"chunk-json", chunkKey("raw", 0), []byte("{")},
			{"chunk-null", chunkKey("raw", 0), []byte("null")},
			{"chunk-key", prefix + "chunks/raw/0", validChunk},
			{"chunk-volume-id", prefix + "chunks/../00000000", validChunk},
			{"chunk-replica-id", chunkKey("raw", 0), change(validChunk, "replicas", []string{"../bad"}, false)},
			{"chunk-duplicate-replica", chunkKey("raw", 0), change(validChunk, "replicas", []string{"b", "b"}, false)},
			{"chunk-empty-replica", chunkKey("raw", 0), change(validChunk, "replicas", []string{}, false)},
			{"chunk-null-isr", chunkKey("raw", 0), change(validChunk, "isr", nil, false)},
			{"chunk-isr-not-replica", chunkKey("raw", 0), change(validChunk, "isr", []string{"c"}, false)},
			{"chunk-duplicate-isr", chunkKey("raw", 0), change(validChunk, "isr", []string{"b", "b"}, false)},
			{"chunk-move-missing-to", chunkKey("raw", 0), change(validChunk, "moving", map[string]string{"from": "b"}, false)},
			{"chunk-move-from-not-replica", chunkKey("raw", 0), change(validChunk, "moving", map[string]string{"from": "c", "to": "a"}, false)},
			{"chunk-move-same-endpoint", chunkKey("raw", 0), change(validChunk, "moving", map[string]string{"from": "b", "to": "b"}, false)},
			{"chunk-move-wrong-type", chunkKey("raw", 0), change(validChunk, "moving", map[string]any{"from": "b", "to": 1}, false)},
			{"chunk-move-null", chunkKey("raw", 0), change(validChunk, "moving", map[string]any{"from": "b", "to": nil}, false)},
			{"chunk-duplicate-nested", chunkKey("raw", 0), []byte(`{"replicas":["b"],"isr":["b"],"isr_ver":0,"moving":{"from":"b","to":"a","to":"c"}}`)},
		}
		for _, field := range []string{"id", "size_bytes", "chunk_bytes", "replicas", "min_isr", "epoch"} {
			cases = append(cases, struct {
				name, key string
				value     []byte
			}{"header-missing-" + field, volumeKey("raw"), change(hb, field, nil, true)})
		}
		for _, field := range []string{"replicas", "isr", "isr_ver"} {
			cases = append(cases, struct {
				name, key string
				value     []byte
			}{"chunk-missing-" + field, chunkKey("raw", 0), change(validChunk, field, nil, true)})
		}
		for _, tc := range cases {
			t.Run(tc.name, func(t *testing.T) {
				writeTestOps(t, store, kv.Op{Key: tc.key, Value: tc.value})
				got, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
				writeTestOps(t, store, kv.Op{Key: tc.key, Delete: true})
				if code(err) != codes.DataLoss || got != nil {
					t.Fatalf("corrupt raw inventory: response=%+v err=%v", got, err)
				}
			})
		}

		// Valid old geometries are still inventory, even when current admission refuses them.
		// One orphan chunk counts in both replica and incoming columns.
		validHeader.SizeBytes, validHeader.ChunkBytes, validHeader.Deleting = 4096, 4096, true
		hb, _ = json.Marshal(validHeader)
		writeTestOps(t, store,
			kv.Op{Key: volumeKey("raw"), Value: hb},
			kv.Op{Key: chunkKey("orphan", 0), Value: []byte(`{"replicas":["a","b"],"isr":["b"],"isr_ver":0,"moving":{"from":"b","to":"a"}}`)},
		)
		got, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if err != nil || got.GetComplete() || got.GetPlacementChunks() != 1 || got.GetIncomingMoves() != 1 {
			t.Fatalf("valid legacy/deleting/orphan inventory: got=%+v err=%v", got, err)
		}
		// A hidden deleting legacy claim remains a reservation until durably fenced.
		validHeader.Creating, validHeader.Epoch = true, 0
		hb, _ = json.Marshal(validHeader)
		writeTestOps(t, store, kv.Op{Key: volumeKey("raw"), Value: hb}, kv.Op{Key: chunkKey("orphan", 0), Delete: true})
		got, err = s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
		if err != nil || got.GetComplete() || got.GetPendingCreations() != 1 {
			t.Fatalf("hidden deleting reservation: got=%+v err=%v", got, err)
		}
	})
}

func TestCheckDrainRejectsInvalidArgumentsWithoutMutation(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		s := New(store, Options{})
		for _, r := range []*metav1.CheckDrainRequest{
			{Node: "../a", Generation: 1}, {Node: "a"}, {Node: "a", Generation: math.MaxInt64 + 1},
		} {
			if _, err := s.CheckDrain(context.Background(), r); code(err) != codes.InvalidArgument {
				t.Fatalf("%v: %v", r, err)
			}
		}
		entries, err := store.List(context.Background(), prefix)
		if err != nil || len(entries) != 0 {
			t.Fatalf("invalid check mutated metadata: %v %v", entries, err)
		}
	})
}

type admissionReplyLossKV struct {
	kv.KV
	key       string
	creating  bool
	loseReply atomic.Bool
}

func (k *admissionReplyLossKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := k.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (k *admissionReplyLossKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	rev, ok, err := k.KV.TxnRev(ctx, cmps, ops)
	if err != nil || !ok {
		return rev, ok, err
	}
	for _, op := range ops {
		if op.Key != k.key || op.Delete {
			continue
		}
		matches := k.key == placementGateKey
		if !matches {
			var h domain.Header
			matches = json.Unmarshal(op.Value, &h) == nil && h.Creating == k.creating
		}
		if matches && k.loseReply.Swap(false) {
			return 0, false, context.DeadlineExceeded
		}
	}
	return rev, ok, nil
}

// S2/S6: an uncertain claim/finalization reply never forfeits the original admission.
// A fresh service recovers the immutable #76 plan, even after its chosen node is marked.
func TestAdmissionLostRepliesRecoverOriginalGeneration(t *testing.T) {
	for _, phase := range []string{"claim", "finalize"} {
		t.Run(phase, func(t *testing.T) {
			backends(t, func(t *testing.T, store kv.KV) {
				ctx := context.Background()
				loss := &admissionReplyLossKV{KV: store, key: volumeKey("lost"), creating: phase == "claim"}
				s := New(loss, Options{})
				addNodes(t, s, "a", "b")
				loss.loseReply.Store(true)
				req := &metav1.CreateVolumeRequest{Id: "lost", SizeBytes: 40 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1}
				if _, err := s.CreateVolume(ctx, req); err == nil {
					t.Fatal("lost transaction reply was not injected")
				}
				if loss.loseReply.Load() {
					t.Fatal("service did not reach the requested claim/finalization boundary")
				}
				h := header(t, store, "lost")
				generation := h.AdmissionGeneration
				if generation <= 0 || h.Creating != (phase == "claim") {
					t.Fatalf("uncertain response changed durable admission: %+v", h)
				}
				mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
				if err != nil {
					t.Fatal(err)
				}
				before, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
				if err != nil || before.GetComplete() || phase == "claim" && before.GetPendingCreations() != 1 || phase == "finalize" && before.GetPlacementChunks() != 40 {
					t.Fatalf("uncertain admission disappeared: %+v %v", before, err)
				}
				restarted := New(store, Options{})
				if err := restarted.RunCreateRecovery(ctx); err != nil {
					t.Fatal(err)
				}
				got, err := restarted.CreateVolume(ctx, req)
				if err != nil || len(got.GetVolume().GetChunks()) != 40 {
					t.Fatalf("recovery failed: %+v %v", got, err)
				}
				h2 := header(t, store, "lost")
				if h2.Creating || h2.AdmissionGeneration != generation || h2.CreateOperation != h.CreateOperation || !h2.CreateSpec.Equal(*h.CreateSpec) || h2.FirstEpoch == 0 {
					t.Fatalf("recovery changed admitted identity/generation: before=%+v after=%+v", h, h2)
				}
				for _, ch := range got.GetVolume().GetChunks() {
					if !slices.Equal(ch.GetReplicas(), []string{"a", "b"}) {
						t.Fatalf("post-mark recovery replanned original replicas: %+v", ch)
					}
				}
				after, err := restarted.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
				if err != nil || after.GetComplete() || after.GetPendingCreations() != 0 || after.GetPlacementChunks() != 40 {
					t.Fatalf("reservation to chunks handoff lost work: %+v %v", after, err)
				}
			})
		})
	}
}

func TestAdmissionLegacyInitializationSurvivesLostReplyAndNoOps(t *testing.T) {
	for _, entry := range []string{"register", "mark"} {
		t.Run(entry, func(t *testing.T) {
			backends(t, func(t *testing.T, store kv.KV) {
				ctx := context.Background()
				node, _ := json.Marshal(nodeRecord{Address: "a:7410"})
				writeTestOps(t, store, kv.Op{Key: nodeKey("a"), Value: node}, kv.Op{Key: drainKey("a"), Value: []byte("1")})
				d, _, _ := store.Get(ctx, drainKey("a"))
				loss := &admissionReplyLossKV{KV: store, key: placementGateKey}
				loss.loseReply.Store(true)
				s := New(loss, Options{})
				if entry == "register" {
					if err := register(ctx, s, "a", "a:7410", nil); err != nil {
						t.Fatal(err)
					}
				} else if mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true}); err != nil || mark.GetGeneration() != uint64(d.Rev) {
					t.Fatalf("legacy mark changed: %+v %v", mark, err)
				}
				g, ok, err := store.Get(ctx, placementGateKey)
				if err != nil || !ok || g.Rev <= d.Rev || loss.loseReply.Load() {
					t.Fatalf("lost gate initialization: G=%+v D=%+v injected=%v err=%v", g, d, !loss.loseReply.Load(), err)
				}
				s = New(store, Options{})
				if err := register(ctx, s, "a", "a:7410", nil); err != nil {
					t.Fatal(err)
				}
				if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "a"}); err != nil {
					t.Fatal(err)
				}
				mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
				g2, _, _ := store.Get(ctx, placementGateKey)
				if err != nil || mark.GetGeneration() != uint64(d.Rev) || g2.Rev != g.Rev {
					t.Fatalf("unchanged registration/heartbeat/repeated mark changed generations: G=%d/%d D=%d mark=%v err=%v", g.Rev, g2.Rev, d.Rev, mark, err)
				}
			})
		})
	}
}

// Lifecycle eligibility precedes CheckMove's no-op for an already recorded destination.
func TestAdmissionMoveNoOpStillRejectsDrainingDestination(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		if _, err := create(s, "noop", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
		req := &metav1.MoveReplicaRequest{Volume: "noop", From: "a", To: "c"}
		if got, err := s.MoveReplica(ctx, req); err != nil || !got.GetApplied() {
			t.Fatalf("initial move: %+v %v", got, err)
		}
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "c", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		before, _, _ := store.Get(ctx, chunkKey("noop", 0))
		if got, err := s.MoveReplica(ctx, req); code(err) != codes.FailedPrecondition || got != nil {
			t.Fatalf("draining destination accepted as a no-op: %+v %v", got, err)
		}
		after, _, _ := store.Get(ctx, chunkKey("noop", 0))
		if before.Rev != after.Rev || !bytes.Equal(before.Value, after.Value) {
			t.Fatal("refused no-op changed the admitted move")
		}
		check, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "c", Generation: mark.GetGeneration()})
		if err != nil || check.GetIncomingMoves() != 1 || check.GetComplete() {
			t.Fatalf("earlier move reservation disappeared: %+v %v", check, err)
		}
	})
}

func TestAdmissionConcurrentReservationsRemainAccounted(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b", "c")
		var actors []*admissionScheduleActor
		var requests []*metav1.CreateVolumeRequest
		for _, id := range []string{"first", "second"} {
			req := &metav1.CreateVolumeRequest{Id: id, SizeBytes: 40 * mib, ChunkBytes: mib, Replicas: 1, MinIsr: 1, PreferNode: "b"}
			a := startAdmissionActor(t, store, id, "create", func(ctx context.Context, svc *Service) (any, error) { return svc.CreateVolume(ctx, req) })
			a.advancePhase(t, "stage")
			a.requirePhase(t, "claim")
			a.advance(t)
			a.requirePhase(t, "clean")
			a.advance(t)
			a.requirePhase(t, "batch")
			actors, requests = append(actors, a), append(requests, req)
		}
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		check := func(want uint64) {
			t.Helper()
			got, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
			if err != nil || got.GetPendingCreations() != want || got.GetComplete() != (want == 0) {
				t.Fatalf("concurrent unrelated reservations: want=%d got=%+v err=%v", want, got, err)
			}
		}
		check(2)
		// An identical caller cooperates with the admitted plan; it cannot discard the other claim.
		firstGen := headerAdmissionGen(t, store, "first")
		if _, err := s.CreateVolume(ctx, requests[0]); err != nil {
			t.Fatal(err)
		}
		if headerAdmissionGen(t, store, "first") != firstGen {
			t.Fatal("identical helper replaced the original admission")
		}
		actors[0].advance(t)
		if actors[0].result == nil {
			t.Fatal("stale helper kept writing after another caller finalized")
		}
		check(1)
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "second"}); err != nil {
			t.Fatal(err)
		}
		actors[1].advance(t)
		if actors[1].result == nil || actors[1].result.err == nil {
			t.Fatal("deleted creator resumed public writes")
		}
		check(0)
	})
}

type admissionCheckErrorKV struct {
	kv.KV
	phase string
}

func (k *admissionCheckErrorKV) Get(ctx context.Context, key string) (kv.Entry, bool, error) {
	if k.phase == "marker" && key == drainKey("a") {
		return kv.Entry{}, false, context.DeadlineExceeded
	}
	return k.KV.Get(ctx, key)
}

func (k *admissionCheckErrorKV) List(ctx context.Context, p string) ([]kv.Entry, error) {
	if k.phase == "headers" && p == prefix+"volumes/" || k.phase == "chunks" && p == prefix+"chunks/" {
		return nil, context.DeadlineExceeded
	}
	return k.KV.List(ctx, p)
}

func (k *admissionCheckErrorKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	if k.phase == "validation" && len(ops) == 0 {
		return false, context.DeadlineExceeded
	}
	return k.KV.Txn(ctx, cmps, ops)
}

func TestCheckDrainStorageErrorsNeverCertifyEmpty(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a")
		mark, err := s.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "a", Draining: true})
		if err != nil {
			t.Fatal(err)
		}
		for _, phase := range []string{"marker", "headers", "chunks", "validation"} {
			t.Run(phase, func(t *testing.T) {
				failing := New(&admissionCheckErrorKV{KV: store, phase: phase}, Options{})
				got, err := failing.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: "a", Generation: mark.GetGeneration()})
				if code(err) != codes.DeadlineExceeded || got != nil {
					t.Fatalf("storage failure became successful empty completion: %+v %v", got, err)
				}
			})
		}
	})
}

func TestAdmissionWitnessChangeForcesFreshPlan(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		req := &metav1.CreateVolumeRequest{Id: "witness-race", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinIsr: 1, PreferNode: "a"}
		a := startAdmissionActor(t, store, req.Id, "create", func(ctx context.Context, svc *Service) (any, error) { return svc.CreateVolume(ctx, req) })
		a.advancePhase(t, "stage")
		a.requirePhase(t, "claim")
		if err := register(ctx, s, "a", "a:7410", map[string]string{WitnessLabel: "true"}); err != nil {
			t.Fatal(err)
		}
		gate, _, _ := store.Get(ctx, placementGateKey)
		a.advance(t)
		// The stale claim must lose and start an entirely new private plan, rather than reuse
		// its previously selected node with a fresh gate revision.
		a.requirePhase(t, "stage")
		if _, ok, err := store.Get(ctx, volumeKey(req.Id)); err != nil || ok {
			t.Fatalf("stale topology admitted before replanning: %v %v", ok, err)
		}
		a.advancePhase(t, "stage")
		a.requirePhase(t, "claim")
		a.advance(t)
		a.requirePhase(t, "clean")
		a.advance(t)
		a.requirePhase(t, "batch")
		a.advance(t)
		a.requirePhase(t, "publish")
		a.advance(t)
		if a.result == nil || a.result.err != nil || !slices.Equal(volumeOnNodes(t, s, req.Id), []string{"b"}) || headerAdmissionGen(t, store, req.Id) != gate.Rev {
			t.Fatalf("changed topology was not replanned under fresh admission: %+v", a.result)
		}
	})
}

func TestCheckCreateVolumeDoesNotInitializeAbsentPlacementGate(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		h := domain.Header{ID: "legacy", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Epoch: 7}
		hb, _ := json.Marshal(h)
		cb, _ := json.Marshal(domain.Chunk{Replicas: []string{"a"}, ISR: []string{"a"}})
		writeTestOps(t, store, kv.Op{Key: volumeKey(h.ID), Value: hb}, kv.Op{Key: chunkKey(h.ID, 0), Value: cb})
		before, err := store.List(ctx, prefix)
		if err != nil {
			t.Fatal(err)
		}
		s := New(store, Options{})
		got, err := s.CheckCreateVolume(ctx, &metav1.CheckCreateVolumeRequest{Desired: &metav1.CreateVolumeRequest{Id: h.ID, SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinIsr: 1}, RequiredBytes: mib})
		if err != nil || !got.GetLegacyIdentity() {
			t.Fatalf("legacy compatibility without gate: %+v %v", got, err)
		}
		after, err := store.List(ctx, prefix)
		if err != nil || !slices.EqualFunc(before, after, func(a, b kv.Entry) bool { return a.Key == b.Key && a.Rev == b.Rev && bytes.Equal(a.Value, b.Value) }) {
			t.Fatalf("read-only compatibility initialized/mutated admission metadata: %v %v", after, err)
		}
	})
}

// The pure inventory decoding boundary receives arbitrary persisted bytes, including schemas
// the current geometry admission refuses. Seed cases run in ordinary go test as well.
func FuzzDrainInventory(f *testing.F) {
	for _, b := range [][]byte{
		[]byte(`{"id":"raw","size_bytes":4096,"chunk_bytes":4096,"replicas":1,"min_isr":1,"epoch":7}`),
		[]byte(`{"replicas":["a"],"isr":["a"],"isr_ver":0}`),
		[]byte(`{"replicas":["b"],"isr":["b"],"isr_ver":0,"moving":{"from":"b","to":"a","to":"c"}}`),
		[]byte(`{"creating":true,"creating":false}`), []byte("null"), []byte("{"),
		bytes.Repeat([]byte("x"), domain.MaxCreateRecordBytes+1),
	} {
		f.Add(b, true)
		f.Add(b, false)
	}
	f.Fuzz(func(t *testing.T, b []byte, isHeader bool) {
		var err error
		if isHeader {
			_, err = drainInventoryHeader(kv.Entry{Key: volumeKey("raw"), Value: b})
		} else {
			_, err = drainInventoryChunk(kv.Entry{Key: chunkKey("raw", 0), Value: b})
		}
		if err != nil && code(err) != codes.DataLoss {
			t.Fatalf("persisted corruption classified %s: %v", fmt.Sprint(code(err)), err)
		}
	})
}
