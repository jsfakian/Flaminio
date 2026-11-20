package kv

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"

	clientv3 "go.etcd.io/etcd/client/v3"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Each request to etcd gets AttemptTimeout, and is tried again while the caller's context
// lasts, up to attempts times. A request sent on the connection to a member cut off by a
// partition, which drops packets, waits on a connection that still looks open; the client's
// keepalives (see flaminio-meta) take that connection out of rotation, and meanwhile the next
// attempt goes to another member. A transaction whose commit was not seen and is tried again
// fails its compare: callers already treat that as contention, and read again.
const (
	AttemptTimeout = 3 * time.Second
	attempts       = 4
)

func retry[T any](ctx context.Context, f func(context.Context) (T, error)) (T, error) {
	var (
		v   T
		err error
	)
	for range attempts {
		if err := charge(ctx); err != nil {
			return v, err
		}
		actx, cancel := context.WithTimeout(ctx, AttemptTimeout)
		v, err = f(actx)
		cancel()
		if err == nil || ctx.Err() != nil {
			return v, err
		}
		if c := status.Code(err); !errors.Is(err, context.DeadlineExceeded) && c != codes.DeadlineExceeded && c != codes.Unavailable {
			return v, err
		}
	}
	return v, err
}

// Etcd is the KV on a dedicated etcd cluster.
type Etcd struct {
	c *clientv3.Client
	// One lease per TTL key, renewed by PutTTL.
	mu     sync.Mutex
	leases map[string]clientv3.LeaseID
}

// NewEtcd wraps a connected client. The KV owns it from now on.
func NewEtcd(c *clientv3.Client) *Etcd {
	return &Etcd{c: c, leases: map[string]clientv3.LeaseID{}}
}

func (e *Etcd) Get(ctx context.Context, key string) (Entry, bool, error) {
	r, err := retry(ctx, func(ctx context.Context) (*clientv3.GetResponse, error) { return e.c.Get(ctx, key) })
	if err != nil {
		return Entry{}, false, err
	}
	if len(r.Kvs) == 0 {
		return Entry{}, false, nil
	}
	kv := r.Kvs[0]
	return Entry{Key: string(kv.Key), Value: kv.Value, Rev: kv.ModRevision}, true, nil
}

func (e *Etcd) List(ctx context.Context, prefix string) ([]Entry, error) {
	r, err := retry(ctx, func(ctx context.Context) (*clientv3.GetResponse, error) {
		return e.c.Get(ctx, prefix, clientv3.WithPrefix(), clientv3.WithSort(clientv3.SortByKey, clientv3.SortAscend))
	})
	if err != nil {
		return nil, err
	}
	out := make([]Entry, len(r.Kvs))
	for i, kv := range r.Kvs {
		out[i] = Entry{Key: string(kv.Key), Value: kv.Value, Rev: kv.ModRevision}
	}
	return out, nil
}

// prefixEnd is the etcd exclusive end key for a prefix range.
func prefixEnd(prefix string) string {
	return string(clientv3.GetPrefixRangeEnd(prefix))
}

func (e *Etcd) ScanKeys(ctx context.Context, prefix, after string, limit int) (ScanPage, error) {
	if err := checkScanLimit(limit); err != nil {
		return ScanPage{}, err
	}
	start := prefix
	if after != "" {
		// Exclusive after: start at after+NUL so the next key is strictly greater.
		start = after + "\x00"
		if start < prefix {
			start = prefix
		}
	}
	end := prefixEnd(prefix)
	// Keys-only + limit keeps the reply under ScanKeysMaxReplyBytes for valid service keys;
	// oversize keys fail closed below. Linearizable Get (default), not serializable.
	r, err := retry(ctx, func(ctx context.Context) (*clientv3.GetResponse, error) {
		return e.c.Get(ctx, start,
			clientv3.WithRange(end),
			clientv3.WithLimit(int64(limit)),
			clientv3.WithKeysOnly(),
		)
	})
	if err != nil {
		return ScanPage{}, err
	}
	page := ScanPage{Entries: make([]KeyRev, 0, len(r.Kvs)), Done: !r.More}
	reply := 0
	for _, kv := range r.Kvs {
		k := string(kv.Key)
		if len(k) > ScanKeysMaxKeyBytes {
			return ScanPage{}, fmt.Errorf("key exceeds %d bytes", ScanKeysMaxKeyBytes)
		}
		cost := len(k) + 8
		if reply+cost > ScanKeysMaxReplyBytes {
			page.Done = false
			break
		}
		page.Entries = append(page.Entries, KeyRev{Key: k, Rev: kv.ModRevision})
		reply += cost
	}
	if len(page.Entries) > 0 {
		page.Next = page.Entries[len(page.Entries)-1].Key
	}
	if len(page.Entries) == 0 {
		page.Done = true
	}
	return page, nil
}

