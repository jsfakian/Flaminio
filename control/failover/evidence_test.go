package failover

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"sync"
	"testing"
	"time"
)

// scriptAuthority is a programmable test double; not an executable production option.
type scriptAuthority struct {
	resolveFn     func(context.Context, Node) (Identity, error)
	verifyFn      func(context.Context, Identity) (ShutdownProof, error)
	validateFn    func(context.Context, Identity, ShutdownProof) error
	verifyRetFn   func(context.Context, Identity, string) (ReturnPermit, error)
	validateRetFn func(context.Context, ReturnPermit) error
	validateN     int
}

func (s *scriptAuthority) ResolveIdentity(ctx context.Context, n Node) (Identity, error) {
	if s.resolveFn != nil {
		return s.resolveFn(ctx, n)
	}
	return Identity{Name: n.Name, UID: n.UID, ProviderID: n.ProviderID, MachineGeneration: "m1"}, nil
}

func (s *scriptAuthority) VerifyShutdown(ctx context.Context, id Identity) (ShutdownProof, error) {
	if s.verifyFn != nil {
		return s.verifyFn(ctx, id)
	}
	return ShutdownProof{Identity: id, Authority: ManualLockoutProfile, Reference: opID("aa"), FenceGeneration: id.MachineGeneration}, nil
}

func (s *scriptAuthority) ValidateShutdown(ctx context.Context, expected Identity, proof ShutdownProof) error {
	s.validateN++
	if err := identityFieldsMatch(expected, proof.Identity); err != nil {
		return fmt.Errorf("%w: proof identity does not match mark target: %v", ErrEvidenceInvalid, err)
	}
	if s.validateFn != nil {
		return s.validateFn(ctx, expected, proof)
	}
	return nil
}

func (s *scriptAuthority) VerifyReturn(ctx context.Context, id Identity, op string) (ReturnPermit, error) {
	if s.verifyRetFn != nil {
		return s.verifyRetFn(ctx, id, op)
	}
	return ReturnPermit{}, errTerminalRetirement
}

func (s *scriptAuthority) ValidateReturn(ctx context.Context, p ReturnPermit) error {
	if s.validateRetFn != nil {
		return s.validateRetFn(ctx, p)
	}
	return errTerminalRetirement
}

func opID(prefix string) string {
	// operation_id is 64 lowercase hex characters.
	const hex = "0123456789abcdef"
	var b strings.Builder
	for i := 0; i < 64; i++ {
		if i < len(prefix) {
			c := prefix[i]
			if (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') {
				b.WriteByte(c)
				continue
			}
		}
		b.WriteByte(hex[i%16])
	}
	return b.String()
}

func candidatePair(now time.Time) (lost, ready Node) {
	lost = Node{Name: "a", UID: "uid-a", Ready: false, Since: now.Add(-time.Minute)}
	ready = Node{Name: "b", UID: "uid-b", Ready: true, Since: now.Add(-time.Hour)}
	return lost, ready
}

