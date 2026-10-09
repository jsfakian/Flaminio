package service

import (
	"context"
	"crypto/ed25519"
	"strings"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/pkg/pki"
)

// Options configure the service.
type Options struct {
	// Nodes stay alive this long after a heartbeat.
	TTL time.Duration
	// Callers must present a certificate with an identity in this trust domain. Empty turns
	// authentication off: for tests and explicitly insecure development servers only.
	TrustDomain string
	// Signs capability tokens. Nil: Attach issues none.
	TokenKey ed25519.PrivateKey
}

type identityKey struct{}

// callerOf returns the authenticated caller, if authentication is on.
func callerOf(ctx context.Context) (pki.Identity, bool) {
	id, ok := ctx.Value(identityKey{}).(pki.Identity)
	return id, ok
}

// Authorize is the gRPC interceptor that authenticates the caller from its TLS certificate and
// applies each method's caller rule (see meta.proto). ChangeIsr's rule, that the caller is the
// volume's current target, needs the volume, so the method checks it itself.
func (s *Service) Authorize(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
	if s.opts.TrustDomain == "" {
		return handler(ctx, req)
	}
	p, ok := peer.FromContext(ctx)
	if !ok {
		return nil, status.Error(codes.Unauthenticated, "no peer")
	}
	tlsInfo, ok := p.AuthInfo.(credentials.TLSInfo)
	if !ok {
		return nil, status.Error(codes.Unauthenticated, "not a TLS connection")
	}
	id, err := pki.PeerIdentity(tlsInfo.State, s.opts.TrustDomain)
	if err != nil {
		return nil, status.Error(codes.Unauthenticated, err.Error())
	}
	admin := id.Admin != ""
	isNode := func(node string) bool {
		return node != "" && id.Node == node && (id.Component == "agent" || id.Component == "io")
	}
	method := info.FullMethod[strings.LastIndex(info.FullMethod, "/")+1:]
	var allowed bool
	switch method {
	case "CreateVolume", "DeleteVolume":
		allowed = admin
	case "Attach":
		// A node's agent restarts its own target under a new epoch; the service checks that
		// it is the target.
		r := req.(*metav1.AttachRequest)
		allowed = admin || (r.GetOnlyIfTarget() && id.Node == r.GetNode() && id.Component == "agent")
	case "RegisterNode":
		allowed = admin || isNode(req.(*metav1.RegisterNodeRequest).GetNode().GetId())
	case "NodeHeartbeat":
		allowed = admin || isNode(req.(*metav1.NodeHeartbeatRequest).GetNode())
	case "GetVolume", "ListNodes":
		allowed = true
	case "ChangeIsr":
		allowed = id.Node != "" && id.Component == "io"
	}
	if !allowed {
		return nil, status.Errorf(codes.PermissionDenied, "%s may not call %s", id, method)
	}
	return handler(context.WithValue(ctx, identityKey{}, id), req)
}
