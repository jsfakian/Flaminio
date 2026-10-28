// Package service implements the metadata service's gRPC API on top of the domain
// transitions and the KV store.
//
// Keys, all under /flaminio/v1/:
//
//	volumes/<id>             volume header (JSON)
//	chunks/<id>/<index>      one chunk's placement and ISR (JSON), index as 8 digits
//	nodes/<id>               registered node (JSON)
//	alive/<id>               liveness record, expires without heartbeats
//	tombstones/<id>          deleted volume awaiting replica cleanup (JSON)
//
// Every update is an optimistic transaction: read, check with the domain rules, then write
// conditional on the revisions read. A conflict means a concurrent update; the operation is
// retried from the read.
package service

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math/rand/v2"
	"slices"
	"strings"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/pkg/captoken"
)

const (
	prefix  = "/flaminio/v1/"
	retries = 32
	// WitnessLabel marks a node that votes in the metadata quorum but holds no volume data
	// (ARCHITECTURE §1.2): placement never uses it.
	WitnessLabel = "flaminio.io/witness"
)

// backoff waits before retrying after a conflicting update: a short, growing, random delay,
// so that contending writers spread out instead of colliding again.
func backoff(ctx context.Context, attempt int) error {
	d := time.Duration(rand.Int64N(int64(time.Millisecond) << min(attempt, 6)))
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(d):
		return nil
	}
}

func volumeKey(id string) string          { return prefix + "volumes/" + id }
func chunkPrefix(id string) string        { return prefix + "chunks/" + id + "/" }
func chunkKey(id string, i uint32) string { return fmt.Sprintf("%s%08d", chunkPrefix(id), i) }
func nodeKey(id string) string            { return prefix + "nodes/" + id }
func aliveKey(id string) string           { return prefix + "alive/" + id }
func tombstoneKey(id string) string       { return prefix + "tombstones/" + id }

// tombstoneRecord is the etcd value for a deleted volume until every node that held its
// chunks has confirmed local deletion (docs/followups.md §8).
type tombstoneRecord struct {
	Nodes     []string        `json:"nodes"`
	Confirmed map[string]bool `json:"confirmed,omitempty"`
}

func (t tombstoneRecord) pending() []string {
	var out []string
	for _, n := range t.Nodes {
		if !t.Confirmed[n] {
			out = append(out, n)
		}
	}
	return out
}

func tombstonePB(id string, t tombstoneRecord) *metav1.Tombstone {
	return &metav1.Tombstone{Volume: id, Nodes: append([]string{}, t.Nodes...), Pending: t.pending()}
}

// replicaNodes returns every node that held (or was moving) a replica of the volume's chunks.
func replicaNodes(chunks []domain.Chunk) []string {
	seen := map[string]struct{}{}
	var out []string
	add := func(n string) {
		if n == "" {
			return
		}
		if _, ok := seen[n]; ok {
			return
		}
		seen[n] = struct{}{}
		out = append(out, n)
	}
	for _, c := range chunks {
		for _, n := range c.Replicas {
			add(n)
		}
		if c.Moving != nil {
			add(c.Moving.From)
			add(c.Moving.To)
		}
	}
	slices.Sort(out)
	return out
}

// Service is the metadata service.
type Service struct {
	metav1.UnimplementedMetadataServiceServer
	kv   kv.KV
	opts Options
}

// New returns a service on store.
func New(store kv.KV, opts Options) *Service {
	if opts.TTL == 0 {
		opts.TTL = 10 * time.Second
	}
	return &Service{kv: store, opts: opts}
}

func errStatus(err error) error {
	switch {
	case err == nil:
		return nil
	case errors.Is(err, domain.ErrInvalid):
		return status.Error(codes.InvalidArgument, err.Error())
	case errors.Is(err, domain.ErrNotFound):
		return status.Error(codes.NotFound, err.Error())
	case errors.Is(err, domain.ErrExists):
		return status.Error(codes.AlreadyExists, err.Error())
	case errors.Is(err, domain.ErrPlacement), errors.Is(err, domain.ErrNotTarget):
		return status.Error(codes.FailedPrecondition, err.Error())
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return status.FromContextError(err).Err()
	}
	if _, ok := status.FromError(err); ok {
		return err
	}
	return status.Error(codes.Unavailable, err.Error())
}

