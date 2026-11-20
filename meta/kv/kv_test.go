package kv_test

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/kv/kvtest"
)

func testKV(t *testing.T, s kv.KV) {
	ctx := context.Background()
	ok, err := s.Txn(ctx, []kv.Cmp{{Key: "/a", Rev: 0}}, []kv.Op{{Key: "/a", Value: []byte("1")}, {Key: "/b/x", Value: []byte("2")}})
	if err != nil || !ok {
		t.Fatalf("create: %v %v", ok, err)
	}
	a, found, err := s.Get(ctx, "/a")
	if err != nil || !found || string(a.Value) != "1" {
		t.Fatalf("get: %+v %v %v", a, found, err)
	}
	// A compare on a stale revision fails and changes nothing.
	ok, err = s.Txn(ctx, []kv.Cmp{{Key: "/a", Rev: a.Rev + 100}}, []kv.Op{{Key: "/a", Value: []byte("bad")}})
	if err != nil || ok {
		t.Fatalf("stale compare applied: %v %v", ok, err)
	}
	ok, err = s.Txn(ctx, []kv.Cmp{{Key: "/a", Rev: 0}}, []kv.Op{{Key: "/a", Value: []byte("bad")}})
	if err != nil || ok {
		t.Fatalf("create-if-absent applied over an existing key: %v %v", ok, err)
	}
	ok, err = s.Txn(ctx, []kv.Cmp{{Key: "/a", Rev: a.Rev}}, []kv.Op{{Key: "/a", Delete: true}, {Key: "/b/y", Value: []byte("3")}})
	if err != nil || !ok {
		t.Fatalf("update: %v %v", ok, err)
	}
	if _, found, _ := s.Get(ctx, "/a"); found {
		t.Fatal("deleted key still there")
	}
	l, err := s.List(ctx, "/b/")
	if err != nil || len(l) != 2 || l[0].Key != "/b/x" || l[1].Key != "/b/y" {
		t.Fatalf("list: %+v %v", l, err)
	}
	if err := s.PutTTL(ctx, "/ttl", []byte("x"), 2*time.Second); err != nil {
		t.Fatal(err)
	}
	if err := s.PutTTL(ctx, "/ttl", []byte("y"), 2*time.Second); err != nil {
		t.Fatal(err)
	}
	if e, found, _ := s.Get(ctx, "/ttl"); !found || string(e.Value) != "y" {
		t.Fatalf("ttl key: %+v %v", e, found)
	}
	// TxnRev gives the revision its writes got: a compare at it holds with no read between.
	rev, ok, err := s.TxnRev(ctx, nil, []kv.Op{{Key: "/r", Value: []byte("1")}, {Key: "/r2", Value: []byte("2")}})
	if err != nil || !ok || rev == 0 {
		t.Fatalf("txnrev: %d %v %v", rev, ok, err)
	}
	for _, k := range []string{"/r", "/r2"} {
		if e, _, _ := s.Get(ctx, k); e.Rev != rev {
			t.Fatalf("%s at revision %d, TxnRev said %d", k, e.Rev, rev)
		}
	}
	if ok, err := s.Txn(ctx, []kv.Cmp{{Key: "/r", Rev: rev}}, []kv.Op{{Key: "/r", Delete: true}}); err != nil || !ok {
		t.Fatalf("compare at the revision TxnRev gave: %v %v", ok, err)
	}
	if rev, ok, err := s.TxnRev(ctx, []kv.Cmp{{Key: "/r2", Rev: 1 << 40}}, nil); err != nil || ok || rev != 0 {
		t.Fatalf("txnrev on a failed compare: %d %v %v", rev, ok, err)
	}
}

func TestMem(t *testing.T) {
	now := time.Unix(0, 0)
	m := kv.NewMem(func() time.Time { return now })
	testKV(t, m)
	now = now.Add(3 * time.Second)
	if _, found, _ := m.Get(context.Background(), "/ttl"); found {
		t.Fatal("ttl key did not expire")
	}
}

// TestMemRevisionStartsAtClock checks that a restarted in-memory store starts above the one
// before it (docs/issues/30, S2).
func TestMemRevisionStartsAtClock(t *testing.T) {
	ctx := context.Background()
	now := time.Unix(1_700_000_000, 0)
	m := kv.NewMem(func() time.Time { return now })
	rev, ok, err := m.TxnRev(ctx, nil, []kv.Op{{Key: "/a", Value: []byte("1")}})
	if err != nil || !ok || rev <= now.UnixMicro() {
		t.Fatalf("first revision %d %v %v, want above %d", rev, ok, err, now.UnixMicro())
	}
	for range 100 {
		if rev, _, err = m.TxnRev(ctx, nil, []kv.Op{{Key: "/a", Value: []byte("2")}}); err != nil {
			t.Fatal(err)
		}
	}
	now = now.Add(time.Second)
	m2 := kv.NewMem(func() time.Time { return now })
	rev2, ok, err := m2.TxnRev(ctx, nil, []kv.Op{{Key: "/a", Value: []byte("1")}})
	if err != nil || !ok || rev2 <= rev {
		t.Fatalf("restarted store at revision %d %v %v, want above %d", rev2, ok, err, rev)
	}
}

