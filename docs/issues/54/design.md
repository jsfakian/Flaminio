# Issue #54: Restore to another cluster — design

[Spec](spec.md), S1–S11; exact [contracts](contracts.md); [Proposed ADR](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review rounds 1 to 5
(decisions.md). Unencrypted archive -> new destination volume only. Metadata/disk,
encrypted, force recovery, Velero and whole-cluster-loss runbooks remain unimplemented later steps.

The facts it starts from:
- Header has Creating/Deleting but no restore state (`meta/domain/domain.go:37–54`). Creation
  batches chunk records (`meta/service/service.go:375–478`); ordinary Attach (:699–751) always
  advances/grants target authority, with no incomplete-image gate. Go's `json.Unmarshal` drops
  unknown fields and Attach rewrites the whole header (:708, :722–724), so an older binary
  erases any new header field it writes back.
- CSI rejects content sources (`control/csi/controller.go:213`), but its stage device shortcut
  (`node.go:58–65`) can return an attached device without metadata. Stage may mkfs a blank device.
  Incomplete restore protection therefore must cover metadata, engine/control and CSI paths.
- `cluster_uuid` on newly formatted disks is NIL (`flaminio-io/src/cli.rs:83`). #52 adds the
  record `/flaminio/v1/cluster` and says #54 reads it; origin IDs identify lineage only. They do
  not implement new-authority recovery or disk admission.
- The control socket is root-only, mode 0600 (`flaminio-io/src/control.rs:9-10`). #52's worker
  runs as `flaminio-backup`, never root, and never holds an administrator pair; its `backup` pair
  and directory are that user's (#52 contracts.md:79-82).
- The tombstone sweep sends plain `delete VOLUME` for each tombstone naming its node
  (`control/csi/tombstones.go:40-47`, `dataplane.go:125-126`); the engine refuses it while the
  node is the volume's target (`flaminio-node/src/engine.rs:696-704`). DeleteVolume's tombstone
  names the chunks' replica nodes only and never looks at attachments (`service.go:516-585`).
- FLCT carries volume, epoch, node and first epoch, and no purpose
  (`pkg/captoken/captoken.go:12-22`); the control socket's `attach VOLUME TOKEN` takes it from the caller (`control.rs:84`).
- Clear and approved plans this one follows, none implemented: #52 (repository, worker, cluster
  record, store profile), #53 (retention checks, outputs), #73 (roles, owners, deletion events),
  #76 (creation roots), #79 (initialization), #17 (maintenance attach), #15 (control socket),
  #18 and #80 (placement, geometry and its reply budget).

## Source validation, origin and claim (S1–S3, S9)

**Two processes (R1-C1, R1-S1).** `flaminio-io restore-volume`, the helper, runs as root on the
selected destination node with the destination's administrator pair, the only process that calls
destination metadata or drives the control socket. It never reads the bucket or the repository
key. It starts `flaminio-backup restore-read` as a system user of its own,
`flaminio-restore-read`, never #52's `flaminio-backup` (R2-S3), under no-new-privileges, a
parent-death signal, its own session and memory, descriptor and core limits (R2-S5; contracts,
Two processes), with no Flaminio pair and no access to the helper's files, the socket, #52's
`backup` pair or its worker's directory: the reader reads the exact #52
ArchiveRef/root/page graph with read/list-version-only bucket credentials and the repository key,
authenticates and decodes it, and streams verified extents to the helper in the bounded frames of
contracts (The reader's stream). An archive crafted with a stolen repository key (#53's
ransomware case) can reach only that unprivileged decoder; what the helper accepts from it is
data written into the new volume, checked for bounds and order. #52's worker rules thus hold for
every process that parses bucket input; the helper is an administrator's command, like
`flaminio-meta`, and is no `flaminio-backup` command.

The reader validates envelope/geometry/data profile before its Header frame, so before the helper
calls metadata. It accepts only unencrypted stored-block encoding; an unknown/encrypted descriptor
refuses by archive ID before any volume claim (exit 3). It makes no source-cluster call and holds
no source-cluster identity. **#53 (R1-C12, R1-S7):** it runs #53's restore checks, the whole
graph's retention as well as its content, on an immutable repository's schema-2 roots, and its
Header carries #53's output fields, which restore-volume reports; exact versions, never a
current-name or automatically selected "newest" backup.

**Reaching the store (R1-C2, R1-S4).** The destination has its own trust domain and CAs, so the
reader's config names the store apart from both clusters: `store_ca`, the CA bundle of the store's
server certificate, used only by the S3 client and never added to a Flaminio bundle (a CA there
could mint destination administrator IDs, `pkg/pki/pki.go:244-283`); `store_identity`, required
and compared exactly, in whatever trust domain the store's certificate names; a store client pair
from the store owner's CA, whose URI lies outside `flaminio/`, so no Flaminio service of either
cluster accepts it; and a list/get-version-only S3 credential. S9 asks for no source-cluster
identity: this client pair is the store owner's credential, not a cluster's.

**Home cluster (R1-C3, R1-S3).** #52's `cluster` record is the home UUID, read by BeginRestore and
created by it when absent, with a CSPRNG value and a compare (contracts, Home cluster). No caller
supplies a UUID, and an archive whose source cluster equals the home one is refused, so a later
backup of the restored volume cannot share its source's `cluster_uuid` and match a base on equal
first epochs. It is metadata provenance only: no disk migration, rewritten superblock, token
invalidation claim or in-place etcd restore is introduced.

