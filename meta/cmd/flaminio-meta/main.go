// Command flaminio-meta runs the metadata service, and talks to it.
//
//	flaminio-meta serve --listen :7400 --etcd http://10.0.0.1:2379,...
//	flaminio-meta [--server host:7400] nodes
//	flaminio-meta [--server host:7400] register <node> <address> [label=value...]
//	flaminio-meta [--server host:7400] heartbeat <node>
//	flaminio-meta [--server host:7400] create <volume> <size> [replicas] [min-isr] [chunk] [prefer-node] [failure-domain] [policy]
//	flaminio-meta [--server host:7400] get <volume>
//	flaminio-meta [--server host:7400] attach <volume> <node> [unless-target|only-if-target]
//	flaminio-meta [--server host:7400] delete <volume>
//	flaminio-meta [--server host:7400] volumes
//	flaminio-meta [--server host:7400] move <volume> <chunk> <from> <to>
//	flaminio-meta [--server host:7400] drain <node> [timeout]     (default 30m; waits until done)
//	flaminio-meta [--server host:7400] undrain <node>
//	flaminio-meta [--server host:7400] tombstones [node]     (default: every registered node)
//	flaminio-meta [--server host:7400] --trust-domain <td> confirm <volume> <node> --replicas-deleted
//
// Sizes take K, M, G and T suffixes (powers of 1024).
//
// The server requires mutual TLS: --tls-cert/--tls-key are its certificate, --tls-ca verifies
// callers, whose SPIFFE IDs must be in --trust-domain. --token-key signs capability tokens.
// --etcd-cert/--etcd-key/--etcd-ca authenticate it to etcd. --insecure turns all of that off,
// for development only. The client takes --cert, --key and --ca.
//
// confirm records that node holds no replica of volume any more, as node's own agent (its
// certificate, in --trust-domain); --replicas-deleted asserts it. Its only supported entry point
// is `lab.sh vol-confirm`, which checks on the node first, never a command typed or taken from
// shell history. A wrong confirmation lets the id be created again while the node still holds
// replicas: the new volume then serves the old one's data there, or is deposed by them
// (docs/issues/14).
package main

import (
	"context"
	"crypto/ed25519"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"slices"
	"strconv"
	"strings"
	"syscall"
	"time"

	clientv3 "go.etcd.io/etcd/client/v3"
	"go.uber.org/zap"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/control/drain"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
	"github.com/flaminio/flaminio/pkg/pki"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "serve" {
		if err := serve(os.Args[2:]); err != nil {
			slog.Error("flaminio-meta", "err", err)
			os.Exit(1)
		}
		return
	}
	if err := client(os.Args[1:], os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "flaminio-meta:", err)
		os.Exit(1)
	}
}

func serve(args []string) error {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	listen := fs.String("listen", ":7400", "gRPC listen address")
	endpoints := fs.String("etcd", "https://127.0.0.1:2379", "comma-separated etcd endpoints")
	ttl := fs.Duration("liveness-ttl", 10*time.Second, "how long a node stays alive after a heartbeat")
	var tlsFiles, etcdFiles pki.Files
	fs.StringVar(&tlsFiles.Cert, "tls-cert", "", "server certificate")
	fs.StringVar(&tlsFiles.Key, "tls-key", "", "server key")
	fs.StringVar(&tlsFiles.CA, "tls-ca", "", "CA bundle for client certificates")
	trustDomain := fs.String("trust-domain", "", "trust domain of callers' SPIFFE IDs")
	tokenKey := fs.String("token-key", "", "Ed25519 key (PKCS #8 PEM) that signs capability tokens")
	fs.StringVar(&etcdFiles.Cert, "etcd-cert", "", "client certificate for etcd")
	fs.StringVar(&etcdFiles.Key, "etcd-key", "", "client key for etcd")
	fs.StringVar(&etcdFiles.CA, "etcd-ca", "", "CA bundle for etcd's certificates")
	insecureMode := fs.Bool("insecure", false, "no TLS, no authentication, no tokens: development only")
	storeKind := fs.String("store", "etcd", "etcd, or memory (development and tests: nothing is kept)")
	_ = fs.Parse(args)

	opts := service.Options{TTL: *ttl}
	var serverOpts []grpc.ServerOption
	etcdCfg := clientv3.Config{
		Endpoints:   strings.Split(*endpoints, ","),
		DialTimeout: 5 * time.Second,
		// A member cut off by a partition leaves an open-looking connection; keepalives notice
		// it (10 s is gRPC's minimum), and the KV retries requests elsewhere meanwhile.
		DialKeepAliveTime:    10 * time.Second,
		DialKeepAliveTimeout: 3 * time.Second,
		Logger:               zap.NewNop(),
	}
	if !*insecureMode {
		if *trustDomain == "" || *tokenKey == "" {
			return fmt.Errorf("--trust-domain and --token-key are required (or --insecure)")
		}
		scfg, err := tlsFiles.ServerConfig()
		if err != nil {
			return fmt.Errorf("server TLS: %w", err)
		}
		if *storeKind == "etcd" {
			if etcdCfg.TLS, err = etcdFiles.ClientConfig(); err != nil {
				return fmt.Errorf("etcd TLS: %w", err)
			}
		}
		if opts.TokenKey, err = loadEd25519(*tokenKey); err != nil {
			return fmt.Errorf("token key: %w", err)
		}
		opts.TrustDomain = *trustDomain
		serverOpts = append(serverOpts, grpc.Creds(credentials.NewTLS(scfg)))
	}

	var store kv.KV
	switch *storeKind {
	case "etcd":
		c, err := clientv3.New(etcdCfg)
		if err != nil {
			return err
		}
		store = kv.NewEtcd(c)
	case "memory":
		store = kv.NewMem(nil)
	default:
		return fmt.Errorf("unknown store %q", *storeKind)
	}
	defer store.Close()

	lis, err := net.Listen("tcp", *listen)
	if err != nil {
		return err
	}
	svc := service.New(store, opts)
	srv := grpc.NewServer(append(serverOpts, grpc.UnaryInterceptor(svc.Authorize))...)
	metav1.RegisterMetadataServiceServer(srv, svc)
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		srv.GracefulStop()
	}()
	slog.Info("flaminio-meta serving", "listen", lis.Addr().String(), "etcd", *endpoints,
		"tls", !*insecureMode, "trust_domain", *trustDomain)
	return srv.Serve(lis)
}

