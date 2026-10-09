package csi

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
)

// Mounter is what the node plugin does to devices and mount points.
type Mounter interface {
	// FSType returns the filesystem (or other signature) on a device, or "" if it has none.
	FSType(ctx context.Context, device string) (string, error)
	Format(ctx context.Context, device, fsType string) error
	Mount(ctx context.Context, source, target, fsType string, options []string) error
	Unmount(ctx context.Context, target string) error
	IsMountPoint(target string) (bool, error)
}

// HostMounter runs the util-linux and filesystem tools.
type HostMounter struct{}

func run(ctx context.Context, name string, args ...string) ([]byte, error) {
	out, err := exec.CommandContext(ctx, name, args...).CombinedOutput()
	if err != nil {
		return out, fmt.Errorf("%s %s: %w: %s", name, strings.Join(args, " "), err, bytes.TrimSpace(out))
	}
	return out, nil
}

func (HostMounter) FSType(ctx context.Context, device string) (string, error) {
	// Low-level probing (-p) looks at the device itself, not a cache, and reports any
	// signature: a filesystem, a partition table, RAID or LVM metadata.
	out, err := exec.CommandContext(ctx, "blkid", "-p", "-o", "export", device).Output()
	var exit *exec.ExitError
	if errors.As(err, &exit) && exit.ExitCode() == 2 {
		return "", nil // nothing found
	}
	if err != nil {
		return "", fmt.Errorf("blkid %s: %w", device, err)
	}
	for line := range strings.Lines(string(out)) {
		if v, ok := strings.CutPrefix(strings.TrimSpace(line), "TYPE="); ok {
			return v, nil
		}
		if v, ok := strings.CutPrefix(strings.TrimSpace(line), "PTTYPE="); ok {
			return "partition table " + v, nil
		}
	}
	return "unknown signature", nil
}

func (HostMounter) Format(ctx context.Context, device, fsType string) error {
	args := []string{device}
	if fsType == "ext4" {
		// The device is new and thin: skip zeroing the inode tables and the journal.
		args = []string{"-F", "-E", "lazy_itable_init=1,lazy_journal_init=1,nodiscard", device}
	}
	_, err := run(ctx, "mkfs."+fsType, args...)
	return err
}

func (HostMounter) Mount(ctx context.Context, source, target, fsType string, options []string) error {
	args := []string{}
	if fsType != "" {
		args = append(args, "-t", fsType)
	}
	if len(options) > 0 {
		args = append(args, "-o", strings.Join(options, ","))
	}
	_, err := run(ctx, "mount", append(args, source, target)...)
	return err
}

func (HostMounter) Unmount(ctx context.Context, target string) error {
	_, err := run(ctx, "umount", target)
	return err
}

// IsMountPoint reports whether target is a mount point, from /proc/self/mountinfo: bind
// mounts of a file (raw block volumes) are mount points that stat does not reveal.
func (HostMounter) IsMountPoint(target string) (bool, error) {
	f, err := os.Open("/proc/self/mountinfo")
	if err != nil {
		return false, err
	}
	defer f.Close()
	target = filepath.Clean(target)
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 64<<10), 1<<20)
	for sc.Scan() {
		fields := strings.Fields(sc.Text())
		if len(fields) > 4 && unescapeMountinfo(fields[4]) == target {
			return true, nil
		}
	}
	return false, sc.Err()
}

// unescapeMountinfo decodes the octal escapes (\040 for a space) of mountinfo paths.
func unescapeMountinfo(s string) string {
	if !strings.Contains(s, `\`) {
		return s
	}
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' && i+3 < len(s) {
			if n, err := strconv.ParseUint(s[i+1:i+4], 8, 8); err == nil {
				b.WriteByte(byte(n))
				i += 3
				continue
			}
		}
		b.WriteByte(s[i])
	}
	return b.String()
}
