// Package failover is the failover controller: it investigates storage nodes that look lost
// and, only with verified shutdown evidence from a configured authority, marks them out of
// service so Kubernetes can delete their pods and detach volumes.
//
// Candidate signals (NotReady/Unknown past Grace, missing metadata heartbeat, another Ready
// storage node) never authorize a node-wide mark by themselves. Without an authority the
// controller stays observe-only. Scoped partition recovery is unavailable; this package never
// mutates pods or VolumeAttachments. Flaminio block I/O still depends on epochs and leases,
// not on Kubernetes marks (docs/replication-protocol.md).
//
// The mark is node.kubernetes.io/out-of-service=nodeshutdown:NoExecute with a versioned
// flaminio.io/out-of-service operation record. Cleanup requires a return permit from the
// authority; Ready plus heartbeat is not enough. The first profile (manual-lockout-v1) refuses
// Clear and uses terminal old-UID retirement instead.
package failover

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"
)

const (
	TaintKey        = "node.kubernetes.io/out-of-service"
	TaintValue      = "nodeshutdown"
	TaintEffect     = "NoExecute"
	OwnerAnnotation = "flaminio.io/out-of-service"
)

// Node is what the controller needs to know about a Kubernetes node.
type Node struct {
	Name            string
	UID             string
	ProviderID      string
	ResourceVersion string
	// Ready is the Ready condition True; Since is when it last changed.
	// NotReady covers both False and Unknown.
	Ready bool
	Since time.Time
	// OutOfService: any out-of-service taint is present.
	OutOfService bool
	// Record is a parsed owned operation, if the annotation is a valid v1 record.
	Record OperationRecord
	Owned  bool
}

// Kube reads storage nodes and applies conditional mark/clear mutations.
type Kube interface {
	StorageNodes(ctx context.Context) ([]Node, error)
	GetNode(ctx context.Context, name string) (Node, error)
	Mark(ctx context.Context, req MarkRequest) error
	Clear(ctx context.Context, req ClearRequest) error
}

// Liveness reports which nodes the metadata service considers alive.
type Liveness interface {
	Alive(ctx context.Context) (map[string]bool, error)
}

// Controller investigates candidates and applies evidence-gated recovery.
type Controller struct {
	Kube      Kube
	Liveness  Liveness
	Authority Authority // nil: observe-only (production default)
	// How long a node must have been not Ready before it may be a candidate.
	Grace time.Duration
	Now   func() time.Time
}

// Step looks at every storage node once: candidates may be marked only with applicable
// shutdown evidence; owned marks are cleared only with a return permit.
func (c *Controller) Step(ctx context.Context) error {
	nodes, err := c.Kube.StorageNodes(ctx)
	if err != nil {
		return err
	}
	alive, err := c.Liveness.Alive(ctx)
	if err != nil {
		return err
	}
	ready := 0
	for _, n := range nodes {
		if n.Ready && !n.OutOfService {
			ready++
		}
	}
	now := c.Now()
	for _, n := range nodes {
		if err := c.handleCandidate(ctx, n, alive, ready, now); err != nil {
			return err
		}
		if err := c.handleCleanup(ctx, n); err != nil {
			return err
		}
	}
	return nil
}

