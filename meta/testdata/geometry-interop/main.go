// Test-only mTLS fixture server using the production metadata service and normal message limits.
package main

import (
	"context"
	"crypto/ed25519"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"flag"
	"fmt"
	"net"
	"os"
	"os/signal"
	"syscall"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
	"github.com/flaminio/flaminio/meta/testdata/geometry-interop/fixture"
	"github.com/flaminio/flaminio/pkg/pki"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
)

func run() error {
	dir := flag.String("dir", "", "interop certificate directory")
	flag.Parse()
	files := pki.Files{Cert: *dir + "/server.crt", Key: *dir + "/server.key", CA: *dir + "/ca/ca.crt"}
	cfg, e := files.ServerConfig()
	if e != nil {
		return e
	}
	raw, e := os.ReadFile(*dir + "/token.key")
	if e != nil {
		return e
	}
	block, _ := pem.Decode(raw)
	if block == nil {
		return fmt.Errorf("invalid token key")
	}
	parsed, e := x509.ParsePKCS8PrivateKey(block.Bytes)
	if e != nil {
		return e
	}
	key, ok := parsed.(ed25519.PrivateKey)
	if !ok {
		return fmt.Errorf("token key is not Ed25519")
	}
	store := kv.NewMem(nil)
	defer store.Close()
	ctx := context.Background()
	put := func(key string, value any) error {
		b, e := json.Marshal(value)
		if e != nil {
			return e
		}
		_, e = store.Txn(ctx, nil, []kv.Op{{Key: key, Value: b}})
		return e
	}
	for r := 1; r <= 5; r++ {
		v := fixture.Volume(r)
		if e := put("/flaminio/v1/volumes/"+v.ID, v.Header); e != nil {
			return e
		}
		for i, c := range v.Chunks {
			if e := put(fmt.Sprintf("/flaminio/v1/chunks/%s/%08d", v.ID, i), c); e != nil {
				return e
			}
		}
	}
	svc := service.New(store, service.Options{TrustDomain: "lab", TokenKey: key})
	server := grpc.NewServer(grpc.Creds(credentials.NewTLS(cfg)), grpc.UnaryInterceptor(svc.Authorize))
	metav1.RegisterMetadataServiceServer(server, svc)
	lis, e := net.Listen("tcp", "127.0.0.1:0")
	if e != nil {
		return e
	}
	if e := os.WriteFile(*dir+"/geometry.addr", []byte(lis.Addr().String()), 0600); e != nil {
		return e
	}
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(sig)
	go func() { <-sig; server.Stop() }()
	return server.Serve(lis)
}
func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
