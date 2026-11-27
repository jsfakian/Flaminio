package captoken

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"errors"
)

const (
	// TargetNonceLen is the daemon's unpredictable process challenge length.
	TargetNonceLen = 32
	// HandoffLen is the exact FLCH envelope length, including its signature.
	HandoffLen     = 4 + 1 + 1 + TargetNonceLen + sha256.Size + ed25519.SignatureSize
	handoffBodyLen = HandoffLen - ed25519.SignatureSize
	handoffMagic   = "FLCH"
	handoffVersion = 1
)

// HandoffPurpose separates ordinary target admission from private initialization.
type HandoffPurpose byte

const (
	HandoffNormal HandoffPurpose = 1
	// HandoffInitialization reserves the separate private-initialization purpose; it does not
	// itself authorize initialization or replace its operation-specific credentials.
	HandoffInitialization HandoffPurpose = 2
)

// ErrInvalidHandoff never exposes either signed credential or the process challenge.
var ErrInvalidHandoff = errors.New("invalid target handoff")

// ValidTargetNonce accepts exactly 32 bytes with at least one nonzero byte. Individual zero
// bytes are valid CSPRNG output.
func ValidTargetNonce(nonce []byte) bool {
	if len(nonce) != TargetNonceLen {
		return false
	}
	for _, b := range nonce {
		if b != 0 {
			return true
		}
	}
	return false
}

func validHandoffPurpose(purpose HandoffPurpose) bool {
	return purpose == HandoffNormal || purpose == HandoffInitialization
}

// SignHandoff binds an exact, signed FLCT capability to a daemon challenge. The caller must
// issue it only after the corresponding new-epoch transaction commits. A legacy capability
// may be encoded, but strict target verification refuses its missing first epoch.
func SignHandoff(key ed25519.PrivateKey, purpose HandoffPurpose, nonce, token []byte) ([]byte, error) {
	if len(key) != ed25519.PrivateKeySize || !ValidTargetNonce(nonce) || !validHandoffPurpose(purpose) {
		return nil, ErrInvalidHandoff
	}
	if _, err := Verify(key.Public().(ed25519.PublicKey), token); err != nil {
		return nil, ErrInvalidHandoff
	}
	b := make([]byte, 0, HandoffLen)
	b = append(b, handoffMagic...)
	b = append(b, handoffVersion, byte(purpose))
	b = append(b, nonce...)
	digest := sha256.Sum256(token)
	b = append(b, digest[:]...)
	return append(b, ed25519.Sign(key, b)...), nil
}

// VerifyHandoff checks both signatures, the exact capability digest and the current process
// nonce/purpose. Strict targets require a nonlegacy incarnation. The caller must also match
// the returned volume/node/epoch against its identity and current metadata view; metadata's
// birth/epoch fence identifies the incarnation, whose first epoch comes from these claims.
func VerifyHandoff(pub ed25519.PublicKey, purpose HandoffPurpose, nonce, token, handoff []byte) (Claims, error) {
	if len(pub) != ed25519.PublicKeySize || len(handoff) != HandoffLen ||
		!ValidTargetNonce(nonce) || !validHandoffPurpose(purpose) {
		return Claims{}, ErrInvalidHandoff
	}
	body, sig := handoff[:handoffBodyLen], handoff[handoffBodyLen:]
	if string(body[:4]) != handoffMagic || body[4] != handoffVersion || body[5] != byte(purpose) ||
		!bytes.Equal(body[6:6+TargetNonceLen], nonce) || !ed25519.Verify(pub, body, sig) {
		return Claims{}, ErrInvalidHandoff
	}
	claims, err := Verify(pub, token)
	if err != nil || claims.FirstEpoch == 0 {
		return Claims{}, ErrInvalidHandoff
	}
	digest := sha256.Sum256(token)
	if !bytes.Equal(body[6+TargetNonceLen:], digest[:]) {
		return Claims{}, ErrInvalidHandoff
	}
	return claims, nil
}
