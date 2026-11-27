// Package fixture builds the independent maximum future-view contract fixtures for Go/Rust tests.
package fixture

import (
	"fmt"
	"strings"

	"github.com/flaminio/flaminio/meta/domain"
)

// Caps are approved literals, deliberately independent of the admission implementation.
var Caps = [...]int{5137, 3881, 3119, 2607, 2239}

func VolumeID(r int) string {
	s := fmt.Sprintf("geometry-r%d-", r)
	return s + strings.Repeat("x", 128-len(s))
}
func NodeID(i int) string { s := fmt.Sprintf("n%d-", i); return s + strings.Repeat("x", 128-len(s)) }

// Volume is the largest legal chunk/member shape, including temporary move growth.
// Epoch is one below max so the real Attach RPC can reach max without overflow.
func Volume(r int) *domain.Volume {
	n := Caps[r-1]
	members := make([]string, r+1)
	for i := range members {
		members[i] = NodeID(i)
	}
	spec := domain.Spec{ID: VolumeID(r), SizeBytes: uint64(n) << 30, ChunkBytes: 1 << 30, Replicas: uint32(r), MinISR: uint32(r)}
	identity := domain.CreateSpecFrom(spec)
	v := &domain.Volume{Header: domain.Header{ID: spec.ID, SizeBytes: spec.SizeBytes, ChunkBytes: spec.ChunkBytes, Replicas: spec.Replicas, MinISR: spec.MinISR, Epoch: ^uint64(0) - 1, FirstEpoch: 1, Target: members[0], CreateOperation: fmt.Sprintf("%032x", r), CreateSpec: &identity}, Chunks: make([]domain.Chunk, n)}
	for i := range v.Chunks {
		v.Chunks[i] = domain.Chunk{Replicas: members, ISR: members, ISRVer: ^uint64(0), Moving: &domain.Move{From: members[0], To: members[r]}}
	}
	return v
}
