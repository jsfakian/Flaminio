package failover

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"syscall"
	"unicode"
)

const (
	ManualLockoutProfile   = "manual-lockout-v1"
	ManualLockoutAssertion = "physically-isolated-until-node-uid-retired-v1"
	ManualLockoutDomain    = "flaminio-manual-lockout-v1\n"

	maxConfigBytes   = 16 << 10
	maxProofBytes    = 8 << 10
	maxInventoryRows = 4096
	maxKeys          = 8
	maxIDLen         = 128
	maxKeyIDLen      = 64
)

var (
	errTerminalRetirement = errors.New("terminal-retirement-required")
	keyIDRe               = regexp.MustCompile(`^[A-Za-z0-9_-]{1,64}$`)
	hex64Re               = regexp.MustCompile(`^[0-9a-f]{64}$`)
	// Kubernetes DNS subdomain / label grammar for node names (simplified).
	nodeNameRe = regexp.MustCompile(`^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$`)
)

// ManualLockoutKey is one trusted Ed25519 public key in a config.
type ManualLockoutKey struct {
	KeyID     string `json:"key_id"`
	PublicKey string `json:"public_key"` // canonical base64 of 32 raw bytes
}

// ManualLockoutInventory maps a node name+UID to one immutable machine id.
type ManualLockoutInventory struct {
	NodeName  string `json:"node_name"`
	NodeUID   string `json:"node_uid"`
	MachineID string `json:"machine_id"`
}

// ManualLockoutConfig is config v1 for the manual-lockout-v1 authority.
type ManualLockoutConfig struct {
	Version    int                      `json:"version"`
	Profile    string                   `json:"profile"`
	ClusterUID string                   `json:"cluster_uid"`
	ProofDir   string                   `json:"proof_dir"`
	Keys       []ManualLockoutKey       `json:"keys"`
	Inventory  []ManualLockoutInventory `json:"inventory"`
}

// ManualLockoutProof is the signed on-disk proof file.
type ManualLockoutProof struct {
	Version     int    `json:"version"`
	Profile     string `json:"profile"`
	ClusterUID  string `json:"cluster_uid"`
	NodeName    string `json:"node_name"`
	NodeUID     string `json:"node_uid"`
	MachineID   string `json:"machine_id"`
	OperationID string `json:"operation_id"`
	KeyID       string `json:"key_id"`
	Assertion   string `json:"assertion"`
	LockedAt    string `json:"locked_at"`
	Signature   string `json:"signature"`
}

// ManualLockout is the first production Authority. VerifyReturn/Clear always refuse.
type ManualLockout struct {
	cfg        ManualLockoutConfig
	clusterUID string
	pubs       map[string]ed25519.PublicKey
	inv        map[string]ManualLockoutInventory // key: name\0uid
}

// LoadAuthorityConfig loads a failover authority config. Only manual-lockout-v1 is accepted;
// any other profile is a fatal configuration error.
func LoadAuthorityConfig(path string) (ManualLockoutConfig, error) {
	raw, err := readTrustedFile(path, maxConfigBytes)
	if err != nil {
		return ManualLockoutConfig{}, err
	}
	cfg, err := ParseManualLockoutConfig(raw)
	if err != nil {
		return ManualLockoutConfig{}, err
	}
	return cfg, nil
}