type nodeRecord struct {
	Address string            `json:"address"`
	Labels  map[string]string `json:"labels,omitempty"`
}

func (s *Service) RegisterNode(ctx context.Context, r *metav1.RegisterNodeRequest) (*metav1.RegisterNodeResponse, error) {
	n := r.GetNode()
	if n == nil || !domain.ValidID(n.GetId()) || n.GetAddress() == "" {
		return nil, errStatus(fmt.Errorf("%w: node needs a valid id and an address", domain.ErrInvalid))
	}
	// Merge labels into any existing record so the data plane's address registration does not
	// wipe failure-domain labels the agent set, and the agent can overlay zone/rack without
	// clearing other labels (e.g. witness).
	labels := map[string]string{}
	if e, ok, err := s.kv.Get(ctx, nodeKey(n.GetId())); err != nil {
		return nil, errStatus(err)
	} else if ok {
		var rec nodeRecord
		if json.Unmarshal(e.Value, &rec) == nil {
			for k, v := range rec.Labels {
				labels[k] = v
			}
		}
	}
	for k, v := range n.GetLabels() {
		labels[k] = v
	}
	b, _ := json.Marshal(nodeRecord{Address: n.GetAddress(), Labels: labels})
	_, err := s.kv.Txn(ctx, nil, []kv.Op{{Key: nodeKey(n.GetId()), Value: b}})
	return &metav1.RegisterNodeResponse{}, errStatus(err)
}

func (s *Service) NodeHeartbeat(ctx context.Context, r *metav1.NodeHeartbeatRequest) (*metav1.NodeHeartbeatResponse, error) {
	if _, ok, err := s.kv.Get(ctx, nodeKey(r.GetNode())); err != nil || !ok {
		if err == nil {
			err = fmt.Errorf("%w: node %q is not registered", domain.ErrNotFound, r.GetNode())
		}
		return nil, errStatus(err)
	}
	if err := s.kv.PutTTL(ctx, aliveKey(r.GetNode()), []byte("1"), s.opts.TTL); err != nil {
		return nil, errStatus(err)
	}
	return &metav1.NodeHeartbeatResponse{TtlSeconds: uint32(s.opts.TTL / time.Second)}, nil
}

func (s *Service) ListNodes(ctx context.Context, _ *metav1.ListNodesRequest) (*metav1.ListNodesResponse, error) {
	nodes, err := s.nodes(ctx)
	return &metav1.ListNodesResponse{Nodes: nodes}, errStatus(err)
}

func (s *Service) nodes(ctx context.Context) ([]*metav1.Node, error) {
	entries, err := s.kv.List(ctx, prefix+"nodes/")
	if err != nil {
		return nil, err
	}
	alive, err := s.kv.List(ctx, prefix+"alive/")
	if err != nil {
		return nil, err
	}
	live := map[string]bool{}
	for _, e := range alive {
		live[strings.TrimPrefix(e.Key, prefix+"alive/")] = true
	}
	// Kept apart from the node record, which the node rewrites whenever it registers.
	drains, err := s.kv.List(ctx, prefix+"drain/")
	if err != nil {
		return nil, err
	}
	draining := map[string]bool{}
	for _, e := range drains {
		draining[strings.TrimPrefix(e.Key, prefix+"drain/")] = true
	}
	var out []*metav1.Node
	for _, e := range entries {
		var rec nodeRecord
		if err := json.Unmarshal(e.Value, &rec); err != nil {
			return nil, err
		}
		id := strings.TrimPrefix(e.Key, prefix+"nodes/")
		out = append(out, &metav1.Node{Id: id, Address: rec.Address, Labels: rec.Labels, Alive: live[id], Draining: draining[id]})
	}
	return out, nil
}

// readHeader returns a volume's header and its revision. A volume still being created does not
// exist yet.
func (s *Service) readHeader(ctx context.Context, id string) (domain.Header, int64, error) {
	e, ok, err := s.kv.Get(ctx, volumeKey(id))
	if err != nil {
		return domain.Header{}, 0, err
	}
	var h domain.Header
	if ok {
		if err := json.Unmarshal(e.Value, &h); err != nil {
			return h, 0, err
		}
	}
	if !ok || h.Creating {
		return h, 0, fmt.Errorf("%w: volume %q", domain.ErrNotFound, id)
	}
	return h, e.Rev, nil
}

