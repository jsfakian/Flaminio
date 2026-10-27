// Command flaminio-csi is Flaminio's CSI driver.
//
//	flaminio-csi controller --meta host:7400,... --cert c.crt --key c.key --ca ca.crt
//	flaminio-csi node --node-id fl-a --meta ... --cert agent.crt --key agent.key --ca ca.crt
//	flaminio-csi failover --meta ... --cert c.crt --key c.key --ca ca.crt [--grace 40s]
//
// The controller authenticates to the metadata service as an administrator identity
// (.../flaminio/admin/<name>); the node plugin as its node's agent (.../flaminio/node/<node>/agent),
// which may only restart targets the metadata service already places on that node. The
// failover controller (control/failover) reads node liveness from the metadata service and
// marks lost nodes out of service in Kubernetes, as its pod's service account.
package main

import (
	"context"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/flaminio/flaminio/control/csi"
	"github.com/flaminio/flaminio/control/failover"
	"github.com/flaminio/flaminio/pkg/pki"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		slog.Error("flaminio-csi", "error", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 || (args[0] != "controller" && args[0] != "node" && args[0] != "failover") {
		return fmt.Errorf("usage: flaminio-csi controller|node|failover [flags]")
	}
	mode := args[0]
	fs := flag.NewFlagSet(mode, flag.ExitOnError)
	endpoint := fs.String("endpoint", "unix:///csi/csi.sock", "CSI endpoint")
	metaAddrs := fs.String("meta", "", "metadata service instances, host:port,...")
	var files pki.Files
	fs.StringVar(&files.Cert, "cert", "", "certificate for the metadata service")
	fs.StringVar(&files.Key, "key", "", "its key")
	fs.StringVar(&files.CA, "ca", "", "CA bundle")
	nodeID := fs.String("node-id", "", "node: this node's name")
	control := fs.String("control-socket", "/run/flaminio/node.sock", "node: the data plane's control socket")
	grace := fs.Duration("grace", 40*time.Second, "failover: how long a node must have been not Ready")
	interval := fs.Duration("interval", 5*time.Second, "failover: how often to look")
	_ = fs.Parse(args[1:])

	if *metaAddrs == "" || files.Cert == "" || files.Key == "" || files.CA == "" {
		return fmt.Errorf("--meta, --cert, --key and --ca are required")
	}
	meta, err := csi.DialMeta(strings.Split(*metaAddrs, ","), files)
	if err != nil {
		return err
	}
	defer meta.Close()

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	switch mode {
	case "controller":
		return csi.Serve(ctx, *endpoint, csi.Identity{Controller: true}, &csi.Controller{Meta: meta}, nil)
	case "failover":
		kube, err := failover.InCluster()
		if err != nil {
			return err
		}
		c := &failover.Controller{Kube: kube, Liveness: liveness{meta}, Grace: *grace, Now: time.Now}
		slog.Info("failover controller", "grace", *grace, "interval", *interval)
		c.Run(ctx, *interval)
		return nil
	}
	if *nodeID == "" {
		return fmt.Errorf("--node-id is required")
	}
	node := &csi.Node{
		NodeID:    *nodeID,
		Meta:      meta,
		Dataplane: csi.ControlSocket{Path: *control},
		Mounter:   csi.HostMounter{},
	}
	// The CSI node plugin is the node's agent for now: copy zone/rack from the Kubernetes
	// Node into the metadata service so placement can spread across failure domains.
	if kube, err := failover.InCluster(); err == nil {
		go csi.SyncTopologyLabels(ctx, meta, kube, *nodeID)
	} else {
		slog.Debug("topology labels: not in cluster", "error", err)
	}
	return csi.Serve(ctx, *endpoint, csi.Identity{}, nil, node)
}

// liveness is the metadata service's view of which nodes are alive.
type liveness struct{ meta *csi.Meta }

func (l liveness) Alive(ctx context.Context) (map[string]bool, error) {
	nodes, err := l.meta.ListNodes(ctx)
	if err != nil {
		return nil, err
	}
	out := make(map[string]bool, len(nodes))
	for _, n := range nodes {
		out[n.GetId()] = n.GetAlive()
	}
	return out, nil
}
