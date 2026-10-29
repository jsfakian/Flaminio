// Package drain moves every chunk replica off a node, while its volumes serve I/O.
//
// The node is marked draining (no new volumes are placed on it), and a move is requested for
// each of its chunk replicas, to the least loaded node that may take it. Each volume's target
// carries the moves out under its epoch: it adds the destination, resyncs it, and drops the
// source once the destination is in sync (docs/replication-protocol.md §7, moves). A volume
// without a target cannot move; it is reported, and moves when it is next attached.
package drain

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"sort"
	"time"

	metav1 "github.com/flaminio/flaminio/api/gen/go/flaminio/meta/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// WitnessLabel marks a node that holds no volume data (as in the metadata service).
const WitnessLabel = "flaminio.io/witness"

// Progress is the state of a drain after one step.
type Progress struct {
	// Chunk replicas still on the node, and how many of them have a move pending.
	Remaining, Moving int
	// Volumes with replicas on the node and no target to move them.
	Unattached []string
}

func (p Progress) String() string {
	s := fmt.Sprintf("%d chunk replicas left, %d moving", p.Remaining, p.Moving)
	if len(p.Unattached) > 0 {
		s += fmt.Sprintf("; waiting for a target: %v", p.Unattached)
	}
	return s
}

// Step requests a move for every chunk replica on the node that has none pending, and
// reports what is left.
func Step(ctx context.Context, c metav1.MetadataServiceClient, node string) (Progress, error) {
	var p Progress
	nodes, err := c.ListNodes(ctx, &metav1.ListNodesRequest{})
	if err != nil {
		return p, err
	}
	ids, err := c.ListVolumes(ctx, &metav1.ListVolumesRequest{})
	if err != nil {
		return p, err
	}
	var vols []*metav1.Volume
	load := map[string]int{}
	for _, id := range ids.GetIds() {
		g, err := c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: id})
		if status.Code(err) == codes.NotFound {
			continue // deleted since it was listed
		}
		if err != nil {
			return p, err
		}
		vols = append(vols, g.GetVolume())
		for _, ch := range g.GetVolume().GetChunks() {
			for _, n := range ch.GetReplicas() {
				load[n]++
			}
			if ch.GetMovingTo() != "" {
				load[ch.GetMovingTo()]++
			}
		}
	}
	var candidates []string
	for _, n := range nodes.GetNodes() {
		if _, witness := n.GetLabels()[WitnessLabel]; n.GetId() != node && n.GetAlive() && !witness && !n.GetDraining() {
			candidates = append(candidates, n.GetId())
		}
	}
	for _, v := range vols {
		vp, moved, err := stepVolume(ctx, c, node, v, candidates, load)
		if errors.Is(err, errVolumeGone) {
			// Deleted since it was listed: its replicas go with its tombstone (docs/8), and the
			// moves it was given no longer weigh on their destinations.
			for _, to := range moved {
				load[to]--
			}
			continue
		}
		if err != nil {
			return p, err
		}
		p.Remaining += vp.Remaining
		p.Moving += vp.Moving
		p.Unattached = append(p.Unattached, vp.Unattached...)
	}
	return p, nil
}

// errVolumeGone reports a volume deleted since the step listed it.
var errVolumeGone = errors.New("volume deleted")

// stepVolume requests the moves of one volume's chunk replicas on the node, and reports what is
// left of it, and the destinations of the moves it was given.
func stepVolume(ctx context.Context, c metav1.MetadataServiceClient, node string, v *metav1.Volume,
	candidates []string, load map[string]int) (p Progress, moved []string, err error) {
	waiting := false
	for i, ch := range v.GetChunks() {
		if !slices.Contains(ch.GetReplicas(), node) {
			continue
		}
		p.Remaining++
		if v.GetTarget() == "" {
			waiting = true
		}
		if ch.GetMovingFrom() != "" {
			p.Moving++
			continue
		}
		to := leastLoaded(candidates, ch.GetReplicas(), load)
		if to == "" {
			return p, moved, fmt.Errorf("volume %s chunk %d: no node to move to (live, not a witness, not draining, without a replica of it)", v.GetId(), i)
		}
		r, err := c.MoveReplica(ctx, &metav1.MoveReplicaRequest{Volume: v.GetId(), Chunk: uint32(i), From: node, To: to})
		if status.Code(err) == codes.NotFound {
			// The volume deleted since it was listed, or the destination gone: tell which.
			switch _, gerr := c.GetVolume(ctx, &metav1.GetVolumeRequest{Id: v.GetId()}); status.Code(gerr) {
			case codes.NotFound:
				return p, moved, errVolumeGone
			case codes.OK:
			default:
				return p, moved, gerr
			}
		}
		if err != nil {
			return p, moved, err
		}
		if r.GetApplied() {
			load[to]++
			moved = append(moved, to)
			p.Moving++
		}
	}
	if waiting {
		p.Unattached = append(p.Unattached, v.GetId())
	}
	return p, moved, nil
}

func leastLoaded(candidates, exclude []string, load map[string]int) string {
	var ok []string
	for _, n := range candidates {
		if !slices.Contains(exclude, n) {
			ok = append(ok, n)
		}
	}
	sort.SliceStable(ok, func(i, j int) bool {
		if load[ok[i]] != load[ok[j]] {
			return load[ok[i]] < load[ok[j]]
		}
		return ok[i] < ok[j]
	})
	if len(ok) == 0 {
		return ""
	}
	return ok[0]
}

// Run marks the node draining and steps until no replica is left on it, reporting progress
// after each step.
func Run(ctx context.Context, c metav1.MetadataServiceClient, node string, interval time.Duration, report func(Progress)) error {
	if _, err := c.DrainNode(ctx, &metav1.DrainNodeRequest{Node: node, Draining: true}); err != nil {
		return err
	}
	for {
		p, err := Step(ctx, c, node)
		if err != nil {
			return err
		}
		report(p)
		if p.Remaining == 0 {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(interval):
		}
	}
}
