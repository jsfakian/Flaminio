// Package csi is Flaminio's CSI driver. The controller creates, deletes and publishes volumes
// in the metadata service. The node plugin starts a volume's target on its node through the
// data plane's control socket, then formats and mounts the device.
package csi

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os"
	"strings"

	"github.com/container-storage-interface/spec/lib/go/csi"
	"google.golang.org/grpc"
)

// Serve runs the given services on a CSI endpoint (unix:///path) until ctx ends. The
// sidecars and the kubelet are the only callers; the socket is only reachable inside the pod
// and through the kubelet's plugin directory.
func Serve(ctx context.Context, endpoint string, id Identity, ctrl *Controller, node *Node) error {
	path, ok := strings.CutPrefix(endpoint, "unix://")
	if !ok {
		return fmt.Errorf("endpoint %q: unix:///path only", endpoint)
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	lis, err := net.Listen("unix", path)
	if err != nil {
		return err
	}
	srv := grpc.NewServer(grpc.UnaryInterceptor(logCalls))
	csi.RegisterIdentityServer(srv, id)
	if ctrl != nil {
		csi.RegisterControllerServer(srv, ctrl)
	}
	if node != nil {
		csi.RegisterNodeServer(srv, node)
	}
	go func() {
		<-ctx.Done()
		srv.GracefulStop()
	}()
	slog.Info("serving", "driver", DriverName, "version", Version, "endpoint", endpoint)
	return srv.Serve(lis)
}

func logCalls(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
	method := info.FullMethod[strings.LastIndex(info.FullMethod, "/")+1:]
	quiet := method == "Probe" || method == "NodeGetCapabilities" || method == "GetPluginInfo"
	resp, err := handler(ctx, req)
	switch {
	case err != nil:
		slog.Warn(method, "request", req, "error", err)
	case !quiet:
		slog.Info(method, "request", req)
	}
	return resp, err
}
