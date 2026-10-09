package csi

import (
	"context"
	"errors"
	"fmt"
	"sync/atomic"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/pkg/pki"
)

// attemptTimeout bounds one call on one endpoint: an instance cut off by a partition does not
// refuse, it just never answers.
const attemptTimeout = 10 * time.Second

// Meta is a client of the metadata service that tries each instance in turn, starting with
// the last one that answered.
type Meta struct {
	conns     []*grpc.ClientConn
	clients   []metav1.MetadataServiceClient
	preferred atomic.Int32
}

// DialMeta connects (lazily) to the given instances over mutual TLS, as the identity in files.
func DialMeta(endpoints []string, files pki.Files) (*Meta, error) {
	cfg, err := files.ClientConfig()
	if err != nil {
		return nil, err
	}
	return dialMeta(endpoints, grpc.WithTransportCredentials(credentials.NewTLS(cfg)))
}

func dialMeta(endpoints []string, opts ...grpc.DialOption) (*Meta, error) {
	if len(endpoints) == 0 {
		return nil, errors.New("no metadata service endpoints")
	}
	m := &Meta{}
	for _, e := range endpoints {
		c, err := grpc.NewClient(e, opts...)
		if err != nil {
			m.Close()
			return nil, fmt.Errorf("%s: %w", e, err)
		}
		m.conns = append(m.conns, c)
		m.clients = append(m.clients, metav1.NewMetadataServiceClient(c))
	}
	return m, nil
}

func (m *Meta) Close() {
	for _, c := range m.conns {
		_ = c.Close()
	}
}

// metaCall makes one call, moving on to the next instance when one does not answer.
func metaCall[T any](ctx context.Context, m *Meta, f func(context.Context, metav1.MetadataServiceClient) (T, error)) (T, error) {
	var (
		v   T
		err error
	)
	first := int(m.preferred.Load())
	for k := range m.clients {
		i := (first + k) % len(m.clients)
		actx, cancel := context.WithTimeout(ctx, attemptTimeout)
		v, err = f(actx, m.clients[i])
		cancel()
		switch status.Code(err) {
		case codes.Unavailable, codes.DeadlineExceeded, codes.Canceled:
			if ctx.Err() != nil {
				return v, err
			}
			continue
		}
		m.preferred.Store(int32(i))
		return v, err
	}
	return v, err
}

func (m *Meta) CreateVolume(ctx context.Context, r *metav1.CreateVolumeRequest) (*metav1.CreateVolumeResponse, error) {
	return metaCall(ctx, m, func(ctx context.Context, c metav1.MetadataServiceClient) (*metav1.CreateVolumeResponse, error) {
		return c.CreateVolume(ctx, r)
	})
}

func (m *Meta) GetVolume(ctx context.Context, id string) (*metav1.Volume, error) {
	r, err := metaCall(ctx, m, func(ctx context.Context, c metav1.MetadataServiceClient) (*metav1.GetVolumeResponse, error) {
		return c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: id})
	})
	return r.GetVolume(), err
}

func (m *Meta) DeleteVolume(ctx context.Context, id string) error {
	_, err := metaCall(ctx, m, func(ctx context.Context, c metav1.MetadataServiceClient) (*metav1.DeleteVolumeResponse, error) {
		return c.DeleteVolume(ctx, &metav1.DeleteVolumeRequest{Id: id})
	})
	return err
}

func (m *Meta) Attach(ctx context.Context, r *metav1.AttachRequest) (*metav1.AttachResponse, error) {
	return metaCall(ctx, m, func(ctx context.Context, c metav1.MetadataServiceClient) (*metav1.AttachResponse, error) {
		return c.Attach(ctx, r)
	})
}

func (m *Meta) ListNodes(ctx context.Context) ([]*metav1.Node, error) {
	r, err := metaCall(ctx, m, func(ctx context.Context, c metav1.MetadataServiceClient) (*metav1.ListNodesResponse, error) {
		return c.ListNodes(ctx, &metav1.ListNodesRequest{})
	})
	return r.GetNodes(), err
}
