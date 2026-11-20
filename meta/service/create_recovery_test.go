package service

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"

	"google.golang.org/grpc/codes"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// docs/issues/76 S7: recovery finishes an admitted Creating claim without a client.
func TestCreateRecoveryResumesAdmittedCreate(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		cs := domain.CreateSpec{SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1}
		op := "ffffffffffffffffffffffffffffffff"
		root := createRoot{
			Schema: createRootSchema, Volume: "rec", Spec: cs,
			State: rootCopying, ChunkCount: 2, Staged: 2, Next: 0,
		}
		rb, _ := encodeRecord(root)
		if _, ok, err := store.TxnRev(ctx, nil, []kv.Op{
			{Key: createRootKey(op), Value: rb},
			{Key: createBirthKey("rec"), Value: []byte("1")},
		}); err != nil || !ok {
			t.Fatal(err)
		}
		for i := uint32(0); i < 2; i++ {
			ch := domain.Chunk{Replicas: []string{"a", "b"}, ISR: []string{"a", "b"}, ISRVer: 1}
			pb, _ := encodeRecord(ch)
			if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, i), Value: pb}}); err != nil {
				t.Fatal(err)
			}
		}
		h := domain.Header{
			ID: "rec", SizeBytes: 2 * mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, CreateOperation: op, CreateSpec: &cs,
		}
		hb, _ := encodeRecord(h)
		claim, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("rec"), Rev: 0}}, []kv.Op{{Key: volumeKey("rec"), Value: hb}})
		if err != nil || !ok {
			t.Fatal(err)
		}
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
		v, err := s.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "rec"})
		if err != nil || v.GetVolume().GetEpoch() != uint64(claim) || len(v.GetVolume().GetChunks()) != 2 {
			t.Fatalf("recovered: %v %+v", err, v.GetVolume())
		}
	})
}

// docs/issues/76 S7: orphan plan keys are delete-only GC'd when the root is absent.
func TestCreateRecoveryGCsOrphanPlans(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		op := "11111111111111111111111111111111"
		for i := 0; i < 5; i++ {
			if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, uint32(i)), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil || !ok {
				t.Fatal(err)
			}
		}
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
		page, err := store.ScanKeys(ctx, createPlanOpPrefix(op), "", 32)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Entries) != 0 {
			t.Fatalf("orphan plans remain: %+v", page.Entries)
		}
	})
}

// docs/issues/76 S7: preparing roots without a header need two observations before GC.
func TestCreateRecoveryGCsAbandonedPreparing(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		op := "22222222222222222222222222222222"
		root := createRoot{
			Schema: createRootSchema, Volume: "prep", Spec: domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootPreparing, ChunkCount: 1,
		}
		rb, _ := encodeRecord(root)
		if _, ok, err := store.TxnRev(ctx, nil, []kv.Op{{Key: createRootKey(op), Value: rb}}); err != nil || !ok {
			t.Fatal(err)
		}
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
		if _, ok, _ := store.Get(ctx, createRootKey(op)); !ok {
			t.Fatal("preparing root GC'd on first observation")
		}
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
		if _, ok, _ := store.Get(ctx, createRootKey(op)); ok {
			t.Fatal("preparing root not GC'd after second observation")
		}
	})
}

// docs/issues/76 S7: a Creating header whose admitted root is gone fails closed; never invents a plan.
func TestCreateMissingRootFailsClosed(t *testing.T) {
	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		addNodes(t, s, "a", "b")
		cs := domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1}
		h := domain.Header{
			ID: "miss", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Creating: true, CreateOperation: "33333333333333333333333333333333", CreateSpec: &cs,
		}
		hb, _ := encodeRecord(h)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("miss"), Rev: 0}}, []kv.Op{{Key: volumeKey("miss"), Value: hb}}); err != nil || !ok {
			t.Fatal(err)
		}
		_, err := s.CreateVolume(ctx, &metav1.CreateVolumeRequest{Id: "miss", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1})
		if err == nil {
			t.Fatal("create succeeded despite missing root")
		}
		if c := code(err); c != codes.DataLoss && c != codes.Aborted {
			t.Fatalf("missing root: %v (code %v), want DataLoss or Aborted", err, c)
		}
		he, ok, _ := store.Get(ctx, volumeKey("miss"))
		if !ok {
			t.Fatal("header was removed")
		}
		var got domain.Header
		if err := json.Unmarshal(he.Value, &got); err != nil || !got.Creating || got.CreateOperation != h.CreateOperation {
			t.Fatalf("header changed: %+v %v", got, err)
		}
	})
}

// docs/issues/76 S7: four populated sources keep page tails bounded; wrap only after the tail is examined.
func TestCreateRecoveryRespectsPageTailBudget(t *testing.T) {
	store := kv.NewMem(nil)
	ctx := context.Background()
	s := New(store, Options{})
	for i := 0; i < recoveryPageSize; i++ {
		op := fmt.Sprintf("%032x", i)
		root := createRoot{
			Schema: createRootSchema, Volume: fmt.Sprintf("v%03d", i),
			Spec: domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootComplete, ChunkCount: 1, Staged: 1, Next: 1,
		}
		rb, _ := encodeRecord(root)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createRootKey(op), Value: rb}}); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(op, 0), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil {
			t.Fatal(err)
		}
		h := domain.Header{ID: fmt.Sprintf("h%03d", i), SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1}
		hb, _ := encodeRecord(h)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey(h.ID), Value: hb}}); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: chunkKey(fmt.Sprintf("c%03d", i), 0), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)}}); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.RunCreateRecovery(ctx); err != nil {
		t.Fatal(err)
	}
	if s.recoveryCursors == nil {
		t.Fatal("cursors discarded")
	}
	for i, cur := range s.recoveryCursors {
		if len(cur.tail) > recoveryPageSize {
			t.Fatalf("source %d tail unbounded: %d", i, len(cur.tail))
		}
		if len(cur.tail) > 0 && cur.after == "" && !cur.fetched {
			t.Fatalf("source %d: unconsumed tail with cleared cursor", i)
		}
	}
	for pass := 0; pass < 16; pass++ {
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
	}
	for i, cur := range s.recoveryCursors {
		if len(cur.tail) > recoveryPageSize {
			t.Fatalf("source %d tail grew: %d", i, len(cur.tail))
		}
	}
}
