// Package kv is the metadata service's narrow storage interface (ADR 0002): a few
// operations that a dedicated etcd cluster provides, and that a custom Raft store could
// provide later. Every implementation must be linearizable.
package kv

import (
	"context"
	"errors"
	"fmt"
	"time"
)

// Entry is a key with its value and the revision of its last change.
type Entry struct {
	Key   string
	Value []byte
	Rev   int64
}

// KeyRev is a key with its modification revision, without a value.
type KeyRev struct {
	Key string
	Rev int64
}

// ScanPage is one page of a keys-only prefix scan.
type ScanPage struct {
	Entries []KeyRev
	// Next is the exclusive continuation cursor (empty when Done).
	Next string
	Done bool
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

// ScanKeys limits (docs/issues/76).
const (
	ScanKeysMinLimit = 1
	ScanKeysMaxLimit = 64
	// ScanKeysMaxKeyBytes is the largest service key ScanKeys may return.
	ScanKeysMaxKeyBytes = 256
	// ScanKeysMaxReplyBytes bounds a ScanKeys wire reply (keys only).
	ScanKeysMaxReplyBytes = 32 << 10
	// GetBoundedMaxBytes is the largest value GetBounded may return for create records.
	GetBoundedMaxBytes = 8192
	// GetBoundedRecvCap is the etcd per-RPC receive cap for a bounded value get.
	GetBoundedRecvCap = 16 << 10
)

// ErrOversized is returned when a persisted value exceeds the caller’s bound.
var ErrOversized = errors.New("value exceeds bound")

// ErrWorkBudget is returned when a context’s work budget is exhausted.
var ErrWorkBudget = errors.New("work budget exhausted")

// KV is the storage interface. Its revisions only grow, across restarts too: a volume's first
// epoch is the revision of the claim that created it, above every epoch an earlier volume of its
// name reached, only while they do (docs/issues/30).
type KV interface {
	// Get returns a key, or ok=false if it does not exist.
	Get(ctx context.Context, key string) (e Entry, ok bool, err error)
	// List returns every key under prefix, sorted by key.
	List(ctx context.Context, prefix string) ([]Entry, error)
	// ScanKeys returns a latest linearizable keys-only page under prefix, exclusive of after
	// (byte-lexicographic). limit must be in [1,64]. Values are never returned. An empty page
	// is complete only with Done=true. No cross-page snapshot.
	ScanKeys(ctx context.Context, prefix, after string, limit int) (ScanPage, error)
	// GetBounded returns the current value of key if present and its encoded size is at most
	// maxBytes. Oversized or invalid values fail closed before the caller decodes.
	GetBounded(ctx context.Context, key string, maxBytes int) (e Entry, ok bool, err error)
	// Txn applies ops atomically if every compare holds. It reports whether it applied.
	Txn(ctx context.Context, cmps []Cmp, ops []Op) (bool, error)
	// TxnRev is Txn, and the revision its writes got (0 if it did not apply): a later compare
	// on a key it wrote needs no read, so nothing can come between (docs/issues/8). It is meant for a
	// transaction that writes a key: without one, the revision names no key, and differs
	// between stores.
	TxnRev(ctx context.Context, cmps []Cmp, ops []Op) (rev int64, ok bool, err error)
	// PutTTL writes a key that disappears ttl after its last PutTTL.
	PutTTL(ctx context.Context, key string, value []byte, ttl time.Duration) error
	Close() error
}

type workBudgetKey struct{}

// WithWorkBudget returns a context that charges every backend wire attempt against budget.
// Exhaustion returns ErrWorkBudget without another send.
func WithWorkBudget(ctx context.Context, budget int) context.Context {
	if budget < 0 {
		budget = 0
	}
	b := &workBudget{n: budget}
	return context.WithValue(ctx, workBudgetKey{}, b)
}

type workBudget struct {
	n int
}

func charge(ctx context.Context) error {
	b, ok := ctx.Value(workBudgetKey{}).(*workBudget)
	if !ok || b == nil {
		return nil
	}
	if b.n <= 0 {
		return ErrWorkBudget
	}
	b.n--
	return nil
}

// RemainingWork returns the remaining budget, or -1 if none was set.
func RemainingWork(ctx context.Context) int {
	b, ok := ctx.Value(workBudgetKey{}).(*workBudget)
	if !ok || b == nil {
		return -1
	}
	return b.n
}

func checkScanLimit(limit int) error {
	if limit < ScanKeysMinLimit || limit > ScanKeysMaxLimit {
		return fmt.Errorf("scan limit %d: want %d..%d", limit, ScanKeysMinLimit, ScanKeysMaxLimit)
	}
	return nil
}
