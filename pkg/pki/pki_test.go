package pki

import (
	"crypto/tls"
	"crypto/x509"
	"encoding/pem"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestParseID(t *testing.T) {
	id, err := ParseID("spiffe://lab/flaminio/node/fl-a/io")
	if err != nil || id.Node != "fl-a" || id.Component != "io" || id.TrustDomain != "lab" {
		t.Fatalf("%+v %v", id, err)
	}
	if id.String() != "spiffe://lab/flaminio/node/fl-a/io" {
		t.Fatal(id.String())
	}
	if id, err := ParseID("spiffe://lab/flaminio/admin/ops"); err != nil || id.Admin != "ops" {
		t.Fatalf("%+v %v", id, err)
	}
	for _, bad := range []string{
		"https://lab/flaminio/node/a/io",
		"spiffe://lab/flaminio/node/a",
		"spiffe://lab/flaminio/node/../io",
		"spiffe://lab/other/node/a/io",
		"spiffe:///flaminio/node/a/io",
		"spiffe://lab/flaminio/node/a/io?x=1",
	} {
		if _, err := ParseID(bad); err == nil {
			t.Errorf("accepted %q", bad)
		}
	}
}

// Writes a certificate and key for id, signed by ca, into dir; returns their Files.
func issue(t *testing.T, ca *CA, dir, name, id string, usage Usage) Files {
	t.Helper()
	key, csr, err := NewKeyAndCSR()
	if err != nil {
		t.Fatal(err)
	}
	cert, err := ca.Sign(csr, Request{ID: id, IPs: []net.IP{net.IPv4(127, 0, 0, 1)}, Validity: time.Hour, Usage: usage})
	if err != nil {
		t.Fatal(err)
	}
	f := Files{Cert: filepath.Join(dir, name+".crt"), Key: filepath.Join(dir, name+".key"), CA: filepath.Join(dir, "ca.crt")}
	for path, b := range map[string][]byte{f.Cert: cert, f.Key: key, f.CA: ca.CertPEM()} {
		if err := os.WriteFile(path, b, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return f
}

// TestMutualTLS checks a handshake between two issued identities, and that a certificate from
// another CA is refused.
func TestMutualTLS(t *testing.T) {
	dir := t.TempDir()
	ca, err := NewCA("test", time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	server := issue(t, ca, dir, "server", NodeID("lab", "fl-a", "meta"), Server)
	client := issue(t, ca, dir, "client", NodeID("lab", "fl-b", "io"), Client)
	other, _ := NewCA("other", time.Hour)
	odir := t.TempDir()
	stranger := issue(t, other, odir, "stranger", NodeID("lab", "fl-b", "io"), Client)
	stranger.CA = client.CA // trusts our CA, but is signed by another

	scfg, err := server.ServerConfig()
	if err != nil {
		t.Fatal(err)
	}
	ln, err := tls.Listen("tcp", "127.0.0.1:0", scfg)
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	peers := make(chan any, 4)
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			tc := c.(*tls.Conn)
			if err := tc.Handshake(); err != nil {
				peers <- err
			} else if id, err := PeerIdentity(tc.ConnectionState(), "lab"); err != nil {
				peers <- err
			} else {
				peers <- id
			}
			c.Close()
		}
	}()

	dial := func(f Files) error {
		cfg, err := f.ClientConfig()
		if err != nil {
			return err
		}
		c, err := tls.Dial("tcp", ln.Addr().String(), cfg)
		if err != nil {
			return err
		}
		defer c.Close()
		_, err = c.Read(make([]byte, 1)) // TLS 1.3: a refused client certificate shows here
		return err
	}
	_ = dial(client)
	if got := <-peers; got != (Identity{TrustDomain: "lab", Node: "fl-b", Component: "io"}) {
		t.Fatalf("server saw %v", got)
	}
	_ = dial(stranger)
	if got := <-peers; got == nil {
		t.Fatal("certificate from another CA accepted")
	} else if _, ok := got.(error); !ok {
		t.Fatalf("certificate from another CA accepted as %v", got)
	}
}

func TestSignRejectsTamperedRequest(t *testing.T) {
	ca, _ := NewCA("test", time.Hour)
	_, csr, _ := NewKeyAndCSR()
	block, _ := pem.Decode(csr)
	block.Bytes[len(block.Bytes)-5] ^= 0xff
	if _, err := ca.Sign(pem.EncodeToMemory(block), Request{ID: AdminID("lab", "x"), Validity: time.Hour, Usage: Client}); err == nil {
		t.Fatal("signed a request with a bad signature")
	}
	_, csr, _ = NewKeyAndCSR()
	cert, err := ca.Sign(csr, Request{ID: AdminID("lab", "ops"), Validity: time.Hour, Usage: Client})
	if err != nil {
		t.Fatal(err)
	}
	block, _ = pem.Decode(cert)
	c, _ := x509.ParseCertificate(block.Bytes)
	if c.IsCA || len(c.URIs) != 1 || c.URIs[0].String() != "spiffe://lab/flaminio/admin/ops" {
		t.Fatalf("issued %+v", c)
	}
}
