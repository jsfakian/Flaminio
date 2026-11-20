package service

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"

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
