# Issue #74, step 1: the privileged helper and the unprivileged I/O process — specification

[Intent](intent.md). [Design](design.md). [Decisions](decisions.md). Step 1 of #74: the
privileged helper and the unprivileged I/O process. Approved 2026-11-13; not implemented.
Architecture: [ADR](adr-proposal.md).

## Terms

- **Helper:** `flaminio-helper`, the root parent of the I/O process. It holds the data disk, the
  control socket and the ublk control device.
- **I/O process:** `flaminio-io node --privsep`, the helper's only child. It parses network and
  control input and serves the store and the ublk devices.
- **Owned device:** a ublk device this helper added at its child's request and still pins
  (design, Owned devices).
- **Direct mode:** the commands that open device paths themselves and need root, as today.

## The invariant

A compromised I/O process holds its disk, its control socket, the character devices of its
owned devices, and what its dedicated UID allows; its writable disk descriptor also allows the
disk's own commands, such as reservations (design, Risks). Through the helper it can add at most
one bounded device at a time, at the id the helper's command line names and with flags the helper
fixes, and start, stop and delete it; nothing else. No root consumer of its answers acts on a device that is not a ublk disk.

## Behaviour

**S1. Credentials.** The helper starts as root and starts the I/O process with a dedicated
non-zero UID and GID, no supplementary groups, empty capability sets (effective, permitted,
inheritable, bounding, ambient) and `no_new_privs`. After that start the helper keeps only
`CAP_SYS_ADMIN`. It refuses UID or GID 0, and refuses to start unless it runs as root.

**S2. Descriptors, not paths.** The helper opens the data disk (a block device, exclusive) and
binds the control socket (root only, mode 0600, as today) before the I/O process starts. The I/O
process receives them, and the ublk character device of each added device, over its helper
socket with `SCM_RIGHTS`. Under `--privsep` it never opens a disk or ublk device path, or the
ublk control device, and refuses the flags that would name one.

**S3. ublk control stays in the helper.** The I/O process may ask for its disk and control
socket once each, and to add, start, stop and delete devices. Add creates an ordinary device
whose flags the helper fixes (no zero copy, no user copy, no unprivileged ownership), sets its
parameters, and returns its character device. Start, stop and delete act only on owned devices.
Queues, depth, buffer size and device size are bounded; the id is the helper's own, so it owns at
most one device; adds are paced, and the device is started at most once. A failed add deletes no
device but the one it added.

**S4. Malformed requests change nothing.** A request that carries descriptors or other
ancillary data, is truncated or longer than allowed, or is not in the grammar, gets
`error invalid request`; the helper closes any descriptor it received and changes no state.
A reply carries at most one descriptor.

**S5. Lifetimes and cleanup.** The helper exits when its child exits, with the child's status,
even while a kernel command waits, and first deletes its owned devices within a bound. The I/O
process exits when the helper's socket closes. A failed add, a failed attach, a delete, the
child's exit, the helper's death and a reused id each end as the design's Owned devices states.

**S6. The CSI node plugin trusts no device path.** Before `blkid`, `mkfs` or `mount`, every device
string the data plane answers must be `/dev/ublkb<N>`, and the node at that path must be the
block device the kernel names `ublkb<N>`. Otherwise the call fails and nothing is run. The
plugin reads at most 4096 printable bytes of an answer.

**S7. Modes.** Direct mode stays for `flaminio-io node` without `--privsep`, `serve`, `del`,
`replica`, `digest`, `flaminio-bench` and `flaminio-disktool`; `--zero-copy` works only there.
The lab node runs under the helper by default; zero-copy measurements run it in direct mode.

**S8. Evidence.** The lab's provisioning, attach, I/O, drain and restart checks pass with the node
under the helper, and a lab check shows the credentials of both processes and the helper's
refusals on the lab's kernel. Nothing in this step claims enforced profiles.

## Tests

- **S1, S2:** a root-only kernel test of the helper on a loop device: the credentials of both
  processes, the I/O process's refused opens of the disk path and the ublk control device.
- **S5:** one test per outcome in the design's Owned devices, including the helper's exit while
  START_DEV waits; client tests of paired replies, the `start` limit and the helper's hangup.
- **S3, S4:** unit tests of the grammar, bounds, ownership and limit on a fake kernel; socket
  tests of ancillary data and truncation; a fuzz target of the request parser; the kernel test
  refusing a device the helper did not add.
- **S6:** CSI unit tests with a data plane that answers foreign paths, or too long or
  non-printable answers.
- **S7, S8:** parse tests of `--privsep`; script tests of the lab node's command line; the lab
  checks in the design.

## Open questions

- **Q1:** without capabilities, the I/O process's queue threads cannot be marked
  `PR_SET_IO_FLUSHER` (it needs `CAP_SYS_RESOURCE`). Keep that one capability, or accept the
  risk of memory reclaim waiting on the device the thread serves? (Design, Risks.)
  Settled 2026-11-13 (user's instruction): accept the risk; the I/O process keeps no capability
  (see decisions.md).
- **Q2:** the libublk change: upstream it, and until then who maintains the vendored copy?
  Settled at approval, 2026-11-13: a vendored libublk 0.4.8 with the patch, offered upstream (see
  decisions.md, Settled at approval). Who maintains it: Settled 2026-11-13 (user's instruction):
  this project, in this repository, until upstream takes the change (see decisions.md).
- **Q3:** the CSI check proves a ublk disk, not this node's: is "no other ublk server on a data
  node" an acceptable deployment rule, or must a later step mark the helper's devices?
  Settled 2026-11-13 (user's instruction): it is this step's deployment rule; marking waits for a
  step that needs another ublk server on a data node (see decisions.md).
- **Q4:** untrusted block contents still reach root parsers (`blkid`, `mkfs`, `mount`, udev's
  probes, the kernel's filesystem code), and udev rules act on them with no parser bug (LVM
  autoactivation, md incremental assembly). A raw block volume's bytes are tenant-controlled even
  today. This step does not close that; it needs a decision. Settled 2026-11-13 (user's
  instruction): accepted as today's residual, for a later plan of #74 (see decisions.md).
