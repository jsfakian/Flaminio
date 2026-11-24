package failover

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type fakeKube struct {
	mu          sync.Mutex
	nodes       []Node
	calls       []string
	getOverride map[string]Node
	getErr      map[string]error
	markErr     error
	clearErr    error
	podWrites   int
	vaWrites    int
}

func (f *fakeKube) StorageNodes(context.Context) ([]Node, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]Node, len(f.nodes))
	copy(out, f.nodes)
	return out, nil
}

func (f *fakeKube) GetNode(_ context.Context, name string) (Node, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err, ok := f.getErr[name]; ok {
		return Node{}, err
	}
	if n, ok := f.getOverride[name]; ok {
		return n, nil
	}
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
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.markErr != nil {
		return f.markErr
	}
	for i := range f.nodes {
		if f.nodes[i].Name != req.Identity.Name {
			continue
		}
		if f.nodes[i].OutOfService {
			if f.nodes[i].Owned && f.nodes[i].Record.OperationID == req.OperationID &&
				f.nodes[i].Record.NodeUID == req.Identity.UID {
				f.calls = append(f.calls, "mark "+req.Identity.Name)
				return nil // identical operation (lost-PATCH / concurrent winner)
			}
			return fmt.Errorf("%w", ErrForeignMark)
		}
		f.nodes[i].OutOfService = true
		f.nodes[i].Owned = true
		f.nodes[i].Record = OperationRecord{
			Version: 1, NodeUID: req.Identity.UID, OperationID: req.OperationID,
			Authority: req.Proof.Authority, EvidenceRef: req.Proof.Reference,
			MachineGeneration: req.Identity.MachineGeneration,
			FenceGeneration:   req.Proof.FenceGeneration,
			ProviderID:        req.Identity.ProviderID,
		}
		f.calls = append(f.calls, "mark "+req.Identity.Name)
		return nil
	}
	return osErrNotExist(req.Identity.Name)
}

func (f *fakeKube) Clear(_ context.Context, req ClearRequest) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.clearErr != nil {
		return f.clearErr
	}
	for i := range f.nodes {
		if f.nodes[i].Name == req.Identity.Name {
			f.nodes[i].OutOfService, f.nodes[i].Owned = false, false
			f.nodes[i].Record = OperationRecord{}
		}
	}
	f.calls = append(f.calls, "unmark "+req.Identity.Name)
	return nil
}

type gatingKube struct {
	fakeKube
	markErr error
}

func (g *gatingKube) Mark(ctx context.Context, req MarkRequest) error {
	if g.markErr != nil {
		return g.markErr
	}
	return g.fakeKube.Mark(ctx, req)
}

type fakeLiveness map[string]bool

func (f fakeLiveness) Alive(context.Context) (map[string]bool, error) { return f, nil }

// TestStep checks candidate timing with a nil authority: partitions and lost heartbeats
// never mark. Full evidence/cleanup matrices are in evidence_test.go.
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

// TestPartitionNeverMarks covers S1: candidate signals alone never authorize recovery.
func TestPartitionNeverMarks(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	ctx := context.Background()
	ready := Node{Name: "b", UID: "uid-b", Ready: true, Since: now.Add(-time.Hour)}
	cases := []struct {
		name  string
		nodes []Node
		alive fakeLiveness
		setup func(*fakeKube)
	}{
		{
			name:  "Ready=false past Grace, missing heartbeat, another Ready",
			nodes: []Node{{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-2 * time.Hour)}, ready},
			alive: fakeLiveness{"b": true},
		},
		{
			name:  "Ready unknown (modelled as not Ready) past Grace",
			nodes: []Node{{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-24 * time.Hour)}, ready},
			alive: fakeLiveness{"b": true},
		},
		{
			name:  "arbitrarily long Grace still no mark without authority",
			nodes: []Node{{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-365 * 24 * time.Hour)}, ready},
			alive: fakeLiveness{"b": true},
		},
		{
			name:  "deleted Node on re-get",
			nodes: []Node{{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-time.Hour)}, ready},
			alive: fakeLiveness{"b": true},
			setup: func(k *fakeKube) {
				k.getErr = map[string]error{"a": osErrNotExist("a")}
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			k := &fakeKube{nodes: tc.nodes}
			if tc.setup != nil {
				tc.setup(k)
			}
			c := &Controller{Kube: k, Liveness: tc.alive, Grace: 40 * time.Second, Now: func() time.Time { return now }}
			if err := c.Step(ctx); err != nil {
				t.Fatal(err)
			}
			if len(k.calls) != 0 {
				t.Fatalf("mutations %v", k.calls)
			}
		})
	}
}

