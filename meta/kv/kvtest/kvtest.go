// Package kvtest starts a throwaway etcd for tests.
package kvtest

import (
	"net/url"
	"testing"
	"time"

	clientv3 "go.etcd.io/etcd/client/v3"
	"go.etcd.io/etcd/server/v3/embed"
	"go.uber.org/zap"

	"github.com/flaminio/flaminio/meta/kv"
)

// Etcd runs a single-member etcd in a temporary directory for the test, and returns a KV on it.
func Etcd(t *testing.T) *kv.Etcd { return kv.NewEtcd(startEtcd(t)) }

func startEtcd(t *testing.T) *clientv3.Client {
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
	return c
}
