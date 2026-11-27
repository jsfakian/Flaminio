// Package geometry defines the volume geometry shared by metadata and the data plane.
// Its limits are representation bounds, not physical capacity reservations.
package geometry

import "fmt"

const (
	BlockBytes    = 4096
	MinChunkBytes = 1 << 20
	MaxChunkBytes = 1 << 30
	MaxChunks     = 1 << 16
	MetadataBytes = 4 << 20
)

// Parameters validates the chunk size and replication parameters before capacity arithmetic.
func Parameters(chunkBytes uint64, replicas, minISR uint32) error {
	if err := chunkSize(chunkBytes); err != nil {
		return err
	}
	if replicas < 1 || replicas > 5 {
		return fmt.Errorf("replicas must be 1 to 5")
	}
	if minISR < 1 || minISR > replicas {
		return fmt.Errorf("min ISR must be 1 to replicas")
	}
	return nil
}

func chunkSize(chunkBytes uint64) error {
	if chunkBytes < MinChunkBytes || chunkBytes > MaxChunkBytes || chunkBytes&(chunkBytes-1) != 0 {
		return fmt.Errorf("chunk size must be a power of two from 1 MiB through 1 GiB")
	}
	return nil
}

// Scalar validates the wire/session envelope and returns the exact whole-chunk count.
func Scalar(sizeBytes, chunkBytes uint64) (uint64, error) {
	if err := chunkSize(chunkBytes); err != nil {
		return 0, err
	}
	if sizeBytes == 0 || sizeBytes%chunkBytes != 0 {
		return 0, fmt.Errorf("size must be a positive whole number of chunks")
	}
	n := sizeBytes / chunkBytes
	if n > MaxChunks {
		return 0, fmt.Errorf("more than %d chunks", MaxChunks)
	}
	return n, nil
}

// MetadataBound is the conservative encoded full-volume reply bound. Validate count and RF
// first so every operation is bounded independently of request-controlled integer widths.
func MetadataBound(chunks uint64, replicas uint32) (uint64, error) {
	if chunks == 0 || chunks > MaxChunks {
		return 0, fmt.Errorf("chunk count must be 1 to %d", MaxChunks)
	}
	if replicas < 1 || replicas > 5 {
		return 0, fmt.Errorf("replicas must be 1 to 5")
	}
	return 2048 + chunks*(288+264*(uint64(replicas)+1)), nil
}

// Admit validates the full geometry and the normal 4 MiB metadata reply budget.
func Admit(sizeBytes, chunkBytes uint64, replicas, minISR uint32) (uint64, error) {
	if err := Parameters(chunkBytes, replicas, minISR); err != nil {
		return 0, err
	}
	n, err := Scalar(sizeBytes, chunkBytes)
	if err != nil {
		return 0, err
	}
	b, err := MetadataBound(n, replicas)
	if err != nil {
		return 0, err
	}
	if b > MetadataBytes {
		return 0, fmt.Errorf("geometry needs a metadata reply bound of %d bytes, exceeding %d bytes", b, MetadataBytes)
	}
	return n, nil
}
