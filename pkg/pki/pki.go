// Package pki issues and checks Flaminio workload identities (ARCHITECTURE §15.2).
//
// Every component gets an X.509 certificate whose only identity is a SPIFFE-style URI SAN:
//
//	spiffe://<trust-domain>/flaminio/node/<node>/<component>   a component on a node
//	spiffe://<trust-domain>/flaminio/admin/<name>                an administrator or controller
//
// The CA signs certificate requests (CSRs), so private keys never leave the machine that uses
// them. Names and SANs in a CSR are ignored: the CA sets them.
package pki

import (
	"crypto"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"errors"
	"fmt"
	"math/big"
	"net"
	"net/url"
	"os"
	"regexp"
	"strings"
	"time"
)

// CA is a certificate authority: its certificate and signing key.
type CA struct {
	Cert *x509.Certificate
	Key  crypto.Signer
}

// NewCA creates a self-signed ECDSA P-256 CA.
func NewCA(name string, validity time.Duration) (*CA, error) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, err
	}
	tmpl := &x509.Certificate{
		SerialNumber:          serial(),
		Subject:               pkix.Name{CommonName: name},
		NotBefore:             time.Now().Add(-5 * time.Minute),
		NotAfter:              time.Now().Add(validity),
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageCRLSign,
		BasicConstraintsValid: true,
		IsCA:                  true,
		MaxPathLenZero:        true,
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, tmpl, key.Public(), key)
	if err != nil {
		return nil, err
	}
	cert, err := x509.ParseCertificate(der)
	if err != nil {
		return nil, err
	}
	return &CA{Cert: cert, Key: key}, nil
}

func serial() *big.Int {
	n, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 127))
	if err != nil {
		panic(err)
	}
	return n
}

// Usage says what a certificate may authenticate.
type Usage int

const (
	Server Usage = 1 << iota
	Client
)

// Request is what the CA puts in a certificate.
type Request struct {
	ID       string // SPIFFE ID
	DNSNames []string
	IPs      []net.IP
	Validity time.Duration
	Usage    Usage
}

// Sign issues a certificate for the public key in csrPEM.
func (ca *CA) Sign(csrPEM []byte, r Request) ([]byte, error) {
	block, _ := pem.Decode(csrPEM)
	if block == nil || block.Type != "CERTIFICATE REQUEST" {
		return nil, errors.New("not a PEM certificate request")
	}
	csr, err := x509.ParseCertificateRequest(block.Bytes)
	if err != nil {
		return nil, err
	}
	if err := csr.CheckSignature(); err != nil {
		return nil, fmt.Errorf("certificate request signature: %w", err)
	}
	return ca.issue(csr.PublicKey, r)
}

func (ca *CA) issue(pub any, r Request) ([]byte, error) {
	id, err := url.Parse(r.ID)
	if err != nil || id.Scheme != "spiffe" {
		return nil, fmt.Errorf("invalid SPIFFE ID %q", r.ID)
	}
	var ext []x509.ExtKeyUsage
	if r.Usage&Server != 0 {
		ext = append(ext, x509.ExtKeyUsageServerAuth)
	}
	if r.Usage&Client != 0 {
		ext = append(ext, x509.ExtKeyUsageClientAuth)
	}
	if len(ext) == 0 {
		return nil, errors.New("certificate needs a usage")
	}
	tmpl := &x509.Certificate{
		SerialNumber: serial(),
		Subject:      pkix.Name{CommonName: id.Path},
		NotBefore:    time.Now().Add(-5 * time.Minute),
		NotAfter:     time.Now().Add(r.Validity),
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  ext,
		URIs:         []*url.URL{id},
		DNSNames:     r.DNSNames,
		IPAddresses:  r.IPs,
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, ca.Cert, pub, ca.Key)
	if err != nil {
		return nil, err
	}
	return pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), nil
}

// CertPEM returns the CA certificate.
func (ca *CA) CertPEM() []byte {
	return pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: ca.Cert.Raw})
}

// KeyPEM returns the CA key, PKCS #8.
func (ca *CA) KeyPEM() ([]byte, error) {
	der, err := x509.MarshalPKCS8PrivateKey(ca.Key)
	if err != nil {
		return nil, err
	}
	return pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: der}), nil
}

// LoadCA reads a CA certificate and key.
func LoadCA(certFile, keyFile string) (*CA, error) {
	pair, err := tls.LoadX509KeyPair(certFile, keyFile)
	if err != nil {
		return nil, err
	}
	cert, err := x509.ParseCertificate(pair.Certificate[0])
	if err != nil {
		return nil, err
	}
	if !cert.IsCA {
		return nil, fmt.Errorf("%s is not a CA certificate", certFile)
	}
	signer, ok := pair.PrivateKey.(crypto.Signer)
	if !ok {
		return nil, errors.New("CA key cannot sign")
	}
	return &CA{Cert: cert, Key: signer}, nil
}

