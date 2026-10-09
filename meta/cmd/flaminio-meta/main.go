// Command flaminio-meta runs the metadata service, and talks to it.
//
//	flaminio-meta serve --listen :7400 --etcd http://10.0.0.1:2379,...
//	flaminio-meta [--server host:7400] nodes
//	flaminio-meta [--server host:7400] register <node> <address> [label=value...]
//	flaminio-meta [--server host:7400] heartbeat <node>
//	flaminio-meta [--server host:7400] create <volume> <size> [replicas] [min-isr] [chunk]
//	flaminio-meta [--server host:7400] get <volume>
//	flaminio-meta [--server host:7400] attach <volume> <node>
//	flaminio-meta [--server host:7400] delete <volume>
//
// Sizes take K, M, G and T suffixes (powers of 1024). Not yet: TLS and authentication
// (phase 1 step 3); run it only on a trusted network.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	clientv3 "go.etcd.io/etcd/client/v3"
	"go.uber.org/zap"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/kv"
	"github.com/flaminio/flaminio/meta/service"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "serve" {
		if err := serve(os.Args[2:]); err != nil {
			slog.Error("flaminio-meta", "err", err)
			os.Exit(1)
		}
		return
	}
	if err := client(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "flaminio-meta:", err)
		os.Exit(1)
	}
}

func serve(args []string) error {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	listen := fs.String("listen", ":7400", "gRPC listen address")
	endpoints := fs.String("etcd", "http://127.0.0.1:2379", "comma-separated etcd endpoints")
	ttl := fs.Duration("liveness-ttl", 10*time.Second, "how long a node stays alive after a heartbeat")
	_ = fs.Parse(args)

	c, err := clientv3.New(clientv3.Config{
		Endpoints:   strings.Split(*endpoints, ","),
		DialTimeout: 5 * time.Second,
		Logger:      zap.NewNop(),
	})
	if err != nil {
		return err
	}
	store := kv.NewEtcd(c)
	defer store.Close()

	lis, err := net.Listen("tcp", *listen)
	if err != nil {
		return err
	}
	srv := grpc.NewServer()
	metav1.RegisterMetadataServiceServer(srv, service.New(store, *ttl))
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		srv.GracefulStop()
	}()
	slog.Info("flaminio-meta serving", "listen", lis.Addr().String(), "etcd", *endpoints)
	return srv.Serve(lis)
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

func client(args []string) error {
	fs := flag.NewFlagSet("flaminio-meta", flag.ExitOnError)
	server := fs.String("server", "127.0.0.1:7400", "metadata service address")
	_ = fs.Parse(args)
	args = fs.Args()
	if len(args) == 0 {
		return fmt.Errorf("usage: flaminio-meta serve|nodes|register|heartbeat|create|get|attach|delete ...")
	}
	conn, err := grpc.NewClient(*server, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return err
	}
	defer conn.Close()
	c := metav1.NewMetadataServiceClient(conn)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
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
		})
	case "get":
		if err = need(1); err == nil {
			out, err = c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: args[1]})
		}
	case "attach":
		if err = need(2); err == nil {
			out, err = c.Attach(ctx, &metav1.AttachRequest{Volume: args[1], Node: args[2]})
		}
	case "delete":
		if err = need(1); err == nil {
			out, err = c.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: args[1]})
		}
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
	fmt.Println(string(b))
	return nil
}