**Creation through #76 (R1-C5).** `BeginRestore` is a #76 creation with a restore binding:
- **Identity and root:** the root's immutable create identity gains `restore: {operation,
  archive_digest}` beside #76's `CreateSpec` and #79's `use_profile` (Unknown). An identical
  BeginRestore finds and helps that root; a CreateVolume with an equal spec has another identity,
  so it gets AlreadyExists and never returns the partial volume as compatible (#76 S1).
- **First header:** #76's admission writes the Creating header with `restore: {phase: Incomplete,
  operation}`, #79's `use` Unknown (R1-C6) and the job record, in one transaction (contracts,
  Transactions); #76 keeps that header byte for byte until visibility.
- **Finalization:** every helper, #76's reconciler included, finalizes a root that carries a
  restore binding into a visible header that keeps `restore` (Incomplete) and `use`, and writes
  the job's `first_epoch`, in the same transaction; it never yields an ordinary volume.
- **Operation ids:** the restore operation UUID is the caller's, reused by every retry; #76's
  create op is the service's, fresh per attempt. The job's `create_op` names the admitted one.
  An ambiguous first CAS abandons only that create op (#76's birth rule); the retry, same restore
  UUID, starts a new create op, and only one can be admitted for the name.
- **Records:** the job `restores/<op>` is written at admission, not before, so an abandoned
  create op leaves none. #76 collects its roots and plans as it does any; the job outlives them
  (contracts, Records).

Destination geometry/RF/minISR/domains use #80's checked geometry and #18's placement, never
source replica/epoch/token records. New FirstEpoch comes from the destination claim revision;
snapshot generation 0/list empty. Provenance stores ArchiveRef/source SnapshotRef but cannot be
used as an incremental base for this new home-cluster incarnation.

**Owner and name (R1-C8, R1-S2).** Once #73's tenancy is active, BeginRestore is a system
creation: `system-provisioner`, a `system` owner record in its claim (charging nothing, #73), and
a tenant-form name (`pvc-<UID>`, a CSI source's default) refused before any write, naming
`--name`, since a system creation cannot take one (#73 S3). Before activation it creates as
CreateVolume does today. A tenant-owned restore (a PVC from a backup) is #52's step 5 and step 4's
seam, not this increment.

The job record holds archive/ref/request digest, destination FirstEpoch, writer/finalization
generation, the proof cursor, progress and the terminal outcome. Exact same-operation retry
resumes; a foreign volume/tombstone or different archive/properties is AlreadyExists. A terminal
job is kept, so deleting/recreating a name cannot make an old job create or overwrite a later
volume (R1-C18: growth named, contracts, Records).

## Admission and specialized write authority (S4, S6, S8, S10)

Metadata ordinary Attach (both only_if_target/unless_target paths) refuses any non-Ready restore.
Snapshot (#51), MoveReplica, growth (#70) and clone admissions refuse it until Ready. CSI
ControllerPublish/NodeStage/NodePublish check typed restore state; dataplane `status VOLUME`
returns `restoring`, never an ordinary device for device()'s shortcut, and plain `status`
skips a restore attachment (R1-C15). `device-info VOLUME` answers a restore entry `ok none` and
`detach-device` `ok absent`, so #15's device check never takes a restore device; the plugin's
`Attach` then meets `reserve`'s `error: VOLUME is restoring here`, which it maps to
`FailedPrecondition`, as stopping (R5-C1). Every #15, #48 and #79 line's answer for a restore
entry is in contracts (A restore entry's lines). #79 ownership checks still apply. Do not bind/mount/mkfs or
return compatible existing mounts for a restore in progress.

**#17 (R1-C7).** The restore writer's epoch is a third kind, `restore`, which only
AcquireRestoreWriter makes; it is neither #17's client nor maintenance kind. #17's maintenance
attach and maintenance start refuse a volume whose restore phase is not Ready, as they refuse one
being created or deleted (#17 S3): a change to #17, made by whichever of #17 and #54 lands second.
A drain reports such a volume blocked until Ready; it never deposes the restore writer.

**#79 (R1-C6).** `use` stays Unknown through the restore: AcquireRestoreWriter is no mode
admission (its token admits only AttachRestore), so it seals nothing. After Ready, the first
ordinary attach seals Raw or ReadyFs and NodeStage mounts an existing filesystem, never mkfs
(#79 S3). #79's list of token-issuing paths gains AcquireRestoreWriter as one that leaves Unknown
unsealed, by whichever of #79 and #54 lands second.

`AcquireRestoreWriter` selects this node and CASes fresh destination epoch/writer generation. It
returns only a signed FLRT (contracts, RestoreTicketV1) binding job, volume incarnation (first
epoch), writer epoch, archive digest, node and writer generation; no FLCT (R2-S1). The FLRT is
the restore target's capability: replicas accept it as a target token; ordinary Attach (control,
engine, agent) refuses its magic; new `Input::AttachRestore` requires it, and restore-start
reserves a #15 entry, counted under `--max-volumes` (R4-C3), and the node's restore slot, then
starts only if GetVolume shows this operation Incomplete
at the ticket's epoch with this node as target (contracts, The restore target's only
capability). So the token can attach nothing in Finalizing or after Ready, though the replicas'
fence stays at the writer epoch until the first user attach. Restore target is not a
user-readable attachment; the helper receives its checked private ublk device, held by registry
instance/generation identity (#15). Its device is added unrecoverable, with neither ublk
recovery flag and no #48 block in its record, so an io restart ends it and #48 never keeps or
resumes it (R4-C2; contracts, Unrecoverable). Restores run in direct mode only (R5-C2): under
`--privsep` #74's helper `add` takes no flag from the child and, with #48, makes a recoverable
device it keeps, so `restore-list` and `restore-start` refuse there (`restore needs direct
mode`) and the helper exits 3 before any metadata call; restore under `--privsep` is deferred
to #74 and #48 (contracts, Direct mode only). With #12 (by whichever of #12 and #54 lands second, R7-C1), once a SIGTERM was requested,
restore-start refuses `error: VOLUME: the node is shutting down` before `reserve`, as #12's `begin_attach`
does, and the helper exits 1; the process waits for a live restore entry as for an attached
volume (R6-C2; contracts, A restore entry's lines). Only one restore/node in this increment; no old
attach/status command fallback on incompatible nodes.

**Ending attachments (R2-C2, R2-S2).** restore-close is idempotent per generation and returns no
nonce: io keeps the closure and reports it itself. restore-detach works in any phase and ends the
attachment first. It reports a closure only for an attachment through which a proof committed,
so only in Finalizing; any other answers `ok not-recorded` (R3-C3). Metadata decides
`superseded` (an aborted or completed job, a later generation) from the job record before any
header or `writer_node` compare, so a cancel and a moved writer's old node get `ok superseded`
where a proof committed through the attachment, and `ok not-recorded` otherwise, the usual case
in Incomplete (R3-C2, R5 Nit). Before restore-start, in every `resume` (a completed job's included, R4-C1) and in
`cancel`, the helper reads `restore-list` (R3-C1) and ends its node's restore attachment if it
is of its own job or of a terminal or superseded job; after that detach it retries
restore-start's `is stopping here`, `limit:` and `restore slot taken`, and restore-detach's `is
attaching` (R6-C1, R7 Nit), for up to 30 s, the same 30 s as #15's detach wait, since the slot and the count are freed only when the #15 entry ends (R5-C3;
contracts, Ending restore attachments). The abort's tombstone also names the
writer node, and the sweep's plain `delete` ends a restore attachment once GetVolume shows the
volume gone, Deleting or of another operation. An ordinary attach never ends a restore
attachment: #15's `reserve` refuses it, `error: VOLUME is restoring here` (R4-C1, round 3's
rule cut). A crashed helper, an abort with a live attachment and a writer moved to another node
therefore never leave deletion stuck; a moved-from node keeps a fenced attachment, which refuses
an ordinary attach of the volume there, until one of these runs there or io restarts (Risks).

Root/local control socket and private device are trusted host boundary, not tenant endpoints.
The helper performs aligned `O_DIRECT` writes through ublk/normal engine replication, then
`fdatasync`s and closes the device before restore-close, which refuses while the device has an
exclusive holder, a mount or another `O_EXCL` open (R1-C17, R3-C4). No direct raw-store
writes, source authority, repository key inside metadata, or new unauthenticated write listener
exists. All restore inputs/outputs carry job/writer/attachment
identity; stale callbacks/replies are ignored without freeing buffers before I/O ends. A
restarted/timed-out writer acquires a fresh epoch and passes normal full-ISR
fencing/takeover/reconciliation, never reuses its old ticket/epoch.

## Data, sparse output and interruption (S5, S6)

The reader resolves the full authenticated sparse map; GETs exact versions, verifies AEAD/keyed
hash before streaming. Process in logical order; write nonzero 4 KiB logical extents and skip
stored-zero extents on this fresh sparse volume. This preserves logical zeros and avoids writes
to never-written logical blocks; physical cluster allocation may differ with destination layout.
Do not claim byte-for-byte page-map/hole equivalence from #52's 4 MiB map. Explicitly retain that
qualification against the intent's “unwritten” wording for approval.
Settled at approval, 2026-11-13: this sparse-layout qualification (see decisions.md, Settled at approval).

Wait for ordinary durable ISR completions; the helper reports bounded progress with
ReportRestoreProgress, conditional on its writer generation (R1-C1). Local checkpoint records no
independent authority to skip data. On worker/process/epoch restart replay the verified image
(nonzero extents) and reverify all current members; optimizing resume is not needed for this
increment. Source S3 failure/corruption/no-space leaves Incomplete and names the
object/range/phase. No automatic rollback/deletion, source fetch substitution or format occurs.
A restarted operation cannot modify an already completed Ready volume, even after user writes.

## Frozen finalization and Ready (S7)

The helper computes each chunk's leaf from the bytes it wrote, zeros included, and their Merkle
root. After restore-close, `BeginRestoreFinalize` CASes phase Finalizing at the current
writer/epoch with that root and the chunk count. Freeze AttachRestore, ChangeIsr, moves and other
chunk membership changes during this phase; prefix client I/O is already forbidden. A failed
verifier/member can return to Incomplete only through a new writer generation/epoch,
invalidating ALL prior proofs, then resume normal membership/reconciliation safely.

Sans-I/O core owns restore write-admission/finalization/proof state; the engine only performs
I/O/reports. Node closes restore-write admission, drains all I/O/buffers/completions and accepts
only bound verification. For each chunk the helper sends `restore-verify` with the leaf and its
Merkle path (R1-C11); io reads the whole logical chunk, including holes, from EVERY recorded ISR
member through authenticated target sessions, compares unkeyed hashes with the leaf, and itself
calls ReportRestoreChunkProof (R1-C1). Node need not hold repository key. Ordinary CRC is
accidental-corruption evidence, not a malicious-replica authenticity claim. A proof is bound to
operation/FirstEpoch/epoch/writer/finalization generation/ISR version/member set/hash plus
expected-root inclusion.

A proof advances the job's cursor by one, conditionally on the unchanged Finalizing header, job
and chunk record; no per-chunk record is kept (R1-S10), and the final commit compares header and
job, not thousands of chunk keys. Duplicate proofs are idempotent, changed/missing/hash-mismatched
members block. Then restore-detach ends the restore target/device and io records the writer's
closure with ReportRestoreWriterClosed, bound to the finalization generation of the proofs made
through that attachment (R3-C3); no ticket can restart in Finalizing, since restore-start
reads the phase (R2-S1). That attachment held the node's only restore slot from its start,
read in Incomplete, to its end, after a proof committed in Finalizing, so no later attachment of
the generation can exist: a closure recorded in Incomplete, which round 2 allowed and which a
replayed restore-start would outlive, is cut. Only after the full cursor AND closure may
`CompleteRestore` CAS Ready/clear restore target, retaining origin/terminal operation. Ordinary
next attach takes a fresh epoch. This ordering prevents a late restore write between Ready and
the first user attach from changing the final image. Against another opener of the device, the
guarantees are admission closure (a later write, a writeback flush included, is refused) and the
proofs (a stray write that reached the replicas earlier mismatches the root); the `O_EXCL` check
only refuses a mount or another exclusive holder, and round 2's `fsync` and held descriptor are
cut (R3-C4).

## Cancel, read pins and permissions (S8–S10)

**Abort (R1-C9, R1-S5).** `AbortRestore` checks the job's operation and destination first epoch,
then performs DeleteVolume's deletion of that volume: its mark (with #73's compares and
`delete.admitted` event) also sets the job's outcome `aborted` and raises its writer generation,
so no later progress, proof, closure or completion of any writer commits. It needs no writer
closure: a writer still running, or on a node gone for good, is fenced from any later
incarnation of the name by #30's first epoch, and the tombstone removes its replicas, a forgotten
node through today's forced confirmation. A plain DeleteVolume of a non-Ready restore does the
same, reading the job key from `header.restore.operation` (Nit). Either call finishes a deletion
the other marked. The tombstone also names the job's writer node, whose sweep ends the restore
attachment (R2-C2). A restore still Creating (the job's `first_epoch` 0) is deleted through
#76's deletion of an incomplete operation (#76 S6, S8), whose revocation writes the job
`aborted` in the same transaction; AbortRestore then compares the job's `create_op` (R2-C6).
`cancel` ends its own node's restore attachment after AbortRestore. Never format/wipe a live
store as cancellation. All metadata writes after read are conditional; stale job/writer cannot
complete or delete anew.

**Roles (R1-C8, R1-S2).** Contracts' method table gives each RPC its rule before and once #73's
tenancy is active: administrator name before; once active `system-provisioner` for the restore's
own calls (the role a security administrator binds, which also obtains the writer token),
`cluster-admin` for EnableRestore, GetRestore for both; the job's writer node's `io` for its
proofs and closure; insecure instances refuse all. "Reader" is dropped: no such role exists. Any
identity sees the phase and operation id through GetVolume; the rest only through GetRestore.
AbortRestore's deletion writes #73's events and meets #73 S5: the volume is published by #73's
test once #76 finalized it, and assigned (`system`) once active, so a `system-provisioner`
deletes it without the override. One begun before activation is unassigned, as any older volume:
#73's `AssignOwner` (`system`) assigns it first.

**Rollout (R1-C10).** #76's maintenance barrier excluding old writers applies: an older metadata
binary would write a header without `restore`. The enabling step is EnableRestore, an
administrator's call made after every metadata instance, node and CSI plugin runs this change;
it writes `features/restore`, which BeginRestore requires. Before it every BeginRestore refuses;
old nodes answer the restore lines unsupported. The step and the barrier are documented with
#76's upgrade barrier, in the operator documentation. **Rollback (R2-C8):** `features/restore`
is never deleted, and an older binary drops `restore` when it rewrites a header, so the same
documentation says a metadata rollback first needs every restore aborted or completed:
ListVolumes shows no volume with restore phase Incomplete or Finalizing. A Ready header losing
`restore` loses provenance only.

Job references pin exact archive/key metadata until terminal resolution for future owner GC.
No bucket writes or source pins are required in the current grow-only/no-GC repository. A future
collector must implement shared read-pin/revocation semantics before it can safely collect while
restoring; do not claim a destination-only record already controls an offline collector. Keep the
repository key in a private no-follow file/zeroized reader memory, not job/progress/log/argv data.

## Files and contracts

- `flaminio-backup` `restore-read`/CLI/repository tests: S1/S5/S9–S10 archive parsing, #53's
  checks, read-only S3, the store's identity, the frame writer.
- `flaminio-io` `restore-volume` (the helper): spawning the reader, frame parsing, leaves and
  root, `O_DIRECT` writes, metadata and control calls, replay, progress and output.
- Metadata domain/service/auth, proto/gen, Rust pb/metaclient: S2–S4/S6–S8 cluster
  record/job/claim/writer/finalization/proofs/Ready/abort/status/enable; #76, #73 and #79
  integration; generation-conditional transactions.
- `pkg/captoken` and `flaminio-net`: RestoreTicketV1 and its vectors; replicas accept it as a
  target token, ordinary Attach refuses it (R2-S1). `docs/replication-protocol.md`: the target
  capability is an FLCT or, for a restore target, an FLRT.
- `flaminio-repl` restore admission/lifecycle model/sim, node engine, io control/ublk/registry,
  CSI controller/node/dataplane parsers: S4–S8 all attach gates, ticket purpose and closed writes.
- The lab's destination and restore check, Makefile/scripts/docs (Lab, below): S10–S11
  compatibility and second-cluster evidence, shared with #58, never wiping first-cluster resources.

[Appendix](contracts.md) fixes messages/phases/ticket/control/hash/CLI/identity contracts; `Volume`
takes field 22, proposed, its number deferred to the tag coordination (R1-C4, R2-C5). `make
proto/contracts` updates Go and handwritten Rust client and per-method auth fixtures. No on-disk format is changed: new volume uses normal chunk records;
durable operation state lives in metadata. It does NOT add self-describing-disk reconstruction,
archive decryption into an encrypted target or root swap.
Settled at approval, 2026-11-13: the proposed contracts; tags coordinated before coding (see decisions.md, Settled at approval).

## Tests (R1-C13)

| Rules | Tests |
|---|---|
| S1–S3 | `meta/service/restore_test.go`: `TestBeginRestoreRefusesEncrypted`, `TestBeginRestoreNotEnabled`, `TestBeginRestoreClusterRecord` (the record from a backup, absent and created, source equal to home refused), `TestBeginRestoreNameTaken` (volume, tombstone), `TestBeginRestoreRetry` (same job resumes; another archive or property AlreadyExists), `TestBeginRestoreAfterAbandonedCreateOp`, `TestCreateVolumeOverRestoreAlreadyExists`, `TestReconcilerFinalizesRestoreRoot` (still Incomplete), `TestOldRestoreOpCannotRecreate`, `TestRestoreUseUnknown`, `TestRestoreTenantNameRefusedOnceActive`, `TestRestoreSystemOwnerOnceActive`, `TestRestoreJobBounds`, `TestRestoreTxnOps` (every transaction ≤ 120). `control/csi/controller_test.go`: `TestCreateVolumeRestoreNotCompatible`. |
| S4, S8 | `restore_test.go`: `TestAttachRefusesRestore` (both Attach paths, the agent's `only_if_target`), `TestMaintenanceAttachRefusesRestore` (with #17), `TestSnapshotMoveGrowthRefuseRestore` (#51, MoveReplica, #70), `TestRestorePhaseTransitions` (each row of contracts' table, each refused elsewhere), `TestAbortRestoreDeadWriter`, `TestAbortRestoreCrashAfterMark` (finished by AbortRestore and by DeleteVolume), `TestAbortRestoreEvents`, `TestAbortRestoreCreating` (R2-C6: #76's deletion, the job `aborted` in its transaction, no visible header), `TestDeleteRestoreTombstoneNamesWriter` (R2-C2: the writer node holding no replica named), `TestRestoreReportsWriterNode` (R2-C1: another node's `io` with the writer's `node` field and a non-writer `io` refused), `TestRestoreProgressErrorBounds` (R2-S7). `meta/service/auth_test.go`: a row per method, both columns and insecure, and for the two io methods the writer node's `io`, an `io` naming another node than its certificate's, the writer node's agent (R2-C1, R2-S6); #73's role matrix gains them (whichever of #73 and #54 lands second). `control/csi/node_test.go`: `TestNodeStageRefusesRestoring`, `TestNodeStageReadyRestoreNeverFormats`, `TestDeviceRestoringHereFailedPrecondition` (a Ready volume on a node the writer moved away from: `device-info` `ok none`, the attach's `is restoring here` answered `FailedPrecondition`, no probe, format or mount; with #79, its `attach-init` and `attach-operation` the same, R5-C1); `dataplane_test.go`: `TestOldParserPlainStatusWithRestore`, `TestListAndStatusVolumeWithRestore` (#15's `list` pair parser and `status VOLUME` parser with a `VOLUME restoring` entry: a pair, no device taken, R3-C1); `resume_test.go`: `TestKeptStateParsingRestoring` (#48's loop skips a `restoring` pair; by whichever of #48 and #54 lands second). `flaminio-io` control tests: `restore_lines_strict`, `plain_status_skips_restore`, `plain_detach_refuses_restore`, `device_info_hides_restore` (`ok none` in starting, running and stopping, R5-C1), `detach_device_absent_for_restore` (`ok absent` for any tuple, no engine input, the entry unchanged, R5-C1), `restore_entry_states` (`list` and `status VOLUME` show `restoring` while the entry starts, runs, stops and after a deposition; restore-start answers `is restoring here`, then `is stopping here` once stopping; the slot and the count freed only when the entry ends, R5-C3, Nit; `restore-list` shows the entry in each state, and restore-detach, and the sweep's `delete` once GetVolume shows the volume gone, Deleting or of another operation, answer a starting entry `is attaching`, nothing changed, R6-C1, R7 Nit), `deposed_restore_entry_holds_slot` (after a routed `Deposed`, the entry stopped: restore-start of another volume answers `restore slot taken` and, at `--max-volumes 1`, an ordinary attach of another volume `limit:`, until restore-detach ends the entry, after which both succeed; the same once the sweep's `delete` ends it, R6-C1), `sigterm_waits_for_restore_entry` (with #12, by whichever of #12 and #54 lands second, R7-C1: #12's S4 with a running restore entry: the process stays; restore-start after the signal, of another volume and, between restore-detach and the entry's end, of the same volume, answers `the node is shutting down` before any slot, limit or state answer, nothing reserved; restore-detach is still served, and the process exits 0 once the entry ends. Run over #15's registry without #48, and with #48's `should_exit` by whichever of #48 and #54 lands second, R6-C2), `restore_refused_under_privsep` (`restore-list` and `restore-start` answer `restore needs direct mode`, nothing reserved, no helper request, R5-C2), `plain_delete_ends_deleted_restore` (gone, Deleting or another operation ends it; a live job refused), `restore_close_idempotent` (once the attachment ended: `no restore attachment`, Nit), `restore_detach_superseded` (aborted job, later generation: `ok superseded`, attachment ended), `restore_detach_not_recorded_without_proof` (in Incomplete, and in Finalizing before any proof: `ok not-recorded`, no RPC sent, R3-C3), `restore_detach_resends_closure` (a lost reply: the repeat answers `ok`, R3-C6), `restore_list_line` (none, one, under 4096 bytes, R3-C1), `status_volume_and_list_forms` (`ok VOLUME restoring`, the pair `VOLUME restoring`, R3-C1), `ordinary_attach_refused_by_restore_attachment` (live, fenced, and of a Ready restore: `error: VOLUME is restoring here`, the restore attachment unchanged, R4-C1), `restore_counts_under_limit` (`--max-volumes 1` with an ordinary volume attached: restore-start answers `limit:`; with a restore attachment, an ordinary attach of another volume answers `limit:`; 16 entries of 128-byte names, one `restoring`, list in 2,221 bytes, R4-C3, R5 Nit), `restore_close_probe_read_only` (the probe opens `O_RDONLY|O_EXCL`, Nit), `old_command_unsupported`; a ublk host test, as #48's: `restore_device_not_kept_across_restart` (direct mode, on a kernel with both flags: the add's flags hold neither recovery flag and its record no #48 block; after a kill and a start under #48, the device is ended, not kept, an ordinary attach at the run mark + 1 resumes nothing, and #48's `recovery` still answers `on`, R4-C2), `cleanup_leaves_restore_record` (after a kill of a node holding a restore entry, without and with #48: `flaminio-io cleanup` and `cleanup --expired` end or leave its record by #15's rule, a dead device deleted with its record and one still held open logged and left, keep nothing; for the record left, and for a start after the kill with no prior `cleanup`, the next start's `first_id` is above its id; for the deleted branch only the record and the device are gone, since the id may be reused, R6-C1, R7-C2); with #48, by whichever of #48 and #54 lands second: `restore_entry_not_stamped`, `restore_start_refuses_kept` (`is kept here`, and a resuming entry #48's `is resuming`, no resume, Nit). The `--privsep` variant is deferred with restore under `--privsep` (R5-C2). `flaminio-node`: `attach_refuses_restoring`, `ordinary_attach_refuses_restore_token` (in Finalizing and after Ready, by control and by engine), `restore_start_checks_phase` (Finalizing, Ready, another operation, a later epoch, another target), `attach_restore_needs_ticket`. `flaminio-net`: `replica_accepts_restore_ticket_as_target`, `restore_ticket_other_node_refused`. `pkg/captoken/restore_test.go`: `TestRestoreTicketVectors`, `TestRestoreTicketBounds`. |
| S5, S6 | `flaminio-io/tests/restore_volume.rs`: `runs_reader_unprivileged` (uid `flaminio-restore-read`; the administrator key, #52's `backup` key and `/var/lib/flaminio-backup` unreadable; refused as uid 0 or `flaminio-backup`, R2-S3), `reader_dies_with_helper` (the helper killed, the reader gone within 1 s; it fails if the signal is set before the uid switch, R3-S2), `reader_survives_thread_exit` (a helper thread that exits, a blocking-pool thread included, leaves the reader running, R3-S2), `reader_memory_bounded` (a test reader allocating past the limit: the helper exits 1, the node unaffected), `reader_address_space_bounded` (under load, the reader's threads at most 7 and its arenas at most 2 whatever the core count, its `VmSize` within the limit, R3-S3), `reader_no_new_privs` (R2-S5), `frames_rejected` (each malformed frame; an Error frame's bad code, long object and control bytes sanitized, R2-S7; a Header of another `archive_ref`, Nit), `o_direct_and_flush`, `close_refused_while_open` (the device held `O_EXCL` or mounted; retried 10 times, R2-C4, R3-C4), `write_after_close_refused` (a plain `O_RDWR` opener's write and its writeback after restore-close refused), `stray_write_fails_proof` (a plain opener's write before restore-close: the chunk's proof mismatches, R3-C4), `replay_after_restart`, `resume_after_helper_crash_ends_old_attachment`, `cancel_ends_local_attachment` (and after the sweep ended it first: `no restore attachment` counts as done, Nit), `abort_with_live_writer_attachment_completes_deletion`, `moved_writer_old_attachment_ended` (R2-C2, R2-S2), `resume_of_completed_job_ends_attachment` (a moved-from node's fenced attachment after Ready, detached in Incomplete: `ok not-recorded`, no RPC; one through which a proof committed: `ok superseded`; then an ordinary attach succeeds in each, R4-C1, R5 Nit), `start_after_detach_retries_stopping` (restore-start right after restore-detach of the same volume retries `is stopping here`; at `--max-volumes 1`, after restore-detach of another volume's restore attachment, it retries `limit:`, and at the default limit `restore slot taken`, and succeeds once that entry ends; an entry held past 30 s: exit 1; `limit:` or `restore slot taken` with no restore-detach of its own: exit 1 at once, R4-C3, R5-C3; `is restoring here` and `the node is shutting down`: exit 1 at once, never retried, R6-C2), `late_restore_start_ended_by_resume` (restore-start answered `no answer in time`: exit 1; a `resume` while the entry still starts: `restore-list` shows it, restore-detach's `is attaching` is retried until it runs, then it is detached and the restore replays; a `resume` once it runs: detached at once; an entry starting past 30 s: exit 1, the restore left Incomplete, R6-C1), `privsep_node_exits_3` (a node answering `restore needs direct mode`: exit 3, no metadata call; `cancel` still aborts, R5-C2; the node restarted under `--privsep` after AcquireRestoreWriter: restore-start's `restore needs direct mode` exits 1, the restore left Incomplete, Nit), `archive_ref_text_form` (Nit). `flaminio-backup/tests/restore_read.rs`: `refuses_encrypted_before_header`, `refuses_flaminio_pair`, `frames_sparse`, `corrupt_object_named`, `read_only_credential`. Fuzz targets `restore_frames`, `restore_ticket`. |
| S7 | `restore_test.go`: `TestFinalizingFreezesChangeIsr`, `TestChunkProofCursor` (in order, idempotent, a gap, a mismatch, another member set, an old finalization generation), `TestCompleteNeedsProofsAndClosure`, `TestWriterClosedSuperseded` (aborted with the header gone, completed, a later generation from another node: `superseded` before any header or `writer_node` compare, R3-C2), `TestWriterClosedIdempotent` (the same closure again: success, nothing written; another closure refused, R3-C6), `TestWriterClosedOnlyInFinalizing` (the current generation in Incomplete, another finalization generation: refused, R3-C3; a writer generation above the job's: `FailedPrecondition`, Nit). `flaminio-node`: `verify_reports_proof`. |
| S4–S8 | Simulator, below. |
| S9, S10 | `restore_read.rs`: `store_ca_separate`, `store_identity_required`, `store_identity_foreign_trust_domain` (accepted only as configured), #53's `output_fields` and `verify_checks_graph_retention` extended to restore-read; Go and Rust TLS tests: the destination's metadata service and node refuse a certificate the store's CA signs. Repository-only, no source RPC, no bucket mutation, key/credential redaction. |
| S11 | The lab's `make restore-check` (below). |

**Simulator (R1-C13).** `flaminio-repl/src/meta.rs` holds the restore lifecycle (the job, phases,
writer generation, finalization, cursor, closure, abort), beside #52's grant model, and
`src/restore.rs` the node side (restore write admission, close, verification), sans I/O. The
chaos phase, in a restore configuration, begins a restore, acquires writers on random nodes,
crashes and restarts the helper, fails members, finalizes, verifies, aborts, completes, and lets a
stale writer and a stale io report after each; it also tries an ordinary attach with the
writer's ticket and a replayed restore-start, in Finalizing and after Ready (R2-S1), and in
Incomplete after a restore-detach of the same generation, then finalizes and calls
CompleteRestore while that replayed attachment is still live, which
`catches_closure_before_finalize` needs (R3-C3, Nit), moves the writer, and runs the tombstone
sweep (R2-C2). With #48's `Action::RestartKept`, by whichever of #48 and #54 lands second
(R4-C2), it restarts the writer node while its restore attachment holds unanswered writes; check
`restore_not_kept` (those writes are never issued again, at any epoch) and bug
`catches_kept_restore_device` (the restore device added recoverable). Checks in `sim/check.rs`: `restore_ready` (at
Ready every current ISR member holds the image and every proof is of the final generation),
`restore_closed_at_ready` (when CompleteRestore commits, no restore attachment at the final
writer epoch is live on any node, R3-C3), `restore_no_write_after_ready` (no write at the
writer epoch of the generation that reached Ready reaches a replica after its
BeginRestoreFinalize committed, by any attach path; Nit), `restore_frozen` (no ISR change
commits in Finalizing),
`restore_abort` (nothing of a writer commits after the abort mark), `restore_deletion_ends`
(once the sweep has run, no restore attachment of a deleted volume is left on a tombstoned node).
`restore::Bugs`, each caught by a test in `sim/tests.rs`: `catches_early_ready`,
`catches_stale_writer_after_completion`, `catches_old_finalization_proof`,
`catches_isr_change_in_finalizing`, `catches_missing_member_proof`,
`catches_abort_without_invalidation`, `catches_restore_ticket_ordinary_attach` (ordinary attach
accepts FLRT), `catches_restore_start_without_phase_check`, `catches_stuck_restore_deletion`
(`delete` refuses a restore attachment), `catches_closure_before_finalize` (io reports every
detach's closure and the service records it in any phase, round 2's rule: caught by
`restore_closed_at_ready`, R3-C3). `examples/meta_vectors.rs`
emits the restore transitions into `meta/testdata/isr-vectors.jsonl`, replayed against
`meta/domain/restore.go` (`make meta-vectors`, checked by `make contracts`).

Future `make lint test race hygiene contracts sim`, relevant crash tests and fuzz regression
inputs. No implementation or tests ran here.

## Lab, docs, risks and ADRs

**Lab (R1-C14, R1-S6), shared with #58.** Whichever of #58 and #54 comes first builds the
destination; #58's review keeps these names or renames them. What #58's shipping needs of it (the
peers' bundle exchange and ports) is #58's (deferred, not settled). Run only on explicit consent.
- `make restore-dest-up` / `make restore-dest-down` (`lab.sh restore-dest up|down`): the one-node
  destination on fl-c beside the first cluster's node there. `lab.conf` gains
  `DEST_TRUST_DOMAIN` (`flaminio-lab-2`), `DEST_ETCD_CLIENT_PORT` 2479, `DEST_ETCD_PEER_PORT`
  2480, `DEST_META_PORT` 7500, `DEST_NODE_PORT` 7511, `VM_C_DEST_GIB` 20 and
  `VM_C_DEST_SERIAL` (`flaminio-dest`). `up` attaches a second disk to fl-c with that serial
  (`virsh attach-disk --serial`), finds it as `/dev/disk/by-id/virtio-$VM_C_DEST_SERIAL`, and
  refuses, before any format, a device that is mounted, holds a Flaminio superblock
  (`flaminio-disktool inspect`) or is the first cluster's claimed disk. A second PKI in
  `$LOCAL_STATE_DIR/pki-dest` (CA, etcd CA, token key; the etcd CA keeps the destination's etcd
  on client-certificate authentication, as `scripts/guest/etcd.sh:49`) issues its identities; on
  fl-c they live in `/etc/flaminio-dest/pki` (root, 0700); units `flaminio-dest-etcd`, `-meta` and `-node` (direct mode, never `--privsep`, whatever #74's `PRIVSEP` default for `node.sh`, R5-C2), etcd data
  in `/var/lib/flaminio-dest/etcd`, the control socket `/run/flaminio-dest/control.sock`. `down`
  stops and removes only these, then detaches and deletes the second disk.
- **The store from fl-c (R1-C2, R1-S4).** `restore-dest up` issues the store client pair on fl-c
  from the workstation's CA, with `issue` as `cmd_pki` uses it, client use only, the ID
  `spiffe://$TRUST_DOMAIN/store-client/$VM_C` and the key made in the VM, into
  `/etc/flaminio/store-client-pki` (`flaminio-restore-read`, 0700, the user made as #52's `pki.sh`
  names make theirs), with the store's CA as `store-ca.crt`, and makes `/var/lib/flaminio-restore`
  (`flaminio-restore-read`, 0700) for the reader's config, key and credential (R2-S3). fl-c gains
  the `flaminio-backup` binary for `restore-read`, never #52's `flaminio-backup` user, `backup-pki`
  or `/var/lib/flaminio-backup`. #16's `pki.sh` list gains `store-client`, by whichever of #16 and
  #54 lands second. In the lab the store owner's CA is the
  first cluster's CA (#52's lab): named, not accepted. `scripts/guest/backup-store.sh
  read-credential NAME` (a change to #52's script) creates a list/get-version-only S3 identity and
  prints its secret once. The check pipes it, and fl-w's repository key file, from one ssh's stdout
  into fl-c's `sudo -u flaminio-restore-read -- dash -c 'umask 077; t=$(mktemp -- "$1.XXXXXX")
  && cat >"$t" && ln -T -- "$t" "$1"; r=$?; rm -f -- "$t"; exit $r' dash DST` (R2-S4, R3-S1):
  written by that user into a fresh file `mktemp` creates `O_EXCL` in the 0700 directory, then
  hard-linked to DST, which `ln -T` (`linkat`) neither follows nor overwrites, so a planted file
  or link at DST, to a regular file, a FIFO or a device, is refused; dash's noclobber is cut, as
  it follows a link to a non-regular file. Root writes nothing there; never in argv, never on the
  host's disk. `scripts/tools/restore-secret-test.sh` plants at DST a file, a link to a file, a
  dangling link, a link to `/dev/null` and a link to a FIFO with a reader, and checks each
  refused (exit nonzero), the targets unchanged, the FIFO's reader given nothing and no
  temporary file left. The key reaching the destination is the
  restore's purpose (outcome 7); #52's lab rule still holds for the backup.
- `make restore-check` (`lab.sh restore-check`, `scripts/checks/restore.sh`, from the host). It
  validates each volume and operation id and each part of the archive REF before any ssh (ids
  `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`, UUIDs 32 lowercase hex, the REF's version id
  `^[A-Za-z0-9._-]{1,128}$`, a lab limit narrower than the CLI's) and passes them quoted (`printf
  %q`); `scripts/tools/restore-check-test.sh` covers the refusals. Steps: (1) refuse unless the
  store and the destination are up; in the first cluster a volume `rs-<UTC stamp>` under
  `write-load.sh` with a known pattern, snapshotted and backed up (#52's steps), its checksum
  kept; (2) stop the first cluster (its nodes, fl-c's included, meta and etcd), the store on fl-w
  left up; (3) on fl-c list the repository and restore the archive, the reader under its 2 GiB
  limit at fl-c's core count (R3-S3); its content equals the checksum on every ISR member, one
  member here (R2-C7: per-member proofs, a member failing in Finalizing and the return to
  Incomplete are proved in the simulator only, a named gap; a two-node destination is a
  follow-up, to be filed); (4) a restore killed midway: an attach is refused, `resume`
  completes it; (5) a taken name refused; (6) an archive with an encryption descriptor,
  written by a test example of `flaminio-backup` (#52 refuses encrypted volumes), refused
  before any volume; (7) an altered block version (#52's administrator path) fails the restore by name;
  (8) the rate recorded, not judged; (9) the first cluster started again. Cleanup removes only
  destination and test objects; locked repository objects stay until store expiry (#53).
- `docs/lab.md` and the Makefile's help describe the three targets.

Update README/docs/lab, restore/protocol docs (the FLRT as a restore target's token, R2-S1),
followups/Makefile and architecture §14.1: new
unencrypted volume restore built only after evidence, remaining keys/metadata/Velero/runbooks
still planned. Broad metadata restore must later satisfy Proposed ADR0008: proven revision high
water/exclusion, not fixed 10^9, highest seq or new UUID; disk-only reconstruction remains absent.

**Risks named, not accepted:** a terminal job record per restore, kept for good (R1-C18); in the
lab, the store owner's CA is the first cluster's (R1-C2); a decoder bug reaches the unprivileged
reader, and a compromised reader can choose the restored volume's bytes and what its Header
reports: #53's `immutable_guarantee`, `backend_profile.qualified` and the source provenance
(R1-S1, Nit); a node a writer moved away from, whether or not it holds a replica, keeps a
fenced restore attachment (a device, a target, the node's restore slot and one of #15's
`--max-volumes` entries) until the helper next runs there, a `resume` of the completed job
included, the volume's deletion sweeps it or io restarts; meanwhile an ordinary attach of that
volume there is refused, and where #15 forces the limit to 1 every ordinary attach there is
(R2-C2, R3-C5, R4-C1, R4-C3); an io restart ends a restore attachment and loses a kept closure,
so the restore takes a new writer and replays (R2-C2, R4-C2); a metadata rollback after EnableRestore is safe only once no restore is
Incomplete or Finalizing, an operator's check (R2-C8); a reader whose reservations (thread
stacks, malloc arenas, mappings) pass 2 GiB of address space fails the restore, exit 1, which
its thread and arena bounds make unlikely, not impossible (R2-S5, R3-S3); the lab proves one ISR
member only (R2-C7); in the lab's secret install, a process of `flaminio-restore-read` running
meanwhile could swap the temporary file (the same user, no escalation; none runs then, R3-S1);
a stray write by a plain opener of the private device fails the restore's proofs rather than
being refused (R3-C4); a node under `--privsep` cannot restore in this increment, so a cluster
whose nodes all run under #74's helper restores nothing until the deferred change (R5-C2);
on a moved-from node the CSI plugin answers a stage of the Ready volume `FailedPrecondition`,
kubelet retrying, until the fenced restore attachment ends (R5-C1); on a destination node run
as #12's DaemonSet, #12's `status --none` (its preStop hook, `pod_nodes_idle`, the upgrade's
steps 4 and 5) reads plain `status`, which never names a restore entry, so a node mid-restore
reads idle: the hook returns, and the process exits only when the restore entry ends or at the
3600 s grace's SIGKILL, which fails the helper's I/O (exit 1; its `resume` replays onto a new
writer generation, the device being unrecoverable). It is not a safety issue, and the SIGTERM
refusal bounds the wait to the restore in flight. The lab's destination node is a systemd unit
on fl-c, which runs no k3s, so no lab check meets it (R6-C2).

**Cross-plan changes, made by whichever lands second:** #17's maintenance attach and start refuse
a non-Ready restore; #79's token-issuing paths list AcquireRestoreWriter as leaving Unknown; #80's
worst-view fixtures add 38 bytes; #73's role matrix gains the new rows; #16's `pki.sh` list gains
`store-client`; #52's `backup-store.sh` gains `read-credential`. Added in round 2: #73's "Attach
is decided by the certificate" gains one exception, AcquireRestoreWriter's FLRT, which gives
`system-provisioner` target authority for a non-Ready restore through restore-start on the
writer node only, never through ordinary Attach (R2-S1); #52's lab puts the `flaminio-backup`
binary on fl-c with the `flaminio-restore-read` user and its directories, while #52's user,
`backup-pki` and worker directory stay on fl-w ("no other VM gains either" holds, R2-S3); #52's
REF takes the text form `REPOSITORY_UUID/BACKUP_UUID/VERSION_ID` (Nit); #76's revocation of a
Creating restore root writes the job `aborted` (R2-C6). Added in round 3: #48's `list` parser
takes `restoring` as a known STATE that its loop skips, and #15's STATE set gains it (R3-C1).
Added in round 4: #15's `reserve` counts a restore entry under `--max-volumes`, its longest
STATE becomes `restoring`, 9 bytes, and its longest `list` 2,221 bytes (its S1 bound and the
16-entry `list` test; 2,236 in round 4, corrected in round 5) (R4-C3); #48 adds no recovery flag and no block for a restore device,
does not read such an add as its fallback (no stop asking, no `off-flags`), and its simulator's
`RestartKept` gains the restore scenario, check `restore_not_kept` and bug
`catches_kept_restore_device` (R4-C2).
Added in round 5 (R5-C1, R5-C3, Nits): #15's contract states that `device-info` answers a
restore entry `ok none` and `detach-device` `ok absent`, its revision 5 grammar unchanged; #15's
S9 maps `error: VOLUME is restoring here` to `FailedPrecondition` in `device()`; #15 states that
a restore entry passes through its starting, running and stopping states, shown as `restoring`,
that restore-start's `reserve` answers a stopping entry `is stopping here`, and that the restore
slot is freed when the entry ends; #79's plugin maps the same answer for `attach-init` and
`attach-operation`, and its `initialization-status` and `attachment-status` answer a restore
entry as another operation's; #48's stamper skips a restore entry, its `should_exit` counts one
unrecoverable, and a restore-start meeting a kept entry answers `is kept here` and resumes
nothing.
Added in round 6 (R6-C2): #15 states that #12's `should_exit`, read over its registry, is true
only once no entry is left, a restore entry included (by whichever of #12, #15 and #54 lands
last, R7-C1).
Added in round 7 (R7-C1): if #12 lands after #54, its `begin_attach` predicate also guards
restore-start's `reserve`, and its `no_attach_after_sigterm` gains a restore-start.
**Deferred, not settled:** `Volume`'s
field number, 22 proposed, to the tag coordination with #70 and #75 before `make proto` (R2-C5);
the restore scenario of #48's `RestartKept`, its check and its bug, to #48's landing, since #48
owns that action and is approved and review-clear but not landed (R4-C2, R6-C3); restore under
`--privsep`, to #74 and #48 (an unrecoverable form of the helper's `add`, no #48 block or
`kept/<ID>` note for it, its deletion at the child's exit, a `--privsep` test), since #74 owns
that request and #48, which owns the helper's keep path and `RestartKept`, is not implemented,
and the intent does not need it (R5-C2, R6-C3; a follow-up, to be filed).

[Proposed ADR](adr-proposal.md) records origin, restricted writer/proof closure, sparse-layout
qualification and owner API as accepted with the plan. Source/runtime drafts/key retention and #83
timing profile are independent gates. No approval/accepted risk/physical clock qualification is
implied. Accepted fencing/ISR/language/etcd ADRs remain consistent; audit0008/9/11 are not
accepted here. Parallel work: Go metadata/proto and Rust reader/helper/admission can build after
appendix agreement; security, simulator and whole-image/closure integration must join before
release.
Settled at approval, 2026-11-13: origin, writer and proof closure, sparse layout and owner API as proposed; the ADR is accepted with the plan (see decisions.md, Settled at approval).
