package failover

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

type fakeKube struct {
	nodes []Node
	calls []string
}

func (f *fakeKube) StorageNodes(context.Context) ([]Node, error) { return f.nodes, nil }

func (f *fakeKube) GetNode(_ context.Context, name string) (Node, error) {
	for _, n := range f.nodes {
		if n.Name == name {
			return n, nil
		}
	}
	return Node{}, osErrNotExist(name)
}

type osErrNotExist string

func (e osErrNotExist) Error() string { return "node not found: " + string(e) }

func (f *fakeKube) Mark(_ context.Context, req MarkRequest) error {
	for i := range f.nodes {
		if f.nodes[i].Name == req.Identity.Name {
			f.nodes[i].OutOfService = true
			f.nodes[i].Owned = true
			f.nodes[i].Record = OperationRecord{
				Version: 1, NodeUID: req.Identity.UID, OperationID: req.OperationID,
				Authority: req.Proof.Authority, EvidenceRef: req.Proof.Reference,
				MachineGeneration: req.Identity.MachineGeneration,
				FenceGeneration:   req.Proof.FenceGeneration,
			}
		}
	}
	f.calls = append(f.calls, "mark "+req.Identity.Name)
	return nil
}

func (f *fakeKube) Clear(_ context.Context, req ClearRequest) error {
	for i := range f.nodes {
		if f.nodes[i].Name == req.Identity.Name {
			f.nodes[i].OutOfService, f.nodes[i].Owned = false, false
			f.nodes[i].Record = OperationRecord{}
		}
	}
	f.calls = append(f.calls, "unmark "+req.Identity.Name)
	return nil
}

type fakeLiveness map[string]bool

func (f fakeLiveness) Alive(context.Context) (map[string]bool, error) { return f, nil }

// TestStep checks candidate timing with a nil authority: partitions and lost heartbeats
// never mark. Full evidence/cleanup matrices are in evidence_test.go (test-engineer).
func TestStep(t *testing.T) {
	now := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	ctx := context.Background()
	run := func(nodes []Node, alive fakeLiveness) []string {
		t.Helper()
		k := &fakeKube{nodes: nodes}
		c := &Controller{Kube: k, Liveness: alive, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		return k.calls
	}
	ready := Node{Name: "b", UID: "uid-b", Ready: true, Since: now.Add(-time.Hour)}
	lost := Node{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-time.Minute)}
	for _, tc := range []struct {
		what  string
		nodes []Node
		alive fakeLiveness
		want  string
	}{
		{"lost partition is only a candidate: no authority, no mark", []Node{lost, ready}, fakeLiveness{"b": true}, ""},
		{"not Ready, but its data plane still heartbeats", []Node{lost, ready}, fakeLiveness{"a": true, "b": true}, ""},
		{"not Ready for less than the grace period", []Node{{Name: "a", UID: "uid-a", Since: now.Add(-10 * time.Second)}, ready}, fakeLiveness{"b": true}, ""},
		{"no other storage node is Ready", []Node{lost, {Name: "b", UID: "uid-b", Since: now.Add(-time.Hour)}}, fakeLiveness{}, ""},
		{"owned mark is not cleared by Ready+heartbeat alone", []Node{
			{Name: "a", UID: "uid-a", Ready: true, OutOfService: true, Owned: true,
				Record: OperationRecord{Version: 1, NodeUID: "uid-a", OperationID: "op", Authority: ManualLockoutProfile}},
			ready,
		}, fakeLiveness{"a": true, "b": true}, ""},
		{"someone else's mark is never removed", []Node{{Name: "a", UID: "uid-a", Ready: true, OutOfService: true}, ready}, fakeLiveness{"a": true, "b": true}, ""},
	} {
		if got := strings.Join(run(tc.nodes, tc.alive), ","); got != tc.want {
			t.Errorf("%s: got %q, want %q", tc.what, got, tc.want)
		}
	}
}

