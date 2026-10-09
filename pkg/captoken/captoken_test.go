package captoken

import (
	"bytes"
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"os"
	"testing"
)

// A fixed key, so the vectors are reproducible. Test use only.
var seed = bytes.Repeat([]byte{7}, ed25519.SeedSize)

func TestRoundTripAndTampering(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	pub := key.Public().(ed25519.PublicKey)
	c := Claims{Volume: "pvc-1", Epoch: 42, Node: "fl-a", IssuedAt: 1791500000}
	tok, err := Sign(key, c)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Verify(pub, tok)
	if err != nil || got != c {
		t.Fatalf("%+v %v", got, err)
	}
	// Every single-bit change, and every truncation, is refused.
	for i := range tok {
		for bit := range 8 {
			bad := bytes.Clone(tok)
			bad[i] ^= 1 << bit
			if _, err := Verify(pub, bad); err == nil {
				t.Fatalf("accepted with byte %d bit %d flipped", i, bit)
			}
		}
		if _, err := Verify(pub, tok[:i]); err == nil {
			t.Fatalf("accepted truncated to %d bytes", i)
		}
	}
	other := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{8}, ed25519.SeedSize))
	if _, err := Verify(other.Public().(ed25519.PublicKey), tok); err == nil {
		t.Fatal("accepted under another key")
	}
}

type vector struct {
	Name   string `json:"name"`
	Token  string `json:"token"`
	Valid  bool   `json:"valid"`
	Volume string `json:"volume,omitempty"`
	Epoch  uint64 `json:"epoch,omitempty"`
	Node   string `json:"node,omitempty"`
	Issued int64  `json:"issued_at,omitempty"`
}

// TestVectors checks testdata/vectors.json, which the Rust verifier uses too. Set
// UPDATE_VECTORS=1 to rewrite it.
func TestVectors(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	pub := key.Public().(ed25519.PublicKey)
	sign := func(c Claims) []byte {
		tok, err := Sign(key, c)
		if err != nil {
			t.Fatal(err)
		}
		return tok
	}
	good := Claims{Volume: "pvc-0b1c", Epoch: 7, Node: "fl-b", IssuedAt: 1791500000}
	tok := sign(good)
	flipped := bytes.Clone(tok)
	flipped[10] ^= 1
	long := sign(Claims{Volume: string(bytes.Repeat([]byte{'v'}, 255)), Epoch: 1<<64 - 1, Node: "n", IssuedAt: -1})
	vs := []vector{
		{Name: "valid", Token: hex.EncodeToString(tok), Valid: true, Volume: good.Volume, Epoch: good.Epoch, Node: good.Node, Issued: good.IssuedAt},
		{Name: "valid, longest volume id, largest epoch", Token: hex.EncodeToString(long), Valid: true,
			Volume: string(bytes.Repeat([]byte{'v'}, 255)), Epoch: 1<<64 - 1, Node: "n", Issued: -1},
		{Name: "flipped bit", Token: hex.EncodeToString(flipped)},
		{Name: "truncated", Token: hex.EncodeToString(tok[:len(tok)-1])},
		{Name: "empty", Token: ""},
		{Name: "trailing byte", Token: hex.EncodeToString(append(bytes.Clone(tok), 0))},
	}
	for _, v := range vs {
		b, _ := hex.DecodeString(v.Token)
		c, err := Verify(pub, b)
		if (err == nil) != v.Valid {
			t.Fatalf("%s: err %v", v.Name, err)
		}
		if v.Valid && (c.Volume != v.Volume || c.Epoch != v.Epoch || c.Node != v.Node || c.IssuedAt != v.Issued) {
			t.Fatalf("%s: %+v", v.Name, c)
		}
	}
	out, _ := json.MarshalIndent(map[string]any{
		"public_key": hex.EncodeToString(pub),
		"vectors":    vs,
	}, "", "  ")
	out = append(out, '\n')
	const path = "testdata/vectors.json"
	if os.Getenv("UPDATE_VECTORS") != "" {
		if err := os.MkdirAll("testdata", 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, out, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	have, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(have, out) {
		t.Fatal("testdata/vectors.json is stale: rerun with UPDATE_VECTORS=1")
	}
}
