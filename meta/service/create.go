package service

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

// Create-plan and progress keyspaces (docs/issues/76). The root index holds no plan values.
// Without #78 there is no placement-admission gate or admission_generation field: creation does
// not serialize against drains.
const (
	createRootPrefix  = prefix + "create-roots/"
	createPlanPrefix  = prefix + "create-plans/"
	createBirthPrefix = prefix + "create-birth/"
	createBatchSize   = 32
	createRootSchema  = 1

	rootPreparing = "preparing"
	rootSealed    = "sealed"
	rootCleaning  = "cleaning"
	rootCopying   = "copying"
	rootComplete  = "complete"
)

func createRootKey(op string) string { return createRootPrefix + op }
func createPlanKey(op string, i uint32) string {
	return fmt.Sprintf("%s%s/%08d", createPlanPrefix, op, i)
}
func createPlanOpPrefix(op string) string   { return createPlanPrefix + op + "/" }
func createBirthKey(volumeID string) string { return createBirthPrefix + birthShard(volumeID) }

func birthShard(volumeID string) string {
	sum := sha256.Sum256([]byte(volumeID))
	return fmt.Sprintf("%x", sum[0]>>4) // first nibble → 16 shards 0..f
}

func newOperationID() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}

// createRoot is the durable progress record for one create operation.
type createRoot struct {
	Schema     int               `json:"schema"`
	Volume     string            `json:"volume"`
	Spec       domain.CreateSpec `json:"spec"`
	State      string            `json:"state"`
	ChunkCount int               `json:"chunk_count"`
	// Staged is the contiguous plan-prefix length written under create-plans/.
	Staged uint32 `json:"staged"`
	// Next is the contiguous public-prefix progress (cleaned then copied).
	Next uint32 `json:"next"`
}

func encodeRecord(v any) ([]byte, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	if len(b) > domain.MaxCreateRecordBytes {
		return nil, fmt.Errorf("%w: encoded record exceeds %d bytes", domain.ErrInvalid, domain.MaxCreateRecordBytes)
	}
	return b, nil
}

func decodeRoot(b []byte) (createRoot, error) {
	if len(b) > domain.MaxCreateRecordBytes {
		return createRoot{}, status.Error(codes.DataLoss, "create root exceeds bound")
	}
	var r createRoot
	if err := json.Unmarshal(b, &r); err != nil {
		return createRoot{}, status.Errorf(codes.DataLoss, "create root: %v", err)
	}
	if r.Schema != createRootSchema {
		return createRoot{}, status.Errorf(codes.DataLoss, "create root schema %d", r.Schema)
	}
	if err := r.Spec.Validate(); err != nil {
		return createRoot{}, status.Errorf(codes.DataLoss, "create root spec: %v", err)
	}
	return r, nil
}

func decodeHeaderBounded(b []byte) (domain.Header, error) {
	if len(b) > domain.MaxCreateRecordBytes {
		return domain.Header{}, status.Error(codes.DataLoss, "volume header exceeds bound")
	}
	var h domain.Header
	if err := json.Unmarshal(b, &h); err != nil {
		return domain.Header{}, status.Errorf(codes.DataLoss, "volume header: %v", err)
	}
	if h.CreateSpec != nil {
		if err := h.CreateSpec.Validate(); err != nil {
			return domain.Header{}, status.Errorf(codes.DataLoss, "create_spec: %v", err)
		}
	}
	return h, nil
}

func decodeChunkBounded(b []byte) (domain.Chunk, error) {
	if len(b) > domain.MaxCreateRecordBytes {
		return domain.Chunk{}, status.Error(codes.DataLoss, "chunk record exceeds bound")
	}
	var c domain.Chunk
	if err := json.Unmarshal(b, &c); err != nil {
		return domain.Chunk{}, status.Errorf(codes.DataLoss, "chunk record: %v", err)
	}
	return c, nil
}

