package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/json"
	"encoding/pem"
	"math/big"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
	"github.com/flaminio/flaminio/pkg/pki"
)

const td = "lab"

// writeIdentity is auth_test.go's (meta/service): a key and a certificate for id, signed by ca,
// and the CA bundle, in dir.
func writeIdentity(t *testing.T, ca *pki.CA, dir, name, id string, usage pki.Usage) pki.Files {
	t.Helper()
	key, csr, err := pki.NewKeyAndCSR()
	if err != nil {
		t.Fatal(err)
	}
	cert, err := ca.Sign(csr, pki.Request{ID: id, IPs: []net.IP{net.IPv4(127, 0, 0, 1)}, Validity: time.Hour, Usage: usage})
	if err != nil {
		t.Fatal(err)
	}
	f := pki.Files{Cert: filepath.Join(dir, name+".crt"), Key: filepath.Join(dir, name+".key"), CA: filepath.Join(dir, "ca.crt")}
	for path, b := range map[string][]byte{f.Cert: cert, f.Key: key, f.CA: ca.CertPEM()} {
		if err := os.WriteFile(path, b, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return f
}

// fixture is the metadata service on an in-memory store, behind mutual TLS on a localhost port,
// counting the calls that reach it.
type fixture struct {
	t     *testing.T
	dir   string
	ca    *pki.CA
	addr  string
	store kv.KV
	svc   *service.Service
	mu    sync.Mutex
	calls map[string]int
}

func newFixture(t *testing.T) *fixture {
	f := &fixture{t: t, dir: t.TempDir(), store: kv.NewMem(nil), calls: map[string]int{}}
	var err error
	if f.ca, err = pki.NewCA("test", time.Hour); err != nil {
		t.Fatal(err)
	}
	scfg, err := writeIdentity(t, f.ca, f.dir, "server", pki.NodeID(td, "fl-w", "meta"), pki.Server).ServerConfig()
	if err != nil {
		t.Fatal(err)
	}
	f.svc = service.New(f.store, service.Options{TrustDomain: td})
	count := func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		f.mu.Lock()
		f.calls[info.FullMethod[strings.LastIndex(info.FullMethod, "/")+1:]]++
		f.mu.Unlock()
		return handler(ctx, req)
	}
	srv := grpc.NewServer(grpc.Creds(credentials.NewTLS(scfg)), grpc.ChainUnaryInterceptor(count, f.svc.Authorize))
	metav1.RegisterMetadataServiceServer(srv, f.svc)
	lis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	go func() { _ = srv.Serve(lis) }()
	t.Cleanup(srv.Stop)
	f.addr = lis.Addr().String()
	// Every node heartbeats before any create; fl-s is registered and stopped.
	ctx := context.Background()
	for _, n := range []string{"fl-a", "fl-b", "fl-c", "fl-d", "fl-s"} {
		if _, err := f.svc.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: n, Address: n + ":7411"}}); err != nil {
			t.Fatal(err)
		}
		if n == "fl-s" {
			continue
		}
		if _, err := f.svc.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: n}); err != nil {
			t.Fatal(err)
		}
	}
	return f
}

// as is a client identity for id.
func (f *fixture) as(name, id string) pki.Files {
	return writeIdentity(f.t, f.ca, f.dir, name, id, pki.Client)
}

// tombstone writes a deleted volume's tombstone as DeleteVolume does.
func (f *fixture) tombstone(volume, record string) {
	f.t.Helper()
	if _, err := f.store.Txn(context.Background(), nil, []kv.Op{{Key: "/flaminio/v1/tombstones/" + volume, Value: []byte(record)}}); err != nil {
		f.t.Fatal(err)
	}
}

// total is the number of calls that reached the service.
func (f *fixture) total() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	n := 0
	for _, c := range f.calls {
		n += c
	}
	return n
}

func (f *fixture) count(method string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls[method]
}

