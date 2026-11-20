package failover

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
)

// ErrEvidenceInvalid marks a resolvable but invalid/revoked/mismatched proof.
var ErrEvidenceInvalid = errors.New("evidence-invalid")

// Outcomes for the structured "failover outcome" log (S9). No credentials or raw
// provider bodies are logged.
const (
	OutcomeCandidate           = "candidate"
	OutcomeEvidenceUnavailable = "evidence-unavailable"
	OutcomeEvidenceInvalid     = "evidence-invalid"
	OutcomeShutdownVerified    = "shutdown-verified"
	OutcomeRecoveryRequested   = "recovery-requested"
	OutcomeCleanupBlocked      = "cleanup-blocked"
	OutcomeCleanupCompleted    = "cleanup-completed"
)

// Identity binds a Kubernetes Node incarnation to the authority's machine generation.
// Names alone are insufficient.
type Identity struct {
	Name              string
	UID               string
	ProviderID        string
	MachineGeneration string
}

// ShutdownProof is an authenticated fence for an exact incarnation. Reference is an opaque
// nonsecret durable authority operation id; it is not a transferable bearer credential.
type ShutdownProof struct {
	Identity
	Authority       string
	Reference       string
	FenceGeneration string
}

// ReturnPermit authorizes cleanup of an owned mark for an exact incarnation and operation.
type ReturnPermit struct {
	Identity
	OperationID string
	Reference   string
}

// Authority is a trusted shutdown/return evidence provider. A nil Authority is the production
// default: observe-only, every evidence call is unavailable, and there is no timeout fallback.
type Authority interface {
	ResolveIdentity(ctx context.Context, n Node) (Identity, error)
	VerifyShutdown(ctx context.Context, id Identity) (ShutdownProof, error)
	ValidateShutdown(ctx context.Context, proof ShutdownProof) error
	VerifyReturn(ctx context.Context, id Identity, operationID string) (ReturnPermit, error)
	ValidateReturn(ctx context.Context, permit ReturnPermit) error
}

// OperationRecord is the versioned owner annotation value under OwnerAnnotation.
// Only an exact parsed v1 record qualifies for owned operations. Legacy RFC3339 timestamps
// are foreign/legacy and must never be adopted or removed automatically.
type OperationRecord struct {
	Version           int    `json:"version"`
	NodeUID           string `json:"nodeUID"`
	ProviderID        string `json:"providerID"`
	MachineGeneration string `json:"machineGeneration"`
	OperationID       string `json:"operationID"`
	Authority         string `json:"authority"`
	EvidenceRef       string `json:"evidenceRef"`
	FenceGeneration   string `json:"fenceGeneration"`
}

// ParseOperationRecord accepts only versioned JSON object records. Timestamp or other
// legacy values return ok=false (foreign/legacy).
func ParseOperationRecord(raw string) (OperationRecord, bool) {
	raw = strings.TrimSpace(raw)
	if raw == "" || raw[0] != '{' {
		return OperationRecord{}, false
	}
	var rec OperationRecord
	if err := json.Unmarshal([]byte(raw), &rec); err != nil {
		return OperationRecord{}, false
	}
	if rec.Version != 1 || rec.NodeUID == "" || rec.OperationID == "" || rec.Authority == "" {
		return OperationRecord{}, false
	}
	return rec, true
}

// EncodeOperationRecord marshals a v1 record for the owner annotation.
func EncodeOperationRecord(rec OperationRecord) (string, error) {
	if rec.Version != 1 {
		return "", fmt.Errorf("operation record version must be 1")
	}
	b, err := json.Marshal(rec)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

// MarkRequest is a conditional node-wide out-of-service mark for one verified incarnation.
type MarkRequest struct {
	Identity    Identity
	Proof       ShutdownProof
	OperationID string
}

// ClearRequest is a conditional removal of an owned mark after a verified return permit.
type ClearRequest struct {
	Identity    Identity
	OperationID string
	Permit      ReturnPermit
}

// ScopedRecoveryAvailable reports whether scoped partition recovery is configured.
// It is never enabled by this correction; callers must report unavailable (S5).
func ScopedRecoveryAvailable() bool { return false }

func logOutcome(node, nodeUID, operationID, evidenceRef, outcome, reason string) {
	slog.Info("failover outcome",
		"node", node,
		"nodeUID", nodeUID,
		"operationID", operationID,
		"evidenceRef", evidenceRef,
		"outcome", outcome,
		"reason", reason,
	)
}