// createVolume persists an immutable plan, admits a Creating header, then cleans/copies/finalizes
// under the claim revision. Identical callers and recovery helpers resume the same operation.
func (s *Service) createVolume(ctx context.Context, spec domain.Spec) (*domain.Volume, error) {
	cs := domain.CreateSpecFrom(spec)
	if err := spec.Validate(); err != nil {
		return nil, err
	}
	if err := cs.Validate(); err != nil {
		return nil, err
	}

	// Classify any existing raw header before planning.
	if v, err := s.resumeOrRefuse(ctx, spec.ID, cs); err != nil || v != nil {
		return v, err
	}

	placement, err := s.planPlacement(ctx, spec)
	if err != nil {
		return nil, err
	}
	v := domain.NewVolume(spec, placement)
	v.Creating = true
	v.CreateOperation = "" // set at admission
	specCopy := cs
	v.CreateSpec = &specCopy

	op, err := newOperationID()
	if err != nil {
		return nil, err
	}
	root := createRoot{
		Schema: createRootSchema, Volume: spec.ID, Spec: cs,
		State: rootPreparing, ChunkCount: len(v.Chunks),
	}
	rootRev, err := s.createInitialRoot(ctx, op, root)
	if err != nil {
		return nil, err
	}
	if rootRev == 0 {
		// Uncertain/false: reread; absent → abandon this op ID.
		if e, ok, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes); err != nil {
			return nil, err
		} else if !ok {
			return nil, status.Errorf(codes.Aborted, "create operation %s was not admitted; retry", truncID(op))
		} else {
			r, err := decodeRoot(e.Value)
			if err != nil {
				return nil, err
			}
			root, rootRev = r, e.Rev
		}
	}

	rootRev, err = s.stagePlans(ctx, op, root, rootRev, v.Chunks)
	if err != nil {
		return nil, err
	}
	root.State = rootSealed
	rb, err := encodeRecord(root)
	if err != nil {
		return nil, err
	}
	ok, err := s.kv.Txn(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: rootRev}}, []kv.Op{{Key: createRootKey(op), Value: rb}})
	if err != nil {
		return nil, err
	}
	if !ok {
		e, found, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
		if err != nil {
			return nil, err
		}
		if !found {
			return nil, status.Errorf(codes.Aborted, "create operation %s disappeared before seal; retry", truncID(op))
		}
		root, err = decodeRoot(e.Value)
		if err != nil {
			return nil, err
		}
		rootRev = e.Rev
		if root.State == rootPreparing {
			return nil, status.Errorf(codes.Aborted, "create operation %s still preparing; retry", truncID(op))
		}
	} else {
		e, found, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
		if err != nil || !found {
			if err != nil {
				return nil, err
			}
			return nil, status.Errorf(codes.Aborted, "create operation %s missing after seal", truncID(op))
		}
		root, err = decodeRoot(e.Value)
		if err != nil {
			return nil, err
		}
		rootRev = e.Rev
	}

	claim, rootRev, err := s.admitCreate(ctx, op, root, rootRev, v)
	if err != nil {
		var done errResumeDone
		if errors.As(err, &done) {
			return done.v, nil
		}
		return nil, err
	}
	root, rootRev, err = s.rereadRoot(ctx, op)
	if err != nil {
		return nil, err
	}
	return s.progressCreate(ctx, op, root, rootRev, claim, v)
}

func truncID(op string) string {
	if len(op) <= 12 {
		return op
	}
	return op[:12]
}

