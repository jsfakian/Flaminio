package service

import (
	"context"
	"errors"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
)

// CheckCreateVolume implements CSI capacity-range retries without changing strict direct
// CreateVolume identity or helping an unfinished creation. Every path is read-only.
func (s *Service) CheckCreateVolume(ctx context.Context, r *metav1.CheckCreateVolumeRequest) (*metav1.CheckCreateVolumeResponse, error) {
	required, limit := r.GetRequiredBytes(), r.GetLimitBytes()
	if required < 0 || limit < 0 || limit > 0 && required > limit {
		return nil, status.Error(codes.InvalidArgument, "invalid signed capacity range")
	}
	spec, err := requestedCreateSpec(r.GetDesired())
	if err != nil {
		return nil, errStatus(err)
	}
	want := domain.CreateSpecFrom(spec)

	// Inspect raw state to distinguish absent, creating and deleting names. This never resumes
	// an operation. Successful compatibility uses only the header in #77's coherent full view.
	if err := s.checkCreateVisible(ctx, spec.ID, want); err != nil {
		return nil, errStatus(err)
	}
	v, err := s.readVolume(ctx, spec.ID)
	if errors.Is(err, domain.ErrNotFound) {
		if stateErr := s.checkCreateVisible(ctx, spec.ID, want); stateErr != nil {
			return nil, errStatus(stateErr)
		}
		return nil, status.Error(codes.Aborted, "volume changed during compatibility check; retry")
	}
	if err != nil {
		return nil, errStatus(err)
	}
	if v.ID != spec.ID {
		return nil, status.Error(codes.DataLoss, "volume header id disagrees with its key")
	}
	legacy, err := compatibleCreateIdentity(v.Header, want)
	if err != nil {
		return nil, err
	}
	if v.SizeBytes < uint64(required) || limit > 0 && v.SizeBytes > uint64(limit) {
		return nil, status.Error(codes.AlreadyExists, "existing volume capacity is outside the requested range")
	}
	return &metav1.CheckCreateVolumeResponse{Volume: volumePB(v), LegacyIdentity: legacy}, nil
}

func (s *Service) checkCreateVisible(ctx context.Context, id string, want domain.CreateSpec) error {
	e, ok, err := s.kv.GetBounded(ctx, volumeKey(id), domain.MaxCreateRecordBytes)
	if err != nil {
		return err
	}
	if !ok {
		if _, tombstone, err := s.kv.Get(ctx, tombstoneKey(id)); err != nil {
			return err
		} else if tombstone {
			return status.Error(codes.AlreadyExists, "volume name has a deletion tombstone")
		}
		return status.Error(codes.Aborted, "volume disappeared; retry creation")
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return err
	}
	if h.Deleting {
		return status.Error(codes.AlreadyExists, "volume is being deleted")
	}
	if h.ID != id {
		return status.Error(codes.DataLoss, "volume header id disagrees with its key")
	}
	if _, err := compatibleCreateIdentity(h, want); err != nil {
		return err
	}
	if h.Creating {
		return status.Error(codes.Aborted, "volume creation is in progress; retry")
	}
	return nil
}

func compatibleCreateIdentity(h domain.Header, want domain.CreateSpec) (bool, error) {
	if !domain.ValidID(h.ID) {
		return false, status.Error(codes.DataLoss, "volume header has an invalid id")
	}
	if h.CreateSpec == nil && h.CreateOperation == "" {
		if h.Creating {
			return false, status.Error(codes.Aborted, "legacy volume creation is in progress; retry")
		}
		// #76's exception applies only to genuinely legacy visible headers. Capacity is
		// checked against the range by the caller, not against the new initial candidate.
		if h.ChunkBytes != want.ChunkBytes || h.Replicas != want.Replicas || h.MinISR != want.MinISR {
			return false, status.Error(codes.AlreadyExists, "existing legacy volume parameters differ")
		}
		return true, nil
	}
	if h.CreateSpec == nil || h.CreateOperation == "" {
		return false, status.Error(codes.DataLoss, "new-format volume has an incomplete create identity")
	}
	if err := h.CreateSpec.Validate(); err != nil {
		return false, status.Errorf(codes.DataLoss, "new-format volume create identity: %v", err)
	}
	if h.ChunkBytes != h.CreateSpec.ChunkBytes || h.Replicas != h.CreateSpec.Replicas || h.MinISR != h.CreateSpec.MinISR {
		return false, status.Error(codes.DataLoss, "volume geometry disagrees with its immutable create identity")
	}
	stored := *h.CreateSpec
	stored.SizeBytes = want.SizeBytes // The sole identity exception is the authorized range.
	if !stored.Equal(want) {
		return false, status.Error(codes.AlreadyExists, "existing volume creation parameters differ")
	}
	return false, nil
}
