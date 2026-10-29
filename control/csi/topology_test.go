package csi

import (
	"context"
	"slices"
	"testing"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
)

func TestTopologyFromKube(t *testing.T) {
	got := topologyFromKube(map[string]string{
		"topology.kubernetes.io/zone": "eu-west",
		"flaminio.io/rack":            "r12",
		"kubernetes.io/hostname":      "fl-a",
		"flaminio.io/witness":         "",
	})
	if got["topology.kubernetes.io/zone"] != "eu-west" || got["flaminio.io/rack"] != "r12" {
		t.Fatalf("got %v", got)
	}
	if _, ok := got["kubernetes.io/hostname"]; ok {
		t.Fatalf("copied unrelated label: %v", got)
	}
	if len(topologyFromKube(nil)) != 0 || len(topologyFromKube(map[string]string{"other": "x"})) != 0 {
		t.Fatal("empty topology expected")
	}
}

// The topology sync registers labels without an address: it waits for the data plane's
// registration, and keeps whatever address the data plane registered, even one registered after
// the sync started (docs/7).
func TestRegisterTopologyKeepsTheDataPlanesAddress(t *testing.T) {
	ctx := context.Background()
	m := metaFixture(t)
	topo := map[string]string{domain.LabelZone: "z1"}
	if err := registerTopology(ctx, m, "n1", topo); err != errNodeNotRegistered {
		t.Fatalf("before the data plane registered: %v", err)
	}
	for _, addr := range []string{"n1:1", "n1:2"} {
		if err := m.RegisterNode(ctx, &metav1.Node{Id: "n1", Address: addr}); err != nil {
			t.Fatal(err)
		}
		if err := registerTopology(ctx, m, "n1", topo); err != nil {
			t.Fatal(err)
		}
		nodes, err := m.ListNodes(ctx)
		if err != nil {
			t.Fatal(err)
		}
		i := slices.IndexFunc(nodes, func(n *metav1.Node) bool { return n.GetId() == "n1" })
		if i < 0 || nodes[i].GetAddress() != addr || nodes[i].GetLabels()[domain.LabelZone] != "z1" {
			t.Fatalf("nodes %v, want n1 at %s with zone z1", nodes, addr)
		}
	}
}
