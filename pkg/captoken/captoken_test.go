package captoken

import (
	"bytes"
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// Deterministic test seed (not a production key). Vectors are generated in-process so
// high-entropy tokens are never committed (GitGuardian).
var seed = bytes.Repeat([]byte{7}, ed25519.SeedSize)

func TestSignVerify(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	pub := key.Public().(ed25519.PublicKey)
	c := Claims{Volume: "pvc-0b1c", Epoch: 7, Node: "fl-b", IssuedAt: 1791500000}
	tok, err := Sign(key, c)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Verify(pub, tok)
	if err != nil {
		t.Fatal(err)
	}
	if got != c {
		t.Fatalf("got %+v want %+v", got, c)
	}
	tok[10] ^= 1
	if _, err := Verify(pub, tok); err != ErrInvalid {
		t.Fatalf("flipped bit: %v", err)
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

func testVectors(t *testing.T) (ed25519.PublicKey, []vector) {
	t.Helper()
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
	return pub, []vector{
		{Name: "valid", Token: hex.EncodeToString(tok), Valid: true, Volume: good.Volume, Epoch: good.Epoch, Node: good.Node, Issued: good.IssuedAt},
		{Name: "valid, longest volume id, largest epoch", Token: hex.EncodeToString(long), Valid: true,
			Volume: string(bytes.Repeat([]byte{'v'}, 255)), Epoch: 1<<64 - 1, Node: "n", Issued: -1},
		{Name: "flipped bit", Token: hex.EncodeToString(flipped)},
		{Name: "truncated", Token: hex.EncodeToString(tok[:len(tok)-1])},
		{Name: "empty", Token: ""},
		{Name: "trailing byte", Token: hex.EncodeToString(append(bytes.Clone(tok), 0))},
	}
}

// TestVectors builds the cross-language cases in memory. Set UPDATE_VECTORS=1 to write a
// local (gitignored) copy for debugging; it is never required for the test to pass.
func TestVectors(t *testing.T) {
	pub, vs := testVectors(t)
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
	if os.Getenv("UPDATE_VECTORS") == "" {
		return
	}
	out, _ := json.MarshalIndent(map[string]any{
		"public_key": hex.EncodeToString(pub),
		"vectors":    vs,
	}, "", "  ")
	out = append(out, '\n')
	dir := filepath.Join("testdata")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "vectors.json"), out, 0o644); err != nil {
		t.Fatal(err)
	}
}
