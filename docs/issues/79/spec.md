# Issue #79: CSI mount retries accept the wrong source or incompatible read-only state — specification

Why: [intent](intent.md). Settled choices: [decisions](decisions.md), if created.
How: [design](design.md). Approved 2026-11-13; not implemented.

## Terms

- **Owner:** a durable association of volume ID, operation kind, canonical path, stage linkage,
  normalized capability, readonly requirement and the device/mount identities created there.
- **Live evidence:** kernel mount topology, effective mount/superblock flags and device identity;
  textual source names or a reused device number alone do not establish ownership.
- **Compatible retry:** the same owner and normalized request, with matching live evidence.
- **Permitted path:** a canonical absolute path within the configured kubelet CSI/pod roots,
  with the expected file/directory type and no symlink or magic-link traversal.

## The invariant

Success means this volume is exposed at the requested path with the requested capability and
access restrictions. Neither a retry nor cleanup may format, unmount, remove or detach another
volume or an object whose ownership is ambiguous. A durable record alone never authorizes a
mutation after live identity changes.

## Behaviour

**S1. Validate before side effects.** Stage, publish, unstage and unpublish validate volume IDs,
required paths and capability fields before device, filesystem or metadata mutations. All
control-socket volume arguments obey the metadata ID grammar, including non-CSI callers.
Paths outside permitted roots, noncanonical paths, symlinks, wrong types and conflicting flags
return `InvalidArgument`. Device replies must identify a real block device beneath `/dev` and
match authoritative volume/FirstEpoch/daemon/attachment/rdev/diskseq evidence before and after
opening it; retain that verified fd. Require #15 strict identity support, never legacy fallback.
Daemon dispatch validates IDs too. Path substitution or bad replies cannot redirect an effect.
Require #83 current-process signed handoff; an initialization operation cannot reuse its old epoch
after a daemon restart. Same-daemon plugin restart may recover its exact existing operation.

**S2. Ownership survives interruption.** Fsync an unbound operation intent before any attachment
or initialization RPC; bind its authoritative incarnation/lifetime before device effects. A lost attach
reply is recovered by the durable operation identity, never a volume-name guess. Persist ownership
before effects that need cleanup; finish it only after checking the effect. A plugin restart reconciles pending/completed records
with live evidence without relying on in-memory maps. Missing, corrupt, conflicting or obsolete
records never justify adopting a live mount or deleting an existing object. Inspection/persistence
errors fail closed. A permanent metadata per-incarnation use/initialization state, atomically
created with new-volume identity, supplies positive formatting authority across nodes. Local
profiles are caches, never authority; missing history, raw use, restore/import and legacy state
never grant first formatting. Keep history beyond path retirement. A format-start marker is durable before the tool; uncertain completion never permits another format.
An absent unowned mount is an idempotent cleanup success without removal. Durable inode numbers
alone cannot prove leaf ownership after interruption; preserve an ambiguous replacement leaf.

**S3. Stage compatibility.** Both block and filesystem staging reserve one owner for the stage
path. Exact retries succeed without another format, mount or attach. An occupied stage owned by
another volume, or the same volume with an incompatible capability, returns `AlreadyExists`.
An unexplained live identity change returns `FailedPrecondition`. Formatting is permitted
only with the metadata one-shot initialization grant for this verified incarnation, with no other stage, raw-block
profile, publication or conflicting live device effect anywhere. A fresh path cannot bypass this
check. Never format as retry repair or after an uncertain prior attempt; keep signature checks.

**S4. Publish compatibility.** Publish requires an owned, compatible stage for this volume and
its verified current device. Exact retries succeed; an occupied target with another volume,
capability, source or readonly request returns `AlreadyExists`, without remounting. An unowned
or invalid stage returns `FailedPrecondition`. Filesystem binds expose the stage root itself;
block binds expose the verified device. A second path follows the CSI access-mode table, including
`SINGLE_NODE_MULTI_WRITER`; single-writer ownership is enforced across recorded publications.

