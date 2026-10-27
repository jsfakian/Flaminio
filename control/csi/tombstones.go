package csi

import (
	"context"
	"log/slog"
	"time"
)

const tombstoneInterval = 5 * time.Second

// SweepTombstones lists this node's pending volume deletions, asks the data plane to drop
// local chunk replicas, and confirms each with the metadata service (docs/followups.md §8).
func SweepTombstones(ctx context.Context, meta *Meta, dp Dataplane, nodeID string) {
	SweepTombstonesEvery(ctx, meta, dp, nodeID, tombstoneInterval)
}

// SweepTombstonesEvery is SweepTombstones with a configurable poll interval (for tests).
func SweepTombstonesEvery(ctx context.Context, meta *Meta, dp Dataplane, nodeID string, every time.Duration) {
	if every <= 0 {
		every = tombstoneInterval
	}
	for {
		if err := sweepTombstonesOnce(ctx, meta, dp, nodeID); err != nil && ctx.Err() == nil {
			slog.Warn("tombstone sweep", "node", nodeID, "error", err)
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(every):
		}
	}
}

func sweepTombstonesOnce(ctx context.Context, meta *Meta, dp Dataplane, nodeID string) error {
	list, err := meta.ListTombstones(ctx, nodeID)
	if err != nil {
		return err
	}
	for _, t := range list {
		if err := dp.Delete(ctx, t.GetVolume()); err != nil {
			slog.Warn("tombstone delete", "node", nodeID, "volume", t.GetVolume(), "error", err)
			continue
		}
		if err := meta.ConfirmDeletion(ctx, t.GetVolume(), nodeID); err != nil {
			slog.Warn("tombstone confirm", "node", nodeID, "volume", t.GetVolume(), "error", err)
		}
	}
	return nil
}
