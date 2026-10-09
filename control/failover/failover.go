// Package failover is the failover controller: it marks a lost storage node out of service,
// so that Kubernetes deletes its pods and detaches their volumes without waiting for a kubelet
// that will not answer, and the pods start elsewhere with their volumes.
//
// That is safe for Flaminio volumes whatever the lost node is still doing: attaching a volume
// elsewhere issues a new epoch, which fences the old target on every replica, and the old
// target serves no read once its lease has run out (docs/replication-protocol.md; checked in
// the lab by `make partition-test`). It is not a statement about the pods' other side effects,
// which is why a node is marked only when two independent signals agree that it is gone:
//   - Kubernetes: the node has not been Ready for Grace;
//   - Flaminio: the node's data plane has stopped renewing its liveness in the metadata service.
//
// The mark is the standard taint node.kubernetes.io/out-of-service=nodeshutdown:NoExecute, with
// an annotation saying this controller set it. The controller removes its own marks once both
// signals say the node is back; it never removes a mark someone else set.
package failover

import (
	"context"
	"log/slog"
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
	Name string
	// Ready is the node's Ready condition; Since is when it last changed.
	Ready bool
	Since time.Time
	// OutOfService: the node has the out-of-service taint. Ours: this controller set it.
	OutOfService bool
	Ours         bool
}

// Kube reads the storage nodes and marks them.
type Kube interface {
	StorageNodes(ctx context.Context) ([]Node, error)
	// SetOutOfService adds (on) or removes (off) the taint and the annotation.
	SetOutOfService(ctx context.Context, node string, on bool) error
}

// Liveness reports which nodes the metadata service considers alive.
type Liveness interface {
	Alive(ctx context.Context) (map[string]bool, error)
}

type Controller struct {
	Kube     Kube
	Liveness Liveness
	// How long a node must have been not Ready before it may be marked.
	Grace time.Duration
	Now   func() time.Time
}

// Step looks at every storage node once, and marks or unmarks them.
func (c *Controller) Step(ctx context.Context) error {
	nodes, err := c.Kube.StorageNodes(ctx)
	if err != nil {
		return err
	}
	// Without an answer from the metadata service nothing is known about the data plane:
	// no change either way.
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
		switch {
		case !n.OutOfService && !n.Ready && now.Sub(n.Since) >= c.Grace && !alive[n.Name]:
			if ready == 0 {
				slog.Warn("node lost, but no other storage node is Ready: not marking it", "node", n.Name)
				continue
			}
			slog.Info("marking a lost node out of service", "node", n.Name, "notReadyFor", now.Sub(n.Since).Round(time.Second))
			if err := c.Kube.SetOutOfService(ctx, n.Name, true); err != nil {
				return err
			}
		case n.OutOfService && n.Ours && n.Ready && alive[n.Name]:
			slog.Info("node back: removing the out-of-service mark", "node", n.Name)
			if err := c.Kube.SetOutOfService(ctx, n.Name, false); err != nil {
				return err
			}
		}
	}
	return nil
}

// Run calls Step every interval until ctx ends.
func (c *Controller) Run(ctx context.Context, interval time.Duration) {
	t := time.NewTicker(interval)
	defer t.Stop()
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
