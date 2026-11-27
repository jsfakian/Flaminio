package service

import (
	"context"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/kv/kvtest"
)

// docs/issues/76 S7: embedded etcd NOSPACE rejects Put-containing transactions while
// delete-only unpublished rollback and orphan plan GC still apply.
func TestEmbeddedEtcdNoSpaceDeleteOnlyGC(t *testing.T) {
	cl := kvtest.Start(t)
	defer cl.KV.Close()
	ctx := context.Background()
	store := cl.KV
	s := New(store, Options{})

	h := domain.Header{
		ID: "ens", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinISR: 1,
		Creating: true, Epoch: 0,
	}
	hb, _ := encodeRecord(h)
	if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("ens"), Rev: 0}}, []kv.Op{
		{Key: volumeKey("ens"), Value: hb},
		{Key: chunkKey("ens", 0), Value: []byte(`{"replicas":["a","b"],"isr":["a","b"],"isr_ver":1}`)},
	}); err != nil || !ok {
		t.Fatal(err)
	}
	op := "cccccccccccccccccccccccccccccccc"
	for i := 0; i < 3; i++ {
		if _, err := store.Txn(ctx, nil, []kv.Op{{
			Key: createPlanKey(op, uint32(i)), Value: []byte(`{"replicas":["a"],"isr":["a"],"isr_ver":1}`),
		}}); err != nil {
			t.Fatal(err)
		}
	}
	// Published header seeded before the alarm: DeleteVolume needs a Deleting mark Put.
	pub := domain.Header{ID: "epub", SizeBytes: mib, ChunkBytes: mib, Replicas: 1, MinISR: 1, Epoch: 1}
	pb, _ := encodeRecord(pub)
	if _, ok, err := store.TxnRev(ctx, []kv.Cmp{{Key: volumeKey("epub"), Rev: 0}}, []kv.Op{
		{Key: volumeKey("epub"), Value: pb},
	}); err != nil || !ok {
		t.Fatal(err)
	}

	cl.ActivateNoSpace(t)

	_, err := store.Txn(ctx, nil, []kv.Op{{Key: "/flaminio/v1/probe", Value: []byte("x")}})
	if err == nil {
		t.Fatal("Put txn succeeded under NOSPACE")
	}
	if status.Code(err) != codes.ResourceExhausted &&
		!strings.Contains(strings.ToLower(err.Error()), "space") {
		t.Fatalf("Put under NOSPACE: %v, want ResourceExhausted/no-space", err)
	}

	if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "ens"}); err != nil {
		t.Fatalf("unpublished rollback under real NOSPACE: %v", err)
	}
	if _, ok, _ := store.Get(ctx, volumeKey("ens")); ok {
		t.Fatal("header remained under NOSPACE")
	}
	if chunks, _ := store.List(ctx, chunkPrefix("ens")); len(chunks) != 0 {
		t.Fatalf("chunks remained under NOSPACE: %d", len(chunks))
	}

	for pass := 0; pass < 8; pass++ {
		if err := s.RunCreateRecovery(ctx); err != nil {
			t.Fatalf("recovery pass %d under NOSPACE: %v", pass, err)
		}
	}
	page, err := store.ScanKeys(ctx, createPlanOpPrefix(op), "", 32)
	if err != nil || len(page.Entries) != 0 {
		t.Fatalf("orphan plans remain under NOSPACE: %+v %v", page.Entries, err)
	}

	if _, err := s.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: "epub"}); err == nil {
		t.Fatal("published DeleteVolume must fail when Puts are refused under NOSPACE")
	}
	if _, ok, _ := store.Get(ctx, volumeKey("epub")); !ok {
		t.Fatal("published header must remain when mark Put is refused")
	}
}
