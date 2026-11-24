// Command flaminio-csi is Flaminio's CSI driver.
//
//	flaminio-csi controller --meta host:7400,... --cert c.crt --key c.key --ca ca.crt
//	flaminio-csi node --node-id fl-a --meta ... --cert agent.crt --key agent.key --ca ca.crt
//	flaminio-csi failover --meta ... --cert c.crt --key c.key --ca ca.crt [--grace 40s]
//	                     [--failover-authority-config PATH]
//	flaminio-csi fence-sign --config PATH --node NAME --node-uid UID --machine-id ID
//	                       --operation-id HEX --locked-at UTC --key-id ID
//	                       --signing-key PATH --output PATH --assert-physical-lockout
//
// The controller authenticates to the metadata service as an administrator identity
// (.../flaminio/admin/<name>); the node plugin as its node's agent (.../flaminio/node/<node>/agent),
// which may only restart targets the metadata service already places on that node. The
// failover controller (control/failover) investigates candidates from node readiness and
// metadata liveness, and marks a node out of service only with verified shutdown evidence
// from an explicitly configured authority (default: observe-only). Scoped partition recovery
// is unavailable. fence-sign mints a manual-lockout-v1 proof for an administrator; it cannot
// verify physical isolation.
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
	if len(args) == 0 {
		return fmt.Errorf("usage: flaminio-csi controller|node|failover|fence-sign [flags]")
	}
	switch args[0] {
	case "fence-sign":
		return runFenceSign(args[1:])
	case "controller", "node", "failover":
	default:
		return fmt.Errorf("usage: flaminio-csi controller|node|failover|fence-sign [flags]")
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
	grace := fs.Duration("grace", 40*time.Second, "failover: how long a node must have been not Ready (candidate timing only)")
	interval := fs.Duration("interval", 5*time.Second, "failover: how often to look")
	authConfig := fs.String("failover-authority-config", "", "failover: authority config path (unset = observe-only)")
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
		return runFailover(ctx, meta, *grace, *interval, *authConfig)
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
	// Node into the metadata service so placement can spread across failure domains, and
	// delete local replicas of volumes the metadata service has tombstoned.
	if kube, err := failover.InCluster(); err == nil {
		go csi.SyncTopologyLabels(ctx, meta, kube, *nodeID)
	} else {
		slog.Debug("topology labels: not in cluster", "error", err)
	}
	go csi.SweepTombstones(ctx, meta, node.Dataplane, *nodeID)
	return csi.Serve(ctx, *endpoint, csi.Identity{}, nil, node)
}

func runFailover(ctx context.Context, meta *csi.Meta, grace, interval time.Duration, authConfig string) error {
	kube, err := failover.InCluster()
	if err != nil {
		return err
	}
	var auth failover.Authority
	if authConfig != "" {
		cfg, err := failover.LoadAuthorityConfig(authConfig)
		if err != nil {
			return fmt.Errorf("failover authority config: %w", err)
		}
		clusterUID, err := kube.KubeSystemUID(ctx)
		if err != nil {
			return fmt.Errorf("cluster uid: %w", err)
		}
		ml, err := failover.NewManualLockout(cfg, clusterUID)
		if err != nil {
			return fmt.Errorf("failover authority: %w", err)
		}
		auth = ml
		kube.Authority = auth
		slog.Info("failover authority configured", "profile", failover.ManualLockoutProfile)
	} else {
		slog.Info("failover observe-only: no --failover-authority-config; candidates will log evidence-unavailable")
	}
	if !failover.ScopedRecoveryAvailable() {
		slog.Info("scoped partition recovery unavailable; refusing pod/VolumeAttachment mutations")
	}
	c := &failover.Controller{
		Kube:      kube,
		Liveness:  liveness{meta},
		Authority: auth,
		Grace:     grace,
		Now:       time.Now,
	}
	slog.Info("failover controller", "grace", grace, "interval", interval, "authorityConfigured", auth != nil)
	c.Run(ctx, interval)
	return nil
}

func runFenceSign(args []string) error {
	fs := flag.NewFlagSet("fence-sign", flag.ExitOnError)
	config := fs.String("config", "", "manual-lockout-v1 authority config")
	node := fs.String("node", "", "Kubernetes node name")
	nodeUID := fs.String("node-uid", "", "Kubernetes node UID")
	machineID := fs.String("machine-id", "", "immutable inventory machine id")
	operationID := fs.String("operation-id", "", "64 lowercase hex operation id")
	lockedAt := fs.String("locked-at", "", "diagnostic UTC timestamp (not an expiry)")
	keyID := fs.String("key-id", "", "key id from config")
	signingKey := fs.String("signing-key", "", "Ed25519 PKCS#8 private key (never mounted in controller pods)")
	output := fs.String("output", "", "output proof path")
	assert := fs.Bool("assert-physical-lockout", false, "required: assert the machine is physically isolated")
	_ = fs.Parse(args)
	if *config == "" || *node == "" || *nodeUID == "" || *machineID == "" || *operationID == "" ||
		*lockedAt == "" || *keyID == "" || *signingKey == "" || *output == "" {
		return fmt.Errorf("fence-sign requires --config --node --node-uid --machine-id --operation-id --locked-at --key-id --signing-key --output")
	}
	cfg, err := failover.LoadAuthorityConfig(*config)
	if err != nil {
		return err
	}
	if err := failover.ValidateFenceSignInputs(cfg, *node, *nodeUID, *machineID, *operationID, *keyID, *lockedAt, *assert); err != nil {
		return err
	}
	priv, err := failover.LoadEd25519PKCS8(*signingKey)
	if err != nil {
		return err
	}
	p := failover.ManualLockoutProof{
		Version:     1,
		Profile:     failover.ManualLockoutProfile,
		ClusterUID:  cfg.ClusterUID,
		NodeName:    *node,
		NodeUID:     *nodeUID,
		MachineID:   *machineID,
		OperationID: *operationID,
		KeyID:       *keyID,
		Assertion:   failover.ManualLockoutAssertion,
		LockedAt:    *lockedAt,
	}
	signed, err := failover.SignManualLockoutProof(p, priv)
	if err != nil {
		return err
	}
	if err := failover.WriteManualLockoutProof(*output, signed); err != nil {
		return err
	}
	fmt.Fprintln(os.Stderr, "wrote signed proof; this command cannot verify physical isolation")
	fmt.Fprintln(os.Stderr, "install the proof as an explicit administrative action; keep the physical lock until terminal UID retirement")
	return nil
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
