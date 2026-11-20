package service

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// Placement gate under /flaminio/v1/: value "1", modification revision is the placement
// generation (docs/issues/78). Heartbeats do not bump it. Generation 0 belongs only to legacy
// headers; new admissions require a positive initialized gate.
const (
	placementGateKey   = prefix + "placement-gate"
	placementGateValue = "1"
)

// errGateConflict means the placement gate moved between planning and the claim; the caller
// must replan from a fresh gate and node list, never reuse the stale plan.
var errGateConflict = errors.New("placement gate conflict")

// ensurePlacementGate returns the gate's modification revision, creating it at value "1" when
// absent. A failed initialization leaves admission unavailable. No writer deletes the gate.
func (s *Service) ensurePlacementGate(ctx context.Context) (int64, error) {
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return 0, err
			}
		}
		e, ok, err := s.kv.Get(ctx, placementGateKey)
		if err != nil {
			return 0, err
		}
		if ok {
			if e.Rev <= 0 {
				return 0, status.Error(codes.DataLoss, "placement gate revision is not positive")
			}
			return e.Rev, nil
		}
		rev, applied, err := s.kv.TxnRev(ctx,
			[]kv.Cmp{{Key: placementGateKey, Rev: 0}},
			[]kv.Op{{Key: placementGateKey, Value: []byte(placementGateValue)}},
		)
		if err != nil {
			return 0, err
		}
		if applied {
			if rev <= 0 {
				return 0, status.Error(codes.DataLoss, "placement gate revision is not positive")
			}
			return rev, nil
		}
	}
	return 0, status.Error(codes.Aborted, "too much contention")
}

// readPlacementGate returns the current gate revision after ensuring it exists.
func (s *Service) readPlacementGate(ctx context.Context) (int64, error) {
	g, err := s.ensurePlacementGate(ctx)
	if err != nil {
		return 0, err
	}
	e, ok, err := s.kv.Get(ctx, placementGateKey)
	if err != nil {
		return 0, err
	}
	if !ok {
		return 0, status.Error(codes.FailedPrecondition, "placement gate is unavailable")
	}
	if e.Rev != g {
		// Advanced since ensure; use the fresh revision for planning.
		if e.Rev <= 0 {
			return 0, status.Error(codes.DataLoss, "placement gate revision is not positive")
		}
		return e.Rev, nil
	}
	return g, nil
}

func generationToUint64(rev int64) (uint64, error) {
	if rev <= 0 || rev > math.MaxInt64 {
		return 0, fmt.Errorf("%w: generation %d is not a positive int64", domain.ErrInvalid, rev)
	}
	return uint64(rev), nil
}

func generationFromUint64(g uint64) (int64, error) {
	if g == 0 || g > math.MaxInt64 {
		return 0, fmt.Errorf("%w: generation must be a positive int64", domain.ErrInvalid)
	}
	return int64(g), nil
}

// CheckDrain reports whether drain generation D may complete for node (docs/issues/78).
func (s *Service) CheckDrain(ctx context.Context, r *metav1.CheckDrainRequest) (*metav1.CheckDrainResponse, error) {
	node := r.GetNode()
	if !domain.ValidID(node) {
		return nil, errStatus(fmt.Errorf("%w: node id %q", domain.ErrInvalid, node))
	}
	d, err := generationFromUint64(r.GetGeneration())
	if err != nil {
		return nil, errStatus(err)
	}
	key := drainKey(node)
	e, ok, err := s.kv.Get(ctx, key)
	if err != nil {
		return nil, errStatus(err)
	}
	if !ok || e.Rev != d {
		return nil, status.Error(codes.FailedPrecondition, "drain generation is no longer current")
	}

	pending, err := s.countPendingCreations(ctx, d)
	if err != nil {
		return nil, errStatus(err)
	}
	placement, incoming, err := s.countDrainChunks(ctx, node)
	if err != nil {
		return nil, errStatus(err)
	}

	applied, err := s.kv.Txn(ctx, []kv.Cmp{{Key: key, Rev: d}}, nil)
	if err != nil {
		return nil, errStatus(err)
	}
	if !applied {
		return nil, status.Error(codes.FailedPrecondition, "drain generation is no longer current")
	}
	complete := pending == 0 && placement == 0 && incoming == 0
	return &metav1.CheckDrainResponse{
		Complete:         complete,
		PendingCreations: pending,
		PlacementChunks:  placement,
		IncomingMoves:    incoming,
	}, nil
}

func (s *Service) countPendingCreations(ctx context.Context, d int64) (uint64, error) {
	entries, err := s.kv.List(ctx, prefix+"volumes/")
	if err != nil {
		return 0, err
	}
	var n uint64
	for _, e := range entries {
		if len(e.Value) > domain.MaxCreateRecordBytes {
			return 0, status.Error(codes.DataLoss, "volume header exceeds bound")
		}
		var h domain.Header
		if err := json.Unmarshal(e.Value, &h); err != nil {
			return 0, status.Errorf(codes.DataLoss, "volume header: %v", err)
		}
		if h.AdmissionGeneration < 0 {
			return 0, status.Error(codes.DataLoss, "volume header has negative admission_generation")
		}
		if h.Creating && h.AdmissionGeneration < d {
			n++
		}
	}
	return n, nil
}

func (s *Service) countDrainChunks(ctx context.Context, node string) (placement, incoming uint64, err error) {
	entries, err := s.kv.List(ctx, prefix+"chunks/")
	if err != nil {
		return 0, 0, err
	}
	for _, e := range entries {
		if len(e.Value) > domain.MaxCreateRecordBytes {
			return 0, 0, status.Error(codes.DataLoss, "chunk record exceeds bound")
		}
		var c domain.Chunk
		if err := json.Unmarshal(e.Value, &c); err != nil {
			return 0, 0, status.Errorf(codes.DataLoss, "chunk record: %v", err)
		}
		for _, r := range c.Replicas {
			if r == node {
				placement++
				break
			}
		}
		if c.Moving != nil && c.Moving.To == node {
			incoming++
		}
	}
	return placement, incoming, nil
}

// bumpPlacementGateOp returns a Put of the gate to value "1" (advances its modification revision).
func bumpPlacementGateOp() kv.Op {
	return kv.Op{Key: placementGateKey, Value: []byte(placementGateValue)}
}

// drainMarkerPresent reports whether node has a drain marker.
func (s *Service) drainMarkerPresent(ctx context.Context, node string) (bool, int64, error) {
	e, ok, err := s.kv.Get(ctx, drainKey(node))
	if err != nil || !ok {
		return false, 0, err
	}
	return true, e.Rev, nil
}

// nodeIsDraining is a linearizable read of the drain marker for lifecycle checks.
func (s *Service) nodeIsDraining(ctx context.Context, node string) (bool, error) {
	ok, _, err := s.drainMarkerPresent(ctx, node)
	return ok, err
}
