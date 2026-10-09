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

func (f *fakeKube) SetOutOfService(_ context.Context, name string, on bool) error {
	for i := range f.nodes {
		if f.nodes[i].Name == name {
			f.nodes[i].OutOfService, f.nodes[i].Ours = on, on
		}
	}
	if on {
		f.calls = append(f.calls, "mark "+name)
	} else {
		f.calls = append(f.calls, "unmark "+name)
	}
	return nil
}

type fakeLiveness map[string]bool

func (f fakeLiveness) Alive(context.Context) (map[string]bool, error) { return f, nil }

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
	ready := Node{Name: "b", Ready: true, Since: now.Add(-time.Hour)}
	lost := Node{Name: "a", Ready: false, Since: now.Add(-time.Minute)}
	for _, tc := range []struct {
		what  string
		nodes []Node
		alive fakeLiveness
		want  string
	}{
		{"lost: not Ready past the grace period, data plane silent", []Node{lost, ready}, fakeLiveness{"b": true}, "mark a"},
		{"not Ready, but its data plane still heartbeats", []Node{lost, ready}, fakeLiveness{"a": true, "b": true}, ""},
		{"not Ready for less than the grace period", []Node{{Name: "a", Since: now.Add(-10 * time.Second)}, ready}, fakeLiveness{"b": true}, ""},
		{"no other storage node is Ready", []Node{lost, {Name: "b", Since: now.Add(-time.Hour)}}, fakeLiveness{}, ""},
		{"back: Ready and alive, our mark comes off", []Node{{Name: "a", Ready: true, OutOfService: true, Ours: true}, ready}, fakeLiveness{"a": true, "b": true}, "unmark a"},
		{"Ready again, data plane not yet: the mark stays", []Node{{Name: "a", Ready: true, OutOfService: true, Ours: true}, ready}, fakeLiveness{"b": true}, ""},
		{"someone else's mark is never removed", []Node{{Name: "a", Ready: true, OutOfService: true}, ready}, fakeLiveness{"a": true, "b": true}, ""},
	} {
		if got := strings.Join(run(tc.nodes, tc.alive), ","); got != tc.want {
			t.Errorf("%s: got %q, want %q", tc.what, got, tc.want)
		}
	}
}

// TestAPIClient checks the requests against a fake API server: the list's selector, and a
// patch that keeps other taints and carries the resourceVersion it read.
func TestAPIClient(t *testing.T) {
	node := `{"metadata":{"name":"fl-a","resourceVersion":"42","annotations":{}},
	  "spec":{"taints":[{"key":"other","effect":"NoSchedule"}]},
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
			http.Error(w, "unexpected", http.StatusBadRequest)
		}
	}))
	defer srv.Close()
	c := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client()}
	ctx := context.Background()

	nodes, err := c.StorageNodes(ctx)
	if err != nil || len(nodes) != 1 || nodes[0].Name != "fl-a" || nodes[0].Ready || nodes[0].OutOfService ||
		!nodes[0].Since.Equal(time.Date(2026, 10, 9, 11, 0, 0, 0, time.UTC)) {
		t.Fatalf("nodes %+v %v", nodes, err)
	}
	if err := c.SetOutOfService(ctx, "fl-a", true); err != nil {
		t.Fatal(err)
	}
	b, _ := json.Marshal(patch)
	got := string(b)
	for _, want := range []string{`"resourceVersion":"42"`, `{"effect":"NoSchedule","key":"other"}`,
		`{"effect":"NoExecute","key":"node.kubernetes.io/out-of-service","value":"nodeshutdown"}`, `"flaminio.io/out-of-service":"20`} {
		if !strings.Contains(got, want) {
			t.Fatalf("patch %s lacks %s", got, want)
		}
	}
}
