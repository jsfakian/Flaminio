// Command flaminio-agent is the node agent's device side: it lists the node's block devices
// with the claim policy's verdict on each, and claims the node's data disk.
//
//	flaminio-agent disks [--serial REGEXP] [--min-size BYTES] [--disk-tool PATH]
//	flaminio-agent claim [--serial REGEXP] [--min-size BYTES] [--disk-tool PATH] [--label TEXT]
//
// claim prints the data disk's path: the node's Flaminio disk if it has one, else the one
// claimable device, which it formats first. It refuses any device that holds anything (see
// control/agent).
package main

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"regexp"

	"github.com/flaminio/flaminio/control/agent"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "flaminio-agent:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 || (args[0] != "disks" && args[0] != "claim") {
		return fmt.Errorf("usage: flaminio-agent disks|claim [--serial REGEXP] [--min-size BYTES] [--disk-tool PATH] [--label TEXT]")
	}
	fs := flag.NewFlagSet(args[0], flag.ExitOnError)
	serial := fs.String("serial", "", "only devices whose serial matches this regular expression")
	minSize := fs.Uint64("min-size", 1<<30, "smallest device to claim, in bytes")
	diskTool := fs.String("disk-tool", "flaminio-disk", "the Flaminio disk tool")
	label := fs.String("label", "", "label of a newly formatted disk (default: the host name)")
	_ = fs.Parse(args[1:])

	policy := agent.Policy{MinSize: *minSize}
	if *serial != "" {
		re, err := regexp.Compile(*serial)
		if err != nil {
			return err
		}
		policy.Serial = re
	}
	out, err := exec.Command("lsblk", agent.LsblkArgs...).Output()
	if err != nil {
		return fmt.Errorf("lsblk: %w", err)
	}
	devs, err := agent.ParseLsblk(out)
	if err != nil {
		return err
	}
	decisions := agent.Decide(devs, policy, agent.HostProber{DiskTool: *diskTool})
	if args[0] == "disks" {
		for _, d := range decisions {
			fmt.Printf("%-14s %-9s %7.1f GiB  serial %-16q %s\n", d.Path, d.State, float64(d.Size)/(1<<30), d.Serial, d.Reason)
		}
		return nil
	}
	d, err := agent.Pick(decisions)
	if err != nil {
		for _, x := range decisions {
			fmt.Fprintf(os.Stderr, "  %s: %s (%s)\n", x.Path, x.State, x.Reason)
		}
		return err
	}
	if d.State == agent.Claimable {
		if *label == "" {
			*label, _ = os.Hostname()
		}
		if err := agent.Format(*diskTool, d.Path, *label); err != nil {
			return err
		}
		fmt.Fprintf(os.Stderr, "flaminio-agent: claimed %s (blank, serial %q) and formatted it\n", d.Path, d.Serial)
	} else {
		fmt.Fprintf(os.Stderr, "flaminio-agent: %s is already a Flaminio disk\n", d.Path)
	}
	fmt.Println(d.Path)
	return nil
}
