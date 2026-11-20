# Issue #74 — plan review, round 1, code focus

Scope: `docs/issues/74/` (intent, spec, design, adr-proposal) against the worktree
`/tmp/flaminio-plans-2026-11-12/74`. The issue's own text was not readable offline, so "Done
when" is judged from the intent and ARCHITECTURE §5.1, §15.5–15.6. Kernel claims below were
checked in a local Linux 6.8.12 tree (Ubuntu 24.04's version, the first distribution ADR 0003
lists); where a claim is from memory only, it says so.

### R1-C1 — High: unprivileged ublk ownership removes user recovery and raw block volumes

**Summary.** design.md:42–47 makes `UBLK_F_UNPRIVILEGED_DEV` ("kernel unprivileged per-device
ownership for the I/O UID", created by a worker that dropped to the I/O UID) the mechanism of
S2, with no fallback. On the baseline kernel that flag costs three things the product needs:

- **User recovery is cleared.** `ublk_ctrl_add_dev` strips `UBLK_F_USER_RECOVERY` and
  `UBLK_F_USER_RECOVERY_REISSUE` from an unprivileged device (6.8.12
  `drivers/block/ublk_drv.c:2396–2406`: "can't support recovery features for unprivileged ublk
  now"). The design's `Recover` op (design.md:36, 159–162), S6 ("restart/recovery and upgrade
  integration work with unprivileged I/O"), ADR 0003's context, ARCHITECTURE §5.1/§18, #12
  (intent.md:114) and #48 (design.md:20, "Require both USER_RECOVERY and USER_RECOVERY_REISSUE")
  all need it.
- **Only the owner may open the disk.** `ublk_open` refuses any opener that is neither
  `CAP_SYS_ADMIN` nor the owner uid/gid (same file, 768–790). The CSI driver serves raw block
  volumes (`control/csi/controller.go:118`, `node.go:95`, `:179`): a non-root tenant pod then
  gets `EPERM` on `/dev/ublkbN`.
- **Copy modes are refused.** `UBLK_F_USER_COPY` returns `EINVAL` (2408–2414); from memory, newer
  kernels refuse `UBLK_F_SUPPORT_ZERO_COPY` and `UBLK_F_AUTO_BUF_REG` the same way, which
  `add_device` sets under `--zero-copy` (`flaminio-io/src/ublk.rs:906–907`).

"Probe ... else hardened mode is unsupported" (design.md:46) therefore resolves to unsupported
on every kernel checked, and S6 cannot be met.

**Fix.** Drop the unprivileged flag. Evaluate the shape ARCHITECTURE §5.1 already describes: the
root helper adds an ordinary device, opens `/dev/ublkcN` once (the kernel allows one open,
`UB_STATE_OPEN`) and passes that descriptor; the helper keeps START/STOP/DEL and recovery
commands. The kernel anticipates an unprivileged daemon on such a device (`nr_privileged_daemon`
only suppresses the partition scan). Prototype it before the design gate (spec Q1), and state
the trust consequence (recovery with a daemon the kernel calls untrusted). If the flag stays,
the spec must say that hardened mode has no recovery, no raw block for non-root pods and no zero
copy, and reconcile S6 with #48.

### R1-C2 — High: the ring handoff leaves the replica store without rings and breaks single-issuer rings

**Summary.** design.md:75–79 has the broker create every ring and the I/O seccomp profile deny
`io_uring_setup`. Against the code:

- **The store's rings have no source.** `UringDevice::open` builds two rings at node start,
  before any attach (`flaminio-uring/src/uring.rs:138–143`, called from
  `flaminio-io/src/node.rs:403`). Protocol v1 returns rings only from `CreateUblk`/`Recover`;
  `OpenDisk` "returns raw FD" (design.md:162). A node that only hosts replicas cannot do I/O.
- **The FD bound is below the queue bound.** "At most 16 SCM_RIGHTS FDs" (design.md:158) against
  `--queues` 1 to 64 (`node.rs:203`), one ring per queue plus the char device.
- **Broker-side enable binds the wrong task.** With `--defer-taskrun` the queue ring is
  `SINGLE_ISSUER | DEFER_TASKRUN` (`ublk.rs:1079–1085`). For a ring created `R_DISABLED`, the
  submitter is the task that enables it (6.8.12 `io_uring/register.c:229–236`). "Broker ...
  installs ... restrictions, then enables them" makes the broker the only allowed submitter.
- **The opcode list is incomplete.** It names read/write/fsync, poll/timeout/cancel and
  URING_CMD; the code also submits `ReadFixed`/`WriteFixed` (`ublk.rs:1352`, `:1419`, `:1509`),
  and libublk registers files (and buffer tables for zero copy) on the queue ring.
- **"FD whitelist"** (design.md:76) names no mechanism. Restrictions cannot filter descriptors;
  requiring `IOSQE_FIXED_FILE` would change every SQE, all of which use `types::Fd` today
  (`uring.rs:176`, `:308`, `:432`; `ublk.rs:382`, `:1137`, `:1309`).

**Fix.** Give `OpenDisk` (or a `Rings` op) the store's two rings; size the FD bound from the
queue bound or split the reply; have the broker register restrictions and leave the ring
disabled, the I/O thread enabling it; list the exact SQE and register opcodes from the code,
libublk's included; define or delete the FD whitelist. `UringDevice::from_owned_fd` must take
the rings as well as the file.

### R1-C3 — High: the registry pins `dev_t`, which does not survive a reboot

**Summary.** The registry "maps node/UUID→canonical dev_t" and an open must match "fstat dev_t
... after open", never retrying another pathname (design.md:16–18, 28–31). SCSI and NVMe
major:minor numbers follow probe order: after a reboot with several disks the same disk can
have another `dev_t`, and every replica on it stays refused until an administrator re-claims.
The superblock already carries the stable identity (`disk_uuid`,
`flaminio-format/src/superblock.rs:26`).

**Fix.** Make the durable grant the superblock `disk_uuid` plus a hardware identity (WWN or
serial, as the agent reads, ARCHITECTURE §5.2) and size. Treat `dev_t` as a within-boot cache
the helper re-resolves (for example through `/dev/disk/by-id`, opened with the same openat2
rules) and re-verifies after open. Add the test: two disks swap names across a simulated
reboot.

### R1-C4 — Medium: work is assigned to issues whose plans exclude it

**Summary.** The design leans on other issues for mechanisms those plans do not contain:

- **#43** "installs" the helper as a host service and its "installer" becomes the OCI Helm chart
  (design.md:15–16, 114–115). #43 puts registry, publishing and signing out of scope
  (`flaminio-43/docs/issues/43/intent.md:87`, `design.md:150`) and has no chart or host unit. An
  operator can install a host service only through a privileged pod, which S3/S10 would have
  to state.
- **#16** "supplies distinct helper/IO identities" (design.md:93); #16's plan has no helper
  identity.
- **#73** audit of claim/format/release (design.md:19) and **#80** ring/queue quotas
  (design.md:164–165): neither plan has such a hook.
- **`Recover`** exists only with #48 and **`Resize`** with volume growth; neither is in the code
  (`ublk.rs` has no `USER_RECOVERY` or size update), so their tests cannot fail without this
  change.

**Fix.** For each item, either own it in #74 (and add it to Files and Tests) or name the issue
that must land first and what it must provide. Drop `Resize`/`Recover` from protocol v1 until
their features exist, or state the ordering after #48 and growth.

### R1-C5 — Medium: the helper's metadata identity has no authorization design

**Summary.** design.md:37–39 gives the helper "a separate read-only mTLS identity" and "a current
coherent target read", and design.md:166 says no metadata contract changes. A new component
needs a rule: `Authorize` knows only `agent`, `io` and admins (`meta/service/auth.go:58–84`), and
`GetVolume` is `allowed = true` for any identity. "Read-only" would hold only by the absence of
a matching case. The RPC behind "target read" is not named. `pkg/pki`, `auth.go` and
`flaminio-net/src/identity.rs` are missing from Files, and no test denies the helper `Attach`,
`ChangeIsr` or `RegisterNode`. A gRPC/TLS client also grows the root process that §15.5 calls
"small".

**Fix.** Name the component and the one RPC (`GetVolume`), add the `auth_test` cases, and list
the files. Or verify only the capability token offline: its claims already bind volume, node,
epoch and first epoch (`flaminio-net/src/token.rs:15–23`). Then the helper needs no network.

### R1-C6 — Medium: "unprivileged discovery" contradicts how the agent probes disks

**Summary.** S3 and design.md:18, 69 say discovery is read-only sysfs with no capabilities. The
agent's claim policy reads the devices themselves: `blkid -p` on the path, `flaminio-disktool
inspect`, and a direct open that reads both ends (`control/agent/host.go:18–41`, `:46–66`).
Without that, "never claims a device that has any signature" (ARCHITECTURE §5.2) cannot be
checked.

**Fix.** Say how the unprivileged agent gets those reads: a read-only probe op in the helper with
its own authorization, or a named group with read access, and test both the positive probe and
the refusal of a write.

### R1-C7 — Medium: the helper socket's directory is the one the I/O process must write

**Summary.** `/run/flaminio/helper.sock` sits in a root-owned 0750 parent (design.md:22–23). The
I/O process creates that directory, unlinks and binds `node.sock` there
(`flaminio-io/src/node.rs:137`, `control.rs:46–52`); as UID 2000 it can do none of these. The
lab keeps the attach token there under mode 700 (`scripts/guest/node.sh:61–63`). The CSI node
plugin mounts the whole directory (`deploy/csi/driver.yaml:167`), which also exposes the helper
socket to it, and #12 records that a hostPath PV reaches `/run/flaminio`
(`flaminio-12/docs/issues/12/design.md:205`). "Keep existing public control socket
compatibility" (design.md:167) is not shown to hold.

**Fix.** Put the helper socket in its own directory, outside the CSI hostPath; state who creates
`/run/flaminio` for the I/O user, the control socket's owner and mode (root-only 0600 today,
`control.rs:10`) and where the token file lives.

### R1-C8 — Medium: the ledger's crash ordering and the kernel-side identity are not specified

**Summary.** S2's "never delete a replacement" rests on design.md:49–52: a ledger written "before
control acknowledgement" and "inspect kernel identity". Missing:

- **The order of the kernel command and the record.** A crash after `ADD_DEV` and before the
  rename leaves a device with no record (quarantined for good, its id lost); the retry with the
  same request id then creates a second one.
- **What kernel identity is.** A device carries no nonce unless the helper puts one there.
  `ublksrv_ctrl_dev_info.ublksrv_flags` is free for the server's use and returned by
  `GET_DEV_INFO`; the design does not name it or any other field.
- **Where the ledger lives and what a reboot does.** Devices die with the boot, ids restart, a
  persistent ledger then names devices that are other volumes'.

**Fix.** Write an intent record (id, nonce, request id) durably before `ADD_DEV`; store the nonce
in the device; state for each pair of steps what a crash leaves and what reconciliation does;
key the ledger by boot id or keep it on tmpfs. Add one test per crash point.

### R1-C9 — Medium: how hardened mode is selected, and what the default is, is not stated

**Summary.** design.md:88 implies a second, non-hardened mode; S4 allows to "explicitly disable
the hardened mode"; S2, intent and Q3 forbid privileged I/O in the completed product. Nothing
says which flag or configuration selects the helper path over `UringDevice::open(path)`, what
the default is, or what happens to the callers that open devices themselves: the lab's root
unit (`scripts/guest/node.sh:45`), `flaminio-io` `ublk`/`serve` (`ublk.rs:141`, `cli.rs:99`),
`flaminio-bench`, `flaminio-disktool` and `--format` inside the node (`node.rs:160`).

**Fix.** Name the switch and its default, list which binaries and lab paths keep the direct open
and why, and add a test that a hardened start never opens a device path.

### R1-C10 — Medium: the test plan names categories, not tests; the lab plan is not runnable

**Summary.** The table (design.md:170–177) lists subjects ("helper unit/fuzz/socket tests",
"disposable kernel/container matrix") without test names, crates, fuzz target names or make
targets, so none can be checked for failing without the change. The lab section
(design.md:184–188) has no commands, hosts, scripts or pass criteria; no harness for the
kernel/LSM matrix exists or is designed. Clauses with no test at all:

- S3: controllers non-root, read-only root filesystem, distroless (a manifest check);
- S10: a helper refusing an unknown protocol version; install by digest; packaging that cannot
  restore privileged I/O;
- S5: "tenant pods cannot reach helper sockets or node keys";
- S8: an untrusted pull request obtaining no signing authority ("credential tests" is not a
  test).

**Fix.** Name each test with its file or target and the rule it proves; add the missing ones;
write the lab plan as the commands to run, on which disposable host, with the expected result
and the restore steps.

### R1-C11 — Low: the helper's capability set does not match what the design has it do

**Summary.** design.md:66–68 starts the bounding set at `DAC_OVERRIDE` and `SYS_ADMIN`. The
worker that "drops to the I/O UID" (design.md:43–44) needs `SETUID`/`SETGID`. Root opening
root-owned `/dev` nodes needs no `DAC_OVERRIDE`.

**Fix.** Derive the set from the operations (with R1-C1's fix the worker and its capabilities may
go away) and test that the helper starts with exactly that set.

### R1-C12 — Low: the inventory of privileged call sites is incomplete

**Summary.** The design's facts cite `ublk.rs:168–176`, which is the single-volume `ublk`
command. The node's device is added in `add_device` (`ublk.rs:902–934`). Global control is also
opened to kill or delete a device at `ublk.rs:129–131`, `ublk.rs:626` and `control.rs:202`.
Raw opens also happen at `node.rs:403`, `ublk.rs:141`, `cli.rs:99`,
`flaminio-bench/src/main.rs:167` and `flaminio-disktool/src/main.rs:162`.

**Fix.** List every site and say which becomes a helper request, which stays direct (R1-C9) and
which is removed.

### R1-C13 — Low: pinning the provenance builder by commit conflicts with the generator the design cites

**Summary.** design.md:105–106 pins "actions and builder workflows by immutable commit/digest".
From memory of the cited slsa-github-generator's documentation (not checked offline): its
reusable builder must be referenced by a full version tag, and a hash reference fails
verification of the builder identity.

**Fix.** Exempt the trusted builder from commit pinning, pin it by exact tag and verify that tag
in the policy; or record this as a criterion of Q2's builder selection.

### R1-C14 — Low: third-party images in the manifests are outside the digest and inventory rules

**Summary.** S10 installs by digest and forbids "unverified mutable images". The manifests pull
five sidecars by tag (`deploy/csi/driver.yaml:64`, `:73`, `:78`, `:138`, `:150`). The design
covers only Flaminio's own images and the chart.

**Fix.** Say that the chart pins these by digest, whether their upstream signatures are verified,
and whether the release manifest lists them.

## Nits

- **N1.** `flaminio-uring/src/uring.rs:125–151` (intent.md:9, design.md:6): `open` spans 123–154.
- **N2.** spec S1 and Terms say "incarnation"; the code, the token and the design say
  `first_epoch`. Use one name.
- **N3.** ARCHITECTURE §5.1 says the helper passes the ublk control device; the plan keeps it in
  the helper. design.md:187–188 should name §5.1 as text to change, not only "architecture".
- **N4.** design.md:23 "SO_PEERCRED plus pidfd": name `SO_PEERPIDFD` (Linux 6.5), since PID
  lookups by number are what the sentence warns against.

Reviewed commit: 9d1db414e1f83bc77be64b57f5abd48e079c4984