func (s *Service) readVolume(ctx context.Context, id string) (*domain.Volume, error) {
	h, _, err := s.readHeader(ctx, id)
	if err != nil {
		return nil, err
	}
	entries, err := s.kv.List(ctx, chunkPrefix(id))
	if err != nil {
		return nil, err
	}
	if len(entries) != h.ChunkCount() {
		return nil, fmt.Errorf("volume %q: %d chunk records, want %d", id, len(entries), h.ChunkCount())
	}
	v := &domain.Volume{Header: h, Chunks: make([]domain.Chunk, len(entries))}
	for i, e := range entries {
		if err := json.Unmarshal(e.Value, &v.Chunks[i]); err != nil {
			return nil, err
		}
	}
	return v, nil
}

func chunkPB(c domain.Chunk) *metav1.ChunkMeta {
	out := &metav1.ChunkMeta{Replicas: c.Replicas, Isr: c.ISR, IsrVer: c.ISRVer}
	if c.Moving != nil {
		out.MovingFrom, out.MovingTo = c.Moving.From, c.Moving.To
	}
	return out
}

func volumePB(v *domain.Volume) *metav1.Volume {
	out := &metav1.Volume{
		Id: v.ID, SizeBytes: v.SizeBytes, ChunkBytes: v.ChunkBytes, Replicas: v.Replicas,
		MinIsr: v.MinISR, Epoch: v.Epoch, Target: v.Target,
	}
	for _, c := range v.Chunks {
		out.Chunks = append(out.Chunks, chunkPB(c))
	}
	return out
}

func (s *Service) CreateVolume(ctx context.Context, r *metav1.CreateVolumeRequest) (*metav1.CreateVolumeResponse, error) {
	fd, err := domain.ParseFailureDomain(r.GetFailureDomain())
	if err != nil {
		return nil, errStatus(err)
	}
	pol, err := domain.ParsePlacementPolicy(r.GetFailureDomainPolicy())
	if err != nil {
		return nil, errStatus(err)
	}
	spec := domain.Spec{
		ID: r.GetId(), SizeBytes: r.GetSizeBytes(), ChunkBytes: r.GetChunkBytes(),
		Replicas: r.GetReplicas(), MinISR: r.GetMinIsr(), PreferNode: r.GetPreferNode(),
		Domain: fd, Policy: pol,
	}
	if err := spec.Validate(); err != nil {
		return nil, errStatus(err)
	}
	v, err := s.createVolume(ctx, spec)
	if err != nil {
		return nil, errStatus(err)
	}
	return &metav1.CreateVolumeResponse{Volume: volumePB(v)}, nil
}

