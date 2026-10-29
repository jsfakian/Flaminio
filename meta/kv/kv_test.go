package kv_test

import (
	"context"
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

func TestEtcd(t *testing.T) {
	e := kvtest.Etcd(t)
	defer e.Close()
	testKV(t, e)
	time.Sleep(3500 * time.Millisecond)
	if _, found, _ := e.Get(context.Background(), "/ttl"); found {
		t.Fatal("ttl key did not expire")
	}
}
