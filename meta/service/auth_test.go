package service

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/pkg/captoken"
	"github.com/flaminio/flaminio/pkg/pki"
)

const td = "lab"

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

// TestAuthorization runs the service behind mutual TLS and checks each method's caller rule.
func TestAuthorization(t *testing.T) {
	dir := t.TempDir()
	ca, err := pki.NewCA("test", time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	pub, key, _ := ed25519.GenerateKey(rand.Reader)
	server := writeIdentity(t, ca, dir, "server", pki.NodeID(td, "fl-a", "meta"), pki.Server)
	scfg, err := server.ServerConfig()
	if err != nil {
		t.Fatal(err)
	}
	svc := New(kv.NewMem(nil), Options{TrustDomain: td, TokenKey: key})
	srv := grpc.NewServer(grpc.Creds(credentials.NewTLS(scfg)), grpc.UnaryInterceptor(svc.Authorize))
	metav1.RegisterMetadataServiceServer(srv, svc)
	lis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	go func() { _ = srv.Serve(lis) }()
	defer srv.Stop()

	connect := func(f pki.Files) metav1.MetadataServiceClient {
		cfg, err := f.ClientConfig()
		if err != nil {
			t.Fatal(err)
		}
		conn, err := grpc.NewClient(lis.Addr().String(), grpc.WithTransportCredentials(credentials.NewTLS(cfg)))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { conn.Close() })
		return metav1.NewMetadataServiceClient(conn)
	}
	as := func(name, id string) metav1.MetadataServiceClient {
		return connect(writeIdentity(t, ca, dir, name, id, pki.Client))
	}
	admin := as("admin", pki.AdminID(td, "ops"))
	nodeA := as("a", pki.NodeID(td, "fl-a", "io"))
	nodeB := as("b", pki.NodeID(td, "fl-b", "io"))
	metaA := as("meta-a", pki.NodeID(td, "fl-a", "meta"))
	agentA := as("agent-a", pki.NodeID(td, "fl-a", "agent"))
	agentB := as("agent-b", pki.NodeID(td, "fl-b", "agent"))
	foreign := as("foreign", "spiffe://other/flaminio/admin/ops")
	ctx := context.Background()

	expect := func(what string, err error, want codes.Code) {
		t.Helper()
		if code(err) != want {
			t.Fatalf("%s: got %v, want %v", what, err, want)
		}
	}

	// Nodes register and heartbeat only themselves.
	for _, c := range []struct {
		client metav1.MetadataServiceClient
		node   string
	}{{nodeA, "fl-a"}, {nodeB, "fl-b"}} {
		_, err := c.client.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: c.node, Address: c.node + ":7410"}})
		expect("register self", err, codes.OK)
		_, err = c.client.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: c.node})
		expect("heartbeat self", err, codes.OK)
	}
	_, err = nodeA.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: "fl-b", Address: "evil:1"}})
	expect("register another node", err, codes.PermissionDenied)
	_, err = nodeA.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "fl-b"})
	expect("heartbeat for another node", err, codes.PermissionDenied)
	_, err = metaA.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: "fl-a"})
	expect("heartbeat from the wrong component", err, codes.PermissionDenied)
	_, err = foreign.ListNodes(ctx, &metav1.ListNodesRequest{})
	expect("another trust domain", err, codes.Unauthenticated)

	// Only administrators create and attach.
	req := &metav1.CreateVolumeRequest{Id: "v", SizeBytes: mib, ChunkBytes: mib, Replicas: 2, MinIsr: 1}
	_, err = nodeA.CreateVolume(ctx, req)
	expect("node creates a volume", err, codes.PermissionDenied)
	_, err = admin.CreateVolume(ctx, req)
	expect("admin creates a volume", err, codes.OK)
	_, err = nodeA.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a"})
	expect("node attaches itself", err, codes.PermissionDenied)
	att, err := admin.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a"})
	expect("admin attaches", err, codes.OK)
	// A node's agent restarts its own target, and nothing else.
	_, err = agentB.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-b", OnlyIfTarget: true})
	expect("agent restarts a target it is not", err, codes.FailedPrecondition)
	_, err = agentB.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a", OnlyIfTarget: true})
	expect("agent restarts another node's target", err, codes.PermissionDenied)
	_, err = agentA.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a"})
	expect("agent attaches without only_if_target", err, codes.PermissionDenied)
	_, err = nodeA.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a", OnlyIfTarget: true})
	expect("data plane restarts its target", err, codes.PermissionDenied)
	restart, err := agentA.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a", OnlyIfTarget: true})
	expect("agent restarts its own target", err, codes.OK)
	if restart.Volume.GetEpoch() != 2 {
		t.Fatalf("restart epoch %d, want 2", restart.Volume.GetEpoch())
	}
	att, err = admin.Attach(ctx, &metav1.AttachRequest{Volume: "v", Node: "fl-a"})
	expect("admin attaches again", err, codes.OK)

	// The token names the volume, the epoch and the node.
	claims, err := captoken.Verify(pub, att.Token)
	if err != nil || claims.Volume != "v" || claims.Epoch != 3 || claims.Node != "fl-a" {
		t.Fatalf("token %+v %v", claims, err)
	}

	// Only administrators request moves and drains.
	_, err = nodeA.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: "v", From: "fl-a", To: "fl-b"})
	expect("node requests a move", err, codes.PermissionDenied)
	_, err = agentA.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "fl-a", Draining: true})
	expect("agent drains its node", err, codes.PermissionDenied)
	_, err = admin.DrainNode(ctx, &metav1.DrainNodeRequest{Node: "fl-a"})
	expect("admin undrains", err, codes.OK)
	_, err = nodeB.ListVolumes(ctx, &metav1.ListVolumesRequest{})
	expect("node lists volumes", err, codes.OK)

	// Anyone authenticated reads.
	_, err = nodeB.GetVolume(ctx, &metav1.GetVolumeRequest{Id: "v"})
	expect("node reads", err, codes.OK)

	// Only the target's data plane changes the ISR, even with the right epoch and version.
	c0 := att.Volume.Chunks[0]
	change := &metav1.ChangeIsrRequest{Volume: "v", Epoch: 3, Changes: []*metav1.IsrChange{{Chunk: 0, ExpectedVer: c0.IsrVer, NewIsr: c0.Replicas[:1]}}}
	r, err := nodeB.ChangeIsr(ctx, change)
	if err != nil || r.Applied {
		t.Fatalf("non-target changed the ISR: %v %v", r, err)
	}
	_, err = admin.ChangeIsr(ctx, change)
	expect("admin changes the ISR", err, codes.PermissionDenied)
	r, err = nodeA.ChangeIsr(ctx, change)
	if err != nil || !r.Applied {
		t.Fatalf("target could not change the ISR: %v %v", r, err)
	}

	// No client certificate at all: the handshake fails.
	scfgNoCert, _ := server.ClientConfig()
	scfgNoCert.Certificates = nil
	conn, err := grpc.NewClient(lis.Addr().String(), grpc.WithTransportCredentials(credentials.NewTLS(scfgNoCert)))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_, err = metav1.NewMetadataServiceClient(conn).ListNodes(ctx, &metav1.ListNodesRequest{})
	expect("no client certificate", err, codes.Unavailable)

	// A certificate from another CA is refused too.
	other, _ := pki.NewCA("other", time.Hour)
	odir := t.TempDir()
	stranger := writeIdentity(t, other, odir, "s", pki.AdminID(td, "ops"), pki.Client)
	stranger.CA = server.CA
	_, err = connect(stranger).ListNodes(ctx, &metav1.ListNodesRequest{})
	expect("certificate from another CA", err, codes.Unavailable)
}
