# The unprivileged ublk server is the child of a root helper that keeps ublk control

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when
  implemented. It binds this plan's implementation from the approval on.
- **Date:** 2026-11-13
- **Scope:** Issue-local draft for step 1 of #74; no global ADR number is assigned.

## Context

[Issue #74](https://github.com/jsfakian/Flaminio/issues/74) and ARCHITECTURE §5.1 want the data
plane unprivileged, with a privileged helper passing descriptors over a Unix socket. Today one
root process opens the disk, creates and serves its ublk devices and parses every network and
control input ([design](design.md), facts).

What the kernel allows decides the boundary (read in Linux 6.8.12, the Ubuntu 24.04 kernel ADR 0003
lists; [decisions](decisions.md)):
- `UBLK_F_UNPRIVILEGED_DEV` devices lose user recovery, refuse user copy and may be opened only by
  their owner (`ublk_drv.c:2391–2414`, `:768–795`). ADR 0003 depends on recovery, and raw block
  volumes are opened by tenant pods.
- An ordinary device needs `CAP_SYS_ADMIN` for each control command (`:2852–2854`), but its server
  needs no capability to mmap `/dev/ublkcN` and fetch and complete requests (`:1376–1390`,
  `:1746–1769`); the kernel only skips partition scanning for it (`:2288–2290`).
- io_uring descriptors cannot be sent with `SCM_RIGHTS` (`net/core/scm.c:107–111`), so rings
  cannot be made by a broker and handed over.

## Decision

- **Two processes.** `flaminio-helper` starts as root, opens the data disk, binds the control socket
  and keeps `/dev/ublk-control`. It starts `flaminio-io node --privsep` as its only child, with a
  dedicated UID and GID, no capabilities and `no_new_privs`; it keeps UID 0 and, of the
  capabilities, only `CAP_SYS_ADMIN`. It exits with its child, deleting the device it owns.
- **One unnamed socket.** The child's stdin is a `SOCK_SEQPACKET` socketpair end: only the child can
  reach the helper. The helper passes the disk, the control socket and each device's
  `/dev/ublkcN` with `SCM_RIGHTS`, and serves a fixed text protocol (add, start, stop, delete) for
  the device it owns: at most one, at the id its own command line names, whose `/dev/ublkcN` it
  holds open, which keeps the id from being reused. Flags are its own; geometry is bounded; a
  failed add deletes no device but the one it added.
- **Ordinary devices.** No unprivileged ublk devices, so recovery stays available (#48) and raw
  block volumes keep working; zero copy stays in direct mode only.
- **The I/O process makes its own rings.** Restricting io_uring operations belongs to the profiles
  step of #74, not to this boundary.
- **Dependencies:** a vendored libublk 0.4.8 with two public functions (`UblkDev::from_cdev`,
  `UblkCtrl::start_dev_for`) and a failed ADD_DEV that sends no DEL_DEV, to be offered upstream; `rustix`, already in the lockfile, used directly
  for safe socket, capability and pidfd calls, so the workspace's `forbid(unsafe_code)` holds.

## Options

- **Root parent with an unnamed socket (chosen):** no peer authentication, filesystem socket or
  host install; UID 0 and one capability retained. The helper lives and dies with its child,
  which #48 must change for recovery.
- **Unprivileged ublk devices:** no `CAP_SYS_ADMIN` anywhere, but no recovery, no user copy, and
  owner-only opens of the disk; contradicts ADR 0003's reason for ublk recovery.
- **A host-service broker with a named socket:** survives I/O restarts, but needs peer
  authentication, a socket directory the I/O process and the CSI plugin do not share, and a host
  installer no plan provides.
- **Rings made by the broker:** impossible across `SCM_RIGHTS`, and a single-issuer ring would bind to
  the broker.
- **Keep the root process:** the issue's starting point; does not meet it.

## Consequences

- Where the node runs under the helper (the lab's systemd unit), the process that parses network
  input runs without capabilities; a memory-safety bug in it no longer holds root, the disk paths
  or `/dev/ublk-control`. Kubernetes stays privileged in direct mode until #12 adopts the helper,
  and recovery waits for #48 ([intent](intent.md), "Which step"), which must close the helper's pin
  of a device before START_USER_RECOVERY and take a new one after.
- "At most one" device is this step's rule: the node attaches one volume at a time. Several
  volumes per node (#15) need one owned id per volume, which changes this decision.
- Retained: the helper's UID 0 and `CAP_SYS_ADMIN`; the CSI node plugin and agent stay privileged;
  the CSI node plugin must check that a device path names a ublk disk and bound what it reads.
  The I/O process's writable disk descriptor still allows `SG_IO` write-class commands,
  persistent reservations and NVMe passthrough on that disk, as the root process does today;
  denying them is #74's profiles step. It also holds `io.key`, which metadata clients would accept
  at the node's address until they check the `meta` URI SAN (design, Risks).
- Settled at approval, 2026-11-13: the vendored library, offered upstream (spec Q2; see
  decisions.md, Settled at approval). Settled 2026-11-13 (user's instruction): queue threads run
  without `PR_SET_IO_FLUSHER`, the risk accepted (Q1); this project maintains the vendored copy
  (Q2); no other ublk server on a data node is the deployment rule (Q3); untrusted contents
  reaching root parsers and udev rules are today's residual, for a later plan (Q4) (see
  decisions.md).
- No replication, format or metadata contract changes. Accepted ADRs 0001–0006 stay consistent.

## Confirmation

The [design](design.md)'s tests: unit tests of the protocol and ownership on a fake kernel, a fuzz
target of the request parser, root kernel tests of credentials and refusals (`make
privsep-check`), CSI tests with a lying data plane, and the lab lifecycle checks with the node
under the helper. Nothing here is implemented or run.