func (s *Service) createVolume(ctx context.Context, spec domain.Spec) (*domain.Volume, error) {
	// A tombstone for this id means nodes may still be deleting the previous volume's
	// replicas; reusing the id would let that sweep wipe the new volume.
	if _, ok, err := s.kv.Get(ctx, tombstoneKey(spec.ID)); err != nil {
		return nil, err
	} else if ok {
		return nil, fmt.Errorf("%w: volume %q still has a deletion tombstone", domain.ErrExists, spec.ID)
	}
	nodes, err := s.nodes(ctx)
	if err != nil {
		return nil, err
	}
	load := map[string]int{}
	chunks, err := s.kv.List(ctx, prefix+"chunks/")
	if err != nil {
		return nil, err
	}
	for _, e := range chunks {
		var c domain.Chunk
		if json.Unmarshal(e.Value, &c) == nil {
			for _, n := range c.Replicas {
				load[n]++
			}
		}
	}
	var candidates []domain.NodeLoad
	for _, n := range nodes {
		if _, witness := n.GetLabels()[WitnessLabel]; n.GetAlive() && !witness && !n.GetDraining() {
			candidates = append(candidates, domain.LoadFromLabels(n.GetId(), load[n.GetId()], n.GetLabels()))
		}
	}
	h := domain.Header{ID: spec.ID, SizeBytes: spec.SizeBytes, ChunkBytes: spec.ChunkBytes}
	placement, err := domain.Place(candidates, h.ChunkCount(), int(spec.Replicas), spec.PreferNode, spec.Domain, spec.Policy)
	if err != nil {
		return nil, err
	}
	v := domain.NewVolume(spec, placement)

	// Claim the id with a header marked as being created, write the chunks in batches, then
	// clear the mark. Until then the volume does not exist for anyone else. The tombstone
	// compare keeps a concurrent DeleteVolume's leftover from racing with this claim.
	v.Creating = true
	hb, _ := json.Marshal(v.Header)
	ok, err := s.kv.Txn(ctx,
		[]kv.Cmp{{Key: volumeKey(spec.ID), Rev: 0}, {Key: tombstoneKey(spec.ID), Rev: 0}},
		[]kv.Op{{Key: volumeKey(spec.ID), Value: hb}})
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fmt.Errorf("%w: volume %q", domain.ErrExists, spec.ID)
	}
	if err := s.deleteChunks(ctx, spec.ID); err != nil { // leftovers of an interrupted delete
		return nil, err
	}
	var ops []kv.Op
	for i, c := range v.Chunks {
		b, _ := json.Marshal(c)
		ops = append(ops, kv.Op{Key: chunkKey(spec.ID, uint32(i)), Value: b})
		if len(ops) == kv.MaxTxnOps || i == len(v.Chunks)-1 {
			if _, err := s.kv.Txn(ctx, nil, ops); err != nil {
				return nil, err
			}
			ops = ops[:0]
		}
	}
	e, _, err := s.kv.Get(ctx, volumeKey(spec.ID))
	if err != nil {
		return nil, err
	}
	v.Creating = false
	hb, _ = json.Marshal(v.Header)
	ok, err = s.kv.Txn(ctx, []kv.Cmp{{Key: volumeKey(spec.ID), Rev: e.Rev}}, []kv.Op{{Key: volumeKey(spec.ID), Value: hb}})
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fmt.Errorf("volume %q changed while being created", spec.ID)
	}
	return v, nil
}

func (s *Service) deleteChunks(ctx context.Context, id string) error {
	entries, err := s.kv.List(ctx, chunkPrefix(id))
	if err != nil {
		return err
	}
	for len(entries) > 0 {
		n := min(len(entries), kv.MaxTxnOps)
		ops := make([]kv.Op, n)
		for i := range n {
			ops[i] = kv.Op{Key: entries[i].Key, Delete: true}
		}
		if _, err := s.kv.Txn(ctx, nil, ops); err != nil {
			return err
		}
		entries = entries[n:]
	}
	return nil
}

func (s *Service) GetVolume(ctx context.Context, r *metav1.GetVolumeRequest) (*metav1.GetVolumeResponse, error) {
	v, err := s.readVolume(ctx, r.GetId())
	if err != nil {
		return nil, errStatus(err)
	}
	return &metav1.GetVolumeResponse{Volume: volumePB(v)}, nil
}

// DeleteVolume removes the header at once (so the volume disappears), writes a tombstone naming
// every node that held its chunks, then drops the chunk metadata. Nodes delete their local
// replicas and ConfirmDeletion; the tombstone goes when all have confirmed.
func (s *Service) DeleteVolume(ctx context.Context, r *metav1.DeleteVolumeRequest) (*metav1.DeleteVolumeResponse, error) {
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return nil, errStatus(err)
			}
		}
		e, ok, err := s.kv.Get(ctx, volumeKey(r.GetId()))
		if err != nil {
			return nil, errStatus(err)
		}
		if !ok {
			return nil, errStatus(fmt.Errorf("%w: volume %q", domain.ErrNotFound, r.GetId()))
		}
		entries, err := s.kv.List(ctx, chunkPrefix(r.GetId()))
		if err != nil {
			return nil, errStatus(err)
		}
		chunks := make([]domain.Chunk, 0, len(entries))
		for _, ce := range entries {
			var c domain.Chunk
			if err := json.Unmarshal(ce.Value, &c); err != nil {
				return nil, errStatus(err)
			}
			chunks = append(chunks, c)
		}
		ops := []kv.Op{{Key: e.Key, Delete: true}}
		if nodes := replicaNodes(chunks); len(nodes) > 0 {
			tb, _ := json.Marshal(tombstoneRecord{Nodes: nodes})
			ops = append(ops, kv.Op{Key: tombstoneKey(r.GetId()), Value: tb})
		}
		ok, err = s.kv.Txn(ctx, []kv.Cmp{{Key: e.Key, Rev: e.Rev}}, ops)
		if err != nil {
			return nil, errStatus(err)
		}
		if ok {
			return &metav1.DeleteVolumeResponse{}, errStatus(s.deleteChunks(ctx, r.GetId()))
		}
	}
	return nil, status.Error(codes.Aborted, "too much contention")
}