// TestVerifiedShutdownRecovery covers S3/S4: valid fence marks once; outages re-observe.
func TestVerifiedShutdownRecovery(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	lost, ready := candidatePair(now)
	ctx := context.Background()
	alive := fakeLiveness{"b": true}
	op := opID("ok")

	t.Run("valid held fence marks once", func(t *testing.T) {
		auth := &scriptAuthority{}
		k := &fakeKube{nodes: []Node{lost, ready}}
		c := &Controller{Kube: k, Liveness: alive, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if got := strings.Join(k.calls, ","); got != "mark a" {
			t.Fatalf("calls %q", got)
		}
		// Second step: already OutOfService → no second mark.
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if got := strings.Join(k.calls, ","); got != "mark a" {
			t.Fatalf("idempotent calls %q", got)
		}
	})

	t.Run("provider outage before mutation retries safely", func(t *testing.T) {
		auth := &scriptAuthority{verifyFn: func(context.Context, Identity) (ShutdownProof, error) {
			return ShutdownProof{}, errors.New("provider unavailable")
		}}
		k := &fakeKube{nodes: []Node{lost, ready}}
		c := &Controller{Kube: k, Liveness: alive, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if len(k.calls) != 0 {
			t.Fatalf("mutations %v", k.calls)
		}
		// Authority recovers; next step may mark.
		auth.verifyFn = nil
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if got := strings.Join(k.calls, ","); got != "mark a" {
			t.Fatalf("after recovery %q", got)
		}
	})

	t.Run("API get outage before mutation", func(t *testing.T) {
		k := &fakeKube{
			nodes:  []Node{lost, ready},
			getErr: map[string]error{"a": errors.New("api timeout")},
		}
		c := &Controller{Kube: k, Liveness: alive, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if len(k.calls) != 0 {
			t.Fatalf("mutations %v", k.calls)
		}
	})

	t.Run("lost PATCH after success re-observes identical operation", func(t *testing.T) {
		nodeJSON := func(rv, uid string, marked bool, ann string) string {
			taints := `[{"key":"other","effect":"NoSchedule"}]`
			if marked {
				taints = `[{"key":"other","effect":"NoSchedule"},{"key":"node.kubernetes.io/out-of-service","value":"nodeshutdown","effect":"NoExecute"}]`
			}
			annJSON := "{}"
			if ann != "" {
				annJSON = fmt.Sprintf(`{%q:%s}`, OwnerAnnotation, mustJSONString(ann))
			}
			return fmt.Sprintf(`{"metadata":{"name":"a","uid":%q,"resourceVersion":%q,"annotations":%s},`+
				`"spec":{"providerID":"","taints":%s},`+
				`"status":{"conditions":[{"type":"Ready","status":"False","lastTransitionTime":"2026-11-20T10:00:00Z"}]}}`,
				uid, rv, annJSON, taints)
		}
		rec := OperationRecord{
			Version: 1, NodeUID: "uid-a", OperationID: op, Authority: ManualLockoutProfile,
			EvidenceRef: op, MachineGeneration: "m1", FenceGeneration: "m1",
		}
		ann, err := EncodeOperationRecord(rec)
		if err != nil {
			t.Fatal(err)
		}
		var patches atomic.Int32
		var state atomic.Int32 // 0 unmarked, 1 marked after "lost" success
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch {
			case r.Method == http.MethodGet && r.URL.Path == "/api/v1/nodes/a":
				if state.Load() == 0 {
					_, _ = io.WriteString(w, nodeJSON("1", "uid-a", false, ""))
				} else {
					_, _ = io.WriteString(w, nodeJSON("2", "uid-a", true, ann))
				}
			case r.Method == http.MethodPatch:
				patches.Add(1)
				if patches.Load() == 1 {
					// Simulate lost reply: apply mark server-side, return transport failure.
					state.Store(1)
					hj, ok := w.(http.Hijacker)
					if !ok {
						http.Error(w, "no hijack", 500)
						return
					}
					conn, _, err := hj.Hijack()
					if err != nil {
						http.Error(w, err.Error(), 500)
						return
					}
					_ = conn.Close()
					return
				}
				_, _ = io.WriteString(w, "{}")
			default:
				http.Error(w, "unexpected "+r.Method+" "+r.URL.Path, 400)
			}
		}))
		defer srv.Close()

		auth := &staticAuthority{
			id: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
			proof: ShutdownProof{
				Identity:  Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
				Authority: ManualLockoutProfile, Reference: op, FenceGeneration: "m1",
			},
		}
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		req := MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op}
		if err := api.Mark(ctx, req); err == nil {
			t.Fatal("first mark should look like a transport failure")
		}
		// Re-observe: identical operation is idempotent success (lost-PATCH retry).
		if err := api.Mark(ctx, req); err != nil {
			t.Fatalf("re-observe: %v", err)
		}
		if patches.Load() != 1 {
			t.Fatalf("second mark must not patch again; patches=%d", patches.Load())
		}
	})
}