**S5. Enforce effective access.** Derive readonly for both block and filesystem from the request
and reader-only access mode; enforce it and effective permitted flags on every success. Conflicting
readonly/readwrite requests are rejected before mounting. Existing writable publication retried
readonly (or the reverse) fails S4; it is never silently converted. A new mount that fails
post-verification returns an error and cleans up only its own verified effects. Raw-block readonly
requires actual device write prevention; when that cannot be guaranteed, fail safely rather than
report success. Do not change a shared writable device to readonly behind another publication.

**S6. Unpublish only its owner.** Matching ownership and live identity allow unmounting and
removal of only the plugin-created empty file/directory. Keep recoverable state until cleanup is
verified complete. Another owner or unexplained live mount returns `FailedPrecondition` without
mutation. Repeat cleanup succeeds after interruption; an unrelated pre-existing file/directory is
left intact. An interrupted removal cannot use matching dev/inode/type alone to unlink a reused
leaf. Normal unmount releases mount-root references first and fails safely if busy or ambiguous;
never recurse, lazily unmount or affect stacked/child mounts not created here. Normal pathname
unmount requires the pinned kubelet serialization/path profile and enforced deployment ownership
contract in [the cleanup appendix](kubelet-cleanup-contract.md); unsupported profiles refuse startup.

**S7. Unstage only when unused.** Unstage checks stage ownership and refuses while publications
or unresolved owned mount effects depend on it (`FailedPrecondition`). Only matching live evidence
allows unmount; detach only the same verified volume/device after confirming no dependent mounts.
Detach atomically compares the expected complete lifetime at the daemon; name-only detach is
not allowed. Retries after unmount/detach interruptions complete safely. Another volume's stage,
device or mount is untouched, even if the request's volume currently exists elsewhere.

**S8. Upgrade and unsupported state.** Existing installations without ownership records are
not silently adopted. Document a quiesced upgrade/reconciliation procedure that establishes or
reconciles ownership/profiles with explicit checks, without deleting volume data. Unknown legacy
capability/history blocks formatting; state loss cannot reset first-initialization eligibility. Uncertain old mounts stop
operations with an actionable error. Changing configuration or boot/device identity cannot make
stale ownership authorize a new object. #70 extends the same contract to XFS/expansion; #74
preserves the ownership and path boundary when mounts move to a helper.

## Tests

- **S1:** unit/request tests for ID and line injection, malformed device replies, paths outside
  roots, traversal, wrong types, symlink ancestors/leaves, replacement races and unsafe flags.
- **S2–S4:** lifecycle tests with rich fake mount/device evidence; both audit reproductions,
  exact/normalized retries, cross-volume stage/target reuse, block/filesystem mismatch, differing
  access modes, raw stage A followed by filesystem stage B (zero probe/format), daemon
  status/open/status lifetime replacement, pre-signature format crash, restart and state failures;
  fresh-node/erased-cache raw-to-filesystem attempts, positive new-filesystem initialization,
  initialization CAS/auth/model-mutant checks (including independent public-admission bypass
  detection), daemon-process nonce changes and every before/after-attach crash boundary.
- **S5:** option/readonly mismatch tests and real disposable Linux bind/remount tests with the
  shipped CSI image: real ext4 loop stage/probe/mkfs/FD operands, native bind/unmount and cleanup;
  inspect flags and attempt writes. Reader-only block mode with Readonly=false still fails closed.
- **S6–S7:** cleanup tests for absent paths, another owner, stacked/child mounts, multiple
  publications, conditional-detach replacement before server execution, successful ordinary
  unmount after closing mount-root FDs, pinned kubelet concurrent operation/timeout ordering,
  denied external namespace writers, and identical-inode foreign leaf after an unlink crash.
- **S8:** upgrade and stale-state/boot tests; refused operations have zero destructive calls.

## Open questions

- Raw-block readonly enforcement: choose a proven per-publication device mechanism or explicitly
  restrict unsupported combinations. A VFS readonly bit is insufficient proof; no accepted risk
  permits silently writable success. The proposal records this unresolved release gate.
  Settled 2026-11-13 (user's instruction): neither is chosen in this plan; a later plan of #79
  chooses before S5's raw-block readonly is implemented, and readonly block publish fails closed
  meanwhile (see decisions.md).
