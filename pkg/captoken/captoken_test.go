package captoken

import (
	"bytes"
	"crypto/ed25519"
	"encoding/binary"
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

// The token bodies (everything before the signature), shared with flaminio-net's token tests
// (go_compatible_vectors): a volume without a first epoch gets version 1, byte for byte as before
// it was added; one with a first epoch gets version 2 (docs/issues/30).
const (
	bodyV1 = "464c43540100087076632d3062316300000000000000070004666c2d62000000006ac81ee0"
	bodyV2 = "464c43540200087076632d3062316300066517289880080004666c2d62000000006ac81ee00006651728988000"
)

func TestBodies(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	for _, x := range []struct {
		c    Claims
		body string
	}{
		{Claims{Volume: "pvc-0b1c", Epoch: 7, Node: "fl-b", IssuedAt: 1791500000}, bodyV1},
		{Claims{Volume: "pvc-0b1c", Epoch: 1_800_000_000_000_008, Node: "fl-b", IssuedAt: 1791500000,
			FirstEpoch: 1_800_000_000_000_000}, bodyV2},
	} {
		tok, err := Sign(key, x.c)
		if err != nil {
			t.Fatal(err)
		}
		if got := hex.EncodeToString(tok[:len(tok)-ed25519.SignatureSize]); got != x.body {
			t.Fatalf("%+v: body %s, want %s", x.c, got, x.body)
		}
	}
}

// TestSignRefusesFirstEpoch: no attach issues a first epoch that is not below the epoch.
func TestSignRefusesFirstEpoch(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	for _, first := range []uint64{7, 8, 1<<64 - 1} {
		if _, err := Sign(key, Claims{Volume: "v", Epoch: 7, Node: "n", FirstEpoch: first}); err == nil {
			t.Fatalf("first epoch %d at epoch 7 signed", first)
		}
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
	First  uint64 `json:"first_epoch,omitempty"`
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
	good2 := Claims{Volume: "pvc-0b1c", Epoch: 1_800_000_000_000_008, Node: "fl-b", IssuedAt: 1791500000,
		FirstEpoch: 1_800_000_000_000_000}
	tok2 := sign(good2)
	long255 := string(bytes.Repeat([]byte{'v'}, 255))
	longest := sign(Claims{Volume: long255, Epoch: 1<<64 - 1, Node: long255, IssuedAt: -1, FirstEpoch: 1<<64 - 2})
	if len(longest) != MaxLen {
		t.Fatalf("longest version 2 token: %d bytes, MaxLen %d", len(longest), MaxLen)
	}
	// A version 2 body with the first epoch given, signed: Sign refuses to make these.
	v2 := func(first uint64) string {
		b := bytes.Clone(tok[:len(tok)-ed25519.SignatureSize])
		b[4] = version2
		b = binary.BigEndian.AppendUint64(b, first)
		return hex.EncodeToString(append(b, ed25519.Sign(key, b)...))
	}
	v3 := bytes.Clone(tok2[:len(tok2)-ed25519.SignatureSize])
	v3[4] = 3
	v3 = append(v3, ed25519.Sign(key, v3)...)
	return pub, []vector{
		{Name: "valid", Token: hex.EncodeToString(tok), Valid: true, Volume: good.Volume, Epoch: good.Epoch, Node: good.Node, Issued: good.IssuedAt},
		{Name: "valid, longest volume id, largest epoch", Token: hex.EncodeToString(long), Valid: true,
			Volume: string(bytes.Repeat([]byte{'v'}, 255)), Epoch: 1<<64 - 1, Node: "n", Issued: -1},
		{Name: "valid version 2", Token: hex.EncodeToString(tok2), Valid: true, Volume: good2.Volume, Epoch: good2.Epoch,
			Node: good2.Node, Issued: good2.IssuedAt, First: good2.FirstEpoch},
		{Name: "valid version 2, longest", Token: hex.EncodeToString(longest), Valid: true, Volume: long255, Epoch: 1<<64 - 1,
			Node: long255, Issued: -1, First: 1<<64 - 2},
		{Name: "version 2, first epoch 0", Token: v2(0)},
		{Name: "version 2, first epoch equal to the epoch", Token: v2(7)},
		{Name: "version 2, first epoch above the epoch", Token: v2(8)},
		{Name: "version 2 without its first epoch", Token: hex.EncodeToString(append(bytes.Clone(tok2[:len(tok2)-ed25519.SignatureSize-8]),
			ed25519.Sign(key, tok2[:len(tok2)-ed25519.SignatureSize-8])...))},
		{Name: "version 3", Token: hex.EncodeToString(v3)},
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
		if v.Valid && (c.Volume != v.Volume || c.Epoch != v.Epoch || c.Node != v.Node || c.IssuedAt != v.Issued ||
			c.FirstEpoch != v.First) {
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
