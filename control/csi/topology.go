package csi

import (
	"context"
	"log/slog"
	"time"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/control/failover"
	"github.com/flaminio/flaminio/meta/domain"
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
// the metadata service registration. It waits until the data plane has registered an address.
// Missing labels are skipped; an empty topology is fine (placement stays at node level).
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
	topo := topologyFromKube(kubeLabels)
	nodes, err := meta.ListNodes(ctx)
	if err != nil {
		return err
	}
	var address string
	for _, n := range nodes {
		if n.GetId() == nodeID {
			address = n.GetAddress()
			break
		}
	}
	if address == "" {
		return errNodeNotRegistered
	}
	return meta.RegisterNode(ctx, &metav1.Node{Id: nodeID, Address: address, Labels: topo})
}

type notRegisteredError struct{}

func (notRegisteredError) Error() string { return "node not yet registered with the metadata service" }

var errNodeNotRegistered = notRegisteredError{}