func TestScopedRecoveryUnavailable(t *testing.T) {
	if ScopedRecoveryAvailable() {
		t.Fatal("scoped recovery must stay unavailable")
	}
}

// TestAPIClient checks list selector and a conditional Mark that keeps other taints and
// writes a versioned operation record under the resourceVersion it read.
func TestAPIClient(t *testing.T) {
	node := `{"metadata":{"name":"fl-a","uid":"uid-a","resourceVersion":"42","annotations":{}},
	  "spec":{"providerID":"","taints":[{"key":"other","effect":"NoSchedule"}]},
	  "status":{"conditions":[{"type":"Ready","status":"Unknown","lastTransitionTime":"2026-10-09T11:00:00Z"}]}}`
	var patch map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer tok" {
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/nodes":
			if r.URL.Query().Get("labelSelector") != StorageLabel {
				http.Error(w, "selector", http.StatusBadRequest)
				return
			}
			_, _ = io.WriteString(w, `{"items":[`+node+`]}`)
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/nodes/fl-a":
			_, _ = io.WriteString(w, node)
		case r.Method == http.MethodPatch && r.Header.Get("Content-Type") == "application/merge-patch+json":
			_ = json.NewDecoder(r.Body).Decode(&patch)
			_, _ = io.WriteString(w, "{}")
		default:
			http.Error(w, "unexpected "+r.Method+" "+r.URL.Path, http.StatusBadRequest)
		}
	}))
	defer srv.Close()

	auth := &staticAuthority{
		id: Identity{Name: "fl-a", UID: "uid-a", MachineGeneration: "machine-a"},
		proof: ShutdownProof{
			Identity:        Identity{Name: "fl-a", UID: "uid-a", MachineGeneration: "machine-a"},
			Authority:       ManualLockoutProfile,
			Reference:       strings.Repeat("ab", 32),
			FenceGeneration: "machine-a",
		},
	}
	c := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
	ctx := context.Background()

	nodes, err := c.StorageNodes(ctx)
	if err != nil || len(nodes) != 1 || nodes[0].Name != "fl-a" || nodes[0].UID != "uid-a" || nodes[0].Ready || nodes[0].OutOfService ||
		!nodes[0].Since.Equal(time.Date(2026, 10, 9, 11, 0, 0, 0, time.UTC)) {
		t.Fatalf("nodes %+v %v", nodes, err)
	}
	req := MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: auth.proof.Reference}
	if err := c.Mark(ctx, req); err != nil {
		t.Fatal(err)
	}
	b, _ := json.Marshal(patch)
	got := string(b)
	for _, want := range []string{`"resourceVersion":"42"`, `"uid":"uid-a"`, `{"effect":"NoSchedule","key":"other"}`,
		`{"effect":"NoExecute","key":"node.kubernetes.io/out-of-service","value":"nodeshutdown"}`,
		`"flaminio.io/out-of-service":"`, `\"version\":1`, `\"operationID\":\"` + auth.proof.Reference} {
		if !strings.Contains(got, want) {
			t.Fatalf("patch %s lacks %s", got, want)
		}
	}
}

// staticAuthority is a test double; not an executable production option.
type staticAuthority struct {
	id    Identity
	proof ShutdownProof
}

func (s *staticAuthority) ResolveIdentity(context.Context, Node) (Identity, error) { return s.id, nil }
func (s *staticAuthority) VerifyShutdown(context.Context, Identity) (ShutdownProof, error) {
	return s.proof, nil
}
func (s *staticAuthority) ValidateShutdown(context.Context, ShutdownProof) error { return nil }
func (s *staticAuthority) VerifyReturn(context.Context, Identity, string) (ReturnPermit, error) {
	return ReturnPermit{}, errTerminalRetirement
}
func (s *staticAuthority) ValidateReturn(context.Context, ReturnPermit) error {
	return errTerminalRetirement
}