// run runs the client as id (no certificate if id.Cert is empty), with args after the
// connection's flags.
func (f *fixture) run(id pki.Files, args ...string) (stdout, stderr string, err error) {
	full := []string{"--server", f.addr, "--ca", filepath.Join(f.dir, "ca.crt")}
	if id.Cert != "" {
		full = append(full, "--cert", id.Cert, "--key", id.Key)
	}
	var out, errOut bytes.Buffer
	err = client(append(full, args...), &out, &errOut)
	return out.String(), errOut.String(), err
}

// create creates a 1 MiB volume of one replica, as the service's own caller.
func (f *fixture) create(id string) error {
	_, err := f.svc.CreateVolume(context.Background(), &metav1.CreateVolumeRequest{Id: id, SizeBytes: 1 << 20, ChunkBytes: 1 << 20, Replicas: 1, MinIsr: 1})
	return err
}

type listing struct {
	Tombstones []struct {
		Volume  string   `json:"volume"`
		Nodes   []string `json:"nodes"`
		Pending []string `json:"pending"`
	} `json:"tombstones"`
}

func parse(t *testing.T, s string) listing {
	t.Helper()
	var l listing
	if err := json.Unmarshal([]byte(s), &l); err != nil {
		t.Fatalf("%q: %v", s, err)
	}
	return l
}

// tombstones lists one node's tombstones, or every registered node's, each once, by volume
// (docs/issues/14, S1).
func TestTombstones(t *testing.T) {
	f := newFixture(t)
	admin := f.as("admin", pki.AdminID(td, "ops"))
	f.tombstone("v2", `{"nodes":["fl-b","fl-c"],"confirmed":{"fl-b":true}}`)
	f.tombstone("v1", `{"nodes":["fl-a","fl-b"]}`)
	// Pending only for a node that is not registered: not listed, and still the id's.
	f.tombstone("v3", `{"nodes":["fl-x"]}`)

	out, _, err := f.run(admin, "tombstones")
	if err != nil {
		t.Fatal(err)
	}
	l := parse(t, out)
	if len(l.Tombstones) != 2 ||
		l.Tombstones[0].Volume != "v1" || strings.Join(l.Tombstones[0].Nodes, " ") != "fl-a fl-b" || strings.Join(l.Tombstones[0].Pending, " ") != "fl-a fl-b" ||
		l.Tombstones[1].Volume != "v2" || strings.Join(l.Tombstones[1].Nodes, " ") != "fl-b fl-c" || strings.Join(l.Tombstones[1].Pending, " ") != "fl-c" {
		t.Fatalf("every node's tombstones: %s", out)
	}
	out, _, err = f.run(admin, "tombstones", "fl-c")
	if l := parse(t, out); err != nil || len(l.Tombstones) != 1 || l.Tombstones[0].Volume != "v2" {
		t.Fatalf("fl-c's tombstones: %s %v", out, err)
	}
	out, _, err = f.run(admin, "tombstones", "fl-d")
	if err != nil || strings.TrimSpace(out) != "{}" {
		t.Fatalf("a node with none: %q %v", out, err)
	}
	if err := f.create("v3"); status.Code(err) != codes.AlreadyExists {
		t.Fatalf("create over a tombstone pending for an unregistered node: %v", err)
	}
	// An agent may list only its own node's: the listing of every node fails, and prints nothing.
	out, _, err = f.run(f.as("agent-a", pki.NodeID(td, "fl-a", "agent")), "tombstones")
	if status.Code(err) != codes.PermissionDenied || out != "" {
		t.Fatalf("an agent's listing of every node: %q %v", out, err)
	}
}