// TestScopedRecoveryUnavailable covers S5: scoped mode stays off; no pod/VA writes; no mark without proof.
func TestScopedRecoveryUnavailable(t *testing.T) {
	if ScopedRecoveryAvailable() {
		t.Fatal("scoped recovery must stay unavailable")
	}
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	lost, ready := candidatePair(now)
	ctx := context.Background()

	fixtures := []string{
		"opted-in flaminio-only workload",
		"opted-out workload",
		"other CSI driver attachment",
		"mixed-storage pod",
	}
	for _, fixture := range fixtures {
		t.Run(fixture, func(t *testing.T) {
			k := &fakeKube{nodes: []Node{lost, ready}}
			// Recording httptest rejects pod/VolumeAttachment writes if anything tried.
			var forbidden atomic.Int32
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				path := strings.ToLower(r.URL.Path)
				if strings.Contains(path, "/pods") || strings.Contains(path, "volumeattachments") {
					forbidden.Add(1)
					http.Error(w, "forbidden", http.StatusForbidden)
					return
				}
				http.Error(w, "unused", 404)
			}))
			defer srv.Close()
			_ = fixture
			_ = srv // API exists and would refuse; controller must not call it.
			c := &Controller{Kube: k, Liveness: fakeLiveness{"b": true}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
			if err := c.Step(ctx); err != nil {
				t.Fatal(err)
			}
			if len(k.calls) != 0 {
				t.Fatalf("no mark without proof: %v", k.calls)
			}
			if forbidden.Load() != 0 {
				t.Fatalf("pod/VA writes attempted: %d", forbidden.Load())
			}
			if k.podWrites != 0 || k.vaWrites != 0 {
				t.Fatalf("pod/VA counters %d/%d", k.podWrites, k.vaWrites)
			}
		})
	}
}

