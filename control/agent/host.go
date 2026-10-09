package agent

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"
)

// HostProber probes devices with blkid, the Flaminio disk tool and direct reads.
type HostProber struct {
	// DiskTool is the flaminio-disk binary.
	DiskTool string
}

func (HostProber) Signature(path string) (string, error) {
	out, err := exec.Command("blkid", "-p", "-o", "export", path).Output()
	var exit *exec.ExitError
	if errors.As(err, &exit) && exit.ExitCode() == 2 {
		return "", nil // nothing found
	}
	if err != nil {
		return "", fmt.Errorf("blkid %s: %w", path, err)
	}
	var found []string
	for line := range strings.Lines(string(out)) {
		k, v, ok := strings.Cut(strings.TrimSpace(line), "=")
		if ok && (k == "TYPE" || k == "PTTYPE" || k == "USAGE") {
			found = append(found, k+"="+v)
		}
	}
	if len(found) == 0 {
		return "unknown", nil
	}
	return strings.Join(found, " "), nil
}

func (h HostProber) Ours(path string) bool {
	return exec.Command(h.DiskTool, "inspect", path).Run() == nil
}

const edge = 1 << 20

func (HostProber) Blank(path string, size uint64) (bool, error) {
	f, err := os.Open(path)
	if err != nil {
		return false, err
	}
	defer f.Close()
	buf := make([]byte, edge)
	for _, off := range []int64{0, int64(size) - edge} {
		if off < 0 {
			off = 0
		}
		n, err := f.ReadAt(buf, off)
		if err != nil && n < len(buf) && int64(n) < int64(size)-off {
			return false, err
		}
		if !bytes.Equal(buf[:n], make([]byte, n)) {
			return false, nil
		}
	}
	return true, nil
}

func (HostProber) Holders(name string) ([]string, error) {
	entries, err := os.ReadDir("/sys/block/" + name + "/holders")
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	var out []string
	for _, e := range entries {
		out = append(out, e.Name())
	}
	return out, err
}

// Format makes a claimable device a Flaminio disk. The disk tool refuses an existing Flaminio
// disk on its own; the claim policy has already refused everything else.
func Format(diskTool, path, label string) error {
	out, err := exec.Command(diskTool, "format", path, "--label", label).CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s format %s: %w: %s", diskTool, path, err, bytes.TrimSpace(out))
	}
	return nil
}
