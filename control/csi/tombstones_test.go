package csi

import (
	"context"
	"sync"
	"testing"
	"time"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
)

type recordingDataplane struct {
	fakeDataplane
	mu      sync.Mutex
	deleted []string
	fail    map[string]error
}

func (d *recordingDataplane) Delete(_ context.Context, volume string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := d.fail[volume]; err != nil {
		return err
	}
	d.deleted = append(d.deleted, volume)
	return nil
}

func (d *recordingDataplane) deletedVolumes() []string {
	d.mu.Lock()
	defer d.mu.Unlock()
	out := make([]string, len(d.deleted))
	copy(out, d.deleted)
	return out
}

// TestSweepTombstonesOnce: create → delete → one node's sweep deletes and confirms; the other
// node still sees the tombstone until it sweeps too.
func TestSweepTombstonesOnce(t *testing.T) {
	m := metaFixture(t)
	ctx := context.Background()
	if _, err := m.CreateVolume(ctx, &metav1.CreateVolumeRequest{
		Id: "v", SizeBytes: 1 << 20, ChunkBytes: 1 << 20, Replicas: 2, MinIsr: 1,
	}); err != nil {
		t.Fatal(err)
	}
	if err := m.DeleteVolume(ctx, "v"); err != nil {
		t.Fatal(err)
	}
	for _, n := range []string{"a", "b"} {
		list, err := m.ListTombstones(ctx, n)
		if err != nil || len(list) != 1 || list[0].GetVolume() != "v" {
			t.Fatalf("before sweep, %s: %v %v", n, list, err)
		}
	}

	dpA := &recordingDataplane{}
	if err := sweepTombstonesOnce(ctx, m, dpA, "a"); err != nil {
		t.Fatal(err)
	}
	if got := dpA.deletedVolumes(); len(got) != 1 || got[0] != "v" {
		t.Fatalf("dataplane deletes: %v", got)
	}
	listA, err := m.ListTombstones(ctx, "a")
	if err != nil || len(listA) != 0 {
		t.Fatalf("a after confirm: %v %v", listA, err)
	}
	listB, err := m.ListTombstones(ctx, "b")
	if err != nil || len(listB) != 1 {
		t.Fatalf("b still pending: %v %v", listB, err)
	}

	dpB := &recordingDataplane{}
	if err := sweepTombstonesOnce(ctx, m, dpB, "b"); err != nil {
		t.Fatal(err)
	}
	listB, err = m.ListTombstones(ctx, "b")
	if err != nil || len(listB) != 0 {
		t.Fatalf("tombstone after both confirms: %v %v", listB, err)
	}
	// A second sweep is a no-op.
	if err := sweepTombstonesOnce(ctx, m, dpA, "a"); err != nil {
		t.Fatal(err)
	}
	if got := dpA.deletedVolumes(); len(got) != 1 {
		t.Fatalf("second sweep should not delete again: %v", got)
	}
}

func TestSweepTombstonesLoopStops(t *testing.T) {
	m := metaFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		SweepTombstonesEvery(ctx, m, &recordingDataplane{}, "a", time.Millisecond)
		close(done)
	}()
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("sweep did not stop")
	}
}