// ParseManualLockoutConfig parses and bounds-checks config JSON (no file ownership checks).
func ParseManualLockoutConfig(raw []byte) (ManualLockoutConfig, error) {
	if len(raw) > maxConfigBytes {
		return ManualLockoutConfig{}, fmt.Errorf("config exceeds %d bytes", maxConfigBytes)
	}
	if err := rejectDuplicateJSONKeys(raw); err != nil {
		return ManualLockoutConfig{}, fmt.Errorf("config json: %w", err)
	}
	var cfg ManualLockoutConfig
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&cfg); err != nil {
		return ManualLockoutConfig{}, fmt.Errorf("config json: %w", err)
	}
	if err := dec.Decode(&struct{}{}); err != io.EOF {
		return ManualLockoutConfig{}, fmt.Errorf("config json: trailing data")
	}
	if cfg.Version != 1 {
		return ManualLockoutConfig{}, fmt.Errorf("unsupported config version %d", cfg.Version)
	}
	if cfg.Profile != ManualLockoutProfile {
		return ManualLockoutConfig{}, fmt.Errorf("unsupported failover authority profile %q", cfg.Profile)
	}
	if cfg.ClusterUID == "" || !printableASCII(cfg.ClusterUID, 1, maxIDLen) {
		return ManualLockoutConfig{}, fmt.Errorf("invalid cluster_uid")
	}
	if cfg.ProofDir == "" || !filepath.IsAbs(cfg.ProofDir) {
		return ManualLockoutConfig{}, fmt.Errorf("proof_dir must be an absolute path")
	}
	cfg.ProofDir = filepath.Clean(cfg.ProofDir)
	if len(cfg.Keys) == 0 || len(cfg.Keys) > maxKeys {
		return ManualLockoutConfig{}, fmt.Errorf("keys: need 1..%d distinct key ids", maxKeys)
	}
	seenKey := map[string]struct{}{}
	for _, k := range cfg.Keys {
		if !keyIDRe.MatchString(k.KeyID) {
			return ManualLockoutConfig{}, fmt.Errorf("invalid key_id %q", k.KeyID)
		}
		if _, dup := seenKey[k.KeyID]; dup {
			return ManualLockoutConfig{}, fmt.Errorf("duplicate key_id %q", k.KeyID)
		}
		seenKey[k.KeyID] = struct{}{}
		if _, err := decodeEd25519Pub(k.PublicKey); err != nil {
			return ManualLockoutConfig{}, fmt.Errorf("key %s: %w", k.KeyID, err)
		}
	}
	if len(cfg.Inventory) == 0 || len(cfg.Inventory) > maxInventoryRows {
		return ManualLockoutConfig{}, fmt.Errorf("inventory: need 1..%d rows", maxInventoryRows)
	}
	seenNameUID := map[string]struct{}{}
	seenMachine := map[string]string{} // machine_id -> name\0uid
	for _, row := range cfg.Inventory {
		if !nodeNameRe.MatchString(row.NodeName) || len(row.NodeName) > 253 {
			return ManualLockoutConfig{}, fmt.Errorf("invalid inventory node_name %q", row.NodeName)
		}
		if !printableASCII(row.NodeUID, 1, maxIDLen) {
			return ManualLockoutConfig{}, fmt.Errorf("invalid inventory node_uid")
		}
		if !printableASCII(row.MachineID, 1, maxIDLen) {
			return ManualLockoutConfig{}, fmt.Errorf("invalid inventory machine_id")
		}
		k := row.NodeName + "\x00" + row.NodeUID
		if _, dup := seenNameUID[k]; dup {
			return ManualLockoutConfig{}, fmt.Errorf("duplicate inventory mapping for %s/%s", row.NodeName, row.NodeUID)
		}
		seenNameUID[k] = struct{}{}
		if prev, ok := seenMachine[row.MachineID]; ok && prev != k {
			return ManualLockoutConfig{}, fmt.Errorf("ambiguous inventory: machine_id %q maps to multiple nodes", row.MachineID)
		}
		seenMachine[row.MachineID] = k
	}
	return cfg, nil
}

// NewManualLockout builds an Authority bound to the live cluster UID.
func NewManualLockout(cfg ManualLockoutConfig, clusterUID string) (*ManualLockout, error) {
	if cfg.Profile != ManualLockoutProfile {
		return nil, fmt.Errorf("unsupported profile %q", cfg.Profile)
	}
	if clusterUID == "" || cfg.ClusterUID != clusterUID {
		return nil, fmt.Errorf("cluster_uid mismatch: config %q live %q", cfg.ClusterUID, clusterUID)
	}
	pubs := make(map[string]ed25519.PublicKey, len(cfg.Keys))
	for _, k := range cfg.Keys {
		pub, err := decodeEd25519Pub(k.PublicKey)
		if err != nil {
			return nil, err
		}
		pubs[k.KeyID] = pub
	}
	inv := make(map[string]ManualLockoutInventory, len(cfg.Inventory))
	for _, row := range cfg.Inventory {
		inv[row.NodeName+"\x00"+row.NodeUID] = row
	}
	return &ManualLockout{cfg: cfg, clusterUID: clusterUID, pubs: pubs, inv: inv}, nil
}

