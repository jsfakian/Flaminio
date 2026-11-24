package service

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
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

// countingKV instruments ScanKeys / GetBounded / Txn for large-inventory ceilings (docs/issues/76 S7).
type countingKV struct {
	kv.KV
	scanCalls, getCalls, txnCalls int
	scanKeys, scanBytes           int
	mutations                     int
}

func (c *countingKV) ScanKeys(ctx context.Context, prefix, after string, limit int) (kv.ScanPage, error) {
	c.scanCalls++
	page, err := c.KV.ScanKeys(ctx, prefix, after, limit)
	c.scanKeys += len(page.Entries)
	for _, e := range page.Entries {
		c.scanBytes += len(e.Key)
	}
	return page, err
}

func (c *countingKV) GetBounded(ctx context.Context, key string, maxBytes int) (kv.Entry, bool, error) {
	c.getCalls++
	return c.KV.GetBounded(ctx, key, maxBytes)
}

func (c *countingKV) Txn(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (bool, error) {
	c.txnCalls++
	if len(ops) > 0 {
		c.mutations++
	}
	return c.KV.Txn(ctx, cmps, ops)
}

func (c *countingKV) TxnRev(ctx context.Context, cmps []kv.Cmp, ops []kv.Op) (int64, bool, error) {
	c.txnCalls++
	if len(ops) > 0 {
		c.mutations++
	}
	return c.KV.TxnRev(ctx, cmps, ops)
}

func (c *countingKV) reset() {
	c.scanCalls, c.getCalls, c.txnCalls = 0, 0, 0
	c.scanKeys, c.scanBytes, c.mutations = 0, 0, 0
}

// docs/issues/76 S7: four sources each with ≥64 stable eligible keys; page tails stay bounded;
// every eligible key is eventually examined under the 64-item / 8-mutation budget.
func TestCreateRecoveryRespectsPageTailBudget(t *testing.T) {
	store := kv.NewMem(nil)
	ctx := context.Background()
	s := New(store, Options{})
	const n = recoveryPageSize
	chunkJSON := []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)

	for i := 0; i < n; i++ {
		// srcPlanOrphans: plans with no live root.
		planOp := fmt.Sprintf("p%031x", i)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createPlanKey(planOp, 0), Value: chunkJSON}}); err != nil {
			t.Fatal(err)
		}
		// srcChunkOrphans: public chunks with no header.
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: chunkKey(fmt.Sprintf("c%03d", i), 0), Value: chunkJSON}}); err != nil {
			t.Fatal(err)
		}
		// srcRoots: complete roots whose published header matches CreateOperation → GC-eligible.
		rootOp := fmt.Sprintf("r%031x", i)
		vol := fmt.Sprintf("rv%03d", i)
		root := createRoot{
			Schema: createRootSchema, Volume: vol,
			Spec:  domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootComplete, ChunkCount: 1, Staged: 1, Next: 1,
		}
		rb, _ := encodeRecord(root)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: createRootKey(rootOp), Value: rb}}); err != nil {
			t.Fatal(err)
		}
		rh := domain.Header{
			ID: vol, SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1,
			CreateOperation: rootOp, Epoch: 1,
		}
		rhb, _ := encodeRecord(rh)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey(vol), Value: rhb}}); err != nil {
			t.Fatal(err)
		}
		// srcHeaders: never-published Creating+Deleting → unpublished rollback.
		hid := fmt.Sprintf("uh%03d", i)
		uh := domain.Header{
			ID: hid, SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1,
			Creating: true, Deleting: true, Epoch: 0,
		}
		uhb, _ := encodeRecord(uh)
		if _, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey(hid), Value: uhb}}); err != nil {
			t.Fatal(err)
		}
	}

	sawPendingTail := false
	// Enough passes for 4×64 eligible items at ≤8 mutations/pass, rotating firstSrc.
	const maxPasses = (n*int(srcCount))/recoveryMaxMutations + n + int(srcCount)
	for pass := 0; pass < maxPasses; pass++ {
		s.recoveryFirstSrc = pass % int(srcCount)
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatal(err)
		}
		if s.recoveryCursors == nil {
			t.Fatal("cursors discarded")
		}
		for i, cur := range s.recoveryCursors {
			if len(cur.tail) > recoveryPageSize {
				t.Fatalf("pass %d source %d tail unbounded: %d", pass, i, len(cur.tail))
			}
			if len(cur.tail) == 0 {
				continue
			}
			sawPendingTail = true
			if cur.after == "" && !cur.fetched {
				t.Fatalf("pass %d source %d: unconsumed tail with cleared cursor", pass, i)
			}
			if cur.next != "" && cur.after == cur.next {
				t.Fatalf("pass %d source %d advanced after to fetched next while tail remains", pass, i)
			}
		}
	}
	if !sawPendingTail {
		t.Fatal("expected a retained unexamined page tail under the mutation budget")
	}
	for _, pfx := range []string{createPlanPrefix, "/flaminio/v1/chunks/", createRootPrefix} {
		left, err := store.ScanKeys(ctx, pfx, "", recoveryPageSize)
		if err != nil {
			t.Fatal(err)
		}
		if len(left.Entries) != 0 {
			t.Fatalf("%s leftovers after fairness passes: %d (sample %+v)", pfx, len(left.Entries), left.Entries[0])
		}
	}
	// Unpublished headers must be gone; published rv* headers remain (root GC only).
	for i := 0; i < n; i++ {
		if _, ok, _ := store.Get(ctx, volumeKey(fmt.Sprintf("uh%03d", i))); ok {
			t.Fatalf("unpublished header uh%03d not rolled back", i)
		}
		if _, ok, _ := store.Get(ctx, volumeKey(fmt.Sprintf("rv%03d", i))); !ok {
			t.Fatalf("published header rv%03d must remain after root GC", i)
		}
	}
}