// resumeOrRefuse returns an existing volume when an identical create is already admitted or
// complete, AlreadyExists on conflict, Aborted for deleting/published cases, or (nil,nil) to plan.
func (s *Service) resumeOrRefuse(ctx context.Context, id string, want domain.CreateSpec) (*domain.Volume, error) {
	e, ok, err := s.kv.GetBounded(ctx, volumeKey(id), domain.MaxCreateRecordBytes)
	if err != nil {
		return nil, err
	}
	if !ok {
		if _, tok, err := s.kv.Get(ctx, tombstoneKey(id)); err != nil {
			return nil, err
		} else if tok {
			return nil, fmt.Errorf("%w: volume %q still has a deletion tombstone", domain.ErrExists, id)
		}
		return nil, nil
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return nil, err
	}
	// Deleting before Creating / missing-root (S6/S7).
	if h.Deleting {
		if isUnpublishedCreating(h) {
			return nil, status.Errorf(codes.Aborted, "volume %q is being deleted; retry after deletion finishes", id)
		}
		// Published Deleting: conservative public path owns it.
		return nil, status.Errorf(codes.Aborted, "volume %q is being deleted", id)
	}
	if h.Creating {
		if h.CreateOperation == "" || h.CreateSpec == nil {
			// Legacy creating claim: incompatible with adoption; may finish via rollback then fresh admit.
			if want.ScalarEqual(domain.CreateSpec{
				SizeBytes: h.SizeBytes, ChunkBytes: h.ChunkBytes, Replicas: h.Replicas, MinISR: h.MinISR,
			}) {
				if err := s.finishLegacyCreate(ctx, id, e.Rev, h, want); err != nil {
					return nil, err
				}
				return s.resumeOrRefuse(ctx, id, want)
			}
			return nil, fmt.Errorf("%w: volume %q", domain.ErrExists, id)
		}
		if !h.CreateSpec.Equal(want) {
			return nil, fmt.Errorf("%w: volume %q", domain.ErrExists, id)
		}
		return s.helpCreate(ctx, h.CreateOperation, e.Rev, h)
	}
	// Visible volume.
	if h.CreateSpec != nil {
		if !h.CreateSpec.Equal(want) {
			return nil, fmt.Errorf("%w: volume %q", domain.ErrExists, id)
		}
	} else if !want.ScalarEqual(domain.CreateSpec{
		SizeBytes: h.SizeBytes, ChunkBytes: h.ChunkBytes, Replicas: h.Replicas, MinISR: h.MinISR,
	}) {
		return nil, fmt.Errorf("%w: volume %q", domain.ErrExists, id)
	} else {
		// Legacy visible: scalar-only match succeeds without mutation.
	}
	return s.readVolume(ctx, id)
}

func (s *Service) planPlacement(ctx context.Context, spec domain.Spec) ([][]string, error) {
	nodes, err := s.nodes(ctx)
	if err != nil {
		return nil, err
	}
	load, err := s.replicaLoad(ctx)
	if err != nil {
		return nil, err
	}
	var candidates []domain.NodeLoad
	for _, n := range nodes {
		if _, witness := n.GetLabels()[WitnessLabel]; n.GetAlive() && !witness && !n.GetDraining() {
			candidates = append(candidates, domain.LoadFromLabels(n.GetId(), load[n.GetId()], n.GetLabels()))
		}
	}
	h := domain.Header{ID: spec.ID, SizeBytes: spec.SizeBytes, ChunkBytes: spec.ChunkBytes}
	return domain.Place(candidates, h.ChunkCount(), int(spec.Replicas), spec.PreferNode, spec.Domain, spec.Policy)
}

// replicaLoad pages public chunk keys and counts replicas. Private create-plans are not listed.
func (s *Service) replicaLoad(ctx context.Context) (map[string]int, error) {
	load := map[string]int{}
	after := ""
	for {
		page, err := s.kv.ScanKeys(ctx, prefix+"chunks/", after, kv.ScanKeysMaxLimit)
		if err != nil {
			return nil, err
		}
		for _, kr := range page.Entries {
			e, ok, err := s.kv.GetBounded(ctx, kr.Key, domain.MaxCreateRecordBytes)
			if err != nil {
				if errors.Is(err, kv.ErrOversized) || status.Code(err) == codes.DataLoss {
					continue
				}
				return nil, err
			}
			if !ok {
				continue
			}
			c, err := decodeChunkBounded(e.Value)
			if err != nil {
				continue
			}
			for _, n := range c.Replicas {
				load[n]++
			}
			after = kr.Key
		}
		if page.Done {
			break
		}
		if page.Next != "" {
			after = page.Next
		}
		if len(page.Entries) == 0 {
			break
		}
	}
	return load, nil
}

