// Package domain holds the metadata service's state transitions (docs/replication-protocol.md
// §2), with no storage or RPC. The Rust reference model is
// dataplane/crates/flaminio-repl/src/meta.rs; both replay the vectors in
// meta/testdata/isr-vectors.jsonl.
package domain

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"sort"

	"github.com/flaminio/flaminio/pkg/geometry"
)

// MaxChangesPerRequest bounds one ChangeIsr request, so that it fits one etcd transaction.
const MaxChangesPerRequest = 64

// Chunk is the placement and ISR of one chunk.
type Chunk struct {
	// Nodes that hold a replica, in placement order.
	Replicas []string `json:"replicas"`
	// In-sync replicas, sorted.
	ISR []string `json:"isr"`
	// Incremented on every ISR change, and on every change of Replicas.
	ISRVer uint64 `json:"isr_ver"`
	// A requested move, until the target completes it.
	Moving *Move `json:"moving,omitempty"`
}

// Move is a requested move of a chunk's replica (docs/replication-protocol.md §7, moves).
type Move struct {
	From string `json:"from"`
	To   string `json:"to"`
}

// MaxCreateIdentityBytes bounds the canonical create-identity encoding (docs/issues/76).
const MaxCreateIdentityBytes = 1024

// MaxCreateRecordBytes bounds one root, header or plan/chunk JSON value at admission and reread.
const MaxCreateRecordBytes = 8192

// Header is everything about a volume except its chunks.
type Header struct {
	ID         string `json:"id"`
	SizeBytes  uint64 `json:"size_bytes"`
	ChunkBytes uint64 `json:"chunk_bytes"`
	Replicas   uint32 `json:"replicas"`
	MinISR     uint32 `json:"min_isr"`
	Epoch      uint64 `json:"epoch"`
	// The volume's first epoch: the store revision of the claim that created it, above every
	// epoch an earlier volume of its name reached (docs/issues/30). 0 for a volume created before
	// it was recorded.
	FirstEpoch uint64 `json:"first_epoch,omitempty"`
	Target     string `json:"target,omitempty"`
	// True until every chunk record is written.
	Creating bool `json:"creating,omitempty"`
	// True once a deletion has begun: the volume no longer exists for any other operation, and
	// its chunks' replicas can no longer change (docs/issues/8).
	Deleting bool `json:"deleting,omitempty"`
	// CreateOperation is the durable operation id that admitted this incarnation (docs/issues/76).
	// Retained after visibility.
	CreateOperation string `json:"create_operation,omitempty"`
	// CreateSpec is the normalized full create identity. Retained after visibility. Absent on
	// legacy headers (scalar matching only).
	CreateSpec *CreateSpec `json:"create_spec,omitempty"`
	// AdmissionGeneration is the placement-gate revision the winning create claim compared
	// (docs/issues/78). Unchanged through creation/recovery, finalization, attach and deletion.
	// Legacy absence is 0; new admissions never record 0.
	AdmissionGeneration int64 `json:"admission_generation,omitempty"`
}

// ChunkCount is the number of chunks the volume is split into.
func (h *Header) ChunkCount() (int, error) {
	n, err := geometry.Admit(h.SizeBytes, h.ChunkBytes, h.Replicas, h.MinISR)
	if err != nil {
		return 0, fmt.Errorf("unsupported volume geometry: %w", err)
	}
	return int(n), nil // Admission bounds n to at most 65,536 on every platform.
}

// Volume is a header with its chunks.
type Volume struct {
	Header
	Chunks []Chunk
}

// IsrChange is one chunk's part of a ChangeIsr request.
type IsrChange struct {
	Chunk       uint32
	ExpectedVer uint64
	NewISR      []string
	// For a move's steps only: the new replica set (ChangeReplicas set).
	ChangeReplicas bool
	NewReplicas    []string
}