func (s *Service) ListTombstones(ctx context.Context, r *metav1.ListTombstonesRequest) (*metav1.ListTombstonesResponse, error) {
	if !domain.ValidID(r.GetNode()) {
		return nil, errStatus(fmt.Errorf("%w: node id %q", domain.ErrInvalid, r.GetNode()))
	}
	entries, err := s.kv.List(ctx, prefix+"tombstones/")
	if err != nil {
		return nil, errStatus(err)
	}
	out := &metav1.ListTombstonesResponse{}
	for _, e := range entries {
		var t tombstoneRecord
		if err := json.Unmarshal(e.Value, &t); err != nil {
			return nil, errStatus(err)
		}
		if t.Confirmed[r.GetNode()] {
			continue
		}
		pending := false
		for _, n := range t.Nodes {
			if n == r.GetNode() {
				pending = true
				break
			}
		}
		if !pending {
			continue
		}
		id := strings.TrimPrefix(e.Key, prefix+"tombstones/")
		out.Tombstones = append(out.Tombstones, tombstonePB(id, t))
	}
	return out, nil
}

func (s *Service) ConfirmDeletion(ctx context.Context, r *metav1.ConfirmDeletionRequest) (*metav1.ConfirmDeletionResponse, error) {
	if !domain.ValidID(r.GetVolume()) || !domain.ValidID(r.GetNode()) {
		return nil, errStatus(fmt.Errorf("%w: volume %q, node %q", domain.ErrInvalid, r.GetVolume(), r.GetNode()))
	}
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return nil, errStatus(err)
			}
		}
		e, ok, err := s.kv.Get(ctx, tombstoneKey(r.GetVolume()))
		if err != nil {
			return nil, errStatus(err)
		}
		if !ok {
			// Already fully confirmed and removed: idempotent success.
			return &metav1.ConfirmDeletionResponse{}, nil
		}
		var t tombstoneRecord
		if err := json.Unmarshal(e.Value, &t); err != nil {
			return nil, errStatus(err)
		}
		named := false
		for _, n := range t.Nodes {
			if n == r.GetNode() {
				named = true
				break
			}
		}
		if !named {
			return nil, errStatus(fmt.Errorf("%w: node %q is not on tombstone %q", domain.ErrInvalid, r.GetNode(), r.GetVolume()))
		}
		if t.Confirmed == nil {
			t.Confirmed = map[string]bool{}
		}
		t.Confirmed[r.GetNode()] = true
		var op kv.Op
		if len(t.pending()) == 0 {
			op = kv.Op{Key: e.Key, Delete: true}
		} else {
			b, _ := json.Marshal(t)
			op = kv.Op{Key: e.Key, Value: b}
		}
		ok, err = s.kv.Txn(ctx, []kv.Cmp{{Key: e.Key, Rev: e.Rev}}, []kv.Op{op})
		if err != nil {
			return nil, errStatus(err)
		}
		if ok {
			return &metav1.ConfirmDeletionResponse{}, nil
		}
	}
	return nil, status.Error(codes.Aborted, "too much contention")
}

