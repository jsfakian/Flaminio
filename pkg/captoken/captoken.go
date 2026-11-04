// Package captoken implements capability tokens (ARCHITECTURE §8.4, §15.2).
//
// On every attach the metadata service issues a token naming the volume, the new epoch and the
// node of the new target, signed with Ed25519. A replica accepts I/O for a volume only from a
// session that presented a valid token for that volume, whose node matches the peer's
// certificate, and whose epoch is not below the volume's fenced epoch. A valid node
// certificate alone grants nothing.
//
// Encoding (all integers big-endian), then a 64-byte Ed25519 signature over everything before
// it:
//
//	"FLCT"  magic
//	u8      version (1 or 2)
//	u16     length of the volume id, then the id
//	u64     epoch
//	u16     length of the node id, then the id
//	i64     issue time, Unix seconds (for audit only: validity comes from the epoch)
//	u64     version 2 only: the volume's first epoch, above 0 and below the epoch
//
// Version 2 carries the volume's first epoch (docs/issues/30): a replica fenced below it holds
// only what earlier volumes of the name left, and deletes it. A volume without a first epoch
// (created before it was recorded) gets version 1 tokens, byte for byte as before.
//
// A token has no expiry: it is worth nothing once its epoch has been fenced off.
package captoken

import (
	"crypto/ed25519"
	"encoding/binary"
	"errors"
	"fmt"
)

const (
	magic = "FLCT"
	// version1 has no first epoch; version2 adds it.
	version1 = 1
	version2 = 2
	// MaxLen bounds a token, so a peer cannot make a replica buffer without limit.
	MaxLen = 4 + 1 + 2 + 255 + 8 + 2 + 255 + 8 + 8 + ed25519.SignatureSize
)

// Claims are what a token grants: I/O on Volume, as Epoch's target running on Node.
type Claims struct {
	Volume   string
	Epoch    uint64
	Node     string
	IssuedAt int64
	// FirstEpoch is the volume's first epoch, below Epoch; 0 for a volume without one, whose
	// tokens are version 1.
	FirstEpoch uint64
}

// Sign encodes and signs claims.
func Sign(key ed25519.PrivateKey, c Claims) ([]byte, error) {
	if len(c.Volume) == 0 || len(c.Volume) > 255 || len(c.Node) == 0 || len(c.Node) > 255 {
		return nil, errors.New("volume and node ids must be 1 to 255 bytes")
	}
	if c.FirstEpoch != 0 && c.FirstEpoch >= c.Epoch {
		return nil, fmt.Errorf("first epoch %d is not below epoch %d", c.FirstEpoch, c.Epoch)
	}
	b := []byte(magic)
	if c.FirstEpoch == 0 {
		b = append(b, version1)
	} else {
		b = append(b, version2)
	}
	b = binary.BigEndian.AppendUint16(b, uint16(len(c.Volume)))
	b = append(b, c.Volume...)
	b = binary.BigEndian.AppendUint64(b, c.Epoch)
	b = binary.BigEndian.AppendUint16(b, uint16(len(c.Node)))
	b = append(b, c.Node...)
	b = binary.BigEndian.AppendUint64(b, uint64(c.IssuedAt))
	if c.FirstEpoch != 0 {
		b = binary.BigEndian.AppendUint64(b, c.FirstEpoch)
	}
	return append(b, ed25519.Sign(key, b)...), nil
}

// ErrInvalid is returned for any token that is malformed or not signed by the key.
var ErrInvalid = errors.New("invalid capability token")

// Verify checks a token's signature and decodes it. It does not check the epoch or the node:
// those depend on the replica's state and on the peer's certificate.
func Verify(pub ed25519.PublicKey, tok []byte) (Claims, error) {
	if len(tok) > MaxLen || len(tok) < ed25519.SignatureSize {
		return Claims{}, ErrInvalid
	}
	body, sig := tok[:len(tok)-ed25519.SignatureSize], tok[len(tok)-ed25519.SignatureSize:]
	if !ed25519.Verify(pub, body, sig) {
		return Claims{}, ErrInvalid
	}
	var c Claims
	r := reader{b: body}
	if string(r.take(4)) != magic {
		return Claims{}, ErrInvalid
	}
	v := r.u8()
	if v != version1 && v != version2 {
		return Claims{}, ErrInvalid
	}
	c.Volume = string(r.take(int(r.u16())))
	c.Epoch = r.u64()
	c.Node = string(r.take(int(r.u16())))
	c.IssuedAt = int64(r.u64())
	if v == version2 {
		c.FirstEpoch = r.u64()
		if c.FirstEpoch == 0 || c.FirstEpoch >= c.Epoch {
			return Claims{}, ErrInvalid
		}
	}
	if r.bad || len(r.b) != 0 || c.Volume == "" || c.Node == "" {
		return Claims{}, ErrInvalid
	}
	return c, nil
}

type reader struct {
	b   []byte
	bad bool
}

func (r *reader) take(n int) []byte {
	if r.bad || n > len(r.b) {
		r.bad = true
		return nil
	}
	out := r.b[:n]
	r.b = r.b[n:]
	return out
}

func (r *reader) u8() byte {
	b := r.take(1)
	if b == nil {
		return 0
	}
	return b[0]
}

func (r *reader) u16() uint16 {
	b := r.take(2)
	if b == nil {
		return 0
	}
	return binary.BigEndian.Uint16(b)
}

func (r *reader) u64() uint64 {
	b := r.take(8)
	if b == nil {
		return 0
	}
	return binary.BigEndian.Uint64(b)
}

// String is for logs. It never shows the signature.
func (c Claims) String() string {
	return fmt.Sprintf("volume %s epoch %d node %s", c.Volume, c.Epoch, c.Node)
}
