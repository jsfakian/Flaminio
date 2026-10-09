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

// Spec is what CreateVolume asks for.
type Spec struct {
	ID         string
	SizeBytes  uint64
	ChunkBytes uint64
	Replicas   uint32
	MinISR     uint32
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
}

// Place picks replicas distinct nodes for each of n chunks, preferring the least loaded, so a
// volume's chunks spread over as many nodes as possible (ARCHITECTURE §9). Failure domains
// above the node level are not considered yet.
func Place(nodes []NodeLoad, n int, replicas int) ([][]string, error) {
	if len(nodes) < replicas {
		return nil, fmt.Errorf("%w: %d live nodes, %d replicas", ErrPlacement, len(nodes), replicas)
	}
	load := slices.Clone(nodes)
	out := make([][]string, 0, n)
	for range n {
		sort.SliceStable(load, func(i, j int) bool {
			if load[i].Replicas != load[j].Replicas {
				return load[i].Replicas < load[j].Replicas
			}
			return load[i].ID < load[j].ID
		})
		p := make([]string, replicas)
		for k := range replicas {
			p[k] = load[k].ID
			load[k].Replicas++
		}
		out = append(out, p)
	}
	return out, nil
}
