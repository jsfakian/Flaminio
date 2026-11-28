package csi

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"os"
	"path/filepath"
	"sync"

	"github.com/container-storage-interface/spec/lib/go/csi"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/pkg/captoken"
)

// Node is the CSI node service. It runs on each data node, beside flaminio-io node.
type Node struct {
	csi.UnimplementedNodeServer
	// NodeID is the node's name in the metadata service (and in Kubernetes).
	NodeID string
	// Meta authenticates as the node's agent (.../node/<node>/agent).
	Meta      *Meta
	Dataplane Dataplane
	Mounter   Mounter

	// One operation at a time: they share the data plane's single attachment.
	mu sync.Mutex
	// Retain an issued epoch across lost socket replies and kubelet retries. The daemon
	// revalidates the exact lifetime on every use; a new process challenge replaces it.
	attachment *targetCredentials
}

type targetCredentials struct {
	volume                string
	nonce, token, handoff []byte
}

func (n *Node) NodeGetInfo(context.Context, *csi.NodeGetInfoRequest) (*csi.NodeGetInfoResponse, error) {
	// A node serves one attached volume at a time in this version (docs/followups.md, item 5);
	// the scheduler places pods accordingly.
	return &csi.NodeGetInfoResponse{
		NodeId:             n.NodeID,
		MaxVolumesPerNode:  1,
		AccessibleTopology: &csi.Topology{Segments: map[string]string{TopologyKey: n.NodeID}},
	}, nil
}

func (n *Node) NodeGetCapabilities(context.Context, *csi.NodeGetCapabilitiesRequest) (*csi.NodeGetCapabilitiesResponse, error) {
	var caps []*csi.NodeServiceCapability
	for _, t := range []csi.NodeServiceCapability_RPC_Type{
		csi.NodeServiceCapability_RPC_STAGE_UNSTAGE_VOLUME,
		csi.NodeServiceCapability_RPC_SINGLE_NODE_MULTI_WRITER,
	} {
		caps = append(caps, &csi.NodeServiceCapability{Type: &csi.NodeServiceCapability_Rpc{
			Rpc: &csi.NodeServiceCapability_RPC{Type: t},
		}})
	}
	return &csi.NodeGetCapabilitiesResponse{Capabilities: caps}, nil
}

// device returns the volume's device on this node, starting its target if needed. Every start
// is under a new epoch, which only this node's agent may take, and only while the metadata
// service names this node the target (ControllerPublishVolume did). Scalar status never
// authorizes adoption: even an existing device is revalidated with its retained credentials.
func (n *Node) device(ctx context.Context, volume string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, DefaultControlTimeout)
	defer cancel()
	if !domain.ValidID(volume) {
		return "", status.Error(codes.InvalidArgument, "invalid volume id")
	}
	nonce, err := n.Dataplane.AttachChallenge(ctx)
	if err != nil || !captoken.ValidTargetNonce(nonce) {
		return "", status.Error(codes.Unavailable, "healthy data plane attach challenge required")
	}
	attached, _, err := n.Dataplane.Status(ctx)
	if err != nil {
		return "", status.Error(codes.Unavailable, err.Error())
	}
	switch attached {
	case volume, "":
	default:
		return "", status.Errorf(codes.ResourceExhausted, "this node serves %s; one volume per node in this version", attached)
	}
	a := n.attachment
	if a == nil || a.volume != volume || !bytes.Equal(a.nonce, nonce) {
		r, err := n.Meta.Attach(ctx, &metav1.AttachRequest{Volume: volume, Node: n.NodeID, OnlyIfTarget: true, TargetNonce: nonce})
		if err != nil {
			return "", err
		}
		token, handoff := r.GetToken(), r.GetTargetHandoff()
		// Detect an old server or malformed result before sending any target-start command.
		// The daemon remains the signature/current-metadata authority.
		digest := sha256.Sum256(token)
		if len(token) == 0 || base64.StdEncoding.EncodedLen(len(token)) > 4096 || len(handoff) != captoken.HandoffLen || string(handoff[:6]) != "FLCH\x01\x01" ||
			!bytes.Equal(handoff[6:38], nonce) || !bytes.Equal(handoff[38:70], digest[:]) {
			return "", status.Error(codes.FailedPrecondition, "metadata server did not return a process-bound target handoff")
		}
		a = &targetCredentials{volume: volume, nonce: bytes.Clone(nonce), token: bytes.Clone(token), handoff: bytes.Clone(handoff)}
		n.attachment = a
	}
	dev, err := n.Dataplane.Attach(ctx, volume, a.token, a.handoff)
	if err != nil {
		// A definitive refusal ends this proof's usefulness. Transport/busy failures keep
		// it, including across a later kubelet retry, because the start may have succeeded.
		var reply controlError
		if errors.As(err, &reply) && !retryAttach(err) {
			n.attachment = nil
		}
		return "", status.Error(codes.Internal, err.Error())
	}
	return dev, nil
}

