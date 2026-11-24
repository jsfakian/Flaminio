package service

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// docs/issues/76 S1: PreferNode empty / 128 valid / invalid / oversized refuse before staging.
func TestCreatePreferNodeBounds(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		okNode := strings.Repeat("n", 128)
		if _, err := s.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: okNode, Address: "n:7410"}}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: okNode}); err != nil {
			t.Fatal(err)
		}
		if _, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: "v-ok", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: okNode,
		}); err != nil {
			t.Fatalf("128-char prefer: %v", err)
		}
		for _, tc := range []struct {
			name, prefer string
		}{
			{"invalid", "../x"},
			{"too long", strings.Repeat("x", 129)},
			{"300KiB", strings.Repeat("y", 300<<10)},
		} {
			before, _ := store.List(ctx, createRootPrefix)
			_, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
				Id: "v-bad-" + tc.name, SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: tc.prefer,
			})
			if code(err) != codes.InvalidArgument {
				t.Fatalf("%s: %v, want InvalidArgument", tc.name, err)
			}
			after, _ := store.List(ctx, createRootPrefix)
			if len(after) != len(before) {
				t.Fatalf("%s: wrote create roots on rejection (%d → %d)", tc.name, len(before), len(after))
			}
		}
	})
}

// docs/issues/76 S1: each conflicting identity field returns AlreadyExists without mutation.
func TestCreateIdentityConflicts(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b", "c")
		resp, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1,
			PreferNode: "a", FailureDomain: "node", FailureDomainPolicy: "strict",
		})
		if err != nil {
			t.Fatal(err)
		}
		epoch := resp.GetVolume().GetEpoch()
		op := header(t, s.kv, "v").CreateOperation
		for _, tc := range []struct {
			name string
			req  *metav1.CreateVolumeRequest
		}{
			{"size", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 3 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: "a"}},
			{"chunk", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: 2 * mib, Replicas: 2, MinIsr: 1, PreferNode: "a"}},
			{"replicas", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 3, MinIsr: 1, PreferNode: "a"}},
			{"min_isr", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 2, PreferNode: "a"}},
			{"prefer", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: "b"}},
			{"domain", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: "a", FailureDomain: "zone"}},
			{"policy", &metav1.CreateVolumeRequest{Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1, PreferNode: "a", FailureDomainPolicy: "bestEffort"}},
		} {
			_, err := s.CreateVolume(ctx, tc.req)
			if code(err) != codes.AlreadyExists {
				t.Fatalf("%s: %v, want AlreadyExists", tc.name, err)
			}
			h := header(t, s.kv, "v")
			if h.Epoch != epoch || h.CreateOperation != op {
				t.Fatalf("%s mutated header: %+v", tc.name, h)
			}
		}
		// Equivalent default spellings are identical (empty domain == "node").
		again, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: "v", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1,
			PreferNode: "a", FailureDomain: "", FailureDomainPolicy: "",
		})
		if err != nil || again.GetVolume().GetEpoch() != epoch {
			t.Fatalf("normalized defaults retry: %v epoch %d want %d", err, again.GetVolume().GetEpoch(), epoch)
		}
	})
}

// docs/issues/76 S1/S3/S5: identical retry returns the same volume; topology changes are not identity.
func TestCreateRetryIgnoresTopologyAndRetainsIdentity(t *testing.T) {
	stores(t, func(t *testing.T, s *Service) {
		ctx := context.Background()
		addNodes(t, s, "a", "b")
		v, err := create(s, "v", 2, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		h0 := header(t, s.kv, "v")
		if h0.CreateOperation == "" || h0.CreateSpec == nil || h0.Creating {
			t.Fatalf("visible header missing create identity: %+v", h0)
		}
		addNodes(t, s, "c")
		again, err := create(s, "v", 2, 2, 1)
		if err != nil || again.GetEpoch() != v.GetEpoch() {
			t.Fatalf("retry: %v epoch %d want %d", err, again.GetEpoch(), v.GetEpoch())
		}
		h1 := header(t, s.kv, "v")
		if h1.CreateOperation != h0.CreateOperation || h1.Epoch != h0.Epoch || h1.FirstEpoch != h0.FirstEpoch {
			t.Fatalf("retry mutated %+v → %+v", h0, h1)
		}
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "a"}); err != nil {
			t.Fatal(err)
		}
		afterAttach := header(t, s.kv, "v")
		again, err = create(s, "v", 2, 2, 1)
		if err != nil || again.GetEpoch() != afterAttach.Epoch {
			t.Fatalf("retry after attach: %v epoch %d want %d", err, again.GetEpoch(), afterAttach.Epoch)
		}
	})
}