// createInitialRoot writes birth+root under birth previous revision and root absence.
// Returns the root revision, or 0 when the transaction did not apply (caller must reread).
func (s *Service) createInitialRoot(ctx context.Context, op string, root createRoot) (int64, error) {
	birth := createBirthKey(root.Volume)
	be, bok, err := s.kv.Get(ctx, birth)
	if err != nil {
		return 0, err
	}
	birthRev := int64(0)
	if bok {
		birthRev = be.Rev
	}
	rb, err := encodeRecord(root)
	if err != nil {
		return 0, err
	}
	cmps := []kv.Cmp{{Key: createRootKey(op), Rev: 0}, {Key: birth, Rev: birthRev}}
	ops := []kv.Op{
		{Key: createRootKey(op), Value: rb},
		{Key: birth, Value: []byte("1")},
	}
	rev, ok, err := s.kv.TxnRev(ctx, cmps, ops)
	if err != nil {
		return 0, err
	}
	if !ok {
		return 0, nil
	}
	return rev, nil
}

func (s *Service) stagePlans(ctx context.Context, op string, root createRoot, rootRev int64, chunks []domain.Chunk) (int64, error) {
	for root.Staged < uint32(len(chunks)) {
		if root.State != rootPreparing {
			break
		}
		start := root.Staged
		end := min(start+createBatchSize, uint32(len(chunks)))
		ops := make([]kv.Op, 0, int(end-start)+1)
		for i := start; i < end; i++ {
			b, err := encodeRecord(chunks[i])
			if err != nil {
				return 0, err
			}
			ops = append(ops, kv.Op{Key: createPlanKey(op, i), Value: b})
		}
		next := root
		next.Staged = end
		rb, err := encodeRecord(next)
		if err != nil {
			return 0, err
		}
		ops = append(ops, kv.Op{Key: createRootKey(op), Value: rb})
		ok, err := s.kv.Txn(ctx, []kv.Cmp{{Key: createRootKey(op), Rev: rootRev}}, ops)
		if err != nil {
			return 0, err
		}
		if !ok {
			e, found, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
			if err != nil {
				return 0, err
			}
			if !found {
				return 0, status.Errorf(codes.Aborted, "create operation %s revoked during staging", truncID(op))
			}
			root, err = decodeRoot(e.Value)
			if err != nil {
				return 0, err
			}
			rootRev = e.Rev
			continue
		}
		root = next
		e, found, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
		if err != nil || !found {
			if err != nil {
				return 0, err
			}
			return 0, status.Errorf(codes.Aborted, "create operation %s missing after staging", truncID(op))
		}
		root, err = decodeRoot(e.Value)
		if err != nil {
			return 0, err
		}
		rootRev = e.Rev
	}
	return rootRev, nil
}