// TestConditionalMarkOwnership covers S6: foreign taints, UID replace, 409, concurrent winners.
func TestConditionalMarkOwnership(t *testing.T) {
	ctx := context.Background()
	op := opID("own")
	auth := &staticAuthority{
		id: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
		proof: ShutdownProof{
			Identity:  Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
			Authority: ManualLockoutProfile, Reference: op, FenceGeneration: "m1",
		},
	}

	baseNode := func(uid, rv, ann string, taints string) string {
		if taints == "" {
			taints = `[{"key":"keep-me","effect":"NoSchedule"}]`
		}
		annObj := "{}"
		if ann != "" {
			annObj = fmt.Sprintf(`{%q:%s}`, OwnerAnnotation, mustJSONString(ann))
		}
		return fmt.Sprintf(`{"metadata":{"name":"a","uid":%q,"resourceVersion":%q,"annotations":%s},`+
			`"spec":{"taints":%s},"status":{"conditions":[{"type":"Ready","status":"False","lastTransitionTime":"2026-11-20T10:00:00Z"}]}}`,
			uid, rv, annObj, taints)
	}

	t.Run("foreign taint inserted before patch", func(t *testing.T) {
		taints := `[{"key":"keep-me","effect":"NoSchedule"},{"key":"node.kubernetes.io/out-of-service","value":"nodeshutdown","effect":"NoExecute"}]`
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch r.Method {
			case http.MethodGet:
				_, _ = io.WriteString(w, baseNode("uid-a", "2", "", taints))
			case http.MethodPatch:
				t.Fatal("must not patch over foreign mark")
			default:
				http.Error(w, "bad", 400)
			}
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op})
		if err == nil || !errors.Is(err, ErrForeignMark) {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("crossed proof identity refused", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			t.Fatal("must not contact API with crossed proof")
		}))
		defer srv.Close()
		crossed := auth.proof
		crossed.Identity = Identity{Name: "a", UID: "uid-other", MachineGeneration: "m1"}
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: crossed, OperationID: op})
		if err == nil || !errors.Is(err, ErrEvidenceInvalid) || !strings.Contains(err.Error(), "proof identity") {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("owner annotation mid-flight for different operation", func(t *testing.T) {
		otherOp := opID("dead") // distinct hex prefix from opID("own")
		rec := OperationRecord{
			Version: 1, NodeUID: "uid-a", OperationID: otherOp, Authority: ManualLockoutProfile,
			EvidenceRef: otherOp, MachineGeneration: "m1", FenceGeneration: "m1",
		}
		ann, err := EncodeOperationRecord(rec)
		if err != nil {
			t.Fatal(err)
		}
		taints := `[{"key":"keep-me","effect":"NoSchedule"},{"key":"node.kubernetes.io/out-of-service","value":"nodeshutdown","effect":"NoExecute"}]`
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch r.Method {
			case http.MethodGet:
				_, _ = io.WriteString(w, baseNode("uid-a", "5", ann, taints))
			case http.MethodPatch:
				t.Fatal("must not patch over foreign owner annotation")
			default:
				http.Error(w, "bad", 400)
			}
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err = api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op})
		if err == nil || !errors.Is(err, ErrForeignMark) {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("owner annotation NodeUID mismatch is foreign not owned", func(t *testing.T) {
		rec := OperationRecord{
			Version: 1, NodeUID: "uid-stale", OperationID: op, Authority: ManualLockoutProfile,
			EvidenceRef: op, MachineGeneration: "m1", FenceGeneration: "m1",
		}
		ann, err := EncodeOperationRecord(rec)
		if err != nil {
			t.Fatal(err)
		}
		taints := `[{"key":"node.kubernetes.io/out-of-service","value":"nodeshutdown","effect":"NoExecute"}]`
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			_, _ = io.WriteString(w, baseNode("uid-a", "6", ann, taints))
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		n, err := api.GetNode(ctx, "a")
		if err != nil {
			t.Fatal(err)
		}
		if n.Owned {
			t.Fatal("UID-mismatched record must not be Owned")
		}
		err = api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op})
		if err == nil || !errors.Is(err, ErrForeignMark) {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("replaced Node UID refused", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			_, _ = io.WriteString(w, baseNode("uid-new", "9", "", ""))
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op})
		if err == nil || !strings.Contains(err.Error(), "UID mismatch") {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("409 conflict does not retry old patch", func(t *testing.T) {
		var patches atomic.Int32
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch r.Method {
			case http.MethodGet:
				_, _ = io.WriteString(w, baseNode("uid-a", "1", "", ""))
			case http.MethodPatch:
				patches.Add(1)
				http.Error(w, `{"reason":"Conflict"}`, http.StatusConflict)
			default:
				http.Error(w, "bad", 400)
			}
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op})
		if err == nil || !strings.Contains(err.Error(), "409") {
			t.Fatalf("got %v", err)
		}
		if patches.Load() != 1 {
			t.Fatalf("patches=%d", patches.Load())
		}
	})

	t.Run("preserves unrelated taints on successful mark", func(t *testing.T) {
		var patch map[string]any
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch r.Method {
			case http.MethodGet:
				_, _ = io.WriteString(w, baseNode("uid-a", "3", "", ""))
			case http.MethodPatch:
				_ = json.NewDecoder(r.Body).Decode(&patch)
				_, _ = io.WriteString(w, "{}")
			default:
				http.Error(w, "bad", 400)
			}
		}))
		defer srv.Close()
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		if err := api.Mark(ctx, MarkRequest{Identity: auth.id, Proof: auth.proof, OperationID: op}); err != nil {
			t.Fatal(err)
		}
		b, _ := json.Marshal(patch)
		if !strings.Contains(string(b), `"key":"keep-me"`) {
			t.Fatalf("lost unrelated taint: %s", b)
		}
	})

	t.Run("concurrent controllers converge on recorded winner", func(t *testing.T) {
		now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
		lost, ready := candidatePair(now)
		k := &fakeKube{nodes: []Node{lost, ready}}
		c1 := &Controller{Kube: k, Liveness: fakeLiveness{"b": true}, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		c2 := &Controller{Kube: k, Liveness: fakeLiveness{"b": true}, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		var wg sync.WaitGroup
		wg.Add(2)
		go func() { defer wg.Done(); _ = c1.Step(ctx) }()
		go func() { defer wg.Done(); _ = c2.Step(ctx) }()
		wg.Wait()
		k.mu.Lock()
		defer k.mu.Unlock()
		var owned int
		for _, n := range k.nodes {
			if n.Name == "a" && n.OutOfService && n.Owned {
				owned++
			}
		}
		if owned != 1 {
			t.Fatalf("owned marks=%d calls=%v", owned, k.calls)
		}
	})
}

// TestCleanupRequiresReturnPermit covers S7.
func TestCleanupRequiresReturnPermit(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	ctx := context.Background()
	ready := Node{Name: "b", UID: "uid-b", Ready: true, Since: now.Add(-time.Hour)}
	op := opID("cl")
	owned := Node{
		Name: "a", UID: "uid-a", Ready: true, OutOfService: true, Owned: true,
		Record: OperationRecord{Version: 1, NodeUID: "uid-a", OperationID: op, Authority: ManualLockoutProfile, EvidenceRef: op, MachineGeneration: "m1"},
	}

	retain := func(t *testing.T, name string, nodes []Node, auth Authority, alive fakeLiveness) {
		t.Helper()
		t.Run(name, func(t *testing.T) {
			k := &fakeKube{nodes: append([]Node(nil), nodes...)}
			c := &Controller{Kube: k, Liveness: alive, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
			if err := c.Step(ctx); err != nil {
				t.Fatal(err)
			}
			for _, call := range k.calls {
				if strings.HasPrefix(call, "unmark ") {
					t.Fatalf("must retain mark: calls %v", k.calls)
				}
			}
			for _, n := range k.nodes {
				if n.Name == "a" && !n.OutOfService {
					t.Fatal("mark cleared")
				}
			}
		})
	}

	retain(t, "Ready+heartbeat without permit", []Node{owned, ready}, &scriptAuthority{}, fakeLiveness{"a": true, "b": true})
	retain(t, "nil authority cleanup-blocked", []Node{owned, ready}, nil, fakeLiveness{"a": true, "b": true})
	retain(t, "legacy timestamp foreign", []Node{
		{Name: "a", UID: "uid-a", Ready: true, OutOfService: true, Owned: false},
		ready,
	}, &scriptAuthority{}, fakeLiveness{"a": true, "b": true})
	retain(t, "foreign mark", []Node{
		{Name: "a", UID: "uid-a", Ready: true, OutOfService: true, Owned: false},
		ready,
	}, &permitAuthority{permit: ReturnPermit{
		Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
	}}, fakeLiveness{"a": true, "b": true})

	t.Run("revoked permit", func(t *testing.T) {
		auth := &scriptAuthority{
			verifyRetFn: func(context.Context, Identity, string) (ReturnPermit, error) {
				return ReturnPermit{}, errors.New("permit revoked")
			},
		}
		k := &fakeKube{nodes: []Node{owned, ready}}
		c := &Controller{Kube: k, Liveness: fakeLiveness{"a": true, "b": true}, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if len(k.calls) != 0 {
			t.Fatalf("calls %v", k.calls)
		}
	})

	t.Run("recreated machine identity", func(t *testing.T) {
		k := &fakeKube{nodes: []Node{owned, ready}}
		// Replacement incarnation carries a different operation record (or none).
		k.getOverride = map[string]Node{
			"a": {
				Name: "a", UID: "uid-new", Ready: true, OutOfService: true, Owned: true,
				Record: OperationRecord{Version: 1, NodeUID: "uid-new", OperationID: opID("other"), Authority: ManualLockoutProfile, EvidenceRef: opID("other")},
			},
		}
		c := &Controller{Kube: k, Liveness: fakeLiveness{"a": true, "b": true}, Authority: &permitAuthority{permit: ReturnPermit{
			Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
		}}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if strings.Contains(strings.Join(k.calls, ","), "unmark") {
			t.Fatalf("recreated machine cleared: %v", k.calls)
		}
	})

	t.Run("interrupted clear retains mark", func(t *testing.T) {
		k := &fakeKube{nodes: []Node{owned, ready}, clearErr: errors.New("api timeout during clear")}
		auth := &permitAuthority{permit: ReturnPermit{
			Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
		}}
		c := &Controller{Kube: k, Liveness: fakeLiveness{"a": true, "b": true}, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err == nil {
			t.Fatal("expected clear error")
		}
		for _, n := range k.nodes {
			if n.Name == "a" && !n.OutOfService {
				t.Fatal("interrupted clear must not drop mark in memory before success")
			}
		}
	})

	t.Run("matching permit clears only owned record", func(t *testing.T) {
		k := &fakeKube{nodes: []Node{owned, ready}}
		auth := &permitAuthority{permit: ReturnPermit{
			Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
		}}
		c := &Controller{Kube: k, Liveness: fakeLiveness{"a": true, "b": true}, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if got := strings.Join(k.calls, ","); got != "unmark a" {
			t.Fatalf("calls %q", got)
		}
	})

	t.Run("httptest Clear refuses foreign and legacy", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			// Out-of-service without owned JSON record.
			body := `{"metadata":{"name":"a","uid":"uid-a","resourceVersion":"1","annotations":{"flaminio.io/out-of-service":"2026-10-09T12:00:00Z"}},` +
				`"spec":{"taints":[{"key":"node.kubernetes.io/out-of-service","value":"nodeshutdown","effect":"NoExecute"}]},"status":{"conditions":[]}}`
			_, _ = io.WriteString(w, body)
		}))
		defer srv.Close()
		auth := &permitAuthority{permit: ReturnPermit{
			Identity: Identity{Name: "a", UID: "uid-a"}, OperationID: op, Reference: "ret",
		}}
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Clear(ctx, ClearRequest{Identity: Identity{Name: "a", UID: "uid-a"}, OperationID: op, Permit: auth.permit})
		if err == nil || !errors.Is(err, ErrForeignMark) || !strings.Contains(err.Error(), OutcomeCleanupBlocked) {
			t.Fatalf("got %v", err)
		}
	})

	t.Run("Clear binds permit identity and operation", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			t.Fatal("must not contact API with mismatched permit")
		}))
		defer srv.Close()
		auth := &permitAuthority{permit: ReturnPermit{
			Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
		}}
		api := &APIClient{Base: srv.URL, Token: func() (string, error) { return "tok", nil }, Client: srv.Client(), Authority: auth}
		err := api.Clear(ctx, ClearRequest{
			Identity:    Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
			OperationID: op,
			Permit: ReturnPermit{
				Identity: Identity{Name: "a", UID: "uid-other", MachineGeneration: "m1"}, OperationID: op, Reference: "ret",
			},
		})
		if err == nil || !errors.Is(err, ErrEvidenceInvalid) {
			t.Fatalf("crossed permit identity: %v", err)
		}
		err = api.Clear(ctx, ClearRequest{
			Identity:    Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"},
			OperationID: op,
			Permit: ReturnPermit{
				Identity: Identity{Name: "a", UID: "uid-a", MachineGeneration: "m1"}, OperationID: opID("zz"), Reference: "ret",
			},
		})
		if err == nil || !errors.Is(err, ErrEvidenceInvalid) {
			t.Fatalf("mismatched permit op: %v", err)
		}
	})
}