func loadEd25519(path string) (ed25519.PrivateKey, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	block, _ := pem.Decode(b)
	if block == nil {
		return nil, fmt.Errorf("%s: no PEM block", path)
	}
	k, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return nil, err
	}
	key, ok := k.(ed25519.PrivateKey)
	if !ok {
		return nil, fmt.Errorf("%s: not an Ed25519 key", path)
	}
	return key, nil
}

func parseSize(s string) (uint64, error) {
	mult := uint64(1)
	if n := len(s); n > 0 {
		switch strings.ToUpper(s[n-1:]) {
		case "K":
			mult = 1 << 10
		case "M":
			mult = 1 << 20
		case "G":
			mult = 1 << 30
		case "T":
			mult = 1 << 40
		}
		if mult > 1 {
			s = s[:n-1]
		}
	}
	v, err := strconv.ParseUint(s, 10, 64)
	return v * mult, err
}

func argOr(args []string, i int, def string) string {
	if i < len(args) {
		return args[i]
	}
	return def
}

// usage is the client's usage text; the doc comment has every command's arguments.
const usage = `usage: flaminio-meta serve|nodes|register|heartbeat|create|get|attach|delete|volumes|move|drain|undrain|tombstones|confirm [args]
  confirm <volume> <node> --replicas-deleted: only through lab.sh vol-confirm, which checks on the
  node first; never typed by hand or taken from shell history. A node confirmed while it still
  holds replicas keeps them, and a volume created again with that name serves their data there,
  or is deposed by them (docs/issues/14).`

// replicasDeleted is confirm's acknowledgement: the caller asserts that every replica of the
// volume on the node is deleted.
const replicasDeleted = "--replicas-deleted"

// confirmArgs reads confirm's arguments: exactly two positionals, the volume and the node, and
// the exact token --replicas-deleted once, before or after them. Anything else is refused.
func confirmArgs(args []string) (volume, node string, err error) {
	refuse := func(format string, a ...any) (string, string, error) {
		return "", "", fmt.Errorf("confirm <volume> <node> %s: %s. %s asserts that every replica of the volume on the node is deleted: lab.sh vol-confirm checks that first, and is the only supported way to confirm",
			replicasDeleted, fmt.Sprintf(format, a...), replicasDeleted)
	}
	var pos []string
	tokens := 0
	for _, a := range args {
		switch {
		case a == replicasDeleted:
			tokens++
		case strings.HasPrefix(a, "-"):
			return refuse("unknown argument %q", a)
		default:
			pos = append(pos, a)
		}
	}
	if tokens != 1 || len(pos) != 2 {
		return refuse("%d positional arguments and %d %s, want 2 and 1", len(pos), tokens, replicasDeleted)
	}
	return pos[0], pos[1], nil
}

