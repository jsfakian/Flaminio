// Package domain holds the metadata service's state transitions (docs/replication-protocol.md
// §2), with no storage or RPC. The Rust reference model is
// dataplane/crates/flaminio-repl/src/meta.rs; both replay the vectors in
// meta/testdata/isr-vectors.jsonl.
package domain

import (
	"errors"
	"fmt"
	"regexp"
	"slices"
	"sort"
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

// Header is everything about a volume except its chunks.
type Header struct {
	ID         string `json:"id"`
	SizeBytes  uint64 `json:"size_bytes"`
	ChunkBytes uint64 `json:"chunk_bytes"`
	Replicas   uint32 `json:"replicas"`
	MinISR     uint32 `json:"min_isr"`
	Epoch      uint64 `json:"epoch"`
	Target     string `json:"target,omitempty"`
	// True until every chunk record is written.
	Creating bool `json:"creating,omitempty"`
	// True once a deletion has begun: the volume no longer exists for any other operation, and
	// its chunks' replicas can no longer change (docs/8).
	Deleting bool `json:"deleting,omitempty"`
}

// ChunkCount is the number of chunks the volume is split into.
func (h *Header) ChunkCount() int {
	return int((h.SizeBytes + h.ChunkBytes - 1) / h.ChunkBytes)
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

// Validate checks a volume specification.
func (s *Spec) Validate() error {
	switch {
	case !ValidID(s.ID):
		return fmt.Errorf("%w: volume id %q", ErrInvalid, s.ID)
	case s.ChunkBytes < 1<<20 || s.ChunkBytes&(s.ChunkBytes-1) != 0:
		return fmt.Errorf("%w: chunk size must be a power of two of at least 1 MiB", ErrInvalid)
	case s.SizeBytes == 0 || s.SizeBytes%4096 != 0:
		return fmt.Errorf("%w: size must be a positive multiple of 4 KiB", ErrInvalid)
	case s.SizeBytes/s.ChunkBytes > 1<<20:
		return fmt.Errorf("%w: more than 2^20 chunks", ErrInvalid)
	case s.Replicas < 1 || s.Replicas > 5:
		return fmt.Errorf("%w: replicas must be 1 to 5", ErrInvalid)
	case s.MinISR < 1 || s.MinISR > s.Replicas:
		return fmt.Errorf("%w: min ISR must be 1 to replicas", ErrInvalid)
	case s.Domain < DomainNode || s.Domain > DomainZone:
		return fmt.Errorf("%w: failure domain", ErrInvalid)
	case s.Policy != PolicyStrict && s.Policy != PolicyBestEffort:
		return fmt.Errorf("%w: failure domain policy", ErrInvalid)
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
