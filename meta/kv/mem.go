package kv

import (
	"context"
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
	expires map[string]time.Time
	now     func() time.Time
}

// NewMem returns an empty in-memory KV. now is the clock for TTL keys (nil: time.Now).
func NewMem(now func() time.Time) *Mem {
	if now == nil {
		now = time.Now
	}
	return &Mem{data: map[string]Entry{}, expires: map[string]time.Time{}, now: now}
}

func (m *Mem) expire() {
	t := m.now()
	for k, e := range m.expires {
		if !t.Before(e) {
			delete(m.data, k)
			delete(m.expires, k)
		}
	}
}

func (m *Mem) Get(_ context.Context, key string) (Entry, bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expire()
	e, ok := m.data[key]
	e.Value = slices.Clone(e.Value)
	return e, ok, nil
}

func (m *Mem) List(_ context.Context, prefix string) ([]Entry, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expire()
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

func (m *Mem) Txn(_ context.Context, cmps []Cmp, ops []Op) (bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.expire()
	for _, c := range cmps {
		if m.data[c.Key].Rev != c.Rev {
			return false, nil
		}
	}
	m.rev++
	for _, o := range ops {
		delete(m.expires, o.Key)
		if o.Delete {
			delete(m.data, o.Key)
		} else {
			m.data[o.Key] = Entry{Key: o.Key, Value: slices.Clone(o.Value), Rev: m.rev}
		}
	}
	return true, nil
}

func (m *Mem) PutTTL(_ context.Context, key string, value []byte, ttl time.Duration) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.rev++
	m.data[key] = Entry{Key: key, Value: slices.Clone(value), Rev: m.rev}
	m.expires[key] = m.now().Add(ttl)
	return nil
}

func (m *Mem) Close() error { return nil }
