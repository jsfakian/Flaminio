package captoken

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"testing"
)

func TestHandoffExactBindingAndPurpose(t *testing.T) {
	key := ed25519.NewKeyFromSeed(seed)
	pub := key.Public().(ed25519.PublicKey)
	nonce := bytes.Repeat([]byte{83}, 32)
	token, err := Sign(key, Claims{Volume: "v", Node: "fl-a", Epoch: 7, FirstEpoch: 1})
	if err != nil {
		t.Fatal(err)
	}
	handoff, err := SignHandoff(key, HandoffNormal, nonce, token)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(token)
	if len(handoff) != 134 || string(handoff[:6]) != "FLCH\x01\x01" || !bytes.Equal(handoff[6:38], nonce) || !bytes.Equal(handoff[38:70], digest[:]) || len(base64.RawURLEncoding.EncodeToString(handoff)) != 179 {
		t.Fatal("wrong envelope ABI")
	}
	if _, err := VerifyHandoff(pub, HandoffNormal, nonce, token, handoff); err != nil {
		t.Fatal(err)
	}
	for _, offset := range []int{0, 4, 5, 6, 37, 38, 69, 70, 133} {
		b := bytes.Clone(handoff)
		b[offset] ^= 1
		if _, err := VerifyHandoff(pub, HandoffNormal, nonce, token, b); err == nil {
			t.Fatalf("accepted changed byte %d", offset)
		}
	}
	for _, length := range []int{0, 1, 69, 70, 133, 135, 4096} {
		b := make([]byte, length)
		copy(b, handoff)
		if _, err := VerifyHandoff(pub, HandoffNormal, nonce, token, b); err == nil {
			t.Fatalf("accepted length %d", length)
		}
	}
	// Correct signatures do not excuse a different purpose, nonce or exact capability digest.
	for _, purpose := range []HandoffPurpose{HandoffInitialization, 0, 3} {
		if _, err := VerifyHandoff(pub, purpose, nonce, token, handoff); err == nil {
			t.Fatal("purpose accepted")
		}
	}
	other := bytes.Clone(nonce)
	other[0] ^= 1
	if _, err := VerifyHandoff(pub, HandoffNormal, other, token, handoff); err == nil {
		t.Fatal("other process accepted")
	}
	changed, err := Sign(key, Claims{Volume: "v", Node: "fl-a", Epoch: 7, FirstEpoch: 1, IssuedAt: 1})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyHandoff(pub, HandoffNormal, nonce, changed, handoff); err == nil {
		t.Fatal("same claims except signed issue time bypassed exact digest")
	}
	b := bytes.Clone(handoff)
	b[38] ^= 1
	copy(b[70:], ed25519.Sign(key, b[:70]))
	if _, err := VerifyHandoff(pub, HandoffNormal, nonce, token, b); err == nil {
		t.Fatal("signed wrong digest accepted")
	}
	legacy, _ := Sign(key, Claims{Volume: "v", Node: "fl-a", Epoch: 7})
	legacyHandoff, _ := SignHandoff(key, HandoffNormal, nonce, legacy)
	if _, err := VerifyHandoff(pub, HandoffNormal, nonce, legacy, legacyHandoff); err == nil {
		t.Fatal("legacy FirstEpoch0 admitted")
	}
	for _, n := range [][]byte{nil, make([]byte, 31), make([]byte, 32), make([]byte, 33)} {
		if _, err := SignHandoff(key, HandoffNormal, n, token); err == nil {
			t.Fatalf("invalid nonce length%d", len(n))
		}
	}
	mixed := make([]byte, 32)
	mixed[31] = 1
	if !ValidTargetNonce(mixed) {
		t.Fatal("valid random zero bytes refused")
	}
}
