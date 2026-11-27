package csi

import (
	"context"
	"fmt"
	"log/slog"
	"math"
	"strconv"
	"strings"

	"github.com/container-storage-interface/spec/lib/go/csi"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/wrapperspb"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"github.com/flaminio/flaminio/pkg/geometry"
)

// DriverName is the CSI driver's name, as StorageClasses and CSIDriver objects name it.
const DriverName = "csi.flaminio.io"

// Version is set at build time.
var Version = "dev"

// Identity is the CSI identity service.
type Identity struct {
	csi.UnimplementedIdentityServer
	Controller bool
}

func (Identity) GetPluginInfo(context.Context, *csi.GetPluginInfoRequest) (*csi.GetPluginInfoResponse, error) {
	return &csi.GetPluginInfoResponse{Name: DriverName, VendorVersion: Version}, nil
}

func (i Identity) GetPluginCapabilities(context.Context, *csi.GetPluginCapabilitiesRequest) (*csi.GetPluginCapabilitiesResponse, error) {
	// Topology tells the controller which node a new volume's first pod will run on
	// (WaitForFirstConsumer), so that node gets a replica of every chunk.
	caps := []*csi.PluginCapability{{Type: &csi.PluginCapability_Service_{
		Service: &csi.PluginCapability_Service{Type: csi.PluginCapability_Service_VOLUME_ACCESSIBILITY_CONSTRAINTS},
	}}}
	if i.Controller {
		caps = append(caps, &csi.PluginCapability{Type: &csi.PluginCapability_Service_{
			Service: &csi.PluginCapability_Service{Type: csi.PluginCapability_Service_CONTROLLER_SERVICE},
		}})
	}
	return &csi.GetPluginCapabilitiesResponse{Capabilities: caps}, nil
}

func (Identity) Probe(context.Context, *csi.ProbeRequest) (*csi.ProbeResponse, error) {
	return &csi.ProbeResponse{Ready: wrapperspb.Bool(true)}, nil
}

// TopologyKey is the topology segment each node reports: its own name. Volumes are reachable
// from every node; the segment only says where a new volume's pod will run.
const TopologyKey = "flaminio.io/node"

// preferredNode is the node Kubernetes picked for the volume's first pod, if any.
func preferredNode(t *csi.TopologyRequirement) string {
	for _, list := range [][]*csi.Topology{t.GetPreferred(), t.GetRequisite()} {
		for _, top := range list {
			if n := top.GetSegments()[TopologyKey]; n != "" {
				return n
			}
		}
	}
	return ""
}

// StorageClass parameters, and their defaults.
const (
	ParamReplicas            = "replicas"            // default 2
	ParamMinISR              = "minIsr"              // default 1
	ParamChunkSize           = "chunkSize"           // default 256Mi; a power of two from 1Mi through 1Gi
	ParamFailureDomain       = "failureDomain"       // default node
	ParamFailureDomainPolicy = "failureDomainPolicy" // default strict
)

const (
	defaultSize = 1 << 30
)

// Controller is the CSI controller service: volumes are created, deleted and moved between
// nodes in the metadata service, as an administrator identity.
type Controller struct {
	csi.UnimplementedControllerServer
	Meta *Meta
}

func (c *Controller) ControllerGetCapabilities(context.Context, *csi.ControllerGetCapabilitiesRequest) (*csi.ControllerGetCapabilitiesResponse, error) {
	var caps []*csi.ControllerServiceCapability
	for _, t := range []csi.ControllerServiceCapability_RPC_Type{
		csi.ControllerServiceCapability_RPC_CREATE_DELETE_VOLUME,
		csi.ControllerServiceCapability_RPC_PUBLISH_UNPUBLISH_VOLUME,
		csi.ControllerServiceCapability_RPC_SINGLE_NODE_MULTI_WRITER,
	} {
		caps = append(caps, &csi.ControllerServiceCapability{Type: &csi.ControllerServiceCapability_Rpc{
			Rpc: &csi.ControllerServiceCapability_RPC{Type: t},
		}})
	}
	return &csi.ControllerGetCapabilitiesResponse{Capabilities: caps}, nil
}

// checkCapabilities accepts single-node access, as a block device or a filesystem. A volume
// has one target at a time, on one node.
func checkCapabilities(caps []*csi.VolumeCapability) error {
	if len(caps) == 0 {
		return status.Error(codes.InvalidArgument, "no volume capabilities")
	}
	for _, c := range caps {
		switch c.GetAccessMode().GetMode() {
		case csi.VolumeCapability_AccessMode_SINGLE_NODE_WRITER,
			csi.VolumeCapability_AccessMode_SINGLE_NODE_READER_ONLY,
			csi.VolumeCapability_AccessMode_SINGLE_NODE_SINGLE_WRITER,
			csi.VolumeCapability_AccessMode_SINGLE_NODE_MULTI_WRITER:
		default:
			return status.Errorf(codes.InvalidArgument, "access mode %s: a volume is served from one node at a time", c.GetAccessMode().GetMode())
		}
		switch {
		case c.GetBlock() != nil:
		case c.GetMount() != nil:
			if fs := c.GetMount().GetFsType(); fs != "" && fs != "ext4" {
				return status.Errorf(codes.InvalidArgument, "filesystem %q: ext4 only, for now", fs)
			}
		default:
			return status.Error(codes.InvalidArgument, "access type: block or mount")
		}
	}
	return nil
}