// agentIdentity is the identity in the client certificate, which must be node's agent in
// trustDomain: the leaf (the file's first certificate, as tls.LoadX509KeyPair loads it) with
// exactly one URI SAN, read as pki.PeerIdentity reads a peer's. It is checked before any call.
func agentIdentity(files pki.Files, trustDomain, node string) (pki.Identity, error) {
	fail := func(format string, a ...any) (pki.Identity, error) {
		return pki.Identity{}, fmt.Errorf("confirm for %s: %s: only %s's own agent confirms, through lab.sh vol-confirm",
			node, fmt.Sprintf(format, a...), node)
	}
	if trustDomain == "" {
		return fail("no --trust-domain")
	}
	if files.Cert == "" {
		return fail("no client certificate (--cert)")
	}
	pair, err := tls.LoadX509KeyPair(files.Cert, files.Key)
	if err != nil {
		return fail("%v", err)
	}
	leaf, err := x509.ParseCertificate(pair.Certificate[0])
	if err != nil {
		return fail("%v", err)
	}
	if len(leaf.URIs) != 1 {
		return fail("the certificate has %d URI SANs, want 1", len(leaf.URIs))
	}
	id, err := pki.ParseID(leaf.URIs[0].String())
	if err != nil {
		return fail("%v", err)
	}
	if id.TrustDomain != trustDomain {
		return fail("%s is in trust domain %q, want %q", id, id.TrustDomain, trustDomain)
	}
	if id.Admin != "" || id.Component != "agent" || id.Node != node {
		return fail("the certificate is %s", id)
	}
	return id, nil
}

// pendingFor returns the tombstone of volume in a node's listing, if it lists it.
func pendingFor(l *metav1.ListTombstonesResponse, volume string) *metav1.Tombstone {
	for _, t := range l.GetTombstones() {
		if t.GetVolume() == volume {
			return t
		}
	}
	return nil
}

