// Package kvtest starts a throwaway etcd for tests.
package kvtest

import (
	"context"
	"net/url"
	"testing"
	"time"

	pb "go.etcd.io/etcd/api/v3/etcdserverpb"
	clientv3 "go.etcd.io/etcd/client/v3"
	"go.etcd.io/etcd/server/v3/embed"
	"go.uber.org/zap"

	"github.com/flaminio/flaminio/meta/kv"
)

// Etcd runs a single-member etcd in a temporary directory for the test, and returns a KV on it.
func Etcd(t *testing.T) *kv.Etcd {
	t.Helper()
	_, c := startEtcd(t)
	return kv.NewEtcd(c)
}

// Cluster is an embedded etcd plus its KV wrapper (docs/issues/76 NOSPACE proofs).
type Cluster struct {
	KV     *kv.Etcd
	Client *clientv3.Client
	Embed  *embed.Etcd
}

// Start runs a single-member etcd and returns the cluster handle.
func Start(t *testing.T) *Cluster {
	t.Helper()
	e, c := startEtcd(t)
	return &Cluster{KV: kv.NewEtcd(c), Client: c, Embed: e}
}

// ActivateNoSpace raises etcd's NOSPACE alarm so Put-containing transactions fail
// while delete-only transactions still apply (etcdserver/apply/capped.go).
func (cl *Cluster) ActivateNoSpace(t *testing.T) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ml, err := cl.Client.MemberList(ctx)
	if err != nil || len(ml.Members) == 0 {
		t.Fatalf("MemberList: %v members=%d", err, len(ml.Members))
	}
	mc := pb.NewMaintenanceClient(cl.Client.ActiveConnection())
	if _, err := mc.Alarm(ctx, &pb.AlarmRequest{
		Action:   pb.AlarmRequest_ACTIVATE,
		MemberID: ml.Members[0].ID,
		Alarm:    pb.AlarmType_NOSPACE,
	}); err != nil {
		t.Fatalf("activate NOSPACE: %v", err)
	}
}

func startEtcd(t *testing.T) (*embed.Etcd, *clientv3.Client) {
	t.Helper()
	cfg := embed.NewConfig()
	cfg.Dir = t.TempDir()
	cfg.ZapLoggerBuilder = embed.NewZapLoggerBuilder(zap.NewNop())
	client, _ := url.Parse("http://127.0.0.1:0")
	peer, _ := url.Parse("http://127.0.0.1:0")
	cfg.ListenClientUrls = []url.URL{*client}
	cfg.AdvertiseClientUrls = []url.URL{*client}
	cfg.ListenPeerUrls = []url.URL{*peer}
	cfg.AdvertisePeerUrls = []url.URL{*peer}
	cfg.InitialCluster = cfg.Name + "=" + peer.String()
	e, err := embed.StartEtcd(cfg)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(e.Close)
	select {
	case <-e.Server.ReadyNotify():
	case <-time.After(30 * time.Second):
		t.Fatal("etcd did not start")
	}
	c, err := clientv3.New(clientv3.Config{
		Endpoints:   []string{e.Clients[0].Addr().String()},
		DialTimeout: 5 * time.Second,
	})
	if err != nil {
		t.Fatal(err)
	}
	// Caller closes the client via kv.Etcd.Close (or Cluster.KV.Close).
	return e, c
}