type volumeParams struct {
	replicas, minISR uint32
	chunk            uint64
	domain, policy   string
}

func parseParams(p map[string]string) (volumeParams, error) {
	out := volumeParams{replicas: 2, minISR: 1, chunk: 256 << 20}
	for k, v := range p {
		var err error
		switch k {
		case ParamReplicas:
			out.replicas, err = parseUint32(v)
		case ParamMinISR:
			out.minISR, err = parseUint32(v)
		case ParamChunkSize:
			out.chunk, err = parseQuantity(v)
		case ParamFailureDomain:
			_, err = parseFailureDomain(v)
			if err == nil {
				out.domain = v
			}
		case ParamFailureDomainPolicy:
			_, err = parseFailureDomainPolicy(v)
			if err == nil {
				out.policy = v
			}
		default:
			// The provisioner's own keys are its business; anything else is a typo.
			if strings.HasPrefix(k, "csi.storage.k8s.io/") {
				continue
			}
			err = fmt.Errorf("unknown parameter")
		}
		if err != nil {
			return out, status.Errorf(codes.InvalidArgument, "parameter %s=%q: %v", k, v, err)
		}
	}
	if err := geometry.Parameters(out.chunk, out.replicas, out.minISR); err != nil {
		return out, status.Errorf(codes.InvalidArgument, "parameters: %v", err)
	}
	return out, nil
}

func parseFailureDomain(s string) (string, error) {
	switch s {
	case "node", "rack", "zone":
		return s, nil
	default:
		return "", fmt.Errorf("want node, rack or zone")
	}
}

func parseFailureDomainPolicy(s string) (string, error) {
	switch s {
	case "strict", "bestEffort":
		return s, nil
	default:
		return "", fmt.Errorf("want strict or bestEffort")
	}
}

func parseUint32(s string) (uint32, error) {
	n, err := strconv.ParseUint(s, 10, 32)
	return uint32(n), err
}

// parseQuantity reads sizes such as 256Mi, 1Gi or 1048576.
func parseQuantity(s string) (uint64, error) {
	units := []struct {
		suffix string
		mult   uint64
	}{{"Ki", 1 << 10}, {"Mi", 1 << 20}, {"Gi", 1 << 30}, {"Ti", 1 << 40}}
	for _, u := range units {
		if n, ok := strings.CutSuffix(s, u.suffix); ok {
			v, err := strconv.ParseUint(n, 10, 64)
			if err != nil {
				return 0, err
			}
			if v > math.MaxUint64/u.mult {
				return 0, fmt.Errorf("quantity overflows uint64")
			}
			return v * u.mult, nil
		}
	}
	return strconv.ParseUint(s, 10, 64)
}