var (
	ErrInvalid  = errors.New("invalid argument")
	ErrNotFound = errors.New("not found")
	ErrExists   = errors.New("already exists")
	// ErrPlacement means there are not enough live nodes for the requested replicas.
	ErrPlacement = errors.New("not enough nodes for placement")
	// ErrNotTarget: a call that requires the node to be the volume's target.
	ErrNotTarget = errors.New("not the volume's target")
)

var idPattern = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`)

// ValidID reports whether s may name a volume or a node.
func ValidID(s string) bool { return idPattern.MatchString(s) }

// Topology label keys nodes register (ARCHITECTURE §4, docs/followups.md §4).
const (
	LabelZone = "topology.kubernetes.io/zone"
	LabelRack = "flaminio.io/rack"
)

// FailureDomain is the topology level that replicas of one chunk must be spread across.
type FailureDomain int

const (
	DomainNode FailureDomain = iota
	DomainRack
	DomainZone
)

// PlacementPolicy controls what happens when the topology is short of distinct domains.
type PlacementPolicy int

const (
	// PolicyStrict refuses placement if there are not enough distinct domains at the
	// requested level.
	PolicyStrict PlacementPolicy = iota
	// PolicyBestEffort falls back a level (zone→rack→node) until placement is possible.
	PolicyBestEffort
)

// ParseFailureDomain reads a StorageClass / API value (node|rack|zone). Empty means node.
func ParseFailureDomain(s string) (FailureDomain, error) {
	switch s {
	case "", "node":
		return DomainNode, nil
	case "rack":
		return DomainRack, nil
	case "zone":
		return DomainZone, nil
	default:
		return 0, fmt.Errorf("%w: failure domain %q", ErrInvalid, s)
	}
}

// ParsePlacementPolicy reads a StorageClass / API value (strict|bestEffort). Empty means strict.
func ParsePlacementPolicy(s string) (PlacementPolicy, error) {
	switch s {
	case "", "strict":
		return PolicyStrict, nil
	case "bestEffort":
		return PolicyBestEffort, nil
	default:
		return 0, fmt.Errorf("%w: failure domain policy %q", ErrInvalid, s)
	}
}

func (d FailureDomain) String() string {
	switch d {
	case DomainRack:
		return "rack"
	case DomainZone:
		return "zone"
	default:
		return "node"
	}
}

func (p PlacementPolicy) String() string {
	if p == PolicyBestEffort {
		return "bestEffort"
	}
	return "strict"
}

// Spec is what CreateVolume asks for.
type Spec struct {
	ID         string
	SizeBytes  uint64
	ChunkBytes uint64
	Replicas   uint32
	MinISR     uint32
	// Optional: a node to hold a replica of every chunk.
	PreferNode string
	Domain     FailureDomain
	Policy     PlacementPolicy
}

// CreateSpec is the canonical create identity persisted with an operation (docs/issues/76).
// PreferNode is empty or a ValidID; Domain/Policy use their normalized defaults.
type CreateSpec struct {
	SizeBytes  uint64          `json:"size_bytes"`
	ChunkBytes uint64          `json:"chunk_bytes"`
	Replicas   uint32          `json:"replicas"`
	MinISR     uint32          `json:"min_isr"`
	PreferNode string          `json:"prefer_node,omitempty"`
	Domain     FailureDomain   `json:"failure_domain"`
	Policy     PlacementPolicy `json:"failure_domain_policy"`
}

// CreateSpecFrom builds the normalized create identity from a validated Spec.
func CreateSpecFrom(s Spec) CreateSpec {
	return CreateSpec{
		SizeBytes: s.SizeBytes, ChunkBytes: s.ChunkBytes, Replicas: s.Replicas, MinISR: s.MinISR,
		PreferNode: s.PreferNode, Domain: s.Domain, Policy: s.Policy,
	}
}

// Equal reports whether two create identities match.
func (a CreateSpec) Equal(b CreateSpec) bool {
	return a == b
}

// ScalarEqual is the legacy visible-header comparison (size/chunk/replicas/minISR only).
func (a CreateSpec) ScalarEqual(b CreateSpec) bool {
	return a.SizeBytes == b.SizeBytes && a.ChunkBytes == b.ChunkBytes &&
		a.Replicas == b.Replicas && a.MinISR == b.MinISR
}

// Validate checks the persisted identity's grammar, including the pre-admission scalar rules.
// Supported geometry is checked separately so an older, well-formed identity can be refused
// without being mistaken for corruption.
func (c CreateSpec) Validate() error {
	if c.SizeBytes == 0 || c.SizeBytes%geometry.BlockBytes != 0 ||
		c.ChunkBytes < geometry.MinChunkBytes || c.ChunkBytes&(c.ChunkBytes-1) != 0 ||
		c.Replicas < 1 || c.Replicas > 5 || c.MinISR < 1 || c.MinISR > c.Replicas ||
		c.Domain < DomainNode || c.Domain > DomainZone ||
		(c.Policy != PolicyStrict && c.Policy != PolicyBestEffort) {
		return fmt.Errorf("%w: create identity parameters", ErrInvalid)
	}
	if c.PreferNode != "" && !ValidID(c.PreferNode) {
		return fmt.Errorf("%w: prefer node", ErrInvalid)
	}
	b, err := json.Marshal(c)
	if err != nil {
		return fmt.Errorf("%w: create identity", ErrInvalid)
	}
	if len(b) > MaxCreateIdentityBytes {
		return fmt.Errorf("%w: create identity exceeds %d bytes", ErrInvalid, MaxCreateIdentityBytes)
	}
	return nil
}

// DecodeCreateSpec strictly decodes a persisted create identity.
func DecodeCreateSpec(b []byte) (CreateSpec, error) {
	if len(b) > MaxCreateIdentityBytes {
		return CreateSpec{}, fmt.Errorf("create identity: data loss: exceeds %d bytes", MaxCreateIdentityBytes)
	}
	var c CreateSpec
	decoder := json.NewDecoder(bytes.NewReader(b))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&c); err != nil {
		return CreateSpec{}, fmt.Errorf("create identity: data loss: %w", err)
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(b, &fields); err != nil {
		return CreateSpec{}, fmt.Errorf("create identity: data loss: %w", err)
	}
	for _, name := range []string{"size_bytes", "chunk_bytes", "replicas", "min_isr", "failure_domain", "failure_domain_policy"} {
		if value, ok := fields[name]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return CreateSpec{}, fmt.Errorf("create identity: data loss: missing %s", name)
		}
	}
	if err := c.Validate(); err != nil {
		return CreateSpec{}, fmt.Errorf("create identity: data loss: %w", err)
	}
	return c, nil
}

// Validate checks a volume specification.
func (s *Spec) Validate() error {
	switch {
	case !ValidID(s.ID):
		return fmt.Errorf("%w: volume id %q", ErrInvalid, s.ID)
	case s.Domain < DomainNode || s.Domain > DomainZone:
		return fmt.Errorf("%w: failure domain", ErrInvalid)
	case s.Policy != PolicyStrict && s.Policy != PolicyBestEffort:
		return fmt.Errorf("%w: failure domain policy", ErrInvalid)
	case s.PreferNode != "" && !ValidID(s.PreferNode):
		return fmt.Errorf("%w: prefer node", ErrInvalid)
	}
	if _, err := geometry.Admit(s.SizeBytes, s.ChunkBytes, s.Replicas, s.MinISR); err != nil {
		return fmt.Errorf("%w: %v", ErrInvalid, err)
	}
	if err := CreateSpecFrom(*s).Validate(); err != nil {
		return err
	}
	return nil
}

// NewVolume builds an unattached volume with every replica in the ISR.
func NewVolume(s Spec, placement [][]string) *Volume {
	v := &Volume{Header: Header{
		ID: s.ID, SizeBytes: s.SizeBytes, ChunkBytes: s.ChunkBytes,
		Replicas: s.Replicas, MinISR: s.MinISR,
	}}
	for _, p := range placement {
		v.Chunks = append(v.Chunks, Chunk{Replicas: slices.Clone(p), ISR: sortedSet(p)})
	}
	return v
}

// Start sets the volume's epoch and first epoch to first, before its first attach.
func (h *Header) Start(first uint64) {
	h.Epoch = first
	h.FirstEpoch = first
}

// Attach makes node the volume's target under a new epoch.
func (h *Header) Attach(node string) {
	h.Epoch++
	h.Target = node
}

// CheckIsrChange reports whether a ChangeIsr request is valid against the volume: the epoch is
// current, each chunk appears once and has the expected version, and each new ISR is a set of
// the chunk's replicas no smaller than the minimum. chunks holds the named chunks only, in
// request order.
func CheckIsrChange(h *Header, chunks []Chunk, epoch uint64, changes []IsrChange) bool {
	if epoch != h.Epoch {
		return false
	}
	seen := map[uint32]bool{}
	for i, c := range changes {
		if seen[c.Chunk] {
			return false
		}
		seen[c.Chunk] = true
		ch := chunks[i]
		isr := sortedSet(c.NewISR)
		if ch.ISRVer != c.ExpectedVer || uint32(len(isr)) < h.MinISR {
			return false
		}
		replicas := ch.Replicas
		if c.ChangeReplicas {
			replicas = c.NewReplicas
			if !validMove(&ch, c.NewReplicas, isr) {
				return false
			}
		}
		for _, n := range isr {
			if !slices.Contains(replicas, n) {
				return false
			}
		}
	}
	return true
}

// validMove reports whether a replica-set change carries out the chunk's requested move: it
// adds the destination, or removes the source once the destination is in the new ISR.
func validMove(ch *Chunk, replicas, isr []string) bool {
	if ch.Moving == nil {
		return false
	}
	from, to := ch.Moving.From, ch.Moving.To
	add := append(slices.Clone(ch.Replicas), to)
	remove := slices.DeleteFunc(slices.Clone(ch.Replicas), func(n string) bool { return n == from })
	return (!slices.Contains(ch.Replicas, to) && slices.Equal(replicas, add)) ||
		(slices.Contains(ch.Replicas, from) && slices.Contains(isr, to) && slices.Equal(replicas, remove))
}

// ApplyIsrChange applies a change that CheckIsrChange accepted, to the named chunks. A change
// that drops a move's source completes the move.
func ApplyIsrChange(chunks []Chunk, changes []IsrChange) {
	for i, c := range changes {
		chunks[i].ISR = sortedSet(c.NewISR)
		chunks[i].ISRVer++
		if c.ChangeReplicas {
			if m := chunks[i].Moving; m != nil && !slices.Contains(c.NewReplicas, m.From) {
				chunks[i].Moving = nil
			}
			chunks[i].Replicas = slices.Clone(c.NewReplicas)
		}
	}
}

// CheckMove reports whether a move of the chunk's replica on from to to may be requested:
// from holds a replica, to does not, and no move is pending.
func CheckMove(ch *Chunk, from, to string) bool {
	return slices.Contains(ch.Replicas, from) && !slices.Contains(ch.Replicas, to) && ch.Moving == nil
}

// ApplyMove records a move that CheckMove accepted. Only the target carries it out.
func ApplyMove(ch *Chunk, from, to string) {
	ch.Moving = &Move{From: from, To: to}
}

func sortedSet(s []string) []string {
	out := slices.Clone(s)
	sort.Strings(out)
	return slices.Compact(out)
}

// NodeLoad is a candidate node for placement with the number of replicas it already holds.
type NodeLoad struct {
	ID       string
	Replicas int
	Zone     string
	Rack     string
}

// LoadFromLabels builds a NodeLoad from a node's id, current replica count and registered labels.
func LoadFromLabels(id string, replicas int, labels map[string]string) NodeLoad {
	n := NodeLoad{ID: id, Replicas: replicas}
	if labels != nil {
		n.Zone = labels[LabelZone]
		n.Rack = labels[LabelRack]
	}
	return n
}

// domainKey is the failure-domain identity of n at level. Missing rack/zone labels share one
// empty domain at that level, so a short topology is visible to placement.
func (n NodeLoad) domainKey(level FailureDomain) string {
	switch level {
	case DomainZone:
		return "z:" + n.Zone
	case DomainRack:
		return "r:" + n.Rack
	default:
		return "n:" + n.ID
	}
}

func distinctDomains(nodes []NodeLoad, level FailureDomain) int {
	seen := map[string]struct{}{}
	for _, n := range nodes {
		seen[n.domainKey(level)] = struct{}{}
	}
	return len(seen)
}

// resolveDomain picks the level to place at: the requested one, or the next lower under
// bestEffort when there are not enough distinct domains.
func resolveDomain(nodes []NodeLoad, replicas int, want FailureDomain, policy PlacementPolicy) (FailureDomain, error) {
	for level := want; ; level-- {
		if distinctDomains(nodes, level) >= replicas {
			return level, nil
		}
		if policy == PolicyStrict || level == DomainNode {
			return 0, fmt.Errorf("%w: %d distinct %s domains, %d replicas", ErrPlacement, distinctDomains(nodes, level), level, replicas)
		}
	}
}

// Place picks replica nodes for each of n chunks so that no two replicas of a chunk share a
// failure domain at the resolved level (ARCHITECTURE §8.2, §9). It prefers the least loaded
// eligible nodes, so a volume's chunks spread. If prefer is one of the nodes, every chunk has
// its first replica there. Under PolicyBestEffort, a short topology falls back rack→node or
// zone→rack→node.
func Place(nodes []NodeLoad, n int, replicas int, prefer string, domain FailureDomain, policy PlacementPolicy) ([][]string, error) {
	if len(nodes) < replicas {
		return nil, fmt.Errorf("%w: %d live nodes, %d replicas", ErrPlacement, len(nodes), replicas)
	}
	level, err := resolveDomain(nodes, replicas, domain, policy)
	if err != nil {
		return nil, err
	}
	if !slices.ContainsFunc(nodes, func(x NodeLoad) bool { return x.ID == prefer }) {
		prefer = ""
	}
	byID := map[string]int{}
	for i, x := range nodes {
		byID[x.ID] = i
	}
	load := slices.Clone(nodes)
	out := make([][]string, 0, n)
	for range n {
		p, err := placeChunk(load, replicas, prefer, level)
		if err != nil {
			return nil, err
		}
		out = append(out, p)
		for _, id := range p {
			load[byID[id]].Replicas++
		}
	}
	return out, nil
}

// placeChunk chooses replicas distinct nodes whose domains at level do not collide.
func placeChunk(nodes []NodeLoad, replicas int, prefer string, level FailureDomain) ([]string, error) {
	order := make([]int, len(nodes))
	for i := range order {
		order[i] = i
	}
	sort.SliceStable(order, func(a, b int) bool {
		i, j := order[a], order[b]
		if (nodes[i].ID == prefer) != (nodes[j].ID == prefer) {
			return nodes[i].ID == prefer
		}
		if nodes[i].Replicas != nodes[j].Replicas {
			return nodes[i].Replicas < nodes[j].Replicas
		}
		return nodes[i].ID < nodes[j].ID
	})
	used := map[string]struct{}{}
	p := make([]string, 0, replicas)
	for _, i := range order {
		if len(p) == replicas {
			break
		}
		d := nodes[i].domainKey(level)
		if _, ok := used[d]; ok {
			continue
		}
		used[d] = struct{}{}
		p = append(p, nodes[i].ID)
	}
	if len(p) < replicas {
		return nil, fmt.Errorf("%w: could not spread %d replicas across %s domains", ErrPlacement, replicas, level)
	}
	return p, nil
}