// docs/issues/76 S7/S8: large plan-key inventory; one recovery pass stays within page/RPC/mutation ceilings.
func TestCreateRecoveryLargeInventoryBounds(t *testing.T) {
	const largeN = 1_000_000
	if testing.Short() {
		t.Skip("million-key inventory skipped under -short")
	}
	inner := kv.NewMem(nil)
	store := &countingKV{KV: inner}
	ctx := context.Background()
	s := New(store, Options{})

	chunkJSON := []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`)
	ops := make([]kv.Op, 0, kv.MaxTxnOps)
	flush := func() {
		t.Helper()
		if len(ops) == 0 {
			return
		}
		if ok, err := inner.Txn(ctx, nil, ops); err != nil || !ok {
			t.Fatalf("seed: %v %v", ok, err)
		}
		ops = ops[:0]
	}
	for i := 0; i < largeN; i++ {
		op := fmt.Sprintf("%032x", i)
		ops = append(ops, kv.Op{Key: createPlanKey(op, 0), Value: chunkJSON})
		if len(ops) >= kv.MaxTxnOps {
			flush()
		}
	}
	flush()
	// A handful of roots/headers so all four sources are populated above the page size.
	for i := 0; i < recoveryPageSize+1; i++ {
		op := fmt.Sprintf("root%028x", i)
		root := createRoot{
			Schema: createRootSchema, Volume: fmt.Sprintf("big%03d", i),
			Spec: domain.CreateSpec{SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1},
			State: rootPreparing, ChunkCount: 1,
		}
		rb, _ := encodeRecord(root)
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: createRootKey(op), Value: rb}}); err != nil {
			t.Fatal(err)
		}
		h := domain.Header{ID: fmt.Sprintf("bh%03d", i), SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Creating: true, Epoch: 0}
		hb, _ := encodeRecord(h)
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: volumeKey(h.ID), Value: hb}}); err != nil {
			t.Fatal(err)
		}
		if _, err := inner.Txn(ctx, nil, []kv.Op{{Key: chunkKey(fmt.Sprintf("bc%03d", i), 0), Value: chunkJSON}}); err != nil {
			t.Fatal(err)
		}
	}

	store.reset()
	s.recoveryFirstSrc = int(srcPlanOrphans)
	if err := s.RunCreateRecovery(ctx); err != nil {
		t.Fatal(err)
	}
	if store.scanKeys > recoveryMaxItems+recoveryPageSize*int(srcCount) {
		// A pass examines ≤recoveryMaxItems but may fetch one page per source; keys returned
		// across ScanKeys calls stay O(pages × page size), never the full million.
		t.Fatalf("scan returned too many keys in one pass: %d", store.scanKeys)
	}
	if store.scanCalls > recoveryMaxKVCalls || store.getCalls > recoveryMaxKVCalls || store.txnCalls > recoveryMaxKVCalls {
		t.Fatalf("KV call ceiling exceeded: scan=%d get=%d txn=%d (max %d)",
			store.scanCalls, store.getCalls, store.txnCalls, recoveryMaxKVCalls)
	}
	if store.mutations > recoveryMaxMutations {
		t.Fatalf("mutations %d > budget %d", store.mutations, recoveryMaxMutations)
	}
	if store.scanBytes > kv.ScanKeysMaxReplyBytes*store.scanCalls {
		t.Fatalf("scan reply bytes %d exceed per-call cap × calls", store.scanBytes)
	}
	// Inventory must still be largely intact after one bounded pass.
	left, err := inner.ScanKeys(ctx, createPlanPrefix, "", recoveryPageSize)
	if err != nil || len(left.Entries) == 0 {
		t.Fatalf("expected remaining plan orphans after one pass: %+v %v", left, err)
	}
}

// docs/issues/76 S6/S8 (security): recovery must not invoke public by-name DeleteVolume.
func TestCreateRecoveryDoesNotCallDeleteVolumeByName(t *testing.T) {
	src, err := os.ReadFile("create_recovery.go")
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(src, []byte("DeleteVolume")) {
		t.Fatal("create_recovery.go must not call by-name DeleteVolume")
	}

	backends(t, func(t *testing.T, store kv.KV) {
		ctx := context.Background()
		s := New(store, Options{})
		// Published Deleting shape: recovery must leave the conservative public path in charge.
		h := domain.Header{
			ID: "pubdel", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
			Deleting: true, Epoch: 7, Target: "a",
		}
		hb, _ := encodeRecord(h)
		if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("pubdel"), Rev: 0}}, []kv.Op{
			{Key: volumeKey("pubdel"), Value: hb},
			{Key: chunkKey("pubdel", 0), Value: []byte(`{"replicas":["a","b"],"isr":["a"],"isr_ver":1}`)},
		}); err != nil || !ok {
			t.Fatal(err)
		}
		for i := 0; i < 4; i++ {
			if err := s.RunCreateRecovery(ctx); err != nil {
				t.Fatal(err)
			}
		}
		he, ok, _ := store.Get(ctx, volumeKey("pubdel"))
		if !ok {
			t.Fatal("recovery removed a published Deleting header (by-name deletion substitution)")
		}
		var got domain.Header
		if err := json.Unmarshal(he.Value, &got); err != nil || !got.Deleting || got.Epoch != 7 {
			t.Fatalf("published deleting header changed: %+v %v", got, err)
		}
		if chunks, err := store.List(ctx, chunkPrefix("pubdel")); err != nil || len(chunks) != 1 {
			t.Fatalf("recovery must not deleteChunks a published deletion: %d %v", len(chunks), err)
		}
		if tombstoned(t, s, "a", "pubdel") {
			t.Fatal("recovery wrote a tombstone for published Deleting")
		}
	})
}
