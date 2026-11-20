# Issue #74 plan review: round 1, security focus

Scope: `docs/issues/74/{intent,spec,design,adr-proposal}.md`, checked against the worktree code,
libublk 0.4.8 / libublk-rs-sys 0.1.2 in the cargo registry, and a Linux 6.8.12 source tree found on
this host (`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`; 6.8 is the Ubuntu 24.04 kernel
ADR 0003 lists). The issue body was not read (no network commands); "Done when" is taken from
intent.md and ARCHITECTURE §5.1, §15.5–15.6. Paths below are relative to
`/tmp/flaminio-plans-2026-11-12/74` unless absolute.

### R1-S1 — High
**The root CSI node plugin trusts the device path the unprivileged I/O process answers.**
The spec's invariant (spec.md:19–22) only forbids turning *the helper* into host privilege. The
CSI node plugin takes the device string from the data plane's control socket unvalidated
(`control/csi/dataplane.go:86–98`, `control/csi/node.go:58–78`) and then runs `blkid`,
`mkfs.<fs> -F ... <device>` and `mount <device>` as root (`control/csi/node.go:107–126`,
`control/csi/mount.go:40,59–65`). Today that socket is served by a root process
(`dataplane/crates/flaminio-io/src/control.rs:9–10`). After #74 it is served by the process the
plan assumes compromised: answering `ok /dev/sdb` (or any unsigned host disk) makes root format
and mount a foreign device into a tenant pod. The design only says "Keep existing public control
socket compatibility" (design.md:167) and "CSI node mounting retains explicitly bounded mount
privilege" (design.md:70).
**Fix:** add a spec rule that every privileged consumer of I/O output (CSI node plugin, agent,
claim job) treats it as untrusted. The plugin must confirm the device with the helper (ledger:
volume/first_epoch/epoch -> ublk id; block major is ublk's; path matches `/dev/ublkb<N>` exactly)
before blkid/mkfs/mount. Add a test with a lying control socket (foreign path, `-`-prefixed
string, another volume's ublk id).

### R1-S2 — High
**io_uring descriptors cannot be sent with SCM_RIGHTS on the supported kernels.**
The S4 approach is: the broker creates rings disabled, restricts them, enables them and returns
"scoped queue/restricted-ring FDs" over the helper socket, while I/O's seccomp denies
`io_uring_setup` (design.md:75–80, 162–163; adr-proposal.md:19, 22). The kernel refuses it:
`net/core/scm.c:107–111` in the 6.8.12 tree, "don't allow io_uring files", returns `-EINVAL`
(upstream since 6.7; to my knowledge also backported to the 6.6.y and 6.1.y stable series, not
verified here). So hardened mode would be "unsupported" on every ADR 0003 kernel, or the
implementer re-allows `io_uring_setup` and S4's protection is gone. Two further gaps in the same
mechanism: (a) a SINGLE_ISSUER ring binds its submitter to the task that enables it
(`io_uring/register.c:234`), and the code builds such rings (`flaminio-io/src/ublk.rs:1079–1083`),
so "broker ... then enables them" locks I/O out; (b) no helper op supplies the store's two rings
(`flaminio-uring/src/uring.rs:138–143`): OpenDisk "returns raw FD" only, so a replica-only node
has no ring source.
**Fix:** let I/O create its own rings (`R_DISABLED`, register restrictions, enable in the
submitting thread), then install a stricter second-stage seccomp filter that denies
`io_uring_setup` and unsafe `io_uring_register` opcodes; size a ring pool from the grant quota
for later attaches. Drop ring FDs and the dropped-UID ring worker from the protocol, and restate
the S4 tests (a new ring after lock-down fails; a forbidden opcode on an existing ring fails).

### R1-S3 — High
**Unprivileged ublk ownership removes user recovery and zero copy.**
design.md:42–44 creates devices with "kernel unprivileged per-device ownership" from a worker
dropped to the I/O UID, yet keeps a Recover op and says "Recovery follows #15/#48" (design.md:53,
159). For `UBLK_F_UNPRIVILEGED_DEV` the kernel clears `UBLK_F_USER_RECOVERY` and
`_REISSUE` silently (`drivers/block/ublk_drv.c:2391–2415`, 6.8.12) and zero copy is "Not
available" (`libublk-rs-sys-0.1.2/ublk_cmd.h:223`), which `--zero-copy` uses
(`flaminio-io/src/ublk.rs:906–910`). This contradicts ADR 0003 (Accepted: depends on
`UBLK_F_USER_RECOVERY`), ARCHITECTURE §5.1 and #48's draft ("Every device is created
recoverable", `/home/jsfakian/Documents/src/flaminio-48/docs/issues/48/intent.md:79`). Also, a
creator holding CAP_SYS_ADMIN has the flag cleared (`ublk_drv.c:2391–2392`), so the helper
parent cannot create such a device itself.
**Fix:** the helper (CAP_SYS_ADMIN) creates an ordinary privileged device, opens `/dev/ublkcN`
and passes that one FD; I/O needs no capability to fetch and commit (an unprivileged daemon only
suppresses partition scan, `ublk_drv.c:1689–1694`); the helper issues START_DEV with the peer's
pid. State that the helper keeps SYS_ADMIN. If unprivileged ownership stays, write the superseding
ADR and reconcile with #48.

### R1-S4 — Medium
**Peer authentication is unspecified where it matters, and the socket sits in a shared directory.**
design.md:16–18, 22–24 checks "cgroup membership against root-controlled registry", but the I/O
pod's cgroup path changes with every pod (pod UID, container id) and nothing says who writes it
or how the helper tells the I/O container from another container of UID/GID 2000 or 0.
`/run/flaminio` is already hostPath-mounted into the CSI node pod
(`deploy/csi/driver.yaml:132,167`) and #12's data-plane pod, and #12 records that a hostPath
PersistentVolume reaches it from any namespace
(`/home/jsfakian/Documents/src/flaminio-12/docs/issues/12/spec.md:48`), so S5's "Tenant pods
cannot reach helper sockets" rests on that unspecified check. A root-owned 0750 parent also stops
UID 2000 from creating `/run/flaminio/node.sock` (`flaminio-io/src/node.rs:82`).
**Fix:** put the helper socket in its own directory that only the I/O container mounts; name the
identity mechanism (for example a per-pod socket or directory the installer creates, or a
kubelet-verified pod UID -> cgroup lookup) and who maintains it; add the tenant-pod and
hostPath-PV negative tests.

### R1-S5 — Medium
**The registry's write path, the most sensitive input, has no design.**
"an administrator-controlled claim/format/release job updates registry through a narrow verified
action" (design.md:18–19) names no channel, authentication, authorization or validation (how a
root/system disk is refused at claim time, S1), and helper protocol v1 has no such op
(design.md:158–160). "Agent readonly sysfs discovery has no caps" (design.md:69) is not what the
code does: the agent's probes open and read each candidate device (`control/agent/host.go:18–47`:
`blkid -p`, `flaminio-disk inspect`, `os.Open`), which needs root or the `disk` group (raw
read/write on every host disk).
**Fix:** specify the claim/format/release action (who may call it, how it is authenticated, its
checks, its audit record) and move the device-reading probes behind it or behind a read-only
helper op; add tests for an unauthorized claim and a system-disk claim.

