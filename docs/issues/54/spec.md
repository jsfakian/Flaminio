# Issue #54: Restore to another cluster — specification

Existing [intent](intent.md)'s unencrypted new-volume restore increment only.
[Design](design.md), [contracts](contracts.md), [Proposed ADR](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review round 1
(decisions.md).
Encrypted recovery, metadata disaster recovery, Velero and full loss/ransomware runbooks remain
later #54 steps; the full issue stays open.

## Terms

- **ArchiveRef:** #52's `{repository_uuid,backup_uuid,version_id}` root identity and authenticated
  complete manifest graph;
  #53 decides trust/retention for an immutable repository, not current-version selection here.
- **Origin:** source cluster/volume incarnation and snapshot plus exact ArchiveRef, retained as
  provenance only. Destination identity/epochs/placement are newly allocated.
- **Restore operation:** durable destination job binding operation UUID, archive digest, destination
  volume FirstEpoch, requested placement and current writer generation/epoch.
- **Incomplete/Finalizing/Ready:** restore admission phases; only Ready permits ordinary attach.
  Deleting a restore is the header's Deleting mark, not a phase.
- **Home cluster UUID:** #52's `cluster` record, random, never supplied by a caller.

## The invariant

Until the complete unencrypted archive has been durably restored and verified on the destination
ISR, with restore-write admission closed, no ordinary user/admin/CSI attach or formatting path can
expose it. Stale writers/retries/completions cannot finish or alter another incarnation. The
operation copies archive data into a fresh volume; it never reuses source authority or claims
that an in-place metadata/disk reconstruction is safe.

## Behaviour

**S1. Validate before creation.** Authenticate/decode the exact #52 archive root/map, validate
format/geometry/zero semantics and confirm it is unencrypted before creating destination metadata.
Encrypted/unknown encoding is refused by archive identity, before volume creation. Require explicit
destination RF/minISR/domain/policy and a valid requested name (source name default; once #73's
tenancy is active a tenant-form name is refused, before any write, and another must be given:
R1-S2); use #80's checked geometry and #18's placement.

**S2. Fresh identity and placement.** Destination is a new volume: its own home cluster UUID
(#52's record, created when absent; an archive of this same cluster is refused), FirstEpoch,
node placement, ISR/version/target epochs and empty snapshot history. Copy only data and source
provenance, never source token/ISR/key/snapshot authority. Same name/epoch numbers in
two clusters cannot create an incremental-backup base match. Minimal origin identity is not a
new-authority recovery/fencing mechanism and does not certify restored metadata safety.

**S3. Claim and resume.** Atomically claim operation/name/incarnation and mark restore state in
the volume's FIRST creating header. Existing volume or tombstone refuses; only an exact same-job/
archive/properties retry resumes its own incomplete volume. Interrupted batched creation uses
#76's resumable conditional ownership, its create root binding the restore and every finalization
keeping the mark; no normal ready header appears between create and marking.
All progress/phase changes compare operation, incarnation, writer generation/epoch and revisions.
A completed/deleted/recreated job cannot re-create or write another volume under the same name.

**S4. Admission and restricted writer.** Ordinary Attach/ControllerPublish/NodeStage/NodePublish,
admin attach, #17's maintenance attach, snapshot/copy/move/growth and raw control attachment paths
refuse incomplete restores. #79's use state is Unknown from the first header, never FreshFs.
A specialized restore writer may attach only for that job/node under a fresh destination epoch,
with explicit purpose/ticket and local attachment identity checks. Worker I/O goes through normal
durable synchronous replication, not direct store writes. It obtains no source-cluster permission.
No plaintext/ciphertext conversion or arbitrary raw-disk/etcd commands implement the restore.

**S5. Exact data and sparse zeros.** Resolve the complete sparse archive map using exact versions;
verify AEAD/hash/bounds before writing. Write nonzero 4 KiB logical extents and leave untouched/
zero extents logically zero on the NEW sparse volume; no old destination data can leak through.
Destination allocation granularity may differ from source, explicitly qualified in the design.
Wait for all writes' ordinary durable ISR acknowledgements. Missing/corrupt object, no space or
worker/store failure leaves Incomplete with a named failing object/phase, never Ready.

**S6. Progress and interruption.** Durable progress binds verified archive ranges to this writer
and operation. Lost replies/restart revalidate identity and replay only safely; a new writer takes
a fresh epoch and passes ordinary takeover/fencing/reconciliation. Stale buffered writes and
completion proofs cannot cross job/writer generation. No local checkpoint alone authorizes
skipping verification after a new writer/ISR change. Operator can inspect/resume/delete incomplete
jobs; offline sources/S3 failures cannot turn a partial volume into a usable one.

**S7. Complete only after closure.** Freeze finalization membership under a conditional metadata
phase; close/drain restore-write admission and verify the whole logical image, including zero
ranges, on every current ISR member. Bind proofs to operation/incarnation/epoch/current ISR versions
and archive digest. Verify/drain/detach the restore device/writer before the final conditional
Ready transition; no later restore write is allowed. Membership/epoch/conflicting operation changes
invalidate proof and require a new safe writing/verification round, never partial Ready publication.

**S8. Delete and authorize.** Destination administrator may cancel/delete an incomplete job using
its exact operation/incarnation; cancellation invalidates writer/completion authority before normal
fenced deletion/tombstones. Do not wipe an active store or infer writer exclusion from liveness.
Only administrator (#73's `system-provisioner` once active) starts/restores; destination io
reports its own bound proofs/closure as specified. Any identity sees phase and operation only;
refs, progress and errors need GetRestore's roles. Existing service method authorization/
default-denial remains binding; no broad restore exception grants a worker arbitrary
attach/mutation. A restore's deletion follows #73's protection rules and events.

**S9. Repository-only source access.** Restore needs bucket read/list-version, the repository key,
and the store's CA, identity and a client credential that is no Flaminio identity of either
cluster; no source metadata or source-cluster identity, and no bucket write/delete/retention right.
The process that parses bucket input runs unprivileged and holds no destination authority. Pin
operation/archive references for future GC coordination, without mutating the bucket. In this
no-GC increment the repository only grows; a future collector requires a real read-pin contract.
#53 retained-version qualification is preserved; missing/unknown key/retention evidence is not
silently strengthened.

**S10. Command and compatibility.** One explicit command lists/selects an exact archive and restores
a new destination volume with versioned JSON/human progress/status. Names/paths/IDs/tickets are
validated; secrets are private file inputs, never arguments/log data. Under #76's maintenance
barrier, an explicit administrator step enables the feature after every metadata/node/CSI
instance runs this change; before it BeginRestore refuses; old/missing capability refuses instead
of using normal attach/mkfs. No source call, mount or format can be hidden in retry/cleanup.

**S11. Second cluster and remaining recovery.** Later lab uses a separately inventoried disk,
metadata directory/ports, UUID/trust domain/PKI/token key/control socket on fl-c, shared with #58's
planned helper. Never overwrite first-cluster disks/units or the only surviving copy. In-place
etcd restore/revision bumps, disk-only reconstruction, encrypted restore, force/data-loss recovery
and whole-cluster runbooks remain unimplemented; proposed audit ADR0008 is an explicit later gate.
A fixed revision bump, maximum sequence, arbitrary-disk majority or new UUID alone is not authority.

## Tests

- **S1–S3:** metadata/worker fixtures reject encrypted/malformed archive before any creation;
  the cluster record present, absent and equal to the source; #76 root/reconciler/abandoned-op
  cases; a tenant-form name once active;
  collision/tombstone, checked geometry/new placement, cross-cluster equal names/epochs, atomic
  incomplete mark, concurrent/lost creation replies, same-job retry and deleted/recreated names.
- **S4, S8:** mTLS/auth matrices and every CSI/raw attach path refuse partial restore; exact writer
  ticket only; foreign/stale operations cannot write/complete/delete another volume.
- **S5–S7:** memory-device/worker/model tests with sparse/nonzero data, missing/corrupt versions,
  every crash/checkpoint, lost acknowledgement, writer takeover and membership/proof races; Ready
  only after complete ISR data and writer closure. Inject early-Ready, stale-writer, old-generation
  proof, Finalizing ISR change, missing member proof and abort-without-invalidation bugs and catch
  them.
- **S9, S10:** read-only bucket credentials and source unavailable; exact-version/immutable handling,
  private-file/redaction/old-peer refusal, progress/error JSON and no silent normal-attach fallback.
- **S11:** later authorized second-cluster restore with source stopped; interrupted job unusable/
  resumable, taken name/encrypted archive/corruption refusals, data verified, rate recorded. No lab
  evidence is inferred from this draft or a protocol-only dependency.

## Open questions

Proposed restore state/writer/finalization protocol, minimal origin identity and source/owner API
seams need review. Physical-allocation equivalence is not guaranteed across destination layouts;
logical sparse-zero correctness is explicit. #52 repository/#51 snapshot/#50 identity drafts and
ADR0008/9/11 remain unaccepted; no broader disaster-recovery support or risk is approved here.

Settled at approval, 2026-11-13: the restore state, writer and finalization protocol as proposed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the minimal origin identity and the source and owner API as proposed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: logical zeros are preserved; physical layout may differ (see decisions.md, Settled at approval).