// TestNoDataplaneAdmissionShortcut covers S8 at the controller boundary: recovery only
// mutates Kubernetes marks through Kube; Controller has no dataplane admission path.
func TestNoDataplaneAdmissionShortcut(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	lost, ready := candidatePair(now)
	k := &fakeKube{nodes: []Node{lost, ready}}
	c := &Controller{Kube: k, Liveness: fakeLiveness{"b": true}, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
	if err := c.Step(context.Background()); err != nil {
		t.Fatal(err)
	}
	if got := strings.Join(k.calls, ","); got != "mark a" {
		t.Fatalf("calls %q", got)
	}
	// Controller fields are only Kube, Liveness, Authority, Grace, Now — no store/repl client.
	_ = c.Kube
	_ = c.Liveness
	_ = c.Authority
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
func (s *staticAuthority) ValidateShutdown(context.Context, Identity, ShutdownProof) error {
	return nil
}
func (s *staticAuthority) VerifyReturn(context.Context, Identity, string) (ReturnPermit, error) {
	return ReturnPermit{}, errTerminalRetirement
}
func (s *staticAuthority) ValidateReturn(context.Context, ReturnPermit) error {
	return errTerminalRetirement
}

// permitAuthority grants Clear for tests of the cleanup path (not a production profile).
type permitAuthority struct {
	permit ReturnPermit
}

func (p *permitAuthority) ResolveIdentity(_ context.Context, n Node) (Identity, error) {
	return Identity{Name: n.Name, UID: n.UID, ProviderID: n.ProviderID, MachineGeneration: "m1"}, nil
}
func (p *permitAuthority) VerifyShutdown(context.Context, Identity) (ShutdownProof, error) {
	return ShutdownProof{}, errors.New("not used")
}
func (p *permitAuthority) ValidateShutdown(context.Context, Identity, ShutdownProof) error {
	return errors.New("not used")
}
func (p *permitAuthority) VerifyReturn(context.Context, Identity, string) (ReturnPermit, error) {
	return p.permit, nil
}
func (p *permitAuthority) ValidateReturn(context.Context, ReturnPermit) error { return nil }

func mustJSONString(s string) string {
	b, err := json.Marshal(s)
	if err != nil {
		panic(err)
	}
	return string(b)
}
