# Accepted with the plan, 2026-11-13: CSI mount ownership is durable and corroborated by the kernel

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#79](https://github.com/jsfakian/Flaminio/issues/79)

## Context

[Intent](intent.md) and [specification](spec.md) describe confirmed existence-only retry defects.
The node is privileged and can unmount caller paths. Process-local knowledge cannot protect
cleanup after a restart, and pathname checks do not protect an operation from symlink races.
Architecture §15.1 requires CSI path isolation; accepted ADR 0003 permits Linux 6.6 facilities.

## Decision proposed

- Persist per-path stage/publication ownership, normalized capability, operation phase and
  kernel/device evidence in a root-only, host-persistent versioned directory. Acquire one
  process lock; writes use file/directory fsync and atomic rename before dependent effects.
- Require both durable ownership and corroborating live evidence before retry success or
  destructive cleanup. Mount source strings, device numbers, mount existence and a record
  alone are insufficient. #15 commit 8af7aeb supplies the required authoritative FirstEpoch/boot/
  daemon-instance/attachment-generation/rdev/diskseq tuple and atomic conditional detach. This
  is an unimplemented dependency; #79 refuses legacy lifecycle fallback.
- Resolve permitted kubelet paths beneath pinned root descriptors without symlink traversal;
  pin device/source/parents through effects. Filesystem binds set detached-tree attributes before
  exposure. Linux6.6 normal umount2 is path-based: close all mounted-root FDs, use trusted pinned
  parent plus basename/UMOUNT_NOFOLLOW under the [pinned kubelet serialization/path and enforced
  deployment contract](kubelet-cleanup-contract.md), not unspecified privileged cooperation.
  Busy/unknown cases refuse; no invented fd-unmount, lazy detach or foreign mount teardown.
- Keep permanent cross-node metadata initialization/use authority, atomically initialized only for
  a new explicit filesystem volume; missing/local/legacy/raw history cannot grant first format.
  The [exact API/auth/model contract](initialization-contract.md) admits a private op-bound init
  attachment, then a one-time Start CAS; no ordinary Attach/token issuer can bypass it. Required
  Proposed #83 FLCH binds starts to a cryptographic daemon-process nonce and exact FLCT; changed
  nonce cannot recover Reserved credentials or reuse that epoch. Same-daemon plugin restart can
  recover its existing operation; daemon restart during initialization needs manual recovery. Local
  profile history is corroboration only. Persist unbound operation intent before attachment, bind
  its tuple afterwards, and recover lost replies by operation, never name. Persist format-start
  before mkfs; uncertain completion requires quiesced maintenance, never automatic reformat. Retain original created-leaf witness
  to unlink; after interruption preserve even an identical-inode replacement leaf.
- Refuse ambiguous legacy mounts, state loss and identity changes. Start the new state format
  through a coordinated quiesced upgrade, preserving volume data; unknown
  legacy history or state loss cannot reset initialization eligibility. #74 must preserve these checks
  if a helper later owns privileged operations; #70 must preserve them for XFS/expansion.
- Do not equate a VFS readonly mount with raw-block write prevention. The mechanism for raw-block
  readonly remains open; fail closed while unsupported. This proposal does not accept that
  incomplete capability as fulfilling #79's done criteria.
  Settled 2026-11-13 (user's instruction): neither is chosen in this plan; a later plan of #79
  chooses before S5's raw-block readonly is implemented, and readonly block publish fails closed
  meanwhile (see decisions.md).

## Options

- **Kernel inspection only:** handles live flags but cannot recover the intended volume/path
  relationship, creation ownership or interrupted cleanup reliably.
- **In-memory map only:** small change but loses the trust boundary on plugin restart.
- **Persistent intent plus live evidence:** adds a local state format and recovery logic; chosen
  as the proposed direction because neither source alone authorizes cleanup safely.

## Consequences

State durability adds host writes and an upgrade procedure. State loss can block operations until
quiesced reconciliation. Native mount calls need supported Linux wrappers and disposable kernel
conformance tests. Root on the host remains trusted; a tenant path cannot select another volume.
Additive local control identity/conditional-detach and daemon grammar changes are owned once in
#15. This R2 scope amendment adds bounded metadata state/RPCs, per-method authorization and
private initialization purpose admission, with #76 canonical-create/#80 size/identity integration.
It changes no replica frame/store format or steady-state I/O RPC/lock. At most one initialization
start per incarnation is a new control-plane safety invariant, checked by model/caught mutants.
Independent public-admission event assertions must catch the named init-gate-bypass mutant, even
if a downstream gate also refuses exposure. Local state stores operation/epoch/nonce, not an
unavailable authority revision; server-side revision CAS remains required.
The pinned kubelet profile excludes unqualified versions and other mount writers. Uncertain-format
markers need explicit maintenance; unprovable leaves may remain.

## Confirmation

The [design](design.md) maps S1–S8 to request/ownership/mount/restart tests. Both audit regressions
must fail under the old existence-only behavior. New-volume first format must positively succeed,
while raw-node A -> filesystem-node B with erased local state must perform zero probe/format.
Crash points, path replacement, mixed owners,
recycled device identities, exact conditional detach, pre-signature mkfs crash, inode ABA and
real readonly/native-unmount writes must be checked in the built CSI image before release. No approval,
accepted risk, implementation or successful privileged test is recorded by this draft.
Settled at approval, 2026-11-13: the plan and this proposal are approved; nothing is implemented or tested (see decisions.md, Settled at approval).