// docs/issues/76 S5: incomplete volumes stay hidden from Get/List and are unattachable.
func TestCreatingVolumeIsHidden(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		cs := domain.CreateSpec{SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1}
		op := "cccccccccccccccccccccccccccccccc"
		root := createRoot{
			Schema: createRootSchema, Volume: "hid", Spec: cs,
			State: rootCopying, ChunkCount: 2, Staged: 2, Next: 0,
		}
		rb, _ := encodeRecord(root)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: 0}}, []kv.Op{
			{Key: createRootKey(op), Value: rb},
			{Key: createBirthKey("hid"), Value: []byte("1")},
		}); err != nil || !ok {
			t.Fatalf("root: %v %v", ok, err)
		}
		for i := uint32(0); i < 2; i++ {
			ch := domain.Chunk{Replicas: []string{"a", "b"}, ISR: []string{"a", "b"}, ISRVer: 1}
			pb, _ := encodeRecord(ch)
			if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, i), Value: pb}}); err != nil || !ok {
				t.Fatal(err)
			}
		}
		h := domain.Header{
			ID: "hid", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, CreateOperation: op, CreateSpec: &cs,
		}
		hb, _ := encodeRecord(h)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("hid"), Rev: 0}}, []kv.Op{{Key: volumeKey("hid"), Value: hb}}); err != nil || !ok {
			t.Fatalf("header: %v %v", ok, err)
		}
		if _, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "hid"}); code(err) != codes.NotFound {
			t.Fatalf("get creating: %v", err)
		}
		list, err := s.ListVolumes(ctx, &metav1.ListVolumesRequest{})
		if err != nil {
			t.Fatal(err)
		}
		if slices.Contains(list.GetIds(), "hid") {
			t.Fatal("creating volume listed")
		}
		if _, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "hid", Node: "a"}); code(err) != codes.NotFound {
			t.Fatalf("attach creating: %v", err)
		}
	})
}

// docs/issues/76 S3–S5: an identical helper resumes an admitted Creating claim through to visibility.
func TestCreateHelperResumesAdmittedPlan(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		cs := domain.CreateSpec{SizeBytes: 3 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1}
		op := "dddddddddddddddddddddddddddddddd"
		root := createRoot{
			Schema: createRootSchema, Volume: "help", Spec: cs,
			State: rootCopying, ChunkCount: 3, Staged: 3, Next: 1,
		}
		rb, _ := encodeRecord(root)
		if rev, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: 0}}, []kv.Op{
			{Key: createRootKey(op), Value: rb},
			{Key: createBirthKey("help"), Value: []byte("1")},
		}); err != nil || !ok {
			t.Fatalf("root: %v %v", ok, err)
		} else {
			_ = rev
		}
		for i := uint32(0); i < 3; i++ {
			ch := domain.Chunk{Replicas: []string{"a", "b"}, ISR: []string{"a", "b"}, ISRVer: 1}
			pb, _ := encodeRecord(ch)
			if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, i), Value: pb}}); err != nil || !ok {
				t.Fatal(err)
			}
		}
		// One public chunk already copied; helpers must not reset it.
		ch0 := domain.Chunk{Replicas: []string{"a", "b"}, ISR: []string{"a", "b"}, ISRVer: 1}
		cb, _ := encodeRecord(ch0)
		if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: chunkKey("help", 0), Value: cb}}); err != nil || !ok {
			t.Fatal(err)
		}
		h := domain.Header{
			ID: "help", SizeBytes: 3 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, CreateOperation: op, CreateSpec: &cs,
		}
		hb, _ := encodeRecord(h)
		claim, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("help"), Rev: 0}}, []kv.Op{{Key: volumeKey("help"), Value: hb}})
		if err != nil || !ok {
			t.Fatalf("admit: %v %v", ok, err)
		}
		v, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: "help", SizeBytes: 3 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1,
		})
		if err != nil {
			t.Fatal(err)
		}
		if v.GetVolume().GetEpoch() != uint64(claim) || len(v.GetVolume().GetChunks()) != 3 {
			t.Fatalf("resumed %+v, want epoch=%d chunks=3", v.GetVolume(), claim)
		}
		h2 := header(t, store, "help")
		if h2.Creating || h2.FirstEpoch != uint64(claim) || h2.CreateOperation != op {
			t.Fatalf("final header %+v", h2)
		}
		again, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: "help", SizeBytes: 3 * mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1,
		})
		if err != nil || again.GetVolume().GetEpoch() != uint64(claim) {
			t.Fatalf("repeat: %v epoch %d", err, again.GetVolume().GetEpoch())
		}
	})
}