### R1-S6 — Medium
**The registry pins values that do not survive a reboot.**
The registry maps UUID -> "canonical dev_t" and "pinned registry paths", opened with no symlinks
and "never retry against another pathname" (design.md:16–17, 28–31). Block dev_t and `/dev/sdX` /
`/dev/nvmeXnY` names are assigned per boot, and stable names (`/dev/disk/by-id`) are symlinks the
open rules reject. After a reboot or hotplug the helper either refuses the claimed disk for good
or takes a transient `O_EXCL` claim on a foreign disk before the identity check fails.
**Fix:** key the grant on the superblock `disk_uuid` + `cluster_uuid`
(`docs/format/on-disk-format-v1.md:87–88`) and a hardware id; resolve the device per boot from
sysfs, record dev_t for that boot only, and verify identity after open as designed. Test a
renumbered disk.

### R1-S7 — Medium
**What "stale" means per operation is undefined.**
Records bind `first_epoch, epoch` and S1 rejects "stale grants" (spec.md:27–30; design.md:36–38),
but the epoch changes on every Attach (`docs/replication-protocol.md:28`) and tokens never expire
(`pkg/captoken/captoken.go:28`). As written: a deposed target cannot Stop/Delete its own device
(its grant is stale); #48's recovery under a fresh epoch presents an epoch above the record's;
a replayed old token is stopped only if the metadata read is mandatory and fails closed, which
the design does not state.
**Fix:** a table per op: which of {token signature, token epoch vs record, metadata epoch and
target, instance nonce} is required, what updates the record, and that Stop/Delete of an owned
instance need no current epoch. Test a replayed token, a deposed target's delete and a
fresh-epoch recover.

### R1-S8 — Medium
**Ledger crash ordering and the "kernel identity" check are not specified.**
The ledger is durable only "before control acknowledgement" (design.md:49–50). A crash between
ADD_DEV and the write leaves a device with no record, quarantined for good, and the retried
request creates a second one; a crash between DEL_DEV and the write leaves a record for a free
id that a later device reuses. "inspect kernel identity" (design.md:50–51) names no attribute:
the kernel stores no caller cookie for a ublk id, so nothing ties an id to the instance nonce
after a helper restart, and nothing invalidates the ledger after a reboot. The ledger and
registry also have no format version.
**Fix:** write an intent record before each kernel create/delete and resolve intents on restart;
put the ledger under `/run` or bind it to the boot id; state the identity test (helper-exclusive
id allocation plus named GET_DEV_INFO fields); version both files and refuse unknown versions.
Test a crash at each step.