// NewKeyAndCSR makes a P-256 key and a certificate request for it. For tests and tools; in the
// lab the key is made where it is used.
func NewKeyAndCSR() (keyPEM, csrPEM []byte, err error) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, nil, err
	}
	der, err := x509.CreateCertificateRequest(rand.Reader, &x509.CertificateRequest{}, key)
	if err != nil {
		return nil, nil, err
	}
	kder, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return nil, nil, err
	}
	return pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: kder}),
		pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE REQUEST", Bytes: der}), nil
}

// Identity is a parsed Flaminio SPIFFE ID.
type Identity struct {
	TrustDomain string
	// Node and Component are set for node identities.
	Node      string
	Component string
	// Admin is set for administrator identities.
	Admin string
}

func (i Identity) String() string {
	if i.Admin != "" {
		return AdminID(i.TrustDomain, i.Admin)
	}
	return NodeID(i.TrustDomain, i.Node, i.Component)
}

var segment = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`)

// NodeID is the SPIFFE ID of a component on a node.
func NodeID(td, node, component string) string {
	return fmt.Sprintf("spiffe://%s/flaminio/node/%s/%s", td, node, component)
}

// AdminID is the SPIFFE ID of an administrator or controller.
func AdminID(td, name string) string {
	return fmt.Sprintf("spiffe://%s/flaminio/admin/%s", td, name)
}

// ParseID parses a Flaminio SPIFFE ID.
func ParseID(id string) (Identity, error) {
	u, err := url.Parse(id)
	if err != nil || u.Scheme != "spiffe" || u.Host == "" || u.RawQuery != "" || u.Fragment != "" {
		return Identity{}, fmt.Errorf("not a SPIFFE ID: %q", id)
	}
	parts := strings.Split(strings.TrimPrefix(u.Path, "/"), "/")
	for _, p := range parts[1:] {
		if !segment.MatchString(p) {
			return Identity{}, fmt.Errorf("bad segment %q in %q", p, id)
		}
	}
	switch {
	case len(parts) == 4 && parts[0] == "flaminio" && parts[1] == "node":
		return Identity{TrustDomain: u.Host, Node: parts[2], Component: parts[3]}, nil
	case len(parts) == 3 && parts[0] == "flaminio" && parts[1] == "admin":
		return Identity{TrustDomain: u.Host, Admin: parts[2]}, nil
	}
	return Identity{}, fmt.Errorf("not a Flaminio identity: %q", id)
}

// PeerIdentity returns the identity of a verified TLS peer: the single SPIFFE URI SAN of its
// leaf certificate, in the expected trust domain.
func PeerIdentity(state tls.ConnectionState, trustDomain string) (Identity, error) {
	if len(state.VerifiedChains) == 0 || len(state.VerifiedChains[0]) == 0 {
		return Identity{}, errors.New("peer certificate not verified")
	}
	leaf := state.VerifiedChains[0][0]
	if len(leaf.URIs) != 1 {
		return Identity{}, fmt.Errorf("peer certificate has %d URI SANs, want 1", len(leaf.URIs))
	}
	id, err := ParseID(leaf.URIs[0].String())
	if err != nil {
		return Identity{}, err
	}
	if id.TrustDomain != trustDomain {
		return Identity{}, fmt.Errorf("peer is in trust domain %q, want %q", id.TrustDomain, trustDomain)
	}
	return id, nil
}

// Files names a certificate, its key and the CA bundle that verifies peers.
type Files struct {
	Cert, Key, CA string
}

func (f Files) load() (tls.Certificate, *x509.CertPool, error) {
	pair, err := tls.LoadX509KeyPair(f.Cert, f.Key)
	if err != nil {
		return pair, nil, err
	}
	b, err := os.ReadFile(f.CA)
	if err != nil {
		return pair, nil, err
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(b) {
		return pair, nil, fmt.Errorf("no certificates in %s", f.CA)
	}
	return pair, pool, nil
}

// ServerConfig is a TLS 1.3 server configuration that requires and verifies client
// certificates.
func (f Files) ServerConfig() (*tls.Config, error) {
	pair, pool, err := f.load()
	if err != nil {
		return nil, err
	}
	return &tls.Config{
		MinVersion:   tls.VersionTLS13,
		Certificates: []tls.Certificate{pair},
		ClientCAs:    pool,
		ClientAuth:   tls.RequireAndVerifyClientCert,
	}, nil
}

// ClientConfig is a TLS 1.3 client configuration that presents a certificate and verifies the
// server against the CA bundle.
func (f Files) ClientConfig() (*tls.Config, error) {
	pair, pool, err := f.load()
	if err != nil {
		return nil, err
	}
	return &tls.Config{
		MinVersion:   tls.VersionTLS13,
		Certificates: []tls.Certificate{pair},
		RootCAs:      pool,
	}, nil
}
