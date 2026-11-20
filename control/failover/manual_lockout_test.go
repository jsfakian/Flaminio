package failover

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func testKeyPair(t *testing.T) (ed25519.PublicKey, ed25519.PrivateKey) {
	t.Helper()
	pub, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	return pub, priv
}

func validConfigJSON(t *testing.T, pub ed25519.PublicKey, proofDir, clusterUID string) []byte {
	t.Helper()
	cfg := map[string]any{
		"version":     1,
		"profile":     ManualLockoutProfile,
		"cluster_uid": clusterUID,
		"proof_dir":   proofDir,
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
	b, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func signedProof(t *testing.T, priv ed25519.PrivateKey, clusterUID, op string) ManualLockoutProof {
	t.Helper()
	p := ManualLockoutProof{
		Version:     1,
		Profile:     ManualLockoutProfile,
		ClusterUID:  clusterUID,
		NodeName:    "fl-a",
		NodeUID:     "uid-a",
		MachineID:   "machine-a",
		OperationID: op,
		KeyID:       "ops1",
		Assertion:   ManualLockoutAssertion,
		LockedAt:    "2026-11-20T12:00:00Z",
	}
	signed, err := SignManualLockoutProof(p, priv)
	if err != nil {
		t.Fatal(err)
	}
	return signed
}

func TestParseManualLockoutConfigRefusals(t *testing.T) {
	pub, _ := testKeyPair(t)
	good := validConfigJSON(t, pub, "/var/lib/flaminio/proofs", "cluster-1")

	t.Run("ok", func(t *testing.T) {
		if _, err := ParseManualLockoutConfig(good); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("unsupported profile", func(t *testing.T) {
		raw := strings.Replace(string(good), ManualLockoutProfile, "aws-imds-v1", 1)
		_, err := ParseManualLockoutConfig([]byte(raw))
		if err == nil || !strings.Contains(err.Error(), "unsupported failover authority profile") {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("unknown field", func(t *testing.T) {
		_, err := ParseManualLockoutConfig([]byte(`{"version":1,"profile":"manual-lockout-v1","cluster_uid":"c","proof_dir":"/p","keys":[{"key_id":"ops1","public_key":"` + base64.StdEncoding.EncodeToString(pub) + `"}],"inventory":[{"node_name":"fl-a","node_uid":"u","machine_id":"m"}],"extra":1}`))
		if err == nil {
			t.Fatal("expected unknown field refusal")
		}
	})
	t.Run("duplicate key", func(t *testing.T) {
		_, err := ParseManualLockoutConfig([]byte(`{"version":1,"version":1}`))
		if err == nil || !strings.Contains(err.Error(), "duplicate") {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("ambiguous inventory", func(t *testing.T) {
		raw := validConfigJSON(t, pub, "/p", "c")
		var cfg map[string]any
		_ = json.Unmarshal(raw, &cfg)
		cfg["inventory"] = []map[string]string{
			{"node_name": "fl-a", "node_uid": "uid-a", "machine_id": "shared"},
			{"node_name": "fl-b", "node_uid": "uid-b", "machine_id": "shared"},
		}
		cfg["proof_dir"] = "/abs/proofs"
		b, _ := json.Marshal(cfg)
		_, err := ParseManualLockoutConfig(b)
		if err == nil || !strings.Contains(err.Error(), "ambiguous") {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("oversize", func(t *testing.T) {
		big := make([]byte, maxConfigBytes+1)
		for i := range big {
			big[i] = 'a'
		}
		_, err := ParseManualLockoutConfig(big)
		if err == nil {
			t.Fatal("expected oversize refusal")
		}
	})
}

func TestManualLockoutSignVerify(t *testing.T) {
	pub, priv := testKeyPair(t)
	cluster := "cluster-1"
	op := opID("sig")
	cfg, err := ParseManualLockoutConfig(validConfigJSON(t, pub, "/var/lib/flaminio/proofs", cluster))
	if err != nil {
		t.Fatal(err)
	}
	m, err := NewManualLockout(cfg, cluster)
	if err != nil {
		t.Fatal(err)
	}
	id, err := m.ResolveIdentity(context.Background(), Node{Name: "fl-a", UID: "uid-a"})
	if err != nil {
		t.Fatal(err)
	}
	if id.MachineGeneration != "machine-a" {
		t.Fatalf("%+v", id)
	}

	proof := signedProof(t, priv, cluster, op)
	if err := m.checkProof(proof, id); err != nil {
		t.Fatal(err)
	}

	t.Run("wrong cluster", func(t *testing.T) {
		bad := proof
		bad.ClusterUID = "other"
		bad, err = SignManualLockoutProof(bad, priv)
		if err != nil {
			t.Fatal(err)
		}
		if err := m.checkProof(bad, id); err == nil || !errors.Is(err, ErrEvidenceInvalid) {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("wrong signature", func(t *testing.T) {
		bad := proof
		bad.Signature = base64.StdEncoding.EncodeToString(make([]byte, ed25519.SignatureSize))
		if err := m.checkProof(bad, id); err == nil {
			t.Fatal("expected signature failure")
		}
	})
	t.Run("wrong UID", func(t *testing.T) {
		badID := id
		badID.UID = "uid-other"
		if err := m.checkProof(proof, badID); err == nil {
			t.Fatal("expected UID mismatch")
		}
	})
	t.Run("wrong machine", func(t *testing.T) {
		badID := id
		badID.MachineGeneration = "other-machine"
		if err := m.checkProof(proof, badID); err == nil {
			t.Fatal("expected machine mismatch")
		}
	})
	t.Run("wrong profile in proof", func(t *testing.T) {
		raw, _ := json.Marshal(proof)
		s := strings.Replace(string(raw), ManualLockoutProfile, "other-profile", 1)
		_, err := ParseManualLockoutProof([]byte(s))
		if err == nil {
			t.Fatal("expected profile refusal")
		}
	})
	t.Run("providerID mismatch", func(t *testing.T) {
		_, err := m.ResolveIdentity(context.Background(), Node{Name: "fl-a", UID: "uid-a", ProviderID: "not-machine-a"})
		if err == nil || !errors.Is(err, ErrEvidenceInvalid) {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("return always refuses", func(t *testing.T) {
		_, err := m.VerifyReturn(context.Background(), id, op)
		if err == nil || !strings.Contains(err.Error(), "terminal-retirement-required") {
			t.Fatalf("got %v", err)
		}
		if err := m.ValidateReturn(context.Background(), ReturnPermit{}); err == nil {
			t.Fatal("expected refuse")
		}
	})
	t.Run("canonical sign bytes stable", func(t *testing.T) {
		a, err := ManualLockoutSignBytes(proof)
		if err != nil {
			t.Fatal(err)
		}
		b, err := ManualLockoutSignBytes(proof)
		if err != nil {
			t.Fatal(err)
		}
		if string(a) != string(b) {
			t.Fatal("canonicalization unstable")
		}
		if !strings.HasPrefix(string(a), ManualLockoutDomain) {
			t.Fatalf("missing domain: %q", a[:32])
		}
	})
}

func TestParseManualLockoutProofRefusals(t *testing.T) {
	_, priv := testKeyPair(t)
	proof := signedProof(t, priv, "c1", opID("pr"))
	raw, err := json.Marshal(proof)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ParseManualLockoutProof(raw); err != nil {
		t.Fatal(err)
	}
	t.Run("unknown field", func(t *testing.T) {
		var obj map[string]any
		_ = json.Unmarshal(raw, &obj)
		obj["extra"] = true
		b, _ := json.Marshal(obj)
		if _, err := ParseManualLockoutProof(b); err == nil {
			t.Fatal("expected refusal")
		}
	})
	t.Run("duplicate field", func(t *testing.T) {
		s := `{"version":1,"version":1,"profile":"manual-lockout-v1"}`
		if _, err := ParseManualLockoutProof([]byte(s)); err == nil {
			t.Fatal("expected refusal")
		}
	})
	t.Run("oversize", func(t *testing.T) {
		big := make([]byte, maxProofBytes+1)
		for i := range big {
			big[i] = 'x'
		}
		if _, err := ParseManualLockoutProof(big); err == nil {
			t.Fatal("expected refusal")
		}
	})
}

func TestReadTrustedFileRefusals(t *testing.T) {
	dir := t.TempDir()

	t.Run("group writable", func(t *testing.T) {
		p := filepath.Join(dir, "gw.json")
		if err := os.WriteFile(p, []byte(`{}`), 0o664); err != nil {
			t.Fatal(err)
		}
		_, err := readTrustedFile(p, 1024)
		if err == nil || !strings.Contains(err.Error(), "writable") {
			t.Fatalf("got %v", err)
		}
	})
	t.Run("symlink", func(t *testing.T) {
		target := filepath.Join(dir, "target.json")
		if err := os.WriteFile(target, []byte(`{}`), 0o600); err != nil {
			t.Fatal(err)
		}
		link := filepath.Join(dir, "link.json")
		if err := os.Symlink(target, link); err != nil {
			t.Fatal(err)
		}
		_, err := readTrustedFile(link, 1024)
		if err == nil {
			t.Fatal("expected symlink refusal")
		}
	})
	t.Run("non-root ownership", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("running as root; ownership refusal not observable")
		}
		p := filepath.Join(dir, "user.json")
		if err := os.WriteFile(p, []byte(`{}`), 0o600); err != nil {
			t.Fatal(err)
		}
		_, err := readTrustedFile(p, 1024)
		if err == nil || !strings.Contains(err.Error(), "root-owned") {
			t.Fatalf("got %v", err)
		}
	})
}

func TestValidateFenceSignInputs(t *testing.T) {
	pub, _ := testKeyPair(t)
	cfg, err := ParseManualLockoutConfig(validConfigJSON(t, pub, "/var/lib/flaminio/proofs", "c1"))
	if err != nil {
		t.Fatal(err)
	}
	err = ValidateFenceSignInputs(cfg, "fl-a", "uid-a", "machine-a", opID("fs"), "ops1", "2026-11-20T12:00:00Z", false)
	if err == nil || !strings.Contains(err.Error(), "--assert-physical-lockout") {
		t.Fatalf("got %v", err)
	}
	if err := ValidateFenceSignInputs(cfg, "fl-a", "uid-a", "machine-a", opID("fs"), "ops1", "2026-11-20T12:00:00Z", true); err != nil {
		t.Fatal(err)
	}
	if err := ValidateFenceSignInputs(cfg, "fl-a", "uid-a", "wrong", opID("fs"), "ops1", "2026-11-20T12:00:00Z", true); err == nil {
		t.Fatal("expected machine mismatch")
	}
}

func TestWriteManualLockoutProofRoundTrip(t *testing.T) {
	_, priv := testKeyPair(t)
	proof := signedProof(t, priv, "c1", opID("wr"))
	dir := t.TempDir()
	path := filepath.Join(dir, "proof.json")
	if err := WriteManualLockoutProof(path, proof); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	got, err := ParseManualLockoutProof(raw)
	if err != nil {
		t.Fatal(err)
	}
	if got.OperationID != proof.OperationID || got.Signature != proof.Signature {
		t.Fatalf("%+v", got)
	}
}

func TestLoadEd25519PKCS8(t *testing.T) {
	_, priv := testKeyPair(t)
	der, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		t.Fatal(err)
	}
	pemBytes := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: der})
	dir := t.TempDir()
	path := filepath.Join(dir, "key.pem")
	if err := os.WriteFile(path, pemBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if os.Geteuid() != 0 {
		_, err := LoadEd25519PKCS8(path)
		if err == nil || !strings.Contains(err.Error(), "root-owned") {
			t.Fatalf("non-root must refuse: %v", err)
		}
		return
	}
	got, err := LoadEd25519PKCS8(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, priv) {
		t.Fatal("key mismatch")
	}
}

func TestNewManualLockoutClusterMismatch(t *testing.T) {
	pub, _ := testKeyPair(t)
	cfg, err := ParseManualLockoutConfig(validConfigJSON(t, pub, "/var/lib/flaminio/proofs", "cluster-a"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := NewManualLockout(cfg, "cluster-b"); err == nil {
		t.Fatal("expected cluster mismatch")
	}
}
