package csi

import (
	"context"
	"slices"
	"sync"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

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

// The topology sync waits for the data plane's registration, then adds its labels and keeps
// whatever address the data plane registered (docs/7).
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

// recordingMeta is a metadata service that records RegisterNode requests and answers them with
// `answer`, and lists one node at `listed`.
type recordingMeta struct {
	metav1.UnimplementedMetadataServiceServer
	mu       sync.Mutex
	requests []*metav1.Node
	answer   func(*metav1.Node) error
	listed   string
}

func (r *recordingMeta) RegisterNode(_ context.Context, req *metav1.RegisterNodeRequest) (*metav1.RegisterNodeResponse, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.requests = append(r.requests, req.GetNode())
	return &metav1.RegisterNodeResponse{}, r.answer(req.GetNode())
}

func (r *recordingMeta) ListNodes(context.Context, *metav1.ListNodesRequest) (*metav1.ListNodesResponse, error) {
	return &metav1.ListNodesResponse{Nodes: []*metav1.Node{{Id: "n1", Address: r.listed}}}, nil
}

// The sync's registration carries no address, so it cannot put back one the data plane
// changed; against an older metadata service, which refuses that, it sends the listed address.
func TestRegisterTopologySendsNoAddress(t *testing.T) {
	ctx := context.Background()
	topo := map[string]string{domain.LabelZone: "z1"}
	r := &recordingMeta{answer: func(*metav1.Node) error { return nil }}
	if err := registerTopology(ctx, serveMeta(t, r), "n1", topo); err != nil {
		t.Fatal(err)
	}
	if len(r.requests) != 1 || r.requests[0].GetAddress() != "" || r.requests[0].GetLabels()[domain.LabelZone] != "z1" {
		t.Fatalf("requests %v", r.requests)
	}

	old := &recordingMeta{listed: "n1:7411", answer: func(n *metav1.Node) error {
		if n.GetAddress() == "" {
			return status.Error(codes.InvalidArgument, "node needs a valid id and an address")
		}
		return nil
	}}
	if err := registerTopology(ctx, serveMeta(t, old), "n1", topo); err != nil {
		t.Fatal(err)
	}
	if len(old.requests) != 2 || old.requests[1].GetAddress() != "n1:7411" {
		t.Fatalf("requests to an older service %v", old.requests)
	}
}

// A refusal that is not about the address (an invalid id) comes back as it is, rather than as
// "not registered", which the sync would retry for good.
func TestRegisterTopologyPassesOnOtherRefusals(t *testing.T) {
	r := &recordingMeta{answer: func(*metav1.Node) error {
		return status.Error(codes.InvalidArgument, "node needs a valid id")
	}}
	err := registerTopology(context.Background(), serveMeta(t, r), "Not A Valid Id", nil)
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("got %v, want the InvalidArgument", err)
	}
}

// Against an older metadata service, a valid node the data plane has not registered yet is
// reported as not registered, as with a current one.
func TestRegisterTopologyOnAnOlderServiceBeforeTheDataPlane(t *testing.T) {
	old := &recordingMeta{answer: func(*metav1.Node) error {
		return status.Error(codes.InvalidArgument, "node needs a valid id and an address")
	}}
	err := registerTopology(context.Background(), serveMeta(t, old), "n2", nil)
	if err != errNodeNotRegistered {
		t.Fatalf("got %v, want errNodeNotRegistered", err)
	}
}