func (n *Node) NodeStageVolume(ctx context.Context, req *csi.NodeStageVolumeRequest) (*csi.NodeStageVolumeResponse, error) {
	volume, staging, vc := req.GetVolumeId(), req.GetStagingTargetPath(), req.GetVolumeCapability()
	if volume == "" || staging == "" {
		return nil, status.Error(codes.InvalidArgument, "volume id and staging path are required")
	}
	if err := checkCapabilities([]*csi.VolumeCapability{vc}); err != nil {
		return nil, err
	}
	n.mu.Lock()
	defer n.mu.Unlock()
	dev, err := n.device(ctx, volume)
	if err != nil {
		return nil, err
	}
	if vc.GetBlock() != nil {
		return &csi.NodeStageVolumeResponse{}, nil
	}
	if mounted, err := n.Mounter.IsMountPoint(staging); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	} else if mounted {
		return &csi.NodeStageVolumeResponse{}, nil
	}
	fsType := vc.GetMount().GetFsType()
	if fsType == "" {
		fsType = "ext4"
	}
	// Format only a device without any signature; never a device that holds something else.
	found, err := n.Mounter.FSType(ctx, dev)
	if err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	switch found {
	case "":
		if err := n.Mounter.Format(ctx, dev, fsType); err != nil {
			return nil, status.Error(codes.Internal, err.Error())
		}
	case fsType:
	default:
		return nil, status.Errorf(codes.FailedPrecondition, "%s holds %s, not %s: not formatting it", volume, found, fsType)
	}
	if err := os.MkdirAll(staging, 0o750); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	if err := n.Mounter.Mount(ctx, dev, staging, fsType, vc.GetMount().GetMountFlags()); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	return &csi.NodeStageVolumeResponse{}, nil
}

func (n *Node) NodeUnstageVolume(ctx context.Context, req *csi.NodeUnstageVolumeRequest) (*csi.NodeUnstageVolumeResponse, error) {
	volume, staging := req.GetVolumeId(), req.GetStagingTargetPath()
	if volume == "" || staging == "" {
		return nil, status.Error(codes.InvalidArgument, "volume id and staging path are required")
	}
	n.mu.Lock()
	defer n.mu.Unlock()
	if err := n.unmount(ctx, staging); err != nil {
		return nil, err
	}
	if err := n.Dataplane.Detach(ctx, volume); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	if n.attachment != nil && n.attachment.volume == volume {
		n.attachment = nil
	}
	return &csi.NodeUnstageVolumeResponse{}, nil
}

func (n *Node) unmount(ctx context.Context, target string) error {
	mounted, err := n.Mounter.IsMountPoint(target)
	if err != nil {
		return status.Error(codes.Internal, err.Error())
	}
	if mounted {
		if err := n.Mounter.Unmount(ctx, target); err != nil {
			return status.Error(codes.Internal, err.Error())
		}
	}
	return nil
}

func (n *Node) NodePublishVolume(ctx context.Context, req *csi.NodePublishVolumeRequest) (*csi.NodePublishVolumeResponse, error) {
	volume, staging, target, vc := req.GetVolumeId(), req.GetStagingTargetPath(), req.GetTargetPath(), req.GetVolumeCapability()
	if volume == "" || staging == "" || target == "" {
		return nil, status.Error(codes.InvalidArgument, "volume id, staging and target paths are required")
	}
	if err := checkCapabilities([]*csi.VolumeCapability{vc}); err != nil {
		return nil, err
	}
	n.mu.Lock()
	defer n.mu.Unlock()
	if mounted, err := n.Mounter.IsMountPoint(target); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	} else if mounted {
		return &csi.NodePublishVolumeResponse{}, nil
	}
	options := []string{"bind"}
	if req.GetReadonly() {
		options = append(options, "ro")
	}
	source := staging
	if vc.GetBlock() != nil {
		attached, dev, err := n.Dataplane.Status(ctx)
		if err != nil {
			return nil, status.Error(codes.Unavailable, err.Error())
		}
		if attached != volume {
			return nil, status.Errorf(codes.FailedPrecondition, "%s is not staged on this node", volume)
		}
		source = dev
		// A raw block volume is a device node bind-mounted on a file.
		if err := os.MkdirAll(filepath.Dir(target), 0o750); err != nil {
			return nil, status.Error(codes.Internal, err.Error())
		}
		f, err := os.OpenFile(target, os.O_CREATE, 0o600)
		if err != nil {
			return nil, status.Error(codes.Internal, err.Error())
		}
		_ = f.Close()
	} else if err := os.MkdirAll(target, 0o750); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	if err := n.Mounter.Mount(ctx, source, target, "", options); err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	return &csi.NodePublishVolumeResponse{}, nil
}

func (n *Node) NodeUnpublishVolume(ctx context.Context, req *csi.NodeUnpublishVolumeRequest) (*csi.NodeUnpublishVolumeResponse, error) {
	if req.GetVolumeId() == "" || req.GetTargetPath() == "" {
		return nil, status.Error(codes.InvalidArgument, "volume id and target path are required")
	}
	n.mu.Lock()
	defer n.mu.Unlock()
	if err := n.unmount(ctx, req.GetTargetPath()); err != nil {
		return nil, err
	}
	if err := os.Remove(req.GetTargetPath()); err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, status.Error(codes.Internal, err.Error())
	}
	return &csi.NodeUnpublishVolumeResponse{}, nil
}