func (s *Service) admitCreate(ctx context.Context, op string, root createRoot, rootRev int64, v *domain.Volume) (claim int64, newRootRev int64, err error) {
	if root.State != rootSealed && root.State != rootCleaning && root.State != rootCopying && root.State != rootComplete {
		return 0, 0, status.Errorf(codes.Aborted, "create operation %s not sealed", truncID(op))
	}
	if root.State != rootSealed {
		// Already admitted: discover header.
		e, ok, err := s.kv.GetBounded(ctx, volumeKey(root.Volume), domain.MaxCreateRecordBytes)
		if err != nil {
			return 0, 0, err
		}
		if !ok {
			return 0, 0, status.Errorf(codes.Aborted, "create operation %s admitted but header missing", truncID(op))
		}
		h, err := decodeHeaderBounded(e.Value)
		if err != nil {
			return 0, 0, err
		}
		if h.CreateOperation != op {
			return 0, 0, status.Errorf(codes.Aborted, "create operation %s no longer owns %q", truncID(op), root.Volume)
		}
		return e.Rev, rootRev, nil
	}
	v.CreateOperation = op
	spec := root.Spec
	v.CreateSpec = &spec
	v.Creating = true
	v.Epoch = 0
	v.Target = ""
	v.FirstEpoch = 0
	hb, err := encodeRecord(v.Header)
	if err != nil {
		return 0, 0, err
	}
	next := root
	next.State = rootCleaning
	next.Next = 0
	rb, err := encodeRecord(next)
	if err != nil {
		return 0, 0, err
	}
	cmps := []kv.Cmp{
		{Key: createRootKey(op), Rev: rootRev},
		{Key: volumeKey(root.Volume), Rev: 0},
		{Key: tombstoneKey(root.Volume), Rev: 0},
	}
	ops := []kv.Op{
		{Key: volumeKey(root.Volume), Value: hb},
		{Key: createRootKey(op), Value: rb},
	}
	claim, ok, err := s.kv.TxnRev(ctx, cmps, ops)
	if err != nil {
		return 0, 0, err
	}
	if !ok {
		// Discover: never infer uncommitted admission from a failed reply.
		if v2, err := s.resumeOrRefuse(ctx, root.Volume, root.Spec); err != nil || v2 != nil {
			if v2 != nil {
				return 0, 0, errResumeDone{v: v2}
			}
			return 0, 0, err
		}
		e, found, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
		if err != nil {
			return 0, 0, err
		}
		if !found {
			return 0, 0, status.Errorf(codes.Aborted, "create operation %s discarded before admission; retry", truncID(op))
		}
		root, err = decodeRoot(e.Value)
		if err != nil {
			return 0, 0, err
		}
		if root.State == rootSealed {
			// Gate/name conflict before admission: discard unadmitted work by GC eligibility later;
			// this caller replans under a fresh op.
			return 0, 0, status.Errorf(codes.Aborted, "volume %q could not be admitted; retry", root.Volume)
		}
		he, hok, err := s.kv.GetBounded(ctx, volumeKey(root.Volume), domain.MaxCreateRecordBytes)
		if err != nil {
			return 0, 0, err
		}
		if !hok {
			return 0, 0, status.Errorf(codes.Aborted, "create operation %s state %s without header", truncID(op), root.State)
		}
		return he.Rev, e.Rev, nil
	}
	return claim, claim, nil // root wrote in same txn → same rev on etcd; Mem too
}

type errResumeDone struct{ v *domain.Volume }

func (e errResumeDone) Error() string { return "create already complete" }

func (s *Service) helpCreate(ctx context.Context, op string, claim int64, h domain.Header) (*domain.Volume, error) {
	e, ok, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
	if err != nil {
		return nil, err
	}
	if !ok {
		if h.Creating && !h.Deleting {
			return nil, status.Errorf(codes.DataLoss, "creating volume %q missing create root %s", h.ID, truncID(op))
		}
		return nil, status.Errorf(codes.Aborted, "create operation %s gone", truncID(op))
	}
	root, err := decodeRoot(e.Value)
	if err != nil {
		return nil, err
	}
	if root.Volume != h.ID || !root.Spec.Equal(*h.CreateSpec) {
		return nil, status.Errorf(codes.Aborted, "create operation %s identity mismatch", truncID(op))
	}
	v := &domain.Volume{Header: h}
	return s.progressCreate(ctx, op, root, e.Rev, claim, v)
}

