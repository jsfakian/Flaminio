package service

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"
	"strconv"
	"strings"

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
			if err := validatePlacementGate(e); err != nil {
				return 0, err
			}
			return e.Rev, nil
		}
		rev, applied, err := s.kv.TxnRev(ctx,
			[]kv.Cmp{{Key: placementGateKey, Rev: 0}},
			[]kv.Op{{Key: placementGateKey, Value: []byte(placementGateValue)}},
		)
		if err != nil {
			// An uncertain reply may follow a committed initialization. A reread discovers it;
			// persistent storage/context errors still keep admissions unavailable.
			e, ok, readErr := s.kv.Get(ctx, placementGateKey)
			if readErr != nil || !ok {
				return 0, err
			}
			if err := validatePlacementGate(e); err != nil {
				return 0, err
			}
			return e.Rev, nil
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

func validatePlacementGate(e kv.Entry) error {
	if e.Rev <= 0 || string(e.Value) != placementGateValue {
		return status.Error(codes.DataLoss, "invalid placement gate")
	}
	return nil
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
	if err := validatePlacementGate(e); err != nil {
		return 0, err
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
	if string(e.Value) != "1" {
		return nil, status.Error(codes.DataLoss, "invalid drain marker")
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
		h, err := drainInventoryHeader(e)
		if err != nil {
			return 0, err
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
		c, err := drainInventoryChunk(e)
		if err != nil {
			return 0, 0, err
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

// Inventory validates stored schema, not current geometry admission. Unsupported legacy
// volumes and orphan/deleting chunks still count; no visible-header join may conceal them.
func inventoryObject(b []byte, required ...string) error {
	if len(b) > domain.MaxCreateRecordBytes {
		return status.Error(codes.DataLoss, "inventory record exceeds bound")
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(b, &fields); err != nil || fields == nil {
		return status.Error(codes.DataLoss, "inventory record is not a JSON object")
	}
	for _, field := range required {
		if _, ok := fields[field]; !ok {
			return status.Errorf(codes.DataLoss, "inventory record missing %s", field)
		}
	}
	for field, value := range fields {
		value = bytes.TrimSpace(value)
		if bytes.Equal(value, []byte("null")) {
			return status.Errorf(codes.DataLoss, "inventory record has null %s", field)
		}
		if len(value) > 0 && value[0] == '{' {
			if err := inventoryObject(value); err != nil {
				return err
			}
		}
	}
	// encoding/json otherwise accepts duplicate fields, including conflicting reservation
	// flags or generations. Such a record is not a trustworthy completion inventory.
	decoder := json.NewDecoder(bytes.NewReader(b))
	if _, err := decoder.Token(); err != nil {
		return status.Error(codes.DataLoss, "invalid inventory object")
	}
	seen := make(map[string]bool, len(fields))
	for decoder.More() {
		field, err := decoder.Token()
		if err != nil {
			return status.Error(codes.DataLoss, "invalid inventory field")
		}
		name, ok := field.(string)
		if !ok || seen[name] {
			return status.Error(codes.DataLoss, "duplicate inventory field")
		}
		seen[name] = true
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return status.Error(codes.DataLoss, "invalid inventory value")
		}
	}
	return nil
}

func drainInventoryHeader(e kv.Entry) (domain.Header, error) {
	if err := inventoryObject(e.Value, "id", "size_bytes", "chunk_bytes", "replicas", "min_isr", "epoch"); err != nil {
		return domain.Header{}, err
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return domain.Header{}, err
	}
	if !domain.ValidID(h.ID) || e.Key != volumeKey(h.ID) || h.AdmissionGeneration < 0 ||
		h.SizeBytes == 0 || h.ChunkBytes == 0 || h.Replicas == 0 || h.MinISR == 0 || h.MinISR > h.Replicas ||
		h.Target != "" && !domain.ValidID(h.Target) || h.FirstEpoch > h.Epoch ||
		h.Creating && (h.Epoch != 0 || h.Target != "" || h.FirstEpoch != 0) ||
		!h.Creating && h.Epoch == 0 {
		return domain.Header{}, status.Error(codes.DataLoss, "inconsistent volume header schema")
	}
	if (h.CreateOperation == "") != (h.CreateSpec == nil) {
		return domain.Header{}, status.Error(codes.DataLoss, "incomplete create identity")
	}
	if h.CreateSpec != nil && (!domain.ValidID(h.CreateOperation) ||
		h.ChunkBytes != h.CreateSpec.ChunkBytes || h.Replicas != h.CreateSpec.Replicas ||
		h.MinISR != h.CreateSpec.MinISR || h.SizeBytes < h.CreateSpec.SizeBytes ||
		h.Creating && h.SizeBytes != h.CreateSpec.SizeBytes) {
		return domain.Header{}, status.Error(codes.DataLoss, "volume header disagrees with create identity")
	}
	return h, nil
}

func drainInventoryChunk(e kv.Entry) (domain.Chunk, error) {
	if err := inventoryObject(e.Value, "replicas", "isr", "isr_ver"); err != nil {
		return domain.Chunk{}, err
	}
	c, err := decodeChunkBounded(e.Value)
	if err != nil {
		return domain.Chunk{}, err
	}
	volume, index, found := strings.Cut(strings.TrimPrefix(e.Key, prefix+"chunks/"), "/")
	i, parseErr := strconv.ParseUint(index, 10, 32)
	if !found || !domain.ValidID(volume) || parseErr != nil || e.Key != chunkKey(volume, uint32(i)) ||
		!inventoryNodeSet(c.Replicas) || !inventoryNodeSet(c.ISR) {
		return domain.Chunk{}, status.Error(codes.DataLoss, "inconsistent chunk schema")
	}
	for _, node := range c.ISR {
		if !slices.Contains(c.Replicas, node) {
			return domain.Chunk{}, status.Error(codes.DataLoss, "chunk ISR is not a replica subset")
		}
	}
	if m := c.Moving; m != nil && (!domain.ValidID(m.From) || !domain.ValidID(m.To) ||
		m.From == m.To || !slices.Contains(c.Replicas, m.From)) {
		return domain.Chunk{}, status.Error(codes.DataLoss, "inconsistent chunk move")
	}
	return c, nil
}

func inventoryNodeSet(nodes []string) bool {
	if len(nodes) == 0 {
		return false
	}
	seen := make(map[string]bool, len(nodes))
	for _, node := range nodes {
		if !domain.ValidID(node) || seen[node] {
			return false
		}
		seen[node] = true
	}
	return true
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