// TestShutdownEvidenceRefusals covers S2/S3: absent provider, timeout/auth/error, unknown
// state, stale/revoked generation, wrong identity, and proof invalidated before patch.
func TestShutdownEvidenceRefusals(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	lost, ready := candidatePair(now)
	ctx := context.Background()
	alive := fakeLiveness{"b": true}

	type tc struct {
		name string
		auth Authority
		want string // substring of last failover-outcome reason, or outcome field
		out  string // expected outcome
	}
	cases := []tc{
		{
			name: "absent provider",
			auth: nil,
			out:  OutcomeEvidenceUnavailable,
			want: "no failover authority",
		},
		{
			name: "provider timeout",
			auth: &scriptAuthority{verifyFn: func(context.Context, Identity) (ShutdownProof, error) {
				return ShutdownProof{}, errors.New("authority timeout")
			}},
			out:  OutcomeEvidenceUnavailable,
			want: "timeout",
		},
		{
			name: "provider auth failure",
			auth: &scriptAuthority{verifyFn: func(context.Context, Identity) (ShutdownProof, error) {
				return ShutdownProof{}, errors.New("authority unauthorized")
			}},
			out:  OutcomeEvidenceUnavailable,
			want: "unauthorized",
		},
		{
			name: "unknown power state",
			auth: &scriptAuthority{verifyFn: func(context.Context, Identity) (ShutdownProof, error) {
				return ShutdownProof{}, errors.New("unknown power state")
			}},
			out:  OutcomeEvidenceUnavailable,
			want: "unknown power state",
		},
		{
			name: "stale/revoked generation",
			auth: &scriptAuthority{verifyFn: func(context.Context, Identity) (ShutdownProof, error) {
				return ShutdownProof{}, errors.Join(ErrEvidenceInvalid, errors.New("fence generation revoked"))
			}},
			out:  OutcomeEvidenceInvalid,
			want: "revoked",
		},
		{
			name: "wrong UID mapping",
			auth: &scriptAuthority{resolveFn: func(context.Context, Node) (Identity, error) {
				return Identity{}, errors.Join(ErrEvidenceInvalid, errors.New("no inventory mapping"))
			}},
			out:  OutcomeEvidenceInvalid,
			want: "inventory",
		},
		{
			name: "wrong machine generation on validate",
			auth: &scriptAuthority{validateFn: func(context.Context, Identity, ShutdownProof) error {
				return errors.Join(ErrEvidenceInvalid, errors.New("machine_id mismatch"))
			}},
			out:  OutcomeEvidenceInvalid,
			want: "machine_id",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			logs := captureLog(t)
			k := &fakeKube{nodes: []Node{lost, ready}}
			c := &Controller{Kube: k, Liveness: alive, Authority: tc.auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
			if err := c.Step(ctx); err != nil {
				t.Fatal(err)
			}
			if len(k.calls) != 0 {
				t.Fatalf("mutations %v; want none", k.calls)
			}
			got := lastOutcome(logs())
			if got["outcome"] != tc.out {
				t.Fatalf("outcome %v, want %s; logs=%v", got["outcome"], tc.out, logs())
			}
			if reason, _ := got["reason"].(string); !strings.Contains(reason, tc.want) {
				t.Fatalf("reason %q lacks %q", reason, tc.want)
			}
		})
	}

	t.Run("proof invalidated between observation and patch", func(t *testing.T) {
		auth := &scriptAuthority{
			validateFn: func(context.Context, Identity, ShutdownProof) error {
				// Controller validates once; Mark via fakeKube does not re-check. Fail Mark.
				return nil
			},
		}
		k := &gatingKube{fakeKube: fakeKube{nodes: []Node{lost, ready}}, markErr: errors.Join(ErrEvidenceInvalid, errors.New("proof revoked before patch"))}
		c := &Controller{Kube: k, Liveness: alive, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		logs := captureLog(t)
		err := c.Step(ctx)
		if err == nil {
			t.Fatal("expected mark error to surface")
		}
		if len(k.calls) != 0 {
			t.Fatalf("successful mutations %v", k.calls)
		}
		got := lastOutcome(logs())
		if got["outcome"] != OutcomeEvidenceInvalid {
			t.Fatalf("outcome %v", got["outcome"])
		}
	})

	t.Run("UID changed since listing", func(t *testing.T) {
		nodes := []Node{lost, ready}
		k := &fakeKube{nodes: nodes}
		k.getOverride = map[string]Node{
			"a": {Name: "a", UID: "uid-replaced", Ready: false, Since: lost.Since},
		}
		logs := captureLog(t)
		c := &Controller{Kube: k, Liveness: alive, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if len(k.calls) != 0 {
			t.Fatalf("mutations %v", k.calls)
		}
		got := lastOutcome(logs())
		if got["outcome"] != OutcomeEvidenceInvalid {
			t.Fatalf("outcome %v", got["outcome"])
		}
	})
}

// TestOutcomeLogs covers S9: required outcomes and no secret fields.
func TestOutcomeLogs(t *testing.T) {
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)
	lost, ready := candidatePair(now)
	ctx := context.Background()
	alive := fakeLiveness{"b": true}
	op := opID("op")

	t.Run("candidate and evidence-unavailable defaults", func(t *testing.T) {
		logs := captureLog(t)
		k := &fakeKube{nodes: []Node{lost, ready}}
		c := &Controller{Kube: k, Liveness: alive, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		recs := outcomeRecords(logs())
		if len(recs) < 2 {
			t.Fatalf("want candidate then evidence-unavailable, got %v", recs)
		}
		if recs[0]["outcome"] != OutcomeCandidate {
			t.Fatalf("first %v", recs[0])
		}
		if recs[len(recs)-1]["outcome"] != OutcomeEvidenceUnavailable {
			t.Fatalf("last %v", recs[len(recs)-1])
		}
		assertNoSecrets(t, recs)
	})

	t.Run("shutdown-verified and recovery-requested", func(t *testing.T) {
		logs := captureLog(t)
		auth := &scriptAuthority{}
		k := &fakeKube{nodes: []Node{lost, ready}}
		c := &Controller{Kube: k, Liveness: alive, Authority: auth, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		outs := outcomes(logs())
		if !containsAll(outs, OutcomeShutdownVerified, OutcomeRecoveryRequested) {
			t.Fatalf("outcomes %v", outs)
		}
		assertNoSecrets(t, outcomeRecords(logs()))
	})

	t.Run("cleanup-blocked without return permit", func(t *testing.T) {
		logs := captureLog(t)
		owned := Node{
			Name: "a", UID: "uid-a", Ready: true, OutOfService: true, Owned: true,
			Record: OperationRecord{Version: 1, NodeUID: "uid-a", OperationID: op, Authority: ManualLockoutProfile, EvidenceRef: op},
		}
		k := &fakeKube{nodes: []Node{owned, ready}}
		c := &Controller{Kube: k, Liveness: fakeLiveness{"a": true, "b": true}, Authority: &scriptAuthority{}, Grace: 40 * time.Second, Now: func() time.Time { return now }}
		if err := c.Step(ctx); err != nil {
			t.Fatal(err)
		}
		if !containsAll(outcomes(logs()), OutcomeCleanupBlocked) {
			t.Fatalf("outcomes %v", outcomes(logs()))
		}
		if len(k.calls) != 0 {
			t.Fatalf("clear must not run: %v", k.calls)
		}
		assertNoSecrets(t, outcomeRecords(logs()))
	})
}

func TestParseOperationRecordForeign(t *testing.T) {
	if _, ok := ParseOperationRecord("2026-10-09T12:00:00Z"); ok {
		t.Fatal("legacy timestamp must be foreign")
	}
	if _, ok := ParseOperationRecord(`{"version":2,"nodeUID":"u","operationID":"o","authority":"x"}`); ok {
		t.Fatal("version 2 must be refused")
	}
	rec, ok := ParseOperationRecord(`{"version":1,"nodeUID":"u","operationID":"o","authority":"manual-lockout-v1"}`)
	if !ok || rec.NodeUID != "u" {
		t.Fatalf("got %+v ok=%v", rec, ok)
	}
}

func TestOwnedRequiresLiveNodeUID(t *testing.T) {
	annMatching, err := EncodeOperationRecord(OperationRecord{
		Version: 1, NodeUID: "uid-a", OperationID: opID("ow"), Authority: ManualLockoutProfile,
		MachineGeneration: "machine-a",
	})
	if err != nil {
		t.Fatal(err)
	}
	annStaleUID, err := EncodeOperationRecord(OperationRecord{
		Version: 1, NodeUID: "uid-other", OperationID: opID("ow"), Authority: ManualLockoutProfile,
		MachineGeneration: "machine-a",
	})
	if err != nil {
		t.Fatal(err)
	}
	annMachineMismatch, err := EncodeOperationRecord(OperationRecord{
		Version: 1, NodeUID: "uid-a", OperationID: opID("ow"), Authority: ManualLockoutProfile,
		MachineGeneration: "machine-b",
	})
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name    string
		ann     string
		prov    string
		wantOwn bool
	}{
		{"matching UID", annMatching, "machine-a", true},
		{"matching UID empty provider", annMatching, "", true},
		{"stale record UID", annStaleUID, "machine-a", false},
		{"machine generation mismatch", annMachineMismatch, "machine-a", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			n := apiNode{}
			n.Metadata.Name = "a"
			n.Metadata.UID = "uid-a"
			n.Metadata.Annotations = map[string]string{OwnerAnnotation: tc.ann}
			n.Spec.ProviderID = tc.prov
			got := n.toNode()
			if got.Owned != tc.wantOwn {
				t.Fatalf("Owned=%v want %v record=%+v", got.Owned, tc.wantOwn, got.Record)
			}
		})
	}
}

func captureLog(t *testing.T) func() []map[string]any {
	t.Helper()
	var mu sync.Mutex
	var buf bytes.Buffer
	prev := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(lockedWriter{&mu, &buf}, nil)))
	t.Cleanup(func() { slog.SetDefault(prev) })
	return func() []map[string]any {
		mu.Lock()
		defer mu.Unlock()
		var out []map[string]any
		for _, line := range bytes.Split(bytes.TrimSpace(buf.Bytes()), []byte("\n")) {
			if len(line) == 0 {
				continue
			}
			var rec map[string]any
			if err := json.Unmarshal(line, &rec); err != nil {
				t.Fatalf("log line %q: %v", line, err)
			}
			out = append(out, rec)
		}
		return out
	}
}