// docs/issues/76 S1/S7: birth compare prevents root resurrection after GC (Go CAS on both stores).
func TestCreateBirthFenceBlocksResurrection(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		op := "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		root := createRoot{
			Schema: createRootSchema, Volume: "birth", Spec: domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootPreparing, ChunkCount: 1,
		}
		rev, err := s.createInitialRoot(ctx, op, root)
		if err != nil || rev == 0 {
			t.Fatalf("initial root: rev=%d err=%v", rev, err)
		}
		birth := createBirthKey("birth")
		be, bok, err := store.Get(ctx, birth)
		if err != nil || !bok || be.Rev == 0 {
			t.Fatalf("birth: %+v %v %v", be, bok, err)
		}
		ok, err := store.Txn(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: rev}}, []kv.Op{{Key: createRootKey(op), Delete: true}})
		if err != nil || !ok {
			t.Fatalf("gc root: %v %v", ok, err)
		}
		rb, _ := encodeRecord(root)
		// Replay with stale birth_rev=0 (backend retry after GC): must not resurrect.
		ok, err = store.Txn(ctx, []kv.Cmp{
			{Key: createRootKey(op), Rev: 0},
			{Key: birth, Rev: 0},
		}, []kv.Op{
			{Key: createRootKey(op), Value: rb},
			{Key: birth, Value: []byte("1")},
		})
		if err != nil || ok {
			t.Fatalf("resurrection with birth_rev=0 applied: ok=%v err=%v", ok, err)
		}
		if _, found, _ := store.Get(ctx, createRootKey(op)); found {
			t.Fatal("root resurrected")
		}
		// Mutant missing_birth_compare: without the birth fence, resurrection would succeed.
		ok, err = store.Txn(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: 0}}, []kv.Op{{Key: createRootKey(op), Value: rb}})
		if err != nil || !ok {
			t.Fatalf("mutant without birth compare should apply: %v %v", ok, err)
		}
		if _, found, _ := store.Get(ctx, createRootKey(op)); !found {
			t.Fatal("mutant counterexample missing")
		}
	})
}

// docs/issues/76 S6/S8: Creating+Deleting resumes as unpublished deletion; no tombstone.
func TestCreatingAndDeletingIsUnpublishedRollback(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		h := domain.Header{
			ID: "cd", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, Deleting: true, Epoch: 0,
		}
		b, _ := encodeRecord(h)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("cd"), Rev: 0}}, []kv.Op{{Key: volumeKey("cd"), Value: b}}); err != nil || !ok {
			t.Fatalf("seed: %v %v", ok, err)
		}
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "cd"}); err != nil {
			t.Fatal(err)
		}
		if _, ok, _ := store.Get(ctx, volumeKey("cd")); ok {
			t.Fatal("header still present")
		}
		if tombstoned(t, s, "a", "cd") || tombstoned(t, s, "b", "cd") {
			t.Fatal("unpublished rollback wrote a tombstone")
		}
		// Create of a Deleting unpublished shape while still present would Aborted; after rollback, fresh admit.
		if _, err := create(s, "cd", 1, 2, 1); err != nil {
			t.Fatal(err)
		}
	})
}

// docs/issues/76 S8: nonzero epoch / target shapes are not unpublished-eligible.
func TestUnpublishedRollbackRejectsUnsupportedShapes(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		for _, tc := range []domain.Header{
			{ID: "ep", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1, Creating: true, Epoch: 5},
			{ID: "tg", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1, Creating: true, Target: "a"},
		} {
			b, _ := json.Marshal(tc)
			if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey(tc.ID), Rev: 0}}, []kv.Op{{Key: volumeKey(tc.ID), Value: b}}); err != nil || !ok {
				t.Fatalf("%s seed: %v %v", tc.ID, ok, err)
			}
			// Conservative deletion path (tombstone), not proof-based rollback alone.
			if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: tc.ID}); err != nil {
				t.Fatalf("%s delete: %v", tc.ID, err)
			}
			if _, ok, _ := store.Get(ctx, volumeKey(tc.ID)); ok {
				t.Fatalf("%s header remains", tc.ID)
			}
		}
	})
}

