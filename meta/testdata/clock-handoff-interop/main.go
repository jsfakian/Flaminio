// Test-only real mTLS/CAS fixture. No production clock/profile admission is bypassed.
package main

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"flag"
	"fmt"
	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
	"github.com/flaminio/flaminio/pkg/pki"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/encoding/protojson"
	"net"
	"os"
	"path/filepath"
	"sync"
	"time"
)

func run() error {
	dir := flag.String("dir", "", "private fixture directory")
	call := flag.String("call", "", "test-only Attach JSON")
	actor := flag.String("actor", "own_agent", "mTLS identity")
	addr := flag.String("addr", "", "fixture address")
	flag.Parse()
	if *call != "" {
		return invoke(*dir, *addr, *actor, *call)
	}
	if *dir == "" {
		return fmt.Errorf("dir required")
	}
	raw, e := os.ReadFile("meta/testdata/lease-clock-handoff.json")
	if e != nil {
		return e
	}
	var fixture struct {
		Schedules []struct {
			Name          string
			FirstEpoch    uint64 `json:"first_epoch"`
			InitialEpoch  uint64 `json:"initial_epoch"`
			InitialTarget uint32 `json:"initial_target"`
			LocalReplica  bool   `json:"local_replica"`
		}
	}
	if e = json.Unmarshal(raw, &fixture); e != nil {
		return e
	}
	ca, e := pki.NewCA("clock acceptance", time.Hour)
	if e != nil {
		return e
	}
	write := func(name string, b []byte) error { return os.WriteFile(filepath.Join(*dir, name), b, 0600) }
	if e = write("ca.crt", ca.CertPEM()); e != nil {
		return e
	}
	issue := func(name, id string, usage pki.Usage) error {
		key, csr, e := pki.NewKeyAndCSR()
		if e != nil {
			return e
		}
		crt, e := ca.Sign(csr, pki.Request{ID: id, IPs: []net.IP{net.IPv4(127, 0, 0, 1)}, Validity: time.Hour, Usage: usage})
		if e != nil {
			return e
		}
		if e = write(name+".key", key); e != nil {
			return e
		}
		return write(name+".crt", crt)
	}
	for _, actor := range []string{"server", "admin", "own_agent", "other_agent", "io", "node", "reader", "fl-a", "fl-b"} {
		id := pki.NodeID("lab", "fl-a", actor)
		usage := pki.Client
		switch actor {
		case "server":
			id = pki.NodeID("lab", "m", "meta")
			usage = pki.Server
		case "admin":
			id = "spiffe://lab/flaminio/admin/test"
		case "own_agent":
			id = pki.NodeID("lab", "fl-a", "agent")
		case "other_agent":
			id = pki.NodeID("lab", "fl-b", "agent")
		case "fl-a", "fl-b":
			id = pki.NodeID("lab", actor, "io")
			usage = pki.Client | pki.Server
		}
		if e = issue(actor, id, usage); e != nil {
			return e
		}
	}
	key := ed25519.NewKeyFromSeed(makeSeed())
	store := kv.NewMem(nil)
	defer store.Close()
	ctx := context.Background()
	ids := []string{}
	put := func(k string, v any) error {
		b, e := json.Marshal(v)
		if e != nil {
			return e
		}
		_, e = store.Txn(ctx, nil, []kv.Op{{Key: k, Value: b}})
		return e
	}
	for i, s := range fixture.Schedules {
		id := "clock-" + s.Name
		ids = append(ids, id)
		target := "fl-a"
		if s.InitialTarget == 1 {
			target = "fl-b"
		}
		member := "fl-a"
		if !s.LocalReplica {
			member = "fl-b"
		}
		spec := domain.Spec{ID: id, SizeBytes: 1 << 20, ChunkBytes: 1 << 20, Replicas: 1, MinISR: 1}
		identity := domain.CreateSpecFrom(spec)
		h := domain.Header{ID: id, SizeBytes: spec.SizeBytes, ChunkBytes: spec.ChunkBytes, Replicas: 1, MinISR: 1, Epoch: s.InitialEpoch, FirstEpoch: s.FirstEpoch, Target: target, CreateOperation: fmt.Sprintf("%032x", i+1), CreateSpec: &identity}
		if e = put("/flaminio/v1/volumes/"+id, h); e != nil {
			return e
		}
		if e = put("/flaminio/v1/chunks/"+id+"/00000000", domain.Chunk{Replicas: []string{member}, ISR: []string{member}}); e != nil {
			return e
		}
	}
	svc := service.New(store, service.Options{TrustDomain: "lab", TokenKey: key})
	files := pki.Files{Cert: filepath.Join(*dir, "server.crt"), Key: filepath.Join(*dir, "server.key"), CA: filepath.Join(*dir, "ca.crt")}
	tls, e := files.ServerConfig()
	if e != nil {
		return e
	}
	var mu sync.Mutex
	snapshot := func() {
		mu.Lock()
		defer mu.Unlock()
		state := map[string]json.RawMessage{}
		for _, id := range ids {
			entry, _, e := store.Get(ctx, "/flaminio/v1/volumes/"+id)
			if e != nil {
				panic(e)
			}
			state[id] = entry.Value
		}
		b, e := json.Marshal(state)
		if e != nil {
			panic(e)
		}
		if e = write("state.tmp", b); e != nil {
			panic(e)
		}
		if e = os.Rename(filepath.Join(*dir, "state.tmp"), filepath.Join(*dir, "state.json")); e != nil {
			panic(e)
		}
	}
	snapshot()
	interceptor := func(c context.Context, r any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		defer snapshot()
		return svc.Authorize(c, r, info, func(c context.Context, r any) (any, error) {
			b, _ := os.ReadFile(filepath.Join(*dir, "available"))
			if string(b) == "0" {
				return nil, status.Error(codes.Unavailable, "fixture metadata outage")
			}
			return h(c, r)
		})
	}
	server := grpc.NewServer(grpc.Creds(credentials.NewTLS(tls)), grpc.UnaryInterceptor(interceptor))
	metav1.RegisterMetadataServiceServer(server, svc)
	lis, e := net.Listen("tcp", "127.0.0.1:0")
	if e != nil {
		return e
	}
	if e = write("addr", []byte(lis.Addr().String())); e != nil {
		return e
	}
	return server.Serve(lis)
}
func invoke(dir, addr, actor, raw string) error {
	identity := actor
	if actor == "anonymous" {
		identity = "reader"
	}
	files := pki.Files{Cert: filepath.Join(dir, identity+".crt"), Key: filepath.Join(dir, identity+".key"), CA: filepath.Join(dir, "ca.crt")}
	cfg, e := files.ClientConfig()
	if e != nil {
		return e
	}
	if actor == "anonymous" {
		cfg.Certificates = nil
	}
	conn, e := grpc.NewClient(addr, grpc.WithTransportCredentials(credentials.NewTLS(cfg)))
	if e != nil {
		return e
	}
	defer conn.Close()
	var request metav1.AttachRequest
	if e = json.Unmarshal([]byte(raw), &request); e != nil {
		return e
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	response, e := metav1.NewMetadataServiceClient(conn).Attach(ctx, &request)
	if e != nil {
		return e
	}
	b, e := protojson.Marshal(response)
	if e != nil {
		return e
	}
	fmt.Println(string(b))
	return nil
}
func makeSeed() []byte {
	b := make([]byte, 32)
	for i := range b {
		b[i] = 83
	}
	return b
}
func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
