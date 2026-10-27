package csi

import "testing"

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
