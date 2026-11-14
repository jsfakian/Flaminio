# Issue #79: CSI mount retries accept the wrong source or incompatible read-only state — design

How the [spec](spec.md) is built. Settled choices: [decisions](decisions.md), if created.
Approved 2026-11-13; not implemented. [Proposal](adr-proposal.md), [initialization](initialization-contract.md), [cleanup](kubelet-cleanup-contract.md).

- `node.go:81–127,159–203` treats mount existence as success; `:130–156,206–217` cleans up
  without ownership. `mount.go:17–24,88–103` cannot inspect identity/flags; its fake loses them too.
- `dataplane.go:86–126` trusts status strings/IDs; Rust `control.rs:82–103` dispatches unchecked IDs.
  Node startup has no path/state policy; #15's required secure lifetime profile is not implemented.
- [CSI v1.13](https://github.com/container-storage-interface/spec/blob/v1.13.0/spec.md#nodepublishvolume)
  defines retries; a successful mount command alone does not prove source/flags/ownership.

## Inputs and pinned paths (S1)

- Add `Node.Paths` and `Node.Owners`, required at startup. Node flags: `--kubelet-root`
  (default `/var/lib/kubelet`) and `--node-state-dir`
  (default `/var/lib/kubelet/plugins/csi.flaminio.io/node-state`). Mount the plugin directory
  separately at that host path; `/csi` stays the socket mount. Require trusted root-owned/non-writable
  roots/parents and the intended mount namespace; reject symlinks and state inside a publication.
- Permit staging below `<root>/plugins/kubernetes.io/csi/`; permit publications below that
  subtree (block) or `<root>/pods/` (filesystem), restricted to appendix templates/mappings. Reject
  state/socket paths and an existing owner path of another kind. Validate kubelet path shape
  and leaf type before opening (directory for filesystem, regular file before block bind). No
  ancestor may be tenant controlled.
- Add `paths_linux.go`: resolve with `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|
  RESOLVE_NO_MAGICLINKS)` from pinned root descriptors; create missing components via pinned
  parents and re-open without links. Pin source/device/parents for effects; release mount-root
  inspection FDs before ordinary unmount below. Reject unknown ancestor/child mounts. Non-Linux
  builds get an unsupported runtime stub; no check-string-then-reopen subprocess path.
- Require #15's [device lifetime contract](../15/device-lifetime-contract.md), commit `8af7aeb`
  in its mapped worktree: unimplemented dependency, not a property of baseline code. Device-info
  supplies expected `(volume,FirstEpoch,boot,instance,generation,rdev,diskseq)` from its held fd.
  Open no-follow, compare block/rdev/diskseq to that reply, then repeat device-info and compare
  the whole tuple; retain fd through effects. Never learn expected identity from the opened path.
  Unsupported/changed/stopping replies fail closed before probe/mkfs/mount/bind; no legacy fallback.
- Validate metadata ID grammar in Go AND Rust daemon dispatch before registry/engine effects:
  attach, detach, conditional detach, delete, status VOLUME and device-info; strict arity/integers.
  Go rejection alone does not secure direct socket callers; share exact Go/Rust boundary vectors.
- Normalize capability into `{access_type, access_mode, fs_type, flags}` (default ext4).
  Allow ro/rw,nosuid,nodev,noexec,noatime,relatime,strictatime,nodiratime; reject conflicts/control
  bytes/commas. Filesystems always nosuid/nodev; block never nodev. No secret logging.

## Ownership and crash boundaries (S2, S8)

- Add `owners.go`: one versioned JSON state per canonical path, named by SHA-256(path),
  root-only files0600/directory0700; verify stored path matches key. Lifetime flock on
  `node-state-dir/lock`, with Node.mu held per RPC.
  Profiles use profiles/SHA256(volume + ':' + decimal FirstEpoch); same private write/lock protocol.
- Persist by exclusive temporary creation, write/fsync, rename and fsync directory. Treat missing
  or malformed fields, unknown schema versions and hash/key conflicts as errors. State errors
  return `Internal` and perform no new mount/format/remove/detach. Never overwrite corrupt state.
- Fsync `reserved_unbound` intent (operation UUID, volume/FirstEpoch, canonical path, normalized
  request, nullable tuple) BEFORE any metadata initialization/attach or daemon attach. Read-only
  GetVolume/preflight may precede it. Then obtain/query the same operation's authority, fsync its
  grant, attach, perform #15 status/open/status and fsync `bound` tuple before probe/tool/mount.
  Binding/profile-write failure leaves the original intent; reconciliation queries operation plus
  initialization-status, never guesses a lifetime by name. Exact lost-reply/crash rules are in
  appendix, composing required Proposed #83 FLCH at5a2fcd8. Publish reserves before device().
- Permanent metadata use state, not local profiles or FirstEpoch alone, grants first formatting.
  FreshFs is established atomically only for a new explicitly filesystem CreateVolume; ordinary
  Attach cannot expose FreshFs/initializing. A one-shot CAS marks Started before a tool is allowed;
  local started is also fsynced first. Uncertain completion is never automatically retried, on
  this or another node. Profiles retain access/history as corroborating caches beyond cleanup.
- Keep a volatile fd witness for each exclusively created underlying leaf before mounting;
  it pins that leaf's inode until cleanup. It is not an fd on the overmounted volume root.
  Persist removing before unmount/unlink. After restart/interruption without that original witness,
  preserve any existing leaf, even matching dev/inode/type; retire path state only once mount and
  device cleanup are verified. Inode ABA cannot justify a second unlink after an unlink crash.
- Include boot ID and mount-namespace identity. Mount IDs are only comparable within their
  recorded namespace; after a plugin namespace restart, recover them only from the same boot,
  device lifetime, filesystem/root identity and path/flag evidence, with no stacked ambiguity.
  Old-boot records cannot authorize a live object; stale absent records can be retired safely.
- Quiesced upgrade drains old driver and upgrades all metadata/daemon admission paths together.
  Missing/legacy/imported history remains Unknown, not FreshFs. Existing recognized filesystems
  can mount without format under Unknown; raw/FS conversion is refused. State loss never resets
  authority. Unrecorded live mounts still fail; no automatic mount adoption or teardown.

## Inspect, stage and publish (S3, S4)

- Replace `IsMountPoint` as the decision API with `Inspect(target)` returning all matching
  mountinfo entries and their topology, mount ID, root, filesystem, source, VFS/super options
  and stat identity. Strictly parse the mountinfo separator/escapes and reject malformed or
  stacked entries. Source strings are diagnostic; compare device lifetime and mounted root.
  For block bind mounts, mountinfo's device may be devtmpfs: stat the bound node and compare
  its `rdev` and device diskseq. For filesystem binds compare stage filesystem, root and inode.
- Before device/probe/format, scan all path records for this volume/device, including preparing
  and raw-block stages with no mount, plus live filesystem/file-bind device uses. Another stage
  path, incompatible incarnation profile, publication or unresolved effect blocks new initialization.
  Raw stage A -> fresh filesystem stage B is AlreadyExists with zero blkid/mkfs. Repeat after the
  authoritative device pin. Ready retry checks the full tuple/mount; unexpected reuse fails.
  New filesystem stage: global preflight/read-only incarnation -> durable unbound intent ->
  metadata reserve/grant -> attach -> verified pin/bind -> probe -> one-shot start/tool/finish
  only for FreshFs+blank -> create leaf -> mount -> inspect -> ready. Existing/Unknown never mkfs.
- Block staging reserves and records the stage relationship/device even though no stage mount
  exists; publish resolves it and all other publications, never an arbitrary directory. Same-target
  incompatible retries are AlreadyExists; forbidden second targets/capability changes follow the
  CSI table as FailedPrecondition.
- New publish records intent and leaf identity, then clones the verified source with
  `open_tree(OPEN_TREE_CLONE|OPEN_TREE_CLOEXEC|AT_EMPTY_PATH)`, sets VFS attributes on the
  detached mount with `mount_setattr`, and attaches via `move_mount` using pinned FDs and
  empty-path flags. Inspect after attachment before marking ready. No visible writable bind
  interval is used for filesystem readonly. Missing required syscalls fail closed.
- Keep util-linux for filesystem probing/format/stage mounting, with pinned descriptors inherited
  as child FDs; pass `--` before operands, disable helpers/canonicalization/mtab updates for mount,
  and reject caller-selected helpers/operations. Prove these FD operands against shipped tools,
  not only a fake or tmpfs. Unmount has the different concrete procedure below.

## Access and cleanup (S5–S7)

- Derive effective readonly for BLOCK AND FILESYSTEM as request OR reader-only access mode; a contradictory
  explicit `rw` is `InvalidArgument`. Compare requested flags with observed VFS/superblock flags,
  not the command arguments. A ready retry never remounts. Roll back a failed new publication
  only when current evidence still matches the new mount; otherwise retain state and report it.
- Raw-block readonly is an unresolved mechanism gate: a mount flag alone is insufficient.
  Until a tested mechanism is approved, return `FailedPrecondition` for readonly block publish,
  before exposure; never global BLKROSET on shared ublk. This unresolved support is not completion.
  Settled 2026-11-13 (user's instruction): neither a mechanism nor a restriction is chosen in
  this plan; a later plan of #79 chooses before S5's raw-block readonly is implemented, and this
  interim refusal holds until then (see decisions.md).
- Normal Linux6.6 unmount uses [umount2](https://man7.org/linux/man-pages/man2/umount.2.html),
  not an invented fd syscall: under flock/Node.mu, confirm namespace/sole mount mutation ownership,
  trusted non-tenant parent, exact unique mount ID/source and no children. Retain parent fd outside
  the volume; CLOSE all fds on the mounted root (including inspection/child duplicates), then call
  umount2(`/proc/self/fd/PARENT/BASENAME`, UMOUNT_NOFOLLOW). No lazy/force/recursive flags.
  The concrete v1.35.0 operation/path and enforced deployment contract is in the cleanup appendix:
  kubelet removes block extra refs before RPC, serializes conflicting work, and removes filesystem
  parents only after success. Do not run under another profile or with other admitted mount writers.
  Recheck mount absence; EBUSY/error retains state. Test actual success with inspection FDs closed.
- Unpublish preserves foreign/pre-existing leaves. Unlink only with the continuous original
  created-leaf witness, matching no-follow parent-relative leaf and exclusive mutation ownership;
  after interruption preserve ambiguous leaves and report cleanup residue. No recursive deletion.
- Unstage refuses publications/dependent effects; after verified unmount call #15 detach-device
  with the record's entire tuple. The daemon compares it atomically under registry/entry locks
  before routing stop to that captured generation. Absence is success; replacement is stale and
  untouched. Lost reply retries conditional detach, never `detach VOLUME`. Keep ambiguous state.

## Files touched

- `control/csi/node.go`: S1–S8 lifecycle; mount.go/new mount_linux.go/paths_linux.go + stubs:
  S1, S3–S7 inspection, pinned paths, readonly binding and device evidence.
- New `control/csi/owners.go`, `owners_test.go`, `mount_test.go`, `node_ownership_test.go`:
  S2–S8 state machine and failure/restart/ownership coverage; extend `csi_test.go` fakes.
- `control/csi/dataplane.go`, tests; Rust `flaminio-io/src/control.rs`/tests: S1 authoritative
  identity/conditional-detach client, direct daemon grammar; shared lifetime implementation is #15.
- `control/cmd/flaminio-csi/main.go`, tests; `deploy/csi/driver.yaml`: S1–S2, S8 wiring/state mount.
- `api/proto`, generated Go/Rust clients, meta/domain/service/auth/KV and init model: S2–S3, S8
  initialization authority/#83 process handoff; controller create reservation; engine init purpose.
- `deploy/csi/` admission/RBAC/profile and disposable `scripts/tools/` tests: S5–S8 validation.

## Contracts

- CSI request/reply protobufs unchanged. `InvalidArgument`: invalid inputs/options;
  `AlreadyExists`: incompatible occupied stage/target; `FailedPrecondition`: unsafe ownership,
  stage/dependency or unsupported readonly state; `Unavailable`: dataplane unavailable;
  `Internal`: inspection/state/system failures. Successful absent cleanup never removes a leaf.
- State schema v1 includes canonical path/kind/stage/request, operation UUID/phase, created-leaf
  evidence, full #15 lifetime and boot/namespace/mount topology/flags. reserved_unbound alone may
  have nullable tuple; bound precedes effects. Store process nonce, not a nonexistent RPC revision.
  Exact metadata/daemon schemas, auth and #76/#80 integration are in the initialization appendix.

## Tests

- **S1:** Go/Rust direct-socket grammar/arity/u64 tables; malformed expected tuples; V status ->
  V dies -> W at same path -> open W; and newer V; require zero probe/format/mount/detach.
- **S2–S4:** raw stage A -> fs stage B and incompatible profile after path retirement (zero mkfs);
  pre-signature killed mkfs -> blank retry still refuses; fsync/state errors and namespace restart.
  Fresh-node/erased-profile, one-shot metadata authority, per-method auth/model mutant and attach
  lost-reply/process-restart and caught public-init bypass mutant follow the appendix; keep other cases.
- **S5:** matrix block/fs x all supported modes x Readonly bool x ro/rw; reader-only block with
  Readonly=false reaches unsupported-RO gate before exposing a device. Verify effective flags.
- **S6–S7:** pause conditional detach before server dispatch, replace V, assert new lifetime/engine
  untouched; lost reply retry. Crash after unlink before state deletion, recreate foreign leaf
  with identical dev/inode/type, retry preserves it. Test busy/successful unmount and the exact
  kubelet ordering, deadline/retry serialization and denied competing writer deployment cases.
- **S1–S8:** opt-in `make csi-mount-test` in the built CSI image on disposable ext4 loop:
  exact shipped blkid/mkfs.ext4/mount inherited-FD operands, stage/publish, writable/RO writes,
  unpublish/unstage via native umount2 and complete loop cleanup; log image digest, kernel and
  tool versions. Raw-block test attempts writes, not merely flags. Inject tool/cleanup failures.
- Planned checks: `make proto lint test race hygiene contracts sim`; new init-authority model and
  caught mutants, with no steady-state I/O RPC/lock or replica membership rule change.
  Only documentation checks ran for this review fix; runtime/kernel/image checks remain future.

## Lab

No lab ran. Later opt-in image/loop test owns and cleans only disposable image/mount/loop objects.
Authorized post-implementation `make csi-test` records PVC deletion/restoration before movement.

## Docs, risks and ADRs

Update lab/help for #15 rollout, manual format repair, residual leaves, kubelet profile/image test.

Open risks: block readonly and kernel/image conformance remain unverified; state loss/profile
maintenance can block cleanup and recovered leaves may remain. Only the appendix kubelet profile
is supported initially; other versions need a new pinned source/test qualification. Host root is
trusted (#74); initialization service/daemon, #76/#80 and required #83 handoff are not implemented.
Settled at approval, 2026-11-13: these consequences and the single kubelet profile are accepted; raw-block readonly: Settled 2026-11-13 (user's instruction): neither is chosen in this plan; a later plan of #79 chooses, with the interim refusal meanwhile (see decisions.md).

Accepted ADRs0001–0006 remain consistent: languages/etcd/Linux baseline and replication unchanged.
Audit ADRs0007–0011 stay Proposed. The [ADR proposal](adr-proposal.md) records ownership/profile/
cleanup/global-authority choices; #15 lifetime, metadata/model and image conformance remain required.
Settled at approval, 2026-11-13: the ADR proposal and both appendix contracts are accepted with the plan (see decisions.md, Settled at approval).

## Parallelizable?

Go/Rust join #15 and initialization contracts; completion needs model/image/deployment checks.