func (m *ManualLockout) ResolveIdentity(_ context.Context, n Node) (Identity, error) {
	if n.Name == "" || n.UID == "" {
		return Identity{}, fmt.Errorf("%w: node name/UID required", ErrEvidenceInvalid)
	}
	row, ok := m.inv[n.Name+"\x00"+n.UID]
	if !ok {
		return Identity{}, fmt.Errorf("%w: no inventory mapping for %s/%s", ErrEvidenceInvalid, n.Name, n.UID)
	}
	if n.ProviderID != "" && n.ProviderID != row.MachineID {
		// ProviderID, when populated, must match inventory machine_id.
		return Identity{}, fmt.Errorf("%w: providerID %q does not match inventory machine_id", ErrEvidenceInvalid, n.ProviderID)
	}
	return Identity{
		Name:              n.Name,
		UID:               n.UID,
		ProviderID:        n.ProviderID,
		MachineGeneration: row.MachineID,
	}, nil
}

func (m *ManualLockout) VerifyShutdown(ctx context.Context, id Identity) (ShutdownProof, error) {
	proof, err := m.loadProof(id.UID)
	if err != nil {
		return ShutdownProof{}, err
	}
	if err := m.checkProof(proof, id); err != nil {
		return ShutdownProof{}, err
	}
	_ = ctx
	return ShutdownProof{
		Identity:        id,
		Authority:       ManualLockoutProfile,
		Reference:       proof.OperationID,
		FenceGeneration: proof.MachineID,
	}, nil
}

func (m *ManualLockout) ValidateShutdown(_ context.Context, proof ShutdownProof) error {
	if proof.Authority != ManualLockoutProfile {
		return fmt.Errorf("%w: authority %q", ErrEvidenceInvalid, proof.Authority)
	}
	raw, err := m.loadProof(proof.UID)
	if err != nil {
		return err
	}
	return m.checkProof(raw, proof.Identity)
}

func (m *ManualLockout) VerifyReturn(context.Context, Identity, string) (ReturnPermit, error) {
	return ReturnPermit{}, fmt.Errorf("%w", errTerminalRetirement)
}

func (m *ManualLockout) ValidateReturn(context.Context, ReturnPermit) error {
	return fmt.Errorf("%w", errTerminalRetirement)
}

func (m *ManualLockout) loadProof(nodeUID string) (ManualLockoutProof, error) {
	sum := sha256.Sum256([]byte(nodeUID))
	path := filepath.Join(m.cfg.ProofDir, hex.EncodeToString(sum[:])+".json")
	// Bound lookup: only this exact path under the pinned proof_dir.
	if filepath.Dir(path) != filepath.Clean(m.cfg.ProofDir) {
		return ManualLockoutProof{}, fmt.Errorf("%w: proof path escapes proof_dir", ErrEvidenceInvalid)
	}
	raw, err := readTrustedFile(path, maxProofBytes)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return ManualLockoutProof{}, fmt.Errorf("%s: proof not found", OutcomeEvidenceUnavailable)
		}
		return ManualLockoutProof{}, fmt.Errorf("%s: %v", OutcomeEvidenceUnavailable, err)
	}
	return ParseManualLockoutProof(raw)
}