func (s *Service) progressCreate(ctx context.Context, op string, root createRoot, rootRev, claim int64, v *domain.Volume) (*domain.Volume, error) {
	for {
		switch root.State {
		case rootCleaning:
			var err error
			root, rootRev, err = s.stepCleaning(ctx, op, root, rootRev, claim)
			if err != nil {
				return nil, err
			}
		case rootCopying:
			var err error
			root, rootRev, err = s.stepCopying(ctx, op, root, rootRev, claim)
			if err != nil {
				return nil, err
			}
		case rootComplete:
			return s.readCompletedCreate(ctx, root.Volume, op, root.Spec)
		default:
			return nil, status.Errorf(codes.Aborted, "create operation %s in state %s", truncID(op), root.State)
		}
		if err := ctx.Err(); err != nil {
			return nil, err
		}
	}
}

func (s *Service) helperCmps(op string, rootRev, claim int64, volume string) []kv.Cmp {
	return []kv.Cmp{
		{Key: volumeKey(volume), Rev: claim},
		{Key: createRootKey(op), Rev: rootRev},
	}
}

func (s *Service) checkHelperHeader(ctx context.Context, volume, op string, claim int64) error {
	e, ok, err := s.kv.GetBounded(ctx, volumeKey(volume), domain.MaxCreateRecordBytes)
	if err != nil {
		return err
	}
	if !ok || e.Rev != claim {
		return status.Errorf(codes.Aborted, "volume %q changed while being created", volume)
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return err
	}
	if h.Deleting || !h.Creating || h.CreateOperation != op {
		return status.Errorf(codes.Aborted, "volume %q changed while being created", volume)
	}
	return nil
}

func (s *Service) stepCleaning(ctx context.Context, op string, root createRoot, rootRev, claim int64) (createRoot, int64, error) {
	if err := s.checkHelperHeader(ctx, root.Volume, op, claim); err != nil {
		return root, rootRev, err
	}
	page, err := s.kv.ScanKeys(ctx, chunkPrefix(root.Volume), "", createBatchSize)
	if err != nil {
		return root, rootRev, err
	}
	if len(page.Entries) == 0 {
		next := root
		next.State = rootCopying
		next.Next = 0
		rb, err := encodeRecord(next)
		if err != nil {
			return root, rootRev, err
		}
		ok, err := s.kv.Txn(ctx, s.helperCmps(op, rootRev, claim, root.Volume), []kv.Op{{Key: createRootKey(op), Value: rb}})
		if err != nil {
			return root, rootRev, err
		}
		if !ok {
			return s.rereadRoot(ctx, op)
		}
		return s.rereadRoot(ctx, op)
	}
	ops := make([]kv.Op, 0, len(page.Entries)+1)
	for _, kr := range page.Entries {
		ops = append(ops, kv.Op{Key: kr.Key, Delete: true})
	}
	// Progress-only: keep cleaning until the public prefix is empty. Next stays 0 until copying.
	rb, err := encodeRecord(root)
	if err != nil {
		return root, rootRev, err
	}
	ops = append(ops, kv.Op{Key: createRootKey(op), Value: rb})
	ok, err := s.kv.Txn(ctx, s.helperCmps(op, rootRev, claim, root.Volume), ops)
	if err != nil {
		return root, rootRev, err
	}
	if !ok {
		return s.rereadRoot(ctx, op)
	}
	return s.rereadRoot(ctx, op)
}