func (c *Controller) CreateVolume(ctx context.Context, req *csi.CreateVolumeRequest) (*csi.CreateVolumeResponse, error) {
	if req.GetName() == "" {
		return nil, status.Error(codes.InvalidArgument, "no name")
	}
	if req.GetVolumeContentSource() != nil {
		return nil, status.Error(codes.InvalidArgument, "volumes from snapshots or clones are not supported yet")
	}
	if err := checkCapabilities(req.GetVolumeCapabilities()); err != nil {
		return nil, err
	}
	p, err := parseParams(req.GetParameters())
	if err != nil {
		return nil, err
	}
	required, limit := req.GetCapacityRange().GetRequiredBytes(), req.GetCapacityRange().GetLimitBytes()
	if required < 0 || limit < 0 || limit > 0 && required > limit {
		return nil, status.Error(codes.InvalidArgument, "invalid signed capacity range")
	}
	var size uint64
	if required > 0 {
		n := uint64(required) / p.chunk
		if uint64(required)%p.chunk != 0 {
			n++
		}
		if n > math.MaxInt64/p.chunk {
			return nil, status.Error(codes.OutOfRange, "capacity rounded to chunks overflows int64")
		}
		size = n * p.chunk
	} else {
		size = defaultSize
		if limit > 0 && uint64(limit) < size {
			size = uint64(limit) / p.chunk * p.chunk
		}
	}
	if size < p.chunk || limit > 0 && size > uint64(limit) {
		return nil, status.Error(codes.OutOfRange, "capacity range contains no whole-chunk volume")
	}
	if _, err := geometry.Admit(size, p.chunk, p.replicas, p.minISR); err != nil {
		return nil, status.Errorf(codes.OutOfRange, "unsupported capacity: %v", err)
	}
	desired := &metav1.CreateVolumeRequest{
		Id: req.GetName(), SizeBytes: size, ChunkBytes: p.chunk, Replicas: p.replicas, MinIsr: p.minISR,
		PreferNode:          preferredNode(req.GetAccessibilityRequirements()),
		FailureDomain:       p.domain,
		FailureDomainPolicy: p.policy,
	}
	created, err := c.Meta.CreateVolume(ctx, desired)
	v := created.GetVolume()
	switch status.Code(err) {
	case codes.OK:
	case codes.AlreadyExists:
		checked, err := c.Meta.CheckCreateVolume(ctx, &metav1.CheckCreateVolumeRequest{
			Desired: desired, RequiredBytes: required, LimitBytes: limit,
		})
		if status.Code(err) == codes.Unimplemented {
			return nil, status.Error(codes.FailedPrecondition, "metadata service lacks CheckCreateVolume; coordinated service upgrade is required")
		}
		if err != nil {
			return nil, err
		}
		v = checked.GetVolume()
		if checked.GetLegacyIdentity() {
			slog.WarnContext(ctx, "legacy volume retry has only scalar creation identity", "volume", req.GetName())
		}
	case codes.FailedPrecondition:
		return nil, status.Error(codes.ResourceExhausted, status.Convert(err).Message())
	default:
		return nil, err
	}
	if v.GetId() != req.GetName() || v.GetChunkBytes() != p.chunk || v.GetReplicas() != p.replicas || v.GetMinIsr() != p.minISR {
		return nil, status.Error(codes.Internal, "metadata returned incompatible volume geometry")
	}
	if _, err := geometry.Admit(v.GetSizeBytes(), v.GetChunkBytes(), v.GetReplicas(), v.GetMinIsr()); err != nil {
		return nil, status.Errorf(codes.Internal, "metadata returned unsupported geometry: %v", err)
	}
	if v.GetSizeBytes() > math.MaxInt64 || v.GetSizeBytes() < uint64(required) || limit > 0 && v.GetSizeBytes() > uint64(limit) {
		return nil, status.Error(codes.Internal, "metadata returned capacity outside the signed range")
	}
	// No accessible topology: the volume is reachable from every node, so its pods can move
	// anywhere (CSI: unset means equally accessible from all nodes).
	return &csi.CreateVolumeResponse{Volume: &csi.Volume{
		VolumeId:      req.GetName(),
		CapacityBytes: int64(v.GetSizeBytes()),
	}}, nil
}

func (c *Controller) DeleteVolume(ctx context.Context, req *csi.DeleteVolumeRequest) (*csi.DeleteVolumeResponse, error) {
	if req.GetVolumeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "no volume id")
	}
	if err := c.Meta.DeleteVolume(ctx, req.GetVolumeId()); err != nil && status.Code(err) != codes.NotFound {
		return nil, err
	}
	return &csi.DeleteVolumeResponse{}, nil
}

// ControllerPublishVolume makes the node the volume's target in the metadata service. A
// repeated publish to the current target changes nothing. The node plugin then starts the
// target under an epoch of its own (NodeStageVolume).
func (c *Controller) ControllerPublishVolume(ctx context.Context, req *csi.ControllerPublishVolumeRequest) (*csi.ControllerPublishVolumeResponse, error) {
	if req.GetVolumeId() == "" || req.GetNodeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "volume and node ids are required")
	}
	if err := checkCapabilities([]*csi.VolumeCapability{req.GetVolumeCapability()}); err != nil {
		return nil, err
	}
	r, err := c.Meta.Attach(ctx, &metav1.AttachRequest{Volume: req.GetVolumeId(), Node: req.GetNodeId(), UnlessTarget: true})
	if err != nil {
		return nil, err
	}
	return &csi.ControllerPublishVolumeResponse{
		PublishContext: map[string]string{"epoch": strconv.FormatUint(r.GetVolume().GetEpoch(), 10)},
	}, nil
}

// ControllerUnpublishVolume changes nothing. On a live node, NodeUnstageVolume has already
// stopped the target. On a lost one, nothing can reach it, and nothing needs to: the next
// attach issues a new epoch, which fences the old target on every replica, and its lease runs
// out (docs/replication-protocol.md).
func (c *Controller) ControllerUnpublishVolume(ctx context.Context, req *csi.ControllerUnpublishVolumeRequest) (*csi.ControllerUnpublishVolumeResponse, error) {
	if req.GetVolumeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "no volume id")
	}
	return &csi.ControllerUnpublishVolumeResponse{}, nil
}

func (c *Controller) ValidateVolumeCapabilities(ctx context.Context, req *csi.ValidateVolumeCapabilitiesRequest) (*csi.ValidateVolumeCapabilitiesResponse, error) {
	if req.GetVolumeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "no volume id")
	}
	if _, err := c.Meta.GetVolume(ctx, req.GetVolumeId()); err != nil {
		return nil, err
	}
	if err := checkCapabilities(req.GetVolumeCapabilities()); err != nil {
		return &csi.ValidateVolumeCapabilitiesResponse{Message: status.Convert(err).Message()}, nil
	}
	return &csi.ValidateVolumeCapabilitiesResponse{Confirmed: &csi.ValidateVolumeCapabilitiesResponse_Confirmed{
		VolumeCapabilities: req.GetVolumeCapabilities(),
		Parameters:         req.GetParameters(),
	}}, nil
}