// docs/issues/76 S6: a stale helper comparing the old claim cannot publish into a replacement.
func TestStaleCreateHelperCannotTouchReplacement(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		cs := domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1}
		op := "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
		root := createRoot{Schema: createRootSchema, Volume: "stale", Spec: cs, State: rootCopying, ChunkCount: 1, Staged: 1, Next: 0}
		rb, _ := encodeRecord(root)
		if _, ok, err := store.TxnRev(ctx, nil, []kv.Op{{Key: createRootKey(op), Value: rb}}); err != nil || !ok {
			t.Fatal(err)
		}
		ch := domain.Chunk{Replicas: []string{"a", "b"}, ISR: []string{"a", "b"}, ISRVer: 1}
		pb, _ := encodeRecord(ch)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, 0), Value: pb}}); err != nil {
			t.Fatal(err)
		}
		h := domain.Header{ID: "stale", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1, Creating: true, CreateOperation: op, CreateSpec: &cs}
		hb, _ := encodeRecord(h)
		claim, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("stale"), Rev: 0}}, []kv.Op{{Key: volumeKey("stale"), Value: hb}})
		if err != nil || !ok {
			t.Fatal(err)
		}
		rootE, _, _ := store.Get(ctx, createRootKey(op))
		if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "stale"}); err != nil {
			t.Fatal(err)
		}
		v2, err := create(s, "stale", 1, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		if v2.GetEpoch() <= uint64(claim) {
			t.Fatalf("replacement epoch %d not above old claim %d", v2.GetEpoch(), claim)
		}
		// Stale helper transaction against the old claim/root must fail.
		ok, err = store.Txn(ctx, []kv.Cmp{
			{Key: volumeKey("stale"), Rev: claim},
			{Key: createRootKey(op), Rev: rootE.Rev},
		}, []kv.Op{{Key: chunkKey("stale", 0), Value: pb}})
		if err != nil || ok {
			t.Fatalf("stale helper write applied: ok=%v err=%v", ok, err)
		}
	})
}

// docs/issues/76 S8: compatible legacy Creating claim rolls back then freshly admits.
func TestLegacyCreatingCompatibleRetry(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		h := domain.Header{ID: "leg", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1, Creating: true}
		b, _ := json.Marshal(h)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("leg"), Rev: 0}}, []kv.Op{{Key: volumeKey("leg"), Value: b}}); err != nil || !ok {
			t.Fatal(err)
		}
		v, err := create(s, "leg", 1, 2, 1)
		if err != nil || v.GetEpoch() == 0 {
			t.Fatalf("legacy retry: %v %+v", err, v)
		}
		h2 := header(t, store, "leg")
		if h2.Creating || h2.CreateOperation == "" || h2.CreateSpec == nil {
			t.Fatalf("expected new-format header: %+v", h2)
		}
	})
}

// failOnceDeleteKV makes the first delete-only Txn report !ok without applying (docs/issues/76).
type failOnceDeleteKV struct {
	kv.KV
	failed bool
}