func (s *Service) stepCopying(ctx context.Context, op string, root createRoot, rootRev, claim int64) (createRoot, int64, error) {
	if err := s.checkHelperHeader(ctx, root.Volume, op, claim); err != nil {
		return root, rootRev, err
	}
	if int(root.Next) >= root.ChunkCount {
		return s.finalizeCreate(ctx, op, root, rootRev, claim)
	}
	start := root.Next
	end := min(start+createBatchSize, uint32(root.ChunkCount))
	ops := make([]kv.Op, 0, int(end-start)+1)
	for i := start; i < end; i++ {
		pe, ok, err := s.kv.GetBounded(ctx, createPlanKey(op, i), domain.MaxCreateRecordBytes)
		if err != nil {
			return root, rootRev, err
		}
		if !ok {
			return root, rootRev, status.Errorf(codes.DataLoss, "create operation %s missing plan chunk %d", truncID(op), i)
		}
		c, err := decodeChunkBounded(pe.Value)
		if err != nil {
			return root, rootRev, err
		}
		if len(c.ISR) == 0 || len(c.Replicas) == 0 {
			return root, rootRev, status.Errorf(codes.DataLoss, "create operation %s plan chunk %d has empty ISR", truncID(op), i)
		}
		ops = append(ops, kv.Op{Key: chunkKey(root.Volume, i), Value: pe.Value})
	}
	next := root
	next.Next = end
	rb, err := encodeRecord(next)
	if err != nil {
		return root, rootRev, err
	}
	ops = append(ops, kv.Op{Key: createRootKey(op), Value: rb})
	ok, err := s.kv.Txn(ctx, s.helperCmps(op, rootRev, claim, root.Volume), ops)
	if err != nil {
		return root, rootRev, err
	}
	if !ok {
		return s.rereadRoot(ctx, op)
	}
	return s.rereadRoot(ctx, op)
}

func (s *Service) finalizeCreate(ctx context.Context, op string, root createRoot, rootRev, claim int64) (createRoot, int64, error) {
	e, ok, err := s.kv.GetBounded(ctx, volumeKey(root.Volume), domain.MaxCreateRecordBytes)
	if err != nil {
		return root, rootRev, err
	}
	if !ok || e.Rev != claim {
		return root, rootRev, status.Errorf(codes.Aborted, "volume %q changed while being created", root.Volume)
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return root, rootRev, err
	}
	if h.Deleting || !h.Creating || h.CreateOperation != op {
		return root, rootRev, status.Errorf(codes.Aborted, "volume %q changed while being created", root.Volume)
	}
	// Keep create_operation / create_spec; set FirstEpoch=claim; clear Creating.
	h.Start(uint64(claim))
	h.Creating = false
	hb, err := encodeRecord(h)
	if err != nil {
		return root, rootRev, err
	}
	next := root
	next.State = rootComplete
	rb, err := encodeRecord(next)
	if err != nil {
		return root, rootRev, err
	}
	ok, err = s.kv.Txn(ctx, s.helperCmps(op, rootRev, claim, root.Volume), []kv.Op{
		{Key: volumeKey(root.Volume), Value: hb},
		{Key: createRootKey(op), Value: rb},
	})
	if err != nil {
		return root, rootRev, err
	}
	if !ok {
		return s.rereadRoot(ctx, op)
	}
	return s.rereadRoot(ctx, op)
}

func (s *Service) rereadRoot(ctx context.Context, op string) (createRoot, int64, error) {
	e, ok, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
	if err != nil {
		return createRoot{}, 0, err
	}
	if !ok {
		return createRoot{}, 0, status.Errorf(codes.Aborted, "create operation %s gone", truncID(op))
	}
	root, err := decodeRoot(e.Value)
	return root, e.Rev, err
}

func (s *Service) readCompletedCreate(ctx context.Context, id, op string, want domain.CreateSpec) (*domain.Volume, error) {
	// #77 coherent ReadSet is not landed yet; verify operation/identity on a fresh ordinary read.
	v, err := s.readVolume(ctx, id)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, status.Errorf(codes.Aborted, "volume %q replaced during create response", id)
		}
		return nil, err
	}
	if v.CreateOperation != op {
		return nil, status.Errorf(codes.Aborted, "volume %q create operation changed", id)
	}
	if v.CreateSpec == nil || !v.CreateSpec.Equal(want) {
		return nil, status.Errorf(codes.Aborted, "volume %q create identity changed", id)
	}
	return v, nil
}