func (m *ManualLockout) checkProof(p ManualLockoutProof, id Identity) error {
	if p.Profile != ManualLockoutProfile || p.Version != 1 {
		return fmt.Errorf("%w: unsupported proof profile/version", ErrEvidenceInvalid)
	}
	if p.ClusterUID != m.clusterUID {
		return fmt.Errorf("%w: proof cluster_uid mismatch", ErrEvidenceInvalid)
	}
	if p.NodeName != id.Name || p.NodeUID != id.UID {
		return fmt.Errorf("%w: proof node identity mismatch", ErrEvidenceInvalid)
	}
	if p.MachineID != id.MachineGeneration {
		return fmt.Errorf("%w: proof machine_id mismatch", ErrEvidenceInvalid)
	}
	if p.Assertion != ManualLockoutAssertion {
		return fmt.Errorf("%w: unexpected assertion", ErrEvidenceInvalid)
	}
	if !hex64Re.MatchString(p.OperationID) {
		return fmt.Errorf("%w: invalid operation_id", ErrEvidenceInvalid)
	}
	pub, ok := m.pubs[p.KeyID]
	if !ok {
		return fmt.Errorf("%w: unknown key_id %q", ErrEvidenceInvalid, p.KeyID)
	}
	sig, err := decodeEd25519Sig(p.Signature)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrEvidenceInvalid, err)
	}
	msg, err := ManualLockoutSignBytes(p)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrEvidenceInvalid, err)
	}
	if !ed25519.Verify(pub, msg, sig) {
		return fmt.Errorf("%w: signature verification failed", ErrEvidenceInvalid)
	}
	// Inventory must still agree.
	row, ok := m.inv[p.NodeName+"\x00"+p.NodeUID]
	if !ok || row.MachineID != p.MachineID {
		return fmt.Errorf("%w: proof not in inventory", ErrEvidenceInvalid)
	}
	return nil
}

// ParseManualLockoutProof parses a proof JSON object with unknown/duplicate field refusal.
func ParseManualLockoutProof(raw []byte) (ManualLockoutProof, error) {
	if len(raw) > maxProofBytes {
		return ManualLockoutProof{}, fmt.Errorf("%w: proof exceeds %d bytes", ErrEvidenceInvalid, maxProofBytes)
	}
	if err := rejectDuplicateJSONKeys(raw); err != nil {
		return ManualLockoutProof{}, fmt.Errorf("%w: proof json: %v", ErrEvidenceInvalid, err)
	}
	var p ManualLockoutProof
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&p); err != nil {
		return ManualLockoutProof{}, fmt.Errorf("%w: proof json: %v", ErrEvidenceInvalid, err)
	}
	if err := dec.Decode(&struct{}{}); err != io.EOF {
		return ManualLockoutProof{}, fmt.Errorf("%w: proof json trailing data", ErrEvidenceInvalid)
	}
	if p.Version != 1 || p.Profile != ManualLockoutProfile {
		return ManualLockoutProof{}, fmt.Errorf("%w: unsupported proof version/profile", ErrEvidenceInvalid)
	}
	if !nodeNameRe.MatchString(p.NodeName) || !printableASCII(p.NodeUID, 1, maxIDLen) ||
		!printableASCII(p.MachineID, 1, maxIDLen) || !printableASCII(p.ClusterUID, 1, maxIDLen) {
		return ManualLockoutProof{}, fmt.Errorf("%w: invalid proof identity fields", ErrEvidenceInvalid)
	}
	if !hex64Re.MatchString(p.OperationID) || !keyIDRe.MatchString(p.KeyID) {
		return ManualLockoutProof{}, fmt.Errorf("%w: invalid operation_id or key_id", ErrEvidenceInvalid)
	}
	if p.Assertion != ManualLockoutAssertion {
		return ManualLockoutProof{}, fmt.Errorf("%w: assertion must be %s", ErrEvidenceInvalid, ManualLockoutAssertion)
	}
	if p.LockedAt == "" {
		return ManualLockoutProof{}, fmt.Errorf("%w: locked_at required", ErrEvidenceInvalid)
	}
	return p, nil
}

// ManualLockoutSignBytes is the canonical message: domain || compact JSON array of fields
// in fixed order. Shared by signer and verifier.
func ManualLockoutSignBytes(p ManualLockoutProof) ([]byte, error) {
	arr := []any{
		p.Version,
		p.Profile,
		p.ClusterUID,
		p.NodeName,
		p.NodeUID,
		p.MachineID,
		p.OperationID,
		p.KeyID,
		p.Assertion,
		p.LockedAt,
	}
	body, err := json.Marshal(arr)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 0, len(ManualLockoutDomain)+len(body))
	out = append(out, ManualLockoutDomain...)
	out = append(out, body...)
	return out, nil
}