func (c *Controller) handleCandidate(ctx context.Context, listed Node, alive map[string]bool, ready int, now time.Time) error {
	if listed.OutOfService || listed.Ready || now.Sub(listed.Since) < c.Grace || alive[listed.Name] {
		return nil
	}
	if ready == 0 {
		logOutcome(listed.Name, listed.UID, "", "", OutcomeCandidate,
			"no other storage node is Ready: not investigating recovery")
		return nil
	}
	logOutcome(listed.Name, listed.UID, "", "", OutcomeCandidate,
		fmt.Sprintf("not Ready for %s; metadata heartbeat missing", now.Sub(listed.Since).Round(time.Second)))

	current, err := c.Kube.GetNode(ctx, listed.Name)
	if err != nil {
		// A missing Node is not shutdown evidence; never recreate it.
		logOutcome(listed.Name, listed.UID, "", "", OutcomeEvidenceUnavailable, "node get: "+err.Error())
		return nil
	}
	if current.UID == "" || current.UID != listed.UID && listed.UID != "" {
		// Replacement UID starts a new investigation; listed UID may be empty on first list.
		if listed.UID != "" && current.UID != listed.UID {
			logOutcome(listed.Name, current.UID, "", "", OutcomeEvidenceInvalid, "node UID changed since listing")
			return nil
		}
	}
	if current.OutOfService {
		return nil
	}
	if c.Authority == nil {
		logOutcome(current.Name, current.UID, "", "", OutcomeEvidenceUnavailable, "no failover authority configured")
		return nil
	}
	id, err := c.Authority.ResolveIdentity(ctx, current)
	if err != nil {
		outcome := OutcomeEvidenceUnavailable
		if errors.Is(err, ErrEvidenceInvalid) {
			outcome = OutcomeEvidenceInvalid
		}
		logOutcome(current.Name, current.UID, "", "", outcome, "resolve identity: "+err.Error())
		return nil
	}
	proof, err := c.Authority.VerifyShutdown(ctx, id)
	if err != nil {
		outcome := OutcomeEvidenceUnavailable
		if errors.Is(err, ErrEvidenceInvalid) {
			outcome = OutcomeEvidenceInvalid
		}
		logOutcome(current.Name, current.UID, "", "", outcome, err.Error())
		return nil
	}
	if err := c.Authority.ValidateShutdown(ctx, id, proof); err != nil {
		logOutcome(current.Name, current.UID, proof.Reference, proof.Reference, OutcomeEvidenceInvalid, err.Error())
		return nil
	}
	logOutcome(current.Name, current.UID, proof.Reference, proof.Reference, OutcomeShutdownVerified, proof.Authority)
	req := MarkRequest{Identity: id, Proof: proof, OperationID: proof.Reference}
	if err := c.Kube.Mark(ctx, req); err != nil {
		// Conflicts restart at observation next step; do not invent success. Leave other
		// nodes alone when the mark is foreign/conflicting rather than a transport failure.
		logOutcome(current.Name, current.UID, proof.Reference, proof.Reference, OutcomeEvidenceInvalid, "mark: "+err.Error())
		if errors.Is(err, ErrForeignMark) || strings.Contains(err.Error(), "foreign/conflicting") {
			return nil
		}
		return err
	}
	logOutcome(current.Name, current.UID, proof.Reference, proof.Reference, OutcomeRecoveryRequested, "out-of-service mark applied")
	return nil
}

func (c *Controller) handleCleanup(ctx context.Context, listed Node) error {
	if !listed.OutOfService || !listed.Owned {
		return nil
	}
	// Ready + heartbeat never supply a return permit; still require the authority (S7).
	current, err := c.Kube.GetNode(ctx, listed.Name)
	if err != nil {
		logOutcome(listed.Name, listed.UID, listed.Record.OperationID, listed.Record.EvidenceRef,
			OutcomeCleanupBlocked, "node get: "+err.Error())
		return nil
	}
	if !current.Owned || current.Record.OperationID != listed.Record.OperationID {
		logOutcome(current.Name, current.UID, listed.Record.OperationID, listed.Record.EvidenceRef,
			OutcomeCleanupBlocked, "owned operation record missing or changed")
		return nil
	}
	if c.Authority == nil {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, "no return policy configured")
		return nil
	}
	id, err := c.Authority.ResolveIdentity(ctx, current)
	if err != nil {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, "resolve identity: "+err.Error())
		return nil
	}
	if id.UID != current.Record.NodeUID {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, "resolved identity does not match owned record")
		return nil
	}
	permit, err := c.Authority.VerifyReturn(ctx, id, current.Record.OperationID)
	if err != nil {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, err.Error())
		return nil
	}
	if err := c.Authority.ValidateReturn(ctx, permit); err != nil {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, err.Error())
		return nil
	}
	if err := c.Kube.Clear(ctx, ClearRequest{
		Identity:    id,
		OperationID: current.Record.OperationID,
		Permit:      permit,
	}); err != nil {
		logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
			OutcomeCleanupBlocked, "clear: "+err.Error())
		return err
	}
	logOutcome(current.Name, current.UID, current.Record.OperationID, current.Record.EvidenceRef,
		OutcomeCleanupCompleted, "owned mark cleared")
	return nil
}

// Run calls Step every interval until ctx ends.
func (c *Controller) Run(ctx context.Context, interval time.Duration) {
	t := time.NewTicker(interval)
	defer t.Stop()
	slog.Info("failover controller running",
		"grace", c.Grace,
		"scopedRecovery", ScopedRecoveryAvailable(),
		"authorityConfigured", c.Authority != nil,
	)
	for {
		if err := c.Step(ctx); err != nil {
			slog.Warn("failover step", "error", err)
		}
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
	}
}