// isUnpublishedCreating is the recognized never-published shape (S8): Creating, epoch 0, empty
// target. May also have Deleting set during rollback. Publication clears Creating while setting
// a positive claim epoch atomically.
func isUnpublishedCreating(h domain.Header) bool {
	return h.Creating && h.Epoch == 0 && h.Target == ""
}

// stepUnpublishedRollback CAS-deletes only the exact unpublished header, then pages orphan chunks.
func (s *Service) stepUnpublishedRollback(ctx context.Context, expectedHeader domain.Header, headerRev int64) error {
	if !isUnpublishedCreating(expectedHeader) {
		return status.Error(codes.FailedPrecondition, "header is not a recognized unpublished creating claim")
	}
	id := expectedHeader.ID
	ok, err := s.kv.Txn(ctx,
		[]kv.Cmp{{Key: volumeKey(id), Rev: headerRev}, {Key: tombstoneKey(id), Rev: 0}},
		[]kv.Op{{Key: volumeKey(id), Delete: true}},
	)
	if err != nil {
		return err
	}
	if !ok {
		return status.Errorf(codes.Aborted, "volume %q changed during unpublished rollback", id)
	}
	return s.cleanupOrphanPublicChunks(ctx, id)
}

func (s *Service) cleanupOrphanPublicChunks(ctx context.Context, id string) error {
	after := ""
	for {
		page, err := s.kv.ScanKeys(ctx, chunkPrefix(id), after, createBatchSize)
		if err != nil {
			return err
		}
		if len(page.Entries) == 0 {
			if page.Done {
				return nil
			}
			after = page.Next
			continue
		}
		// Header must still be absent; each key at its listed revision.
		cmps := []kv.Cmp{{Key: volumeKey(id), Rev: 0}}
		ops := make([]kv.Op, 0, len(page.Entries))
		for _, kr := range page.Entries {
			cmps = append(cmps, kv.Cmp{Key: kr.Key, Rev: kr.Rev})
			ops = append(ops, kv.Op{Key: kr.Key, Delete: true})
			after = kr.Key
		}
		ok, err := s.kv.Txn(ctx, cmps, ops)
		if err != nil {
			return err
		}
		if !ok {
			// Stale: reread; do not cross ABA.
			continue
		}
		if page.Done && len(page.Entries) < createBatchSize {
			return nil
		}
	}
}

// finishLegacyCreate rolls back a recognized legacy creating header then returns so the caller
// can freshly admit. Budget/deadline exhaustion returns Aborted.
func (s *Service) finishLegacyCreate(ctx context.Context, id string, rev int64, h domain.Header, _ domain.CreateSpec) error {
	if !isUnpublishedCreating(h) {
		return fmt.Errorf("%w: volume %q", domain.ErrExists, id)
	}
	h.ID = id
	if err := s.stepUnpublishedRollback(ctx, h, rev); err != nil {
		if status.Code(err) == codes.Aborted || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			return err
		}
		return status.Errorf(codes.Aborted, "legacy create rollback for %q incomplete: %v", id, err)
	}
	return nil
}

// getRawHeader reads a volume header without Creating/Deleting filtering.
func (s *Service) getRawHeader(ctx context.Context, id string) (domain.Header, int64, bool, error) {
	e, ok, err := s.kv.GetBounded(ctx, volumeKey(id), domain.MaxCreateRecordBytes)
	if err != nil || !ok {
		return domain.Header{}, 0, false, err
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return domain.Header{}, e.Rev, true, err
	}
	return h, e.Rev, true, nil
}

func parseCreateOpFromRootKey(key string) string {
	return strings.TrimPrefix(key, createRootPrefix)
}

func parseCreateOpFromPlanKey(key string) (op string, ok bool) {
	rest := strings.TrimPrefix(key, createPlanPrefix)
	i := strings.IndexByte(rest, '/')
	if i <= 0 {
		return "", false
	}
	return rest[:i], true
}
