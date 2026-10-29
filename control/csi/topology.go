package csi

import (
	"context"
	"log/slog"
	"time"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/control/failover"
	"github.com/flaminio/flaminio/meta/domain"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// TopologyKeys are the Kubernetes Node labels copied into the metadata service on register
// (docs/followups.md §4).
var TopologyKeys = []string{domain.LabelZone, domain.LabelRack}

// topologyFromKube copies failure-domain labels from a Kubernetes Node's label map.
func topologyFromKube(labels map[string]string) map[string]string {
	out := map[string]string{}
	for _, k := range TopologyKeys {
		if v := labels[k]; v != "" {
			out[k] = v
		}
	}
	return out
}

// SyncTopologyLabels reads this node's zone/rack labels from Kubernetes and overlays them on
// the metadata service registration. It waits until the data plane has registered an address,
// and retries every 5 s until it succeeds. Missing labels are skipped; an empty topology is fine
// (placement stays at node level).
func SyncTopologyLabels(ctx context.Context, meta *Meta, kube *failover.APIClient, nodeID string) {
	for {
		if err := syncTopologyOnce(ctx, meta, kube, nodeID); err != nil {
			if ctx.Err() != nil {
				return
			}
			slog.Warn("topology labels", "node", nodeID, "error", err)
		} else {
			return
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(5 * time.Second):
		}
	}
}

func syncTopologyOnce(ctx context.Context, meta *Meta, kube *failover.APIClient, nodeID string) error {
	kubeLabels, err := kube.NodeLabels(ctx, nodeID)
	if err != nil {
		return err
	}
	return registerTopology(ctx, meta, nodeID, topologyFromKube(kubeLabels))
}

// registerTopology adds labels to the node's registration without an address, so the metadata
// service keeps the one the data plane registered, even if it changes meanwhile (docs/7). A
// node the data plane has not registered yet is refused with FailedPrecondition. A metadata
// service older than this refuses the missing address; SyncTopologyLabels then retries until
// it is upgraded.
func registerTopology(ctx context.Context, meta *Meta, nodeID string, topo map[string]string) error {
	err := meta.RegisterNode(ctx, &metav1.Node{Id: nodeID, Labels: topo})
	if status.Code(err) == codes.FailedPrecondition {
		return errNodeNotRegistered
	}
	return err
}

type notRegisteredError struct{}

func (notRegisteredError) Error() string { return "node not yet registered with the metadata service" }

var errNodeNotRegistered = notRegisteredError{}