// Every refusal of confirm comes before any call (docs/issues/14, S2): its arguments, then the
// caller's identity, which must be the node's own agent.
func TestConfirmRefusals(t *testing.T) {
	f := newFixture(t)
	f.tombstone("v", `{"nodes":["fl-a","fl-c"]}`)
	agentC := f.as("agent-c", pki.NodeID(td, "fl-c", "agent"))
	refused := func(what string, id pki.Files, args ...string) {
		t.Helper()
		before := f.total()
		out, _, err := f.run(id, args...)
		if err == nil || !strings.Contains(err.Error(), "lab.sh vol-confirm") || out != "" || f.total() != before {
			t.Fatalf("%s: %q %v, %d calls", what, out, err, f.total()-before)
		}
	}

	for what, args := range map[string][]string{
		"no token":          {"v", "fl-c"},
		"=false":            {"v", "fl-c", "--replicas-deleted=false"},
		"single dash":       {"v", "fl-c", "-replicas-deleted"},
		"misspelt":          {"v", "fl-c", "--replica-deleted"},
		"a third argument":  {"v", "fl-c", "x", "--replicas-deleted"},
		"the token twice":   {"v", "--replicas-deleted", "fl-c", "--replicas-deleted"},
		"-h":                {"v", "fl-c", "--replicas-deleted", "-h"},
		"another flag":      {"--server", f.addr, "v", "fl-c", "--replicas-deleted"},
		"no positional":     {"--replicas-deleted"},
		"only the volume":   {"v", "--replicas-deleted"},
		"an empty argument": {"v", "fl-c", "", "--replicas-deleted"},
	} {
		refused(what, agentC, append([]string{"--trust-domain", td, "confirm"}, args...)...)
	}
	if _, _, err := f.run(agentC, "--trust-domain", td, "confirm", "v", "fl-c"); !strings.Contains(err.Error(), "--replicas-deleted asserts that every replica") {
		t.Fatalf("the refusal does not say what the token asserts: %v", err)
	}

	confirm := []string{"confirm", "v", "fl-c", "--replicas-deleted"}
	withTD := append([]string{"--trust-domain", td}, confirm...)
	refused("no certificate", pki.Files{}, withTD...)
	refused("an empty --trust-domain", agentC, confirm...)
	refused("an administrator, for a registered, stopped node", f.as("admin", pki.AdminID(td, "ops")),
		"--trust-domain", td, "confirm", "v", "fl-s", "--replicas-deleted")
	refused("the node's io", f.as("io-c", pki.NodeID(td, "fl-c", "io")), withTD...)
	refused("the node's meta", f.as("meta-c", pki.NodeID(td, "fl-c", "meta")), withTD...)
	refused("another node's agent", f.as("agent-a", pki.NodeID(td, "fl-a", "agent")), withTD...)
	refused("another trust domain", f.as("other-c", pki.NodeID("other", "fl-c", "agent")), withTD...)
	refused("two URI SANs", f.twoSANs(), withTD...)
	// The CA's certificate first, the node's agent second: the leaf is the first.
	leafSecond := agentC
	leafSecond.Cert = filepath.Join(f.dir, "leaf-second.crt")
	agentPEM, err := os.ReadFile(agentC.Cert)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(leafSecond.Cert, append(f.ca.CertPEM(), agentPEM...), 0o600); err != nil {
		t.Fatal(err)
	}
	refused("a certificate file with the leaf second", leafSecond, withTD...)
	if f.total() != 0 {
		t.Fatalf("%d calls reached the service", f.total())
	}
}

// twoSANs is a client certificate for fl-c's agent with a second URI SAN, fl-a's agent.
func (f *fixture) twoSANs() pki.Files {
	f.t.Helper()
	key, csr, err := pki.NewKeyAndCSR()
	if err != nil {
		f.t.Fatal(err)
	}
	block, _ := pem.Decode(csr)
	req, err := x509.ParseCertificateRequest(block.Bytes)
	if err != nil {
		f.t.Fatal(err)
	}
	var uris []*url.URL
	for _, id := range []string{pki.NodeID(td, "fl-c", "agent"), pki.NodeID(td, "fl-a", "agent")} {
		u, _ := url.Parse(id)
		uris = append(uris, u)
	}
	der, err := x509.CreateCertificate(rand.Reader, &x509.Certificate{
		SerialNumber: big.NewInt(1), Subject: pkix.Name{CommonName: "two"}, NotBefore: time.Now().Add(-time.Minute),
		NotAfter: time.Now().Add(time.Hour), KeyUsage: x509.KeyUsageDigitalSignature,
		ExtKeyUsage: []x509.ExtKeyUsage{x509.ExtKeyUsageClientAuth}, URIs: uris,
	}, f.ca.Cert, req.PublicKey, f.ca.Key)
	if err != nil {
		f.t.Fatal(err)
	}
	files := pki.Files{Cert: filepath.Join(f.dir, "two.crt"), Key: filepath.Join(f.dir, "two.key"), CA: filepath.Join(f.dir, "ca.crt")}
	for path, b := range map[string][]byte{files.Cert: pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), files.Key: key} {
		if err := os.WriteFile(path, b, 0o600); err != nil {
			f.t.Fatal(err)
		}
	}
	return files
}