func (f *failOnceDeleteKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := f.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (f *failOnceDeleteKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	onlyDeletes := len(ops) > 0
	for _, o := range ops {
		if !o.Delete {
			onlyDeletes = false
			break
		}
	}
	if onlyDeletes && !f.failed {
		f.failed = true
		return 0, false, nil
	}
	return f.KV.TxnRev(ctx, cmps, ops)
}

// docs/issues/76: orphan chunk cleanup must not advance the scan cursor past a failed delete.
func TestCleanupOrphanChunksDoesNotSkipOnFailedDelete(t *testing.T) {
	ctx := context.Background()
	inner := kv.NewMem(nil)
	store := &failOnceDeleteKV{KV: inner}
	s := New(store, Options{})
	for i := 0; i < 3; i++ {
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: chunkKey("orph", uint32(i)), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.cleanupOrphanPublicChunks(ctx, "orph"); err != nil {
		t.Fatal(err)
	}
	left, err := inner.List(ctx, chunkPrefix("orph"))
	if err != nil || len(left) != 0 {
		t.Fatalf("leftover chunks after failed-then-retried delete: %d %v", len(left), err)
	}
}

// reappearHeaderKV inserts a volume header before the first header-absent chunk-delete txn.
type reappearHeaderKV struct {
	kv.KV
	id   string
	once bool
}

func (r *reappearHeaderKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := r.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (r *reappearHeaderKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	if !r.once {
		for _, c := range cmps {
			if c.Key == volumeKey(r.id) && c.Rev == 0 {
				r.once = true
				h := domain.Header{ID: r.id, SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1}
				b, _ := encodeRecord(h)
				if _, _, err := r.KV.TxnRev(ctx, nil, []kv.Op{{Key: volumeKey(r.id), Value: b}}); err != nil {
					return 0, false, err
				}
				break
			}
		}
	}
	return r.KV.TxnRev(ctx, cmps, ops)
}

// docs/issues/76: a recreated header during orphan cleanup returns Aborted with leftovers intact.
func TestCleanupOrphanChunksAbortedIfHeaderReappears(t *testing.T) {
	ctx := context.Background()
	inner := kv.NewMem(nil)
	store := &reappearHeaderKV{KV: inner, id: "race"}
	s := New(store, Options{})
	for i := 0; i < 2; i++ {
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: chunkKey("race", uint32(i)), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil {
			t.Fatal(err)
		}
	}
	err := s.cleanupOrphanPublicChunks(ctx, "race")
	if code(err) != codes.Aborted {
		t.Fatalf("cleanup: %v, want Aborted", err)
	}
	left, err := inner.List(ctx, chunkPrefix("race"))
	if err != nil || len(left) == 0 {
		t.Fatalf("expected leftovers under recreated header: %d %v", len(left), err)
	}
}

// deleteOnlyKV rejects any Put, simulating etcd NOSPACE (docs/issues/76 S7).
type deleteOnlyKV struct {
	kv.KV
}

func (d *deleteOnlyKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := d.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (d *deleteOnlyKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	for _, o := range ops {
		if !o.Delete {
			return 0, false, errNoSpace
		}
	}
	return d.KV.TxnRev(ctx, cmps, ops)
}

var errNoSpace = status.Error(codes.ResourceExhausted, "mvcc: database space exceeded")

// docs/issues/76 S7/S8: unpublished DeleteVolume and orphan plan GC stay delete-only under NOSPACE.
func TestUnpublishedRollbackAndOrphanGCUnderNoSpace(t *testing.T) {
	ctx := context.Background()
	inner := kv.NewMem(nil)
	store := &deleteOnlyKV{KV: inner}
	s := New(store, Options{})

	h := domain.Header{
		ID: "ns", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
		Creating: true, Epoch: 0,
	}
	hb, _ := encodeRecord(h)
	if _, ok, err := inner.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("ns"), Rev: 0}}, []kv.Op{
		{Key: volumeKey("ns"), Value: hb},
		{Key: chunkKey("ns", 0), Value: []byte(`{"replicas":["a","b"],"isr":["a","b"],"isr_ver":1}`)},
	}); err != nil || !ok {
		t.Fatal(err)
	}
	if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "ns"}); err != nil {
		t.Fatalf("unpublished rollback under NOSPACE: %v", err)
	}
	if _, ok, _ := inner.Get(ctx, volumeKey("ns")); ok {
		t.Fatal("header remained")
	}
	if chunks, _ := inner.List(ctx, chunkPrefix("ns")); len(chunks) != 0 {
		t.Fatalf("chunks remained: %d", len(chunks))
	}
	if tombstoned(t, s, "a", "ns") {
		t.Fatal("unpublished rollback wrote a tombstone under NOSPACE")
	}

	op := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	for i := 0; i < 3; i++ {
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, uint32(i)), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.RunCreateRecovery(ctx); err != nil {
		t.Fatalf("orphan plan GC under NOSPACE: %v", err)
	}
	page, err := inner.ScanKeys(ctx, createPlanOpPrefix(op), "", 32)
	if err != nil || len(page.Entries) != 0 {
		t.Fatalf("orphan plans remain under NOSPACE: %+v %v", page.Entries, err)
	}

	// Contrast: published deletion still needs a Deleting mark Put and must fail closed.
	pub := domain.Header{ID: "pub", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Epoch: 1}
	pb, _ := encodeRecord(pub)
	if _, ok, err := inner.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("pub"), Rev: 0}}, []kv.Op{{Key: volumeKey("pub"), Value: pb}}); err != nil || !ok {
		t.Fatal(err)
	}
	if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "pub"}); err == nil {
		t.Fatal("published DeleteVolume must not succeed when Puts are refused")
	}
}

// sealCaptureKV records sealed create-root payloads (docs/issues/76).
type sealCaptureKV struct {
	kv.KV
	sealed []createRoot
}

