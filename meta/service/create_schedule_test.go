package service

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// Shared schedules from meta/testdata/create-recovery.json (docs/issues/76 S1/S7):
// the same compare/op sequence the Rust model runs, applied through Go kv.Txn on both stores.

type scheduleFile struct {
	Schedules []schedule `json:"schedules"`
}

type schedule struct {
	Name            string `json:"name"`
	Mutant          string `json:"mutant"`
	ExpectViolation string `json:"expect_violation"`
	Steps           []step `json:"steps"`
}

type step struct {
	Action    string         `json:"action"`
	Op        string         `json:"op"`
	Volume    string         `json:"volume"`
	Root      string         `json:"root"`
	BirthRev  *int64         `json:"birth_rev"`
	HeaderRev int64          `json:"header_rev"`
	Header    *domain.Header `json:"header"`
}

func loadCreateRecoverySchedules(t *testing.T) scheduleFile {
	t.Helper()
	path := filepath.Join("..", "testdata", "create-recovery.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var f scheduleFile
	if err := json.Unmarshal(raw, &f); err != nil {
		t.Fatal(err)
	}
	if len(f.Schedules) < 4 {
		t.Fatalf("expected ≥4 schedules, got %d", len(f.Schedules))
	}
	return f
}

// docs/issues/76 S1/S7: replay the saved birth-fence / unpublished schedules as real Go transactions.
func TestCreateRecoverySchedulesReplay(t *testing.T) {
	file := loadCreateRecoverySchedules(t)
	backends(t, func(t *testing.T, store kv.KV) {
		for _, sched := range file.Schedules {
			t.Run(sched.Name, func(t *testing.T) {
				runCreateRecoverySchedule(t, store, sched)
			})
		}
	})
}

func runCreateRecoverySchedule(t *testing.T, store kv.KV, sched schedule) {
	t.Helper()
	ctx := context.Background()
	retired := map[string]bool{}
	published := map[string]bool{}
	tokened := map[string]bool{}
	var gotViolation string

	for _, st := range sched.Steps {
		switch st.Action {
		case "create_root", "replay_create_root":
			birth := createBirthKey(st.Volume)
			birthRev := int64(0)
			if st.BirthRev != nil {
				birthRev = *st.BirthRev
			} else if e, ok, err := store.Get(ctx, birth); err != nil {
				t.Fatal(err)
			} else if ok {
				birthRev = e.Rev
			}
			cmps := []kv.Cmp{{Key: createRootKey(st.Op), Rev: 0}}
			if sched.Mutant != "missing_birth_compare" {
				cmps = append(cmps, kv.Cmp{Key: birth, Rev: birthRev})
			}
			ops := []kv.Op{
				{Key: createRootKey(st.Op), Value: []byte(st.Root)},
				{Key: birth, Value: []byte("1")},
			}
			if retired[st.Op] && sched.Mutant != "missing_birth_compare" {
				// Correct schedule must not resurrect; birth fence should make the txn fail.
			}
			ok, err := store.Txn(ctx, cmps, ops)
			if err != nil {
				t.Fatalf("%s: %v", st.Action, err)
			}
			if retired[st.Op] {
				if sched.Mutant == "missing_birth_compare" {
					if !ok {
						t.Fatal("missing_birth_compare mutant must resurrect (counterexample)")
					}
					gotViolation = "NoRootResurrection"
				} else if ok {
					t.Fatal("birth fence allowed root resurrection")
				}
			} else if !ok {
				t.Fatalf("%s did not apply", st.Action)
			}

		case "gc_root":
			e, ok, err := store.Get(ctx, createRootKey(st.Op))
			if err != nil {
				t.Fatal(err)
			}
			if !ok {
				continue
			}
			ok, err = store.Txn(ctx, []kv.Cmp{{Key: createRootKey(st.Op), Rev: e.Rev}}, []kv.Op{
				{Key: createRootKey(st.Op), Delete: true},
			})
			if err != nil || !ok {
				t.Fatalf("gc_root: %v %v", ok, err)
			}
			retired[st.Op] = true

		case "mark_published":
			published[st.Volume] = true
		case "mark_token", "mark_attach":
			tokened[st.Volume] = true

		case "unpublished_rollback":
			if st.Header == nil {
				t.Fatal("unpublished_rollback missing header")
			}
			h := *st.Header
			if h.ID == "" {
				h.ID = st.Volume
			}
			key := volumeKey(h.ID)
			if _, ok, _ := store.Get(ctx, key); !ok {
				b, err := encodeRecord(h)
				if err != nil {
					t.Fatal(err)
				}
				if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: key, Value: b}}); err != nil || !ok {
					t.Fatalf("seed header: %v %v", ok, err)
				}
			}
			e, ok, err := store.Get(ctx, key)
			if err != nil || !ok {
				t.Fatalf("header: %v %v", ok, err)
			}
			proof := h.Creating && h.Epoch == 0 && h.Target == ""
			if sched.Mutant == "weakened_unpublished_classifier" {
				proof = h.Creating
			}
			if !proof {
				continue
			}
			if published[h.ID] || tokened[h.ID] {
				gotViolation = "NoUnprovedTombstoneBypass"
				continue
			}
			ok, err = store.Txn(ctx,
				[]kv.Cmp{{Key: key, Rev: e.Rev}, {Key: tombstoneKey(h.ID), Rev: 0}},
				[]kv.Op{{Key: key, Delete: true}},
			)
			if err != nil || !ok {
				t.Fatalf("unpublished_rollback: %v %v", ok, err)
			}

		case "put_header":
			if st.Header == nil {
				t.Fatal("put_header missing header")
			}
			b, err := encodeRecord(st.Header)
			if err != nil {
				t.Fatal(err)
			}
			if ok, err := store.Txn(ctx, nil, []kv.Op{{Key: volumeKey(st.Header.ID), Value: b}}); err != nil || !ok {
				t.Fatalf("put_header: %v %v", ok, err)
			}

		default:
			t.Fatalf("unknown action %q", st.Action)
		}
	}

	switch {
	case sched.ExpectViolation == "" && gotViolation != "":
		t.Fatalf("unexpected violation %s", gotViolation)
	case sched.ExpectViolation != "" && gotViolation != sched.ExpectViolation:
		t.Fatalf("expected violation %s, got %q", sched.ExpectViolation, gotViolation)
	}
}