// As the node's agent, confirm reads first, confirms only what the read shows pending for the
// node, and prints what it read (docs/issues/14, S2).
func TestConfirmAsTheNodesAgent(t *testing.T) {
	f := newFixture(t)
	f.tombstone("v", `{"nodes":["fl-a","fl-c"]}`)
	f.tombstone("w", `{"nodes":["fl-a"]}`)
	agentC := f.as("agent-c", pki.NodeID(td, "fl-c", "agent"))

	// The token after the positionals.
	out, errOut, err := f.run(agentC, "--trust-domain", td, "confirm", "v", "fl-c", "--replicas-deleted")
	if err != nil {
		t.Fatal(err)
	}
	var got struct {
		Volume  string
		Nodes   []string
		Pending []string
	}
	if json.Unmarshal([]byte(out), &got) != nil || got.Volume != "v" || strings.Join(got.Nodes, " ") != "fl-a fl-c" || strings.Join(got.Pending, " ") != "fl-a" {
		t.Fatalf("stdout %q", out)
	}
	for _, want := range []string{pki.NodeID(td, "fl-c", "agent"), " v ", "fl-c", "nodes [fl-a fl-c]", "pending [fl-a fl-c]"} {
		if !strings.Contains(errOut, want) {
			t.Fatalf("stderr %q does not say %q", errOut, want)
		}
	}
	if f.count("ListTombstones") != 1 || f.count("ConfirmDeletion") != 1 {
		t.Fatalf("calls %v", f.calls)
	}
	if err := f.create("v"); status.Code(err) != codes.AlreadyExists {
		t.Fatalf("create while fl-a is pending: %v", err)
	}

	// Nothing to confirm: again, an unknown volume, a node not on the tombstone.
	for _, volume := range []string{"v", "nope", "w"} {
		out, errOut, err := f.run(agentC, "--trust-domain", td, "confirm", volume, "fl-c", "--replicas-deleted")
		if err != nil || strings.TrimSpace(out) != "{}" || !strings.Contains(errOut, "nothing to confirm") {
			t.Fatalf("%s: %q %q %v", volume, out, errOut, err)
		}
	}
	if f.count("ConfirmDeletion") != 1 {
		t.Fatalf("ConfirmDeletion called with nothing to confirm: %v", f.calls)
	}

	// The token before the positionals; the last confirmation frees the id.
	out, _, err = f.run(f.as("agent-a", pki.NodeID(td, "fl-a", "agent")), "--trust-domain", td, "confirm", "--replicas-deleted", "v", "fl-a")
	if err != nil || !strings.Contains(out, `"volume": "v"`) || strings.Contains(out, "pending") {
		t.Fatalf("the last confirmation: %q %v", out, err)
	}
	if err := f.create("v"); err != nil {
		t.Fatalf("create once every node confirmed: %v", err)
	}
}

// The usage text names lab.sh vol-confirm; -h and a bad flag fail.
func TestUsage(t *testing.T) {
	var out, errOut bytes.Buffer
	if err := client(nil, &out, &errOut); err == nil || !strings.Contains(errOut.String(), "lab.sh vol-confirm") ||
		!strings.Contains(errOut.String(), "keeps them") {
		t.Fatalf("usage: %v, %q", err, errOut.String())
	}
	errOut.Reset()
	if err := client([]string{"-h"}, &out, &errOut); err == nil || !strings.Contains(errOut.String(), "lab.sh vol-confirm") {
		t.Fatalf("-h: %v, %q", err, errOut.String())
	}
	if err := client([]string{"--bogus", "nodes"}, &out, &errOut); err == nil {
		t.Fatal("a bad flag succeeded")
	}
	if out.Len() != 0 {
		t.Fatalf("stdout %q", out.String())
	}
}