// SignManualLockoutProof signs a fully validated proof (no physical isolation check).
func SignManualLockoutProof(p ManualLockoutProof, priv ed25519.PrivateKey) (ManualLockoutProof, error) {
	if p.Assertion != ManualLockoutAssertion {
		return ManualLockoutProof{}, fmt.Errorf("assertion must be %s (pass --assert-physical-lockout)", ManualLockoutAssertion)
	}
	if p.Version != 1 {
		p.Version = 1
	}
	if p.Profile == "" {
		p.Profile = ManualLockoutProfile
	}
	msg, err := ManualLockoutSignBytes(p)
	if err != nil {
		return ManualLockoutProof{}, err
	}
	sig := ed25519.Sign(priv, msg)
	p.Signature = base64.StdEncoding.EncodeToString(sig)
	return p, nil
}

// WriteManualLockoutProof writes proof via exclusive temp, fsync, rename, and directory fsync.
func WriteManualLockoutProof(path string, p ManualLockoutProof) error {
	b, err := json.Marshal(p)
	if err != nil {
		return err
	}
	dir := filepath.Dir(path)
	tmp, err := os.CreateTemp(dir, ".flaminio-proof-*.tmp")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	cleanup := true
	defer func() {
		if cleanup {
			_ = os.Remove(tmpName)
		}
	}()
	if _, err := tmp.Write(b); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(tmpName, path); err != nil {
		return err
	}
	cleanup = false
	df, err := os.Open(dir)
	if err != nil {
		return err
	}
	defer df.Close()
	return df.Sync()
}

// LoadEd25519PKCS8 loads a root-only, no-follow Ed25519 PKCS#8 private key file.
func LoadEd25519PKCS8(path string) (ed25519.PrivateKey, error) {
	raw, err := readTrustedFile(path, 8<<10)
	if err != nil {
		return nil, err
	}
	block, _ := pem.Decode(raw)
	if block == nil {
		return nil, fmt.Errorf("no PEM block in signing key")
	}
	key, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return nil, fmt.Errorf("parse signing key: %w", err)
	}
	priv, ok := key.(ed25519.PrivateKey)
	if !ok {
		return nil, fmt.Errorf("signing key is not Ed25519")
	}
	return priv, nil
}

func readTrustedFile(path string, max int) ([]byte, error) {
	path = filepath.Clean(path)
	f, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !st.Mode().IsRegular() {
		return nil, fmt.Errorf("%s: not a regular file", path)
	}
	if st.Mode().Perm()&0o022 != 0 {
		return nil, fmt.Errorf("%s: group/world writable (mode %04o)", path, st.Mode().Perm())
	}
	if sys, ok := st.Sys().(*syscall.Stat_t); ok && sys.Uid != 0 {
		return nil, fmt.Errorf("%s: must be root-owned (uid %d)", path, sys.Uid)
	}
	if st.Size() > int64(max) {
		return nil, fmt.Errorf("%s: exceeds %d bytes", path, max)
	}
	b, err := io.ReadAll(io.LimitReader(f, int64(max)+1))
	if err != nil {
		return nil, err
	}
	if len(b) > max {
		return nil, fmt.Errorf("%s: exceeds %d bytes", path, max)
	}
	return b, nil
}

func decodeEd25519Pub(b64 string) (ed25519.PublicKey, error) {
	raw, err := base64.StdEncoding.DecodeString(b64)
	if err != nil {
		return nil, fmt.Errorf("public_key base64: %w", err)
	}
	if len(raw) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("public_key must be %d bytes", ed25519.PublicKeySize)
	}
	// Reject non-canonical encodings (padding/whitespace already handled by StdEncoding).
	if base64.StdEncoding.EncodeToString(raw) != b64 {
		return nil, fmt.Errorf("public_key is not canonical base64")
	}
	return ed25519.PublicKey(raw), nil
}