func (s *Service) Attach(ctx context.Context, r *metav1.AttachRequest) (*metav1.AttachResponse, error) {
	if !domain.ValidID(r.GetNode()) {
		return nil, errStatus(fmt.Errorf("%w: node id %q", domain.ErrInvalid, r.GetNode()))
	}
	if r.GetUnlessTarget() && r.GetOnlyIfTarget() {
		return nil, errStatus(fmt.Errorf("%w: unless_target and only_if_target together", domain.ErrInvalid))
	}
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return nil, errStatus(err)
			}
		}
		h, rev, err := s.readHeader(ctx, r.GetVolume())
		if err != nil {
			return nil, errStatus(err)
		}
		switch {
		case r.GetOnlyIfTarget() && h.Target != r.GetNode():
			return nil, errStatus(fmt.Errorf("%w: %s's target is %q, not %s", domain.ErrNotTarget, h.ID, h.Target, r.GetNode()))
		case r.GetUnlessTarget() && h.Target == r.GetNode():
			v, err := s.readVolume(ctx, h.ID)
			if err != nil {
				return nil, errStatus(err)
			}
			return &metav1.AttachResponse{Volume: volumePB(v)}, nil
		}
		h.Attach(r.GetNode())
		b, _ := json.Marshal(h)
		ok, err := s.kv.Txn(ctx, []kv.Cmp{{Key: volumeKey(h.ID), Rev: rev}}, []kv.Op{{Key: volumeKey(h.ID), Value: b}})
		if err != nil {
			return nil, errStatus(err)
		}
		if !ok {
			continue
		}
		// Chunks change only under the new epoch, by the new target: reading them after the
		// commit gives the state the new target starts from. The attach has committed, so it
		// must not be retried. If another attach followed meanwhile, the chunks may already
		// reflect it; this target is then deposed at its first fence or ISR change.
		v, err := s.readVolume(ctx, h.ID)
		if err != nil {
			return nil, errStatus(err)
		}
		v.Header = h
		out := &metav1.AttachResponse{Volume: volumePB(v)}
		if s.opts.TokenKey != nil {
			out.Token, err = captoken.Sign(s.opts.TokenKey, captoken.Claims{
				Volume: h.ID, Epoch: h.Epoch, Node: h.Target, IssuedAt: time.Now().Unix(),
			})
			if err != nil {
				return nil, status.Error(codes.Internal, err.Error())
			}
		}
		return out, nil
	}
	return nil, status.Error(codes.Aborted, "too much contention")
}

func (s *Service) ChangeIsr(ctx context.Context, r *metav1.ChangeIsrRequest) (*metav1.ChangeIsrResponse, error) {
	if len(r.GetChanges()) > domain.MaxChangesPerRequest {
		return nil, errStatus(fmt.Errorf("%w: more than %d changes", domain.ErrInvalid, domain.MaxChangesPerRequest))
	}
	changes := make([]domain.IsrChange, len(r.GetChanges()))
	for i, c := range r.GetChanges() {
		changes[i] = domain.IsrChange{Chunk: c.GetChunk(), ExpectedVer: c.GetExpectedVer(), NewISR: c.GetNewIsr(),
			ChangeReplicas: c.GetChangeReplicas(), NewReplicas: c.GetNewReplicas()}
	}
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return nil, errStatus(err)
			}
		}
		h, rev, err := s.readHeader(ctx, r.GetVolume())
		if err != nil {
			return nil, errStatus(err)
		}
		cmps := []kv.Cmp{{Key: volumeKey(h.ID), Rev: rev}}
		named := make([]domain.Chunk, len(changes))
		for i, c := range changes {
			if int(c.Chunk) >= h.ChunkCount() {
				return nil, errStatus(fmt.Errorf("%w: chunk %d out of range", domain.ErrInvalid, c.Chunk))
			}
			e, ok, err := s.kv.Get(ctx, chunkKey(h.ID, c.Chunk))
			if err != nil {
				return nil, errStatus(err)
			}
			if !ok {
				return nil, status.Errorf(codes.Internal, "volume %q: chunk %d missing", h.ID, c.Chunk)
			}
			if err := json.Unmarshal(e.Value, &named[i]); err != nil {
				return nil, errStatus(err)
			}
			cmps = append(cmps, kv.Cmp{Key: e.Key, Rev: e.Rev})
		}
		reply := func(applied bool) *metav1.ChangeIsrResponse {
			out := &metav1.ChangeIsrResponse{Applied: applied, Epoch: h.Epoch}
			for _, c := range named {
				out.Chunks = append(out.Chunks, chunkPB(c))
			}
			return out
		}
		// Only the current target may change ISRs. Another caller gets the current state,
		// which tells a deposed target that it has been deposed.
		caller, authenticated := callerOf(ctx)
		if authenticated && caller.Node != h.Target {
			return reply(false), nil
		}
		if !domain.CheckIsrChange(&h, named, r.GetEpoch(), changes) {
			return reply(false), nil
		}
		domain.ApplyIsrChange(named, changes)
		ops := make([]kv.Op, len(changes))
		for i, c := range changes {
			b, _ := json.Marshal(named[i])
			ops[i] = kv.Op{Key: chunkKey(h.ID, c.Chunk), Value: b}
		}
		ok, err := s.kv.Txn(ctx, cmps, ops)
		if err != nil {
			return nil, errStatus(err)
		}
		if ok {
			return reply(true), nil
		}
	}
	return nil, status.Error(codes.Aborted, "too much contention")
}

