# Issue #74, step 1: the privileged helper and the unprivileged I/O process — intent

[Issue #74](https://github.com/jsfakian/Flaminio/issues/74) ("Runtime privilege separation and
verifiable release artifacts"), architecture §5.1 and §15.5–15.6.
[Decisions](decisions.md). Approved 2026-11-13; not implemented.

## Which step this is

The issue has five steps and one "Done when". **This plan is step 1 of #74:** the issue's step 1
(a minimal privileged helper that opens the raw device and the ublk control device and passes
descriptors over a Unix socket with `SCM_RIGHTS`), and the first half of its step 2 (the I/O
process runs unprivileged). It delivers two clauses of "Done when": provisioning, attach, I/O,
drain and restart checks pass with an unprivileged I/O process, and unauthorized helper requests
fail. #74 stays open after it.

**What stays open in #74, for later plans:**
- **Step 2, second half:** fewer privileges for the node agent (its probes read raw disks,
  `control/agent/host.go:18–66`); non-root, read-only, distroless controller images.
- **Step 3:** seccomp, AppArmor and SELinux profiles, io_uring restrictions, denying `SG_IO`,
  persistent-reservation and NVMe passthrough ioctls on the disk, storage-interface binding,
  NetworkPolicies. "Done when" asks for the lifecycle checks *with enforced profiles*: that
  clause waits for step 3.
- **Step 4:** reproducible builds, signatures, SBOMs, provenance. The issue lets it proceed
  independently; the draft's release track left this plan for its own.
- **Step 5:** the security policy and the OSS-Fuzz assessment.

**What this step leaves for Kubernetes and recovery** (stated here once; the other documents
defer to it):
- **#12:** until #12 adopts the helper, the data-plane pod stays privileged in direct mode (#12's
  proposed ADR 0007, "A privileged data-plane pod, until privilege separation") and is not
  hardened by this step. Only the lab's systemd unit runs the I/O process unprivileged. Design,
  Risks, lists what #12's pod needs.
- **#48:** ublk recovery is not covered. With the helper, recovery commands must go through it:
  the I/O process has no `/dev/ublk-control`, and a second root path to it (such as #48's draft
  `flaminio-io end-kept`) would be outside this boundary. The helper's pin keeps the dead
  server's character device open, and START_USER_RECOVERY refuses while it is open
  (`ublk_drv.c:2740–2744`): #48 must close the pin before recovery and take a new one after.

## Why

1. **The I/O process is root.** It opens its disk itself (`flaminio-io/src/node.rs:403–404`,
   `flaminio-uring/src/uring.rs:123–154`) and creates, starts, stops and deletes its ublk device
   with libublk (`flaminio-io/src/ublk.rs:902–934`, `:1050–1052`, `:626`; `control.rs:202`). The
   process that parses network and control input can do anything on the node.
2. **The kernel allows a smaller boundary.** An ordinary ublk device needs `CAP_SYS_ADMIN` for its
   control commands, but its server needs no capability to fetch and complete requests on
   `/dev/ublkcN`; the kernel only skips partition scanning for an unprivileged server. A root
   parent can keep the control commands and hand the child the one descriptor it serves.
   (Unprivileged ublk devices would lose recovery, and io_uring descriptors cannot cross
   `SCM_RIGHTS`: decisions.md, kernel facts.)
3. **Root consumers trust its answers.** The CSI node plugin formats and mounts, as root, the
   device path the data plane names (`control/csi/node.go:58–126`). Once the data plane is
   unprivileged, that answer is untrusted input.

## What we want

1. A small root helper, the parent of the I/O process, that holds the disk, the control socket
   and `/dev/ublk-control`, and serves a fixed set of bounded requests for devices it created.
2. An I/O process with a dedicated UID and GID, no capabilities and `no_new_privs`, that never
   opens a device path.
3. A CSI node plugin that acts only on a ublk disk the kernel names.
4. The existing lab lifecycle checks, run with the node under the helper by default.

## Out of scope

- Changing replication, epochs, tokens or the on-disk format. The helper checks no token and
  reads no metadata: capability tokens stay with the I/O process and the replicas.
- Removing kernel, io_uring or filesystem-parser risk: the I/O process can still use io_uring and
  serve crafted block contents (Risks).
- The other steps above, Kubernetes packaging (#12) and recovery (#48).
- Running lab or destructive checks while drafting.