### R1-S9 — Medium
**The CAP_SYS_ADMIN broker becomes a network client.**
design.md:37–39 has the helper read metadata over mTLS before granting control, which puts a
TLS/HTTP2/gRPC stack in the privileged process and makes attach and recovery depend on that
path, against S3 ("to its actual needs") and S5 ("exclude the helper from unneeded network
access"). Tokens already name volume, epoch, node and first epoch and are Ed25519-signed
(`pkg/captoken/captoken.go:43–51`), so the signature can be checked offline. "#16 supplies
distinct helper/IO identities" (design.md:93) is not in #16's plan (no helper identity under
`/home/jsfakian/Documents/src/flaminio-16/docs/issues/16/`); no timeout bounds the read; egress
limits for a host service are not a NetworkPolicy matter.
**Fix:** verify the token offline and state what the metadata read adds, or keep it and specify
the identity (issuer, component name, the `auth.go` rows it may call, a test that every mutating
RPC is denied), its timeout, and the host-level egress rule.

### R1-S10 — Medium
**verify.sh has no stated trust anchor.**
`release-manifest.json` carries the subject list, "achieved SLSA status" and the runtime-profile
matrix (design.md:143–147) but is not named among the signed objects (design.md:125–127), and
`--policy FILE` has no stated origin. Whoever serves the manifest can drop a subject, raise the
status or replay an older release. Keyless signatures cannot be revoked, and S10's "revocation
response" has only a documentation line (design.md:187).
**Fix:** sign the manifest as a subject and verify it first; say where a consumer gets the policy
(pinned protected ref) and add a revoked-digest list to it; add fixtures for an edited manifest,
a dropped subject, an older manifest and a revoked digest.

### R1-S11 — Medium
**The runtime gate's evidence has no trustworthy source.**
S10 gates promotion on runtime checks, and the design runs them on "disposable hosts with
explicit privileges" (design.md:180–181) without saying where, or how a result is bound to the
digests being promoted. The only ublk-capable CI today is persistent self-hosted runners that
also run `pull_request` code (`.github/workflows/ci.yml:6,24`), which the design excludes for
builds (design.md:103–104).
**Fix:** name the runner class for the S3–S6 matrix, require it to test the exact subject
digests, and record the result as a signed attestation that the promotion job verifies.

### R1-S12 — Medium
**Installing the root helper is assigned to a plan that does not have it.**
"Run helper as a host service installed by #43" (design.md:15–16): #43's plan has no host
service, unit or LSM-module install (`/home/jsfakian/Documents/src/flaminio-43/docs/issues/43/`),
and a Kubernetes operator can only do it through a privileged pod with the host filesystem, the
thing the same sentence rules out. #43 also records that the operator's account can run a
privileged pod on every data node (its spec.md:98), a retained path to host root that S3 must
list.
**Fix:** specify who installs and upgrades the helper, its units, udev rules and LSM modules, with
what authority, and list that authority among the retained privileges; align with #43.

### R1-S13 — Medium
**Rules and abuse cases without a test.**
The table (design.md:170–177) has no test for: S5 tenant pod -> helper socket and node keys;
S10 refusal of an unknown protocol version or incompatible grant; S10 packaging that restores
`privileged`, host `/dev` or tag-pinned images (a static manifest check); S2/S4 startup refusal
when ownership or restrictions are missing; a replayed request id after the grant changed.
"untrusted PR credential tests" names no test: a script test under `scripts/tools/` can assert
that `release.yml` has no `pull_request`/`pull_request_target` trigger, grants `id-token: write`
only to the signing job and pins every action by commit.
**Fix:** add these rows with test names.

### R1-S14 — Low
**Client messages may carry descriptors.**
Protocol v1 allows "at most 16 SCM_RIGHTS FDs" in either direction (design.md:158). No request
needs to hand the root helper a descriptor.
**Fix:** requests carry none: receive with a control buffer, close whatever arrives and close the
connection. With R1-S2 and R1-S3 fixed, replies carry one FD each, so the 16 can go (it is also
below the 64 queues `flaminio-io/src/node.rs:201–204` allows).

### R1-S15 — Low
**A handed-out disk descriptor outlives its grant.**
"already transferred exclusive FDs remain owned by IO until close" (design.md:52–53); the release
and crypto-erase path does not say how it knows I/O let go.
**Fix:** release/format opens `O_EXCL` and refuses on `EBUSY`, after stopping the I/O process;
test a release while I/O holds the FD.

### R1-S16 — Low
**"Independent" rebuild is not defined.**
A "second clean independent worker" (design.md:117) of the same workflow, image and cache passes
without proving anything.
**Fix:** state what must differ (runner instance, no shared cache, build path, clock, hostname)
and have `compare.sh` fail when the two builds report the same run environment.

## Nits
- design.md:167 "existing public control socket": the socket is root-only, mode 0600
  (`flaminio-io/src/control.rs:9`, :51); "public" misleads.
- spec.md:8, 28 "incarnation" against design.md:37, 159 `first_epoch`: the code's term is first
  epoch (`pkg/captoken/captoken.go:49`); use one.
- intent.md:10 and design.md:7 cite `ublk.rs:168–176`, the single-volume `serve` path; the node's
  device is created in `add_device` (`flaminio-io/src/ublk.rs:902–934`), the one to change.
- design.md:60 "No registry source is edited": ambiguous beside the helper's "registry"; say
  crates.io registry source.

Reviewed commit: 9d1db414e1f83bc77be64b57f5abd48e079c4984
