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

func (e *Etcd) Txn(ctx context.Context, cmps []Cmp, ops []Op) (bool, error) {
	if len(cmps) > MaxTxnOps || len(ops) > MaxTxnOps {
		return false, fmt.Errorf("transaction too large: %d compares, %d ops", len(cmps), len(ops))
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
		return false, err
	}
	return r.Succeeded, nil
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
