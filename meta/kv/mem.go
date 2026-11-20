package kv

import (
	"context"
	"fmt"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"
)

// Mem is an in-memory KV for tests.
type Mem struct {
	mu      sync.Mutex
	rev     int64
	data    map[string]Entry
	keys    []string // sorted index for ScanKeys; seek O(log N)
	expires map[string]time.Time
	now     func() time.Time
}

// NewMem returns an empty in-memory KV. now is the clock for TTL keys (nil: time.Now). Its
// revisions start at the clock, in microseconds, so that a restarted service starts above the one
// before it, whose volumes nodes may still hold (docs/issues/30, S2).
func NewMem(now func() time.Time) *Mem {
	if now == nil {
		now = time.Now
	}
	return &Mem{
		rev:     max(now().UnixMicro(), 0),
		data:    map[string]Entry{},
		expires: map[string]time.Time{},
		now:     now,
	}
}

func (m *Mem) expireAll() {
	t := m.now()
	for k, e := range m.expires {
		if !t.Before(e) {
			m.removeKey(k)
		}
	}
}

func (m *Mem) expireOne(key string) {
	if exp, ok := m.expires[key]; ok && !m.now().Before(exp) {
		m.removeKey(key)
	}
}

func (m *Mem) removeKey(key string) {
	if _, ok := m.data[key]; !ok {
		return
	}
	delete(m.data, key)
	delete(m.expires, key)
	if i, found := sort.Find(len(m.keys), func(i int) int {
		return strings.Compare(key, m.keys[i])
	}); found {
		m.keys = slices.Delete(m.keys, i, i+1)
	}
}

func (m *Mem) putKey(key string, value []byte, rev int64) {
	if _, ok := m.data[key]; !ok {
		i, _ := sort.Find(len(m.keys), func(i int) int {
			return strings.Compare(key, m.keys[i])
		})
		m.keys = slices.Insert(m.keys, i, key)
	}
	m.data[key] = Entry{Key: key, Value: slices.Clone(value), Rev: rev}
}

func (m *Mem) Get(ctx context.Context, key string) (Entry, bool, error) {
	if err := charge(ctx); err != nil {
		return Entry{}, false, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expireAll()
	e, ok := m.data[key]
	e.Value = slices.Clone(e.Value)
	return e, ok, nil
}

func (m *Mem) List(ctx context.Context, prefix string) ([]Entry, error) {
	if err := charge(ctx); err != nil {
		return nil, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expireAll()
	var out []Entry
	for k, e := range m.data {
		if strings.HasPrefix(k, prefix) {
			e.Value = slices.Clone(e.Value)
			out = append(out, e)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Key < out[j].Key })
	return out, nil
}

func (m *Mem) ScanKeys(ctx context.Context, prefix, after string, limit int) (ScanPage, error) {
	if err := checkScanLimit(limit); err != nil {
		return ScanPage{}, err
	}
	if err := charge(ctx); err != nil {
		return ScanPage{}, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()

	var i int
	if after != "" {
		i = sort.Search(len(m.keys), func(j int) bool { return m.keys[j] > after })
	} else {
		i = sort.Search(len(m.keys), func(j int) bool { return m.keys[j] >= prefix })
	}

	var page ScanPage
	reply := 0
	for i < len(m.keys) {
		k := m.keys[i]
		m.expireOne(k)
		if _, ok := m.data[k]; !ok {
			// removed; i now points at the next key (or end)
			i = sort.Search(len(m.keys), func(j int) bool { return m.keys[j] >= k })
			continue
		}
		if !strings.HasPrefix(k, prefix) {
			break
		}
		if len(k) > ScanKeysMaxKeyBytes {
			return ScanPage{}, fmt.Errorf("key exceeds %d bytes", ScanKeysMaxKeyBytes)
		}
		if len(page.Entries) >= limit {
			page.Done = false
			page.Next = page.Entries[len(page.Entries)-1].Key
			return page, nil
		}
		cost := len(k) + 8
		if len(page.Entries) > 0 && reply+cost > ScanKeysMaxReplyBytes {
			page.Done = false
			page.Next = page.Entries[len(page.Entries)-1].Key
			return page, nil
		}
		page.Entries = append(page.Entries, KeyRev{Key: k, Rev: m.data[k].Rev})
		reply += cost
		i++
	}
	if len(page.Entries) > 0 {
		page.Next = page.Entries[len(page.Entries)-1].Key
	}
	page.Done = true
	return page, nil
}

func (m *Mem) GetBounded(ctx context.Context, key string, maxBytes int) (Entry, bool, error) {
	if maxBytes < 0 {
		return Entry{}, false, fmt.Errorf("maxBytes %d", maxBytes)
	}
	if err := charge(ctx); err != nil {
		return Entry{}, false, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expireOne(key)
	e, ok := m.data[key]
	if !ok {
		return Entry{}, false, nil
	}
	if len(e.Value) > maxBytes {
		return Entry{}, false, fmt.Errorf("%w: key %q has %d bytes, max %d", ErrOversized, key, len(e.Value), maxBytes)
	}
	e.Value = slices.Clone(e.Value)
	return e, true, nil
}

func (m *Mem) Txn(ctx context.Context, cmps []Cmp, ops []Op) (bool, error) {
	_, ok, err := m.TxnRev(ctx, cmps, ops)
	return ok, err
}

func (m *Mem) TxnRev(ctx context.Context, cmps []Cmp, ops []Op) (int64, bool, error) {
	if err := charge(ctx); err != nil {
		return 0, false, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expireAll()
	for _, c := range cmps {
		if m.data[c.Key].Rev != c.Rev {
			return 0, false, nil
		}
	}
	m.rev++
	for _, o := range ops {
		delete(m.expires, o.Key)
		if o.Delete {
			m.removeKey(o.Key)
		} else {
			m.putKey(o.Key, o.Value, m.rev)
		}
	}
	return m.rev, true, nil
}

func (m *Mem) PutTTL(ctx context.Context, key string, value []byte, ttl time.Duration) error {
	if err := charge(ctx); err != nil {
		return err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.rev++
	m.putKey(key, value, m.rev)
	m.expires[key] = m.now().Add(ttl)
	return nil
}

func (m *Mem) Close() error { return nil }