func client(args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("flaminio-meta", flag.ContinueOnError)
	fs.SetOutput(stderr)
	fs.Usage = func() {
		fmt.Fprintln(stderr, usage)
		fs.PrintDefaults()
	}
	server := fs.String("server", "127.0.0.1:7400", "metadata service address")
	var files pki.Files
	fs.StringVar(&files.Cert, "cert", "", "client certificate (none: plain-text connection)")
	fs.StringVar(&files.Key, "key", "", "client key")
	fs.StringVar(&files.CA, "ca", "", "CA bundle for the server's certificate")
	trustDomain := fs.String("trust-domain", "", "trust domain of the client's identity (confirm only)")
	if err := fs.Parse(args); err != nil {
		return err
	}
	args = fs.Args()
	if len(args) == 0 {
		return errors.New(usage)
	}
	// confirm refuses before any call: its arguments, then the caller's identity.
	var volume, node string
	var identity pki.Identity
	if args[0] == "confirm" {
		var err error
		if volume, node, err = confirmArgs(args[1:]); err != nil {
			return err
		}
		if identity, err = agentIdentity(files, *trustDomain, node); err != nil {
			return err
		}
	}
	creds := insecure.NewCredentials()
	if files.Cert != "" {
		cfg, err := files.ClientConfig()
		if err != nil {
			return err
		}
		creds = credentials.NewTLS(cfg)
	}
	conn, err := grpc.NewClient(*server, grpc.WithTransportCredentials(creds))
	if err != nil {
		return err
	}
	defer conn.Close()
	c := metav1.NewMetadataServiceClient(conn)
	timeout := 30 * time.Second
	if args[0] == "drain" {
		timeout = 30 * time.Minute
		if len(args) > 2 {
			if timeout, err = time.ParseDuration(args[2]); err != nil {
				return err
			}
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	need := func(n int) error {
		if len(args) < n+1 {
			return fmt.Errorf("%s needs %d arguments", args[0], n)
		}
		return nil
	}
	var out proto.Message
	switch args[0] {
	case "nodes":
		out, err = c.ListNodes(ctx, &metav1.ListNodesRequest{})
	case "register":
		if err = need(2); err == nil {
			labels := map[string]string{}
			for _, l := range args[3:] {
				k, v, _ := strings.Cut(l, "=")
				labels[k] = v
			}
			out, err = c.RegisterNode(ctx, &metav1.RegisterNodeRequest{Node: &metav1.Node{Id: args[1], Address: args[2], Labels: labels}})
		}
	case "heartbeat":
		if err = need(1); err == nil {
			out, err = c.NodeHeartbeat(ctx, &metav1.NodeHeartbeatRequest{Node: args[1]})
		}
	case "create":
		if err = need(2); err != nil {
			break
		}
		var size, chunk uint64
		var rf, min uint64
		if size, err = parseSize(args[2]); err != nil {
			break
		}
		if rf, err = strconv.ParseUint(argOr(args, 3, "2"), 10, 32); err != nil {
			break
		}
		if min, err = strconv.ParseUint(argOr(args, 4, "1"), 10, 32); err != nil {
			break
		}
		if chunk, err = parseSize(argOr(args, 5, "256M")); err != nil {
			break
		}
		out, err = c.CreateVolume(ctx, &metav1.CreateVolumeRequest{
			Id: args[1], SizeBytes: size, ChunkBytes: chunk, Replicas: uint32(rf), MinIsr: uint32(min),
			PreferNode:          argOr(args, 6, ""),
			FailureDomain:       argOr(args, 7, ""),
			FailureDomainPolicy: argOr(args, 8, ""),
		})
	case "get":
		if err = need(1); err == nil {
			out, err = c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: args[1]})
		}
	case "attach":
		if err = need(2); err == nil {
			mode := argOr(args, 3, "")
			if mode != "" && mode != "unless-target" && mode != "only-if-target" {
				err = fmt.Errorf("attach mode %q: use unless-target or only-if-target", mode)
				break
			}
			out, err = c.Attach(ctx, &metav1.AttachRequest{
				Volume: args[1], Node: args[2],
				UnlessTarget: mode == "unless-target", OnlyIfTarget: mode == "only-if-target",
			})
		}
	case "delete":
		if err = need(1); err == nil {
			out, err = c.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: args[1]})
		}
	case "volumes":
		out, err = c.ListVolumes(ctx, &metav1.ListVolumesRequest{})
	case "move":
		if err = need(4); err == nil {
			var chunk uint64
			if chunk, err = strconv.ParseUint(args[2], 10, 32); err == nil {
				out, err = c.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: args[1], Chunk: uint32(chunk), From: args[3], To: args[4]})
			}
		}
	case "drain":
		if err = need(1); err == nil {
			start := time.Now()
			err = drain.Run(ctx, c, args[1], 5*time.Second, func(p drain.Progress) {
				fmt.Fprintf(stderr, "%4.0fs %s: %s\n", time.Since(start).Seconds(), args[1], p)
			})
			out = &metav1.DrainNodeResponse{}
		}
	case "undrain":
		if err = need(1); err == nil {
			out, err = c.DrainNode(ctx, &metav1.DrainNodeRequest{Node: args[1], Draining: false})
		}
	case "tombstones":
		// One node's, or every registered node's, each tombstone once.
		var nodes []string
		if len(args) > 1 {
			nodes = []string{args[1]}
		} else {
			var l *metav1.ListNodesResponse
			if l, err = c.ListNodes(ctx, &metav1.ListNodesRequest{}); err != nil {
				break
			}
			for _, n := range l.GetNodes() {
				nodes = append(nodes, n.GetId())
			}
		}
		all := &metav1.ListTombstonesResponse{}
		seen := map[string]bool{}
		for _, n := range nodes {
			var l *metav1.ListTombstonesResponse
			if l, err = c.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: n}); err != nil {
				break
			}
			for _, t := range l.GetTombstones() {
				if !seen[t.GetVolume()] {
					seen[t.GetVolume()] = true
					all.Tombstones = append(all.Tombstones, t)
				}
			}
		}
		slices.SortFunc(all.Tombstones, func(a, b *metav1.Tombstone) int { return strings.Compare(a.GetVolume(), b.GetVolume()) })
		out = all
	case "confirm":
		// Read first: confirm only what the read shows pending for the node. The service's
		// empty answer cannot say more, so what is printed comes from the read.
		var l *metav1.ListTombstonesResponse
		if l, err = c.ListTombstones(ctx, &metav1.ListTombstonesRequest{Node: node}); err != nil {
			break
		}
		t := pendingFor(l, volume)
		if t == nil {
			fmt.Fprintf(stderr, "%s: %s is not pending for %s: nothing to confirm\n", identity, volume, node)
			out = &metav1.Tombstone{}
			break
		}
		fmt.Fprintf(stderr, "%s: confirming %s for %s: tombstone nodes %v, pending %v\n",
			identity, volume, node, t.GetNodes(), t.GetPending())
		if _, err = c.ConfirmDeletion(ctx, &metav1.ConfirmDeletionRequest{Volume: volume, Node: node}); err != nil {
			break
		}
		t.Pending = slices.DeleteFunc(t.Pending, func(n string) bool { return n == node })
		out = t
	default:
		err = fmt.Errorf("unknown command %q", args[0])
	}
	if err != nil {
		return err
	}
	b, err := protojson.MarshalOptions{Multiline: true, EmitUnpopulated: false}.Marshal(out)
	if err != nil {
		return err
	}
	var pretty any
	if json.Unmarshal(b, &pretty) == nil {
		b, _ = json.MarshalIndent(pretty, "", "  ")
	}
	fmt.Fprintln(stdout, string(b))
	return nil
}