func (e *Etcd) GetBounded(ctx context.Context, key string, maxBytes int) (Entry, bool, error) {
	if maxBytes < 0 {
		return Entry{}, false, fmt.Errorf("maxBytes %d", maxBytes)
	}
	// Exact-key Range; reject before JSON decode. Per-RPC receive caps are enforced by the
	// client dial options in production; the value bound is checked here (docs/issues/76).
	_ = GetBoundedRecvCap
	r, err := retry(ctx, func(ctx context.Context) (*clientv3.GetResponse, error) {
		return e.c.Get(ctx, key)
	})
	if err != nil {
		return Entry{}, false, err
	}
	if len(r.Kvs) == 0 {
		return Entry{}, false, nil
	}
	kv := r.Kvs[0]
	if len(kv.Value) > maxBytes {
		return Entry{}, false, fmt.Errorf("%w: key %q has %d bytes, max %d", ErrOversized, key, len(kv.Value), maxBytes)
	}
	return Entry{Key: string(kv.Key), Value: kv.Value, Rev: kv.ModRevision}, true, nil
}

func (e *Etcd) Txn(ctx context.Context, cmps []Cmp, ops []Op) (bool, error) {
	_, ok, err := e.TxnRev(ctx, cmps, ops)
	return ok, err
}

// TxnRev's revision is the transaction's: etcd gives every key written in it that ModRevision.
func (e *Etcd) TxnRev(ctx context.Context, cmps []Cmp, ops []Op) (int64, bool, error) {
	if len(cmps) > MaxTxnOps || len(ops) > MaxTxnOps {
		return 0, false, fmt.Errorf("transaction too large: %d compares, %d ops", len(cmps), len(ops))
	}
	cs := make([]clientv3.Cmp, len(cmps))
	for i, c := range cmps {
		cs[i] = clientv3.Compare(clientv3.ModRevision(c.Key), "=", c.Rev)
	}
	os := make([]clientv3.Op, len(ops))
	for i, o := range ops {
		if o.Delete {
			os[i] = clientv3.OpDelete(o.Key)
		} else {
			os[i] = clientv3.OpPut(o.Key, string(o.Value))
		}
	}
	r, err := retry(ctx, func(ctx context.Context) (*clientv3.TxnResponse, error) {
		return e.c.Txn(ctx).If(cs...).Then(os...).Commit()
	})
	if err != nil {
		return 0, false, err
	}
	if !r.Succeeded {
		return 0, false, nil // nothing written: no revision to give
	}
	return r.Header.GetRevision(), true, nil
}

func (e *Etcd) PutTTL(ctx context.Context, key string, value []byte, ttl time.Duration) error {
	e.mu.Lock()
	id, ok := e.leases[key]
	e.mu.Unlock()
	if ok {
		if _, err := retry(ctx, func(ctx context.Context) (*clientv3.LeaseKeepAliveResponse, error) {
			return e.c.KeepAliveOnce(ctx, id)
		}); err == nil {
			_, err = retry(ctx, func(ctx context.Context) (*clientv3.PutResponse, error) {
				return e.c.Put(ctx, key, string(value), clientv3.WithLease(id))
			})
			return err
		}
		// The lease expired: grant a new one.
	}
	secs := int64(ttl / time.Second)
	if secs < 1 {
		secs = 1
	}
	g, err := retry(ctx, func(ctx context.Context) (*clientv3.LeaseGrantResponse, error) { return e.c.Grant(ctx, secs) })
	if err != nil {
		return err
	}
	if _, err := retry(ctx, func(ctx context.Context) (*clientv3.PutResponse, error) {
		return e.c.Put(ctx, key, string(value), clientv3.WithLease(g.ID))
	}); err != nil {
		return err
	}
	e.mu.Lock()
	e.leases[key] = g.ID
	e.mu.Unlock()
	return nil
}

func (e *Etcd) Close() error { return e.c.Close() }