type lockedWriter struct {
	mu *sync.Mutex
	w  io.Writer
}

func (l lockedWriter) Write(b []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.w.Write(b)
}

func outcomeRecords(all []map[string]any) []map[string]any {
	var out []map[string]any
	for _, r := range all {
		if r["msg"] == "failover outcome" {
			out = append(out, r)
		}
	}
	return out
}

func outcomes(all []map[string]any) []string {
	var out []string
	for _, r := range outcomeRecords(all) {
		if s, ok := r["outcome"].(string); ok {
			out = append(out, s)
		}
	}
	return out
}

func lastOutcome(all []map[string]any) map[string]any {
	recs := outcomeRecords(all)
	if len(recs) == 0 {
		return nil
	}
	return recs[len(recs)-1]
}

func containsAll(have []string, want ...string) bool {
	set := map[string]bool{}
	for _, h := range have {
		set[h] = true
	}
	for _, w := range want {
		if !set[w] {
			return false
		}
	}
	return true
}

func assertNoSecrets(t *testing.T, recs []map[string]any) {
	t.Helper()
	forbid := []string{"password", "token", "private_key", "signing-key", "Bearer", "credential"}
	for _, r := range recs {
		b, _ := json.Marshal(r)
		s := string(b)
		for _, f := range forbid {
			if strings.Contains(strings.ToLower(s), strings.ToLower(f)) {
				t.Fatalf("log record contains secret-like field %q: %s", f, s)
			}
		}
		for k := range r {
			lk := strings.ToLower(k)
			if strings.Contains(lk, "secret") || strings.Contains(lk, "password") || strings.Contains(lk, "token") {
				t.Fatalf("secret-like key %q in %v", k, r)
			}
		}
	}
}
