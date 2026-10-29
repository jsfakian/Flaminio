// Package kv is the metadata service's narrow storage interface (ADR 0002): a few
// operations that a dedicated etcd cluster provides, and that a custom Raft store could
// provide later. Every implementation must be linearizable.
package kv

import (
	"context"
	"time"
)

// Entry is a key with its value and the revision of its last change.
type Entry struct {
	Key   string
	Value []byte
	Rev   int64
}

// Cmp requires a key to be at a revision; Rev 0 means the key must not exist.
type Cmp struct {
	Key string
	Rev int64
}

// Op writes or deletes one key.
type Op struct {
	Key    string
	Value  []byte
	Delete bool
}

// MaxTxnOps is the most compares or operations one transaction may carry: etcd's default
// limit, with room to spare.
const MaxTxnOps = 120

// KV is the storage interface.
type KV interface {
	// Get returns a key, or ok=false if it does not exist.
	Get(ctx context.Context, key string) (e Entry, ok bool, err error)
	// List returns every key under prefix, sorted by key.
	List(ctx context.Context, prefix string) ([]Entry, error)
	// Txn applies ops atomically if every compare holds. It reports whether it applied.
	Txn(ctx context.Context, cmps []Cmp, ops []Op) (bool, error)
	// TxnRev is Txn, and the revision its writes got (0 if it did not apply): a later compare
	// on a key it wrote needs no read, so nothing can come between (docs/8). It is meant for a
	// transaction that writes a key: without one, the revision names no key, and differs
	// between stores.
	TxnRev(ctx context.Context, cmps []Cmp, ops []Op) (rev int64, ok bool, err error)
	// PutTTL writes a key that disappears ttl after its last PutTTL.
	PutTTL(ctx context.Context, key string, value []byte, ttl time.Duration) error
	Close() error
}