func decodeEd25519Sig(b64 string) ([]byte, error) {
	raw, err := base64.StdEncoding.DecodeString(b64)
	if err != nil {
		return nil, fmt.Errorf("signature base64: %w", err)
	}
	if len(raw) != ed25519.SignatureSize {
		return nil, fmt.Errorf("signature must be %d bytes", ed25519.SignatureSize)
	}
	if base64.StdEncoding.EncodeToString(raw) != b64 {
		return nil, fmt.Errorf("signature is not canonical base64")
	}
	return raw, nil
}

// rejectDuplicateJSONKeys walks object keys at every nesting level and refuses duplicates.
func rejectDuplicateJSONKeys(raw []byte) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	return rejectDupKeysToken(dec, false)
}

func rejectDupKeysToken(dec *json.Decoder, inArray bool) error {
	t, err := dec.Token()
	if err != nil {
		return err
	}
	switch delim := t.(type) {
	case json.Delim:
		switch delim {
		case '{':
			seen := map[string]struct{}{}
			for dec.More() {
				kt, err := dec.Token()
				if err != nil {
					return err
				}
				key, ok := kt.(string)
				if !ok {
					return fmt.Errorf("expected object key")
				}
				if _, dup := seen[key]; dup {
					return fmt.Errorf("duplicate field %q", key)
				}
				seen[key] = struct{}{}
				if err := rejectDupKeysToken(dec, false); err != nil {
					return err
				}
			}
			end, err := dec.Token()
			if err != nil {
				return err
			}
			if end != json.Delim('}') {
				return fmt.Errorf("expected end of object")
			}
		case '[':
			for dec.More() {
				if err := rejectDupKeysToken(dec, true); err != nil {
					return err
				}
			}
			end, err := dec.Token()
			if err != nil {
				return err
			}
			if end != json.Delim(']') {
				return fmt.Errorf("expected end of array")
			}
		default:
			return fmt.Errorf("unexpected delimiter %v", delim)
		}
	default:
		if inArray {
			return nil
		}
		// scalar value for an object field already consumed as the value token
		return nil
	}
	return nil
}

func printableASCII(s string, min, max int) bool {
	if len(s) < min || len(s) > max {
		return false
	}
	for _, r := range s {
		if r < 0x20 || r > 0x7e || !unicode.IsPrint(r) {
			return false
		}
	}
	return true
}

// ProofPathForUID returns proof_dir/SHA256(nodeUID).json.
func ProofPathForUID(proofDir, nodeUID string) string {
	sum := sha256.Sum256([]byte(nodeUID))
	return filepath.Join(proofDir, hex.EncodeToString(sum[:])+".json")
}

// ValidateFenceSignInputs checks fence-sign CLI fields against a loaded config.
func ValidateFenceSignInputs(cfg ManualLockoutConfig, nodeName, nodeUID, machineID, operationID, keyID, lockedAt string, assertLockout bool) error {
	if !assertLockout {
		return fmt.Errorf("--assert-physical-lockout is required; this command cannot verify physical isolation")
	}
	if !nodeNameRe.MatchString(nodeName) {
		return fmt.Errorf("invalid --node")
	}
	if !printableASCII(nodeUID, 1, maxIDLen) || !printableASCII(machineID, 1, maxIDLen) {
		return fmt.Errorf("invalid --node-uid or --machine-id")
	}
	if !hex64Re.MatchString(operationID) {
		return fmt.Errorf("--operation-id must be 64 lowercase hex characters")
	}
	if !keyIDRe.MatchString(keyID) {
		return fmt.Errorf("invalid --key-id")
	}
	if lockedAt == "" {
		return fmt.Errorf("--locked-at is required")
	}
	foundKey := false
	for _, k := range cfg.Keys {
		if k.KeyID == keyID {
			foundKey = true
			break
		}
	}
	if !foundKey {
		return fmt.Errorf("key_id %q not in config", keyID)
	}
	match := false
	for _, inv := range cfg.Inventory {
		if inv.NodeName == nodeName && inv.NodeUID == nodeUID {
			if inv.MachineID != machineID {
				return fmt.Errorf("machine_id does not match inventory")
			}
			match = true
			break
		}
	}
	if !match {
		return fmt.Errorf("node not in inventory")
	}
	return nil
}
