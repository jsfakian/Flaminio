// Command flaminio-ca is a small certificate authority for Flaminio identities (pkg/pki), for
// development and the test lab. In a cluster the operator holds the CA (ARCHITECTURE §15.2).
//
//	flaminio-ca init <dir> <name>                     create <dir>/ca.crt and <dir>/ca.key
//	flaminio-ca sign <dir> --id <spiffe-id> [--ip a,b] [--dns a,b] [--days n] [--server] [--client]
//	                                                  sign the CSR on stdin; certificate on stdout
//	flaminio-ca token-key <path>                      create an Ed25519 token-signing key and <path>.pub
//
// Private keys of the identities never pass through here: only certificate requests do.
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/flaminio/flaminio/pkg/pki"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "flaminio-ca:", err)
		os.Exit(1)
	}
}

// writeNew writes a file that must not exist yet.
func writeNew(path string, b []byte, mode os.FileMode) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	if _, err := f.Write(b); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}

func run(args []string) error {
	if len(args) == 0 {
		return errors.New("usage: flaminio-ca init|sign|token-key [args]")
	}
	switch args[0] {
	case "init":
		if len(args) != 3 {
			return errors.New("usage: flaminio-ca init <dir> <name>")
		}
		dir := args[1]
		if err := os.MkdirAll(dir, 0o700); err != nil {
			return err
		}
		ca, err := pki.NewCA(args[2], 5*365*24*time.Hour)
		if err != nil {
			return err
		}
		key, err := ca.KeyPEM()
		if err != nil {
			return err
		}
		if err := writeNew(filepath.Join(dir, "ca.key"), key, 0o600); err != nil {
			return err
		}
		return writeNew(filepath.Join(dir, "ca.crt"), ca.CertPEM(), 0o644)

	case "sign":
		if len(args) < 2 {
			return errors.New("usage: flaminio-ca sign <dir> --id ... < csr")
		}
		dir := args[1]
		fs := flag.NewFlagSet("sign", flag.ContinueOnError)
		id := fs.String("id", "", "SPIFFE ID")
		ips := fs.String("ip", "", "comma-separated IP SANs")
		dns := fs.String("dns", "", "comma-separated DNS SANs")
		days := fs.Int("days", 30, "validity in days")
		server := fs.Bool("server", false, "server authentication")
		client := fs.Bool("client", false, "client authentication")
		if err := fs.Parse(args[2:]); err != nil {
			return err
		}
		if _, err := pki.ParseID(*id); err != nil {
			return err
		}
		ca, err := pki.LoadCA(filepath.Join(dir, "ca.crt"), filepath.Join(dir, "ca.key"))
		if err != nil {
			return err
		}
		r := pki.Request{ID: *id, Validity: time.Duration(*days) * 24 * time.Hour}
		if *server {
			r.Usage |= pki.Server
		}
		if *client {
			r.Usage |= pki.Client
		}
		for _, s := range strings.Split(*ips, ",") {
			if s == "" {
				continue
			}
			ip := net.ParseIP(s)
			if ip == nil {
				return fmt.Errorf("bad IP %q", s)
			}
			r.IPs = append(r.IPs, ip)
		}
		for _, s := range strings.Split(*dns, ",") {
			if s != "" {
				r.DNSNames = append(r.DNSNames, s)
			}
		}
		csr, err := io.ReadAll(io.LimitReader(os.Stdin, 64<<10))
		if err != nil {
			return err
		}
		cert, err := ca.Sign(csr, r)
		if err != nil {
			return err
		}
		_, err = os.Stdout.Write(cert)
		return err

	case "token-key":
		if len(args) != 2 {
			return errors.New("usage: flaminio-ca token-key <path>")
		}
		pub, key, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			return err
		}
		kder, err := x509.MarshalPKCS8PrivateKey(key)
		if err != nil {
			return err
		}
		pder, err := x509.MarshalPKIXPublicKey(pub)
		if err != nil {
			return err
		}
		if err := writeNew(args[1], pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: kder}), 0o600); err != nil {
			return err
		}
		return writeNew(args[1]+".pub", pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pder}), 0o644)
	}
	return fmt.Errorf("unknown command %q", args[0])
}