func (c *sealCaptureKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	_, ok, err := c.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (c *sealCaptureKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	rev, ok, err := c.KV.TxnRev(ctx, cmps, ops)
	if ok {
		for _, o := range ops {
			if o.Delete || !strings.HasPrefix(o.Key, createRootPrefix) {
				continue
			}
			r, derr := decodeRoot(o.Value)
			if derr == nil && r.State == rootSealed {
				c.sealed = append(c.sealed, r)
			}
		}
	}
	return rev, ok, err
}

// docs/issues/76: seal must persist the staged plan count, not a stale Staged:0.
func TestSealPersistsStagedPlanCount(t *testing.T) {
	store := &sealCaptureKV{KV: kv.NewMem(nil)}
	s := New(store, Options{})
	addNodes(t, s, "a", "b")
	if _, err := create(s, "staged", 3, 2, 1); err != nil {
		t.Fatal(err)
	}
	if len(store.sealed) == 0 {
		t.Fatal("no sealed root observed")
	}
	for _, r := range store.sealed {
		if r.Staged != uint32(r.ChunkCount) || r.ChunkCount != 3 {
			t.Fatalf("sealed root Staged=%d ChunkCount=%d, want both 3", r.Staged, r.ChunkCount)
		}
	}
}

// docs/issues/76: helpCreate fail-closed when CreateSpec is missing (no nil deref).
func TestHelpCreateNilCreateSpecDataLoss(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		s := New(store, Options{})
		op := "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		root := createRoot{
			Schema: createRootSchema, Volume: "nilspec", Spec: domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootCopying, ChunkCount: 1, Staged: 1,
		}
		rb, _ := encodeRecord(root)
		if _, ok, err := store.TxnRev(context.Background(), nil, []kv.Op{{Key: createRootKey(op), Value: rb}}); err != nil || !ok {
			t.Fatal(err)
		}
		h := domain.Header{ID: "nilspec", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Creating: true, CreateOperation: op}
		_, err := s.helpCreate(context.Background(), op, 1, h)
		if code(err) != codes.DataLoss {
			t.Fatalf("helpCreate nil CreateSpec: %v, want DataLoss", err)
		}
	})
}

// docs/issues/76 S8 (security): Attach/token requires publication; unpublished classifier refuses that shape.
func TestAttachTokenThenUnpublishedClassifierRefuses(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		_, key, err := ed25519.GenerateKey(nil)
		if err != nil {
			t.Fatal(err)
		}
		s := New(store, Options{TokenKey: key})
		addNodes(t, s, "a", "b")

		creating := domain.Header{
			ID: "tok", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, Epoch: 0,
		}
		if isUnpublishedCreating(creating) != true {
			t.Fatal("precondition: creating claim must be unpublished-eligible")
		}
		cb, _ := encodeRecord(creating)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("tok"), Rev: 0}}, []kv.Op{{Key: volumeKey("tok"), Value: cb}}); err != nil || !ok {
			t.Fatal(err)
		}
		att, err := s.Attach(ctx, &metav1.AttachRequest{Volume: "tok", Node: "a"})
		if code(err) != codes.NotFound || (att != nil && att.GetToken() != nil) {
			t.Fatalf("attach unpublished: %v token=%v", err, att.GetToken())
		}

		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey("tok"), Delete: true}}); err != nil {
			t.Fatal(err)
		}
		v, err := create(s, "tok", 1, 2, 1)
		if err != nil {
			t.Fatal(err)
		}
		att, err = s.Attach(ctx, &metav1.AttachRequest{Volume: "tok", Node: "a"})
		if err != nil || att.GetToken() == nil {
			t.Fatalf("attach published: %v token=%v", err, att.GetToken())
		}
		h := header(t, store, "tok")
		if isUnpublishedCreating(h) {
			t.Fatalf("published+attached header must not pass unpublished classifier: %+v", h)
		}
		if h.Creating || h.Epoch == 0 || h.Target == "" {
			t.Fatalf("attached header must be published with a target: %+v", h)
		}
		if h.FirstEpoch != v.GetEpoch() {
			t.Fatalf("FirstEpoch %d want create epoch %d", h.FirstEpoch, v.GetEpoch())
		}
		// Proof-based rollback must refuse this shape (conservative deletion owns it).
		if err := s.stepUnpublishedRollback(ctx, h, 1); code(err) != codes.FailedPrecondition {
			t.Fatalf("rollback published header: %v, want FailedPrecondition", err)
		}
	})
}
