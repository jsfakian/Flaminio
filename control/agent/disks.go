// Package agent is the node agent's device handling: it finds the node's block devices and
// decides which one Flaminio may use (the claim policy), then formats it.
//
// The claim policy never takes a device that holds anything: a filesystem, a partition table,
// RAID or LVM metadata, any signature blkid knows, or any non-zero data in its first or last
// MiB. A device that already carries a Flaminio superblock is recognised as ours and used as
// it is. Everything else (read-only, removable, virtual, mounted, partitioned, in use, too
// small, or not matching the operator's selector) is refused, with the reason.
package agent

import (
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
)

// Device is a block device as lsblk reports it.
type Device struct {
	Name      string   `json:"name"`
	Path      string   `json:"path"`
	Type      string   `json:"type"`
	Size      uint64   `json:"size"`
	ReadOnly  bool     `json:"ro"`
	Removable bool     `json:"rm"`
	Mounts    []string `json:"mountpoints"`
	Serial    string   `json:"serial"`
	FSType    string   `json:"fstype"`
	PTType    string   `json:"pttype"`
	Children  []Device `json:"children"`
}

// ParseLsblk reads `lsblk -J -b -o NAME,PATH,TYPE,SIZE,RO,RM,MOUNTPOINTS,SERIAL,FSTYPE,PTTYPE`.
func ParseLsblk(b []byte) ([]Device, error) {
	var out struct {
		Devices []Device `json:"blockdevices"`
	}
	if err := json.Unmarshal(b, &out); err != nil {
		return nil, fmt.Errorf("lsblk output: %w", err)
	}
	return out.Devices, nil
}

// LsblkArgs are the arguments ParseLsblk expects lsblk to run with.
var LsblkArgs = []string{"-J", "-b", "-o", "NAME,PATH,TYPE,SIZE,RO,RM,MOUNTPOINTS,SERIAL,FSTYPE,PTTYPE"}

// Prober looks at a device's contents and users.
type Prober interface {
	// Signature returns what blkid's low-level probe finds on the device, or "" for nothing.
	Signature(path string) (string, error)
	// Ours reports whether the device carries a valid Flaminio superblock.
	Ours(path string) bool
	// Blank reports whether the device's first and last MiB are all zeros.
	Blank(path string, size uint64) (bool, error)
	// Holders lists the devices built on top of this one (device mapper, MD, bcache…).
	Holders(name string) ([]string, error)
}

// Policy is the operator's part of the claim policy.
type Policy struct {
	// Only devices whose serial matches, if set.
	Serial  *regexp.Regexp
	MinSize uint64
}

// State of a device under the claim policy.
type State string

const (
	Claimable State = "claimable"
	Ours      State = "ours"
	Refused   State = "refused"
)

// Decision is the claim policy's verdict on one device.
type Decision struct {
	Device
	State  State
	Reason string
}

var virtual = []string{"loop", "ram", "zram", "dm-", "md", "ublk", "nbd", "rbd", "sr"}

// Decide applies the claim policy to every top-level device.
func Decide(devs []Device, p Policy, pr Prober) []Decision {
	out := make([]Decision, 0, len(devs))
	for _, d := range devs {
		state, reason := decide(d, p, pr)
		out = append(out, Decision{Device: d, State: state, Reason: reason})
	}
	return out
}

func decide(d Device, p Policy, pr Prober) (State, string) {
	mounts := nonEmpty(d.Mounts)
	switch {
	case d.Type != "disk":
		return Refused, "not a whole disk (" + d.Type + ")"
	case hasPrefix(d.Name, virtual):
		return Refused, "a virtual device"
	case d.ReadOnly:
		return Refused, "read-only"
	case d.Removable:
		return Refused, "removable"
	case d.Size < p.MinSize:
		return Refused, fmt.Sprintf("smaller than %d bytes", p.MinSize)
	case p.Serial != nil && !p.Serial.MatchString(d.Serial):
		return Refused, fmt.Sprintf("serial %q does not match %s", d.Serial, p.Serial)
	case len(mounts) > 0:
		return Refused, "mounted at " + strings.Join(mounts, ", ")
	case len(d.Children) > 0:
		return Refused, "has partitions"
	}
	if holders, err := pr.Holders(d.Name); err != nil {
		return Refused, "cannot list its holders: " + err.Error()
	} else if len(holders) > 0 {
		return Refused, "in use by " + strings.Join(holders, ", ")
	}
	// Ours before the other signatures: blkid does not know Flaminio's format.
	if pr.Ours(d.Path) {
		return Ours, "a Flaminio disk"
	}
	if d.FSType != "" || d.PTType != "" {
		return Refused, "signature: " + strings.TrimSpace(d.FSType+" "+d.PTType)
	}
	if sig, err := pr.Signature(d.Path); err != nil {
		return Refused, "cannot probe it: " + err.Error()
	} else if sig != "" {
		return Refused, "signature: " + sig
	}
	if blank, err := pr.Blank(d.Path, d.Size); err != nil {
		return Refused, "cannot read it: " + err.Error()
	} else if !blank {
		return Refused, "not blank: data in its first or last MiB, of a kind nothing recognises"
	}
	return Claimable, "blank"
}

func nonEmpty(s []string) []string {
	var out []string
	for _, x := range s {
		if x != "" {
			out = append(out, x)
		}
	}
	return out
}

func hasPrefix(s string, prefixes []string) bool {
	for _, p := range prefixes {
		if strings.HasPrefix(s, p) {
			return true
		}
	}
	return false
}

// Pick returns the device the node should use: the one Flaminio disk if there is one, else
// the one claimable device. Phase 1 nodes use a single data disk, so several candidates are an
// error for the operator to resolve (with a narrower selector).
func Pick(ds []Decision) (Decision, error) {
	var ours, claimable []Decision
	for _, d := range ds {
		switch d.State {
		case Ours:
			ours = append(ours, d)
		case Claimable:
			claimable = append(claimable, d)
		}
	}
	switch {
	case len(ours) == 1:
		return ours[0], nil
	case len(ours) > 1:
		return Decision{}, fmt.Errorf("%d Flaminio disks: one per node in this version", len(ours))
	case len(claimable) == 1:
		return claimable[0], nil
	case len(claimable) > 1:
		return Decision{}, fmt.Errorf("%d claimable devices: narrow the selector", len(claimable))
	}
	return Decision{}, fmt.Errorf("no Flaminio disk and no claimable device")
}