func drainKey(node string) string { return prefix + "drain/" + node }

func (s *Service) DrainNode(ctx context.Context, r *metav1.DrainNodeRequest) (*metav1.DrainNodeResponse, error) {
	if !domain.ValidID(r.GetNode()) {
		return nil, errStatus(fmt.Errorf("%w: node id %q", domain.ErrInvalid, r.GetNode()))
	}
	op := kv.Op{Key: drainKey(r.GetNode()), Delete: !r.GetDraining()}
	if r.GetDraining() {
		if _, ok, err := s.kv.Get(ctx, nodeKey(r.GetNode())); err != nil || !ok {
			if err == nil {
				err = fmt.Errorf("%w: node %q is not registered", domain.ErrNotFound, r.GetNode())
			}
			return nil, errStatus(err)
		}
		op.Value = []byte("1")
	}
	_, err := s.kv.Txn(ctx, nil, []kv.Op{op})
	return &metav1.DrainNodeResponse{}, errStatus(err)
}

func (s *Service) ListVolumes(ctx context.Context, _ *metav1.ListVolumesRequest) (*metav1.ListVolumesResponse, error) {
	entries, err := s.kv.List(ctx, prefix+"volumes/")
	if err != nil {
		return nil, errStatus(err)
	}
	out := &metav1.ListVolumesResponse{}
	for _, e := range entries {
		var h domain.Header
		if json.Unmarshal(e.Value, &h) == nil && !h.Creating {
			out.Ids = append(out.Ids, h.ID)
		}
	}
	return out, nil
}

// MoveReplica records a requested move on the chunk, conditional on the chunk record it read.
// It changes neither replicas nor ISR, and bumps no version: the target does that, under its
// epoch, and a concurrent ISR change of the target's fails its compare and is tried again.
func (s *Service) MoveReplica(ctx context.Context, r *metav1.MoveReplicaRequest) (*metav1.MoveReplicaResponse, error) {
	if !domain.ValidID(r.GetFrom()) || !domain.ValidID(r.GetTo()) {
		return nil, errStatus(fmt.Errorf("%w: node ids %q, %q", domain.ErrInvalid, r.GetFrom(), r.GetTo()))
	}
	if _, ok, err := s.kv.Get(ctx, nodeKey(r.GetTo())); err != nil || !ok {
		if err == nil {
			err = fmt.Errorf("%w: node %q is not registered", domain.ErrNotFound, r.GetTo())
		}
		return nil, errStatus(err)
	}
	for attempt := range retries {
		if attempt > 0 {
			if err := backoff(ctx, attempt); err != nil {
				return nil, errStatus(err)
			}
		}
		h, _, err := s.readHeader(ctx, r.GetVolume())
		if err != nil {
			return nil, errStatus(err)
		}
		if int(r.GetChunk()) >= h.ChunkCount() {
			return nil, errStatus(fmt.Errorf("%w: chunk %d out of range", domain.ErrInvalid, r.GetChunk()))
		}
		e, ok, err := s.kv.Get(ctx, chunkKey(h.ID, r.GetChunk()))
		if err != nil {
			return nil, errStatus(err)
		}
		if !ok {
			return nil, status.Errorf(codes.Internal, "volume %q: chunk %d missing", h.ID, r.GetChunk())
		}
		var c domain.Chunk
		if err := json.Unmarshal(e.Value, &c); err != nil {
			return nil, errStatus(err)
		}
		if !domain.CheckMove(&c, r.GetFrom(), r.GetTo()) {
			return &metav1.MoveReplicaResponse{Applied: false, Chunk: chunkPB(c)}, nil
		}
		domain.ApplyMove(&c, r.GetFrom(), r.GetTo())
		b, _ := json.Marshal(c)
		ok, err = s.kv.Txn(ctx, []kv.Cmp{{Key: e.Key, Rev: e.Rev}}, []kv.Op{{Key: e.Key, Value: b}})
		if err != nil {
			return nil, errStatus(err)
		}
		if ok {
			return &metav1.MoveReplicaResponse{Applied: true, Chunk: chunkPB(c)}, nil
		}
	}
	return nil, status.Error(codes.Aborted, "too much contention")
}
