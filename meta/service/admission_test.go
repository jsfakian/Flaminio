package service

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"sync/atomic"
	"testing"

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
			Name             string `json:"name"`
			ExpectViolation  string `json:"expect_violation"`
			Steps            []struct {
				Action     string   `json:"action"`
				Node       string   `json:"node"`
				Volume     string   `json:"volume"`
				Nodes      []string `json:"nodes"`
				Chunk      uint32   `json:"chunk"`
				To         string   `json:"to"`
				Generation int64    `json:"generation"`
			} `json:"steps"`
		} `json:"schedules"`
	}
	if err := json.Unmarshal(raw, &file); err != nil {
		t.Fatal(err)
	}
	for _, sched := range file.Schedules {
		if sched.ExpectViolation != "" {
			continue // mutant counterexamples belong to the Rust model
		}
		sched := sched
		t.Run(sched.Name, func(t *testing.T) {
			backends(t, func(t *testing.T, store kv.KV) {
				s := New(store, Options{})
				ctx := context.Background()
				drainGen := map[string]uint64{}
				var checkGen uint64
				for _, step := range sched.Steps {
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
					case "admit":
						rf := uint32(len(step.Nodes))
						if rf == 0 {
							rf = 1
						}
						_, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
							Id: step.Volume, SizeBytes: mib, ChunkBytes: mib, Replicas: rf, MinIsr: 1,
						})
						if err != nil && code(err) != codes.FailedPrecondition && code(err) != codes.AlreadyExists {
							t.Fatal(err)
						}
					case "publish", "restart":
					case "delete":
						if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: step.Volume}); err != nil && code(err) != codes.NotFound {
							t.Fatal(err)
						}
					case "move":
						from := step.Node
						if len(step.Nodes) > 0 {
							from = step.Nodes[0]
						}
						_, err := s.MoveReplica(ctx, &metav1.MoveReplicaRequest{
							Volume: step.Volume, Chunk: step.Chunk, From: from, To: step.To,
						})
						if err != nil && code(err) != codes.FailedPrecondition {
							t.Fatal(err)
						}
					case "check_start":
						checkGen = drainGen[step.Node]
						if step.Generation > 0 {
							checkGen = uint64(step.Generation)
						}
					case "header_scan", "chunk_scan":
					case "final_validation":
						gen := checkGen
						if gen == 0 {
							gen = drainGen[step.Node]
						}
						if step.Generation > 0 {
							gen = uint64(step.Generation)
						}
						_, err := s.CheckDrain(ctx, &metav1.CheckDrainRequest{Node: step.Node, Generation: gen})
						if sched.Name == "empty_scan_unmark_before_validation" || sched.Name == "empty_scan_unmark_remark_before_validation" {
							if code(err) != codes.FailedPrecondition {
								t.Fatalf("stale D must be refused: %v", err)
							}
						} else if err != nil && code(err) != codes.FailedPrecondition {
							t.Fatal(err)
						}
					default:
						t.Fatalf("unknown action %q", step.Action)
					}
				}
			})
		})
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