func TestEtcd(t *testing.T) {
	e := kvtest.Etcd(t)
	defer e.Close()
	testKV(t, e)
	time.Sleep(3500 * time.Millisecond)
	if _, found, _ := e.Get(context.Background(), "/ttl"); found {
		t.Fatal("ttl key did not expire")
	}
}

func backends(t *testing.T, f func(t *testing.T, s kv.KV)) {
	t.Run("mem", func(t *testing.T) { f(t, kv.NewMem(nil)) })
	t.Run("etcd", func(t *testing.T) {
		e := kvtest.Etcd(t)
		defer e.Close()
		f(t, e)
	})
}

// ScanKeys pages keys only, continues after, and rejects out-of-range limits (docs/issues/76 S7).
func TestScanKeys(t *testing.T) {
	backends(t, func(t *testing.T, s kv.KV) {
		ctx := context.Background()
		ops := make([]kv.Op, 0, 10)
		for i := 0; i < 10; i++ {
			ops = append(ops, kv.Op{Key: fmt.Sprintf("/scan/k%02d", i), Value: []byte("v")})
		}
		if ok, err := s.Txn(ctx, nil, ops); err != nil || !ok {
			t.Fatalf("seed: %v %v", ok, err)
		}
		page, err := s.ScanKeys(ctx, "/scan/", "", 4)
		if err != nil || len(page.Entries) != 4 || page.Done || page.Next == "" {
			t.Fatalf("first page: %+v %v", page, err)
		}
		if page.Entries[0].Key != "/scan/k00" || page.Entries[3].Key != "/scan/k03" {
			t.Fatalf("order: %+v", page.Entries)
		}
		page2, err := s.ScanKeys(ctx, "/scan/", page.Next, 4)
		if err != nil || len(page2.Entries) != 4 || page2.Entries[0].Key != "/scan/k04" {
			t.Fatalf("second page: %+v %v", page2, err)
		}
		page3, err := s.ScanKeys(ctx, "/scan/", page2.Next, 4)
		if err != nil || len(page3.Entries) != 2 || !page3.Done {
			t.Fatalf("tail: %+v %v", page3, err)
		}
		if _, err := s.ScanKeys(ctx, "/scan/", "", 0); err == nil {
			t.Fatal("limit 0 accepted")
		}
		if _, err := s.ScanKeys(ctx, "/scan/", "", 65); err == nil {
			t.Fatal("limit 65 accepted")
		}
		empty, err := s.ScanKeys(ctx, "/missing/", "", 8)
		if err != nil || len(empty.Entries) != 0 || !empty.Done {
			t.Fatalf("empty: %+v %v", empty, err)
		}
	})
}

// GetBounded refuses oversized values and respects a work budget (docs/issues/76 S1/S7).
func TestGetBounded(t *testing.T) {
	backends(t, func(t *testing.T, s kv.KV) {
		ctx := context.Background()
		if ok, err := s.Txn(ctx, nil, []kv.Op{
			{Key: "/b/ok", Value: []byte("small")},
			{Key: "/b/big", Value: []byte(strings.Repeat("x", 100))},
		}); err != nil || !ok {
			t.Fatalf("seed: %v %v", ok, err)
		}
		e, ok, err := s.GetBounded(ctx, "/b/ok", 16)
		if err != nil || !ok || string(e.Value) != "small" {
			t.Fatalf("ok: %+v %v %v", e, ok, err)
		}
		_, ok, err = s.GetBounded(ctx, "/b/big", 16)
		if ok || !errors.Is(err, kv.ErrOversized) {
			t.Fatalf("oversized: ok=%v err=%v", ok, err)
		}
		_, ok, err = s.GetBounded(ctx, "/b/missing", 16)
		if err != nil || ok {
			t.Fatalf("missing: %v %v", ok, err)
		}
		budget := kv.WithWorkBudget(ctx, 1)
		if _, _, err := s.GetBounded(budget, "/b/ok", 16); err != nil {
			t.Fatal(err)
		}
		if _, _, err := s.GetBounded(budget, "/b/ok", 16); !errors.Is(err, kv.ErrWorkBudget) {
			t.Fatalf("budget: %v", err)
		}
	})
}
