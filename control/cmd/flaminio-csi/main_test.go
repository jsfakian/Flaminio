package main

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"

	"github.com/flaminio/flaminio/control/failover"
	"go.uber.org/goleak"
)

func TestMain(m *testing.M) { goleak.VerifyTestMain(m) }

// TestFailoverDefaultAuthority covers S2/S5/S9 startup defaults: observe-only when unset,
// unsupported profiles rejected, scoped recovery unavailable.
func TestFailoverDefaultAuthority(t *testing.T) {
	if failover.ScopedRecoveryAvailable() {
		t.Fatal("scoped partition recovery must stay unavailable")
	}

	// Empty --failover-authority-config is observe-only: Controller with nil Authority
	// never marks (see control/failover tests). Here we assert config parsing rejects
	// anything other than manual-lockout-v1 before the process would start mutating.
	pub := make([]byte, 32)
	pub[0] = 1
	cfg := map[string]any{
		"version":     1,
		"profile":     "cloud-power-sample-v1",
		"cluster_uid": "cluster-1",
		"proof_dir":   "/var/lib/flaminio/proofs",
		"keys": []map[string]string{{
			"key_id":     "ops1",
			"public_key": base64.StdEncoding.EncodeToString(pub),
		}},
		"inventory": []map[string]string{{
			"node_name":  "fl-a",
			"node_uid":   "uid-a",
			"machine_id": "machine-a",
		}},
	}
	raw, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	_, err = failover.ParseManualLockoutConfig(raw)
	if err == nil || !strings.Contains(err.Error(), "unsupported failover authority profile") {
		t.Fatalf("unsupported profile must be fatal: %v", err)
	}

	cfg["profile"] = failover.ManualLockoutProfile
	raw, err = json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := failover.ParseManualLockoutConfig(raw)
	if err != nil {
		t.Fatal(err)
	}
	if parsed.Profile != failover.ManualLockoutProfile {
		t.Fatalf("%+v", parsed)
	}

	// run() without a mode fails closed.
	if err := run(nil); err == nil || !strings.Contains(err.Error(), "usage:") {
		t.Fatalf("got %v", err)
	}
	if err := run([]string{"failover"}); err == nil || !strings.Contains(err.Error(), "--meta") {
		t.Fatalf("failover without PKI/meta must fail before authority work: %v", err)
	}
}
