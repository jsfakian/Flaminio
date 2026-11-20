# Issue #54: unencrypted restore contracts (Accepted with the plan, 2026-11-13)

Appendix to [design](design.md); reuse #52/contracts.md identities and #53 exact-version profile.
Nothing here implements metadata disaster recovery or encrypted/force recovery.
Changed after approval in plan review rounds 1 to 5 (decisions.md).

## Identity, operation and metadata

**Home cluster (R1-C3, R1-S3).** The home cluster UUID is #52's record `/flaminio/v1/cluster`
(`{cluster_uuid}`, 16 random bytes, hex, never deleted; #52 contracts, Records). BeginRestore
reads it; when it is absent, BeginRestore draws 16 bytes from the OS CSPRNG and writes the record
in its admission transaction, compared absent, as #52's AuthorizeSnapshotRead does; a lost race
rereads it. No caller supplies a UUID (`InitializeClusterOrigin` is cut). BeginRestore refuses an
archive whose `source.cluster_uuid` equals the record, `FailedPrecondition` `source cluster is
this cluster` (restore into the cluster that holds the volume is #52's step 5). This is
provenance, not storage authority, token revocation or disk migration. A restored volume's
identity is `{cluster_uuid,volume_id,first_epoch}`, the fields #52's `SnapshotRefV1` takes at its
later backups; its source origin is informational #52 SnapshotRef + exact ArchiveRef
`{repository_uuid,backup_uuid,version_id}` (#52's root identity) + authenticated archive-map
digest.

**Header and Volume (R1-C4, R1-S8, R1-S9, R1-C16).** Header JSON adds `restore:{phase,operation}`
from the FIRST Creating write (#76's admission), kept for the incarnation's life, Ready included,
as provenance. The protobuf `Volume` gains one field, `RestoreState restore = 22`:
`message RestoreState { RestorePhase phase = 1; string operation = 2; }`, `enum RestorePhase {
RESTORE_PHASE_UNSPECIFIED = 0; RESTORE_PHASE_INCOMPLETE = 1; RESTORE_PHASE_FINALIZING = 2;
RESTORE_PHASE_READY = 3; }`; unset for a volume never restored. 22 is proposed, the lowest
`Volume` number outside every claim (R2-C5): #13's registry (#13 decisions.md, R1-C3) for 9–10,
11–14 (#70's, as #75 design:175 lists them), 20–21, 30–31 and 40; the owners' designs for 15
(#13), 16–17 (#17) and 18–19 (#18). The final number is deferred to the tag coordination with
#70 and #75 before `make proto`.
`home_cluster_uuid` is cut: the cluster record is the home UUID. Refs, writer, generations,
progress and errors live in the job record and are served by GetRestore only, never by the open
reads GetVolume and ListVolumes. No credential, key, ticket or token is in either. Encoded,
`restore` is at most 38 bytes (its tag and length 2, phase 2, operation 34); #80's worst-view
fixtures add them to the header allowance, by whichever of #80 and #54 lands second. The same
first write sets #79's `use` to Unknown with an Unknown `use_profile`, never FreshFs (R1-C6).
Settled at approval, 2026-11-13: the proposed fields and tags, coordinated before freezing (see
decisions.md, Settled at approval); changed after approval in rounds 1 and 2 (R1-C4, R2-C5).

**Phases (R1-C9, R1-S5).** Three, as the spec's Terms: Incomplete, Finalizing, Ready. Deletion is
the header's Deleting mark (today's), not a phase.

| From | To | Call | Condition and effect |
|---|---|---|---|
| absent | Incomplete | BeginRestore, then #76's finalization by any helper | name and tombstone absent |
| Creating (job `first_epoch` 0) | deleted | AbortRestore or DeleteVolume | #76's deletion of an incomplete operation (#76 S6); job outcome `aborted`, generation + 1, in its revocation's transaction (R2-C6) |
| Incomplete | Incomplete | AcquireRestoreWriter | generation + 1, a new epoch, target = node |
| Incomplete | Finalizing | BeginRestoreFinalize | the current generation and epoch; root and count set, cursor 0 |
| Finalizing | Incomplete | AcquireRestoreWriter | generation + 1, a new epoch; root, cursor and closure cleared |
| Finalizing | Ready | CompleteRestore | cursor = chunk count and the current finalization's closure (R3-C3) |
| Incomplete, Finalizing | Deleting | AbortRestore or DeleteVolume | DeleteVolume's mark; job outcome `aborted`, generation + 1 |
| Deleting | deleted | AbortRestore or DeleteVolume, either resuming | today's deletion, tombstone and #73's release |

Ready is terminal: no restore call changes a Ready volume, which is deleted as any volume. A
crash after the mark leaves a Deleting header that the next AbortRestore or DeleteVolume
finishes (#73 S5); a crash before it changes nothing.

**Records (R1-C11, R1-C18, R1-S10).** Under `/flaminio/v1/`, JSON with `"schema":1`; `<op>` is
the restore operation UUID, 32 lowercase hex.

| Key | Value | Written | Deleted |
|---|---|---|---|
| `cluster` | #52's `{cluster_uuid}` | BeginRestore if absent (or #52's AuthorizeSnapshotRead) | never |
| `features/restore` | `{enabled_by, time}` | EnableRestore, once, compared absent | never |
| `restores/<op>` | the job, below | BeginRestore (in #76's admission), #76's finalization, each restore call | never in this increment |

The job: `{volume, first_epoch, create_op, archive_ref, archive_digest, request_digest, phase,
writer_node, writer_epoch, writer_generation, finalization_generation, expected_root,
chunk_count, cursor, closure, bytes_done, bytes_total, error_code, error_object, outcome}`.
`first_epoch` is 0 until #76's finalization writes it with the visible header; `closure` is
`{generation, finalization_generation, instance, attach_gen, nonce}` or null (R3-C3); `outcome`
is empty, `completed` or `aborted`.
Bounds: ids as `domain.ValidID`, `error_code` at most 64 bytes of `[a-z0-9_]`, `error_object` at
most 256 printable bytes, the whole value at most 2048 bytes. There is no per-chunk proof record:
a proof advances `cursor` (below). A terminal job is kept, so an old operation never re-creates
or writes a later volume of its name (S3): one record of at most 2048 bytes per restore ever
begun in the store, growth named, not accepted; a collector is a follow-up, to be filed.

**Transactions** (each at most 120 operations, `meta/kv/etcd.go:91-92`):

| Transaction | Compares | Writes |
|---|---|---|
| BeginRestore's admission | #76's admission's, plus `features/restore` present, `cluster` (absent or at its revision), `restores/<op>` absent, and once active #73's creation compares (authorization and allocation records, owner record absent) | #76's (header, root), plus `restores/<op>`, `cluster` if absent, #73's `system` owner record once active |
| #76's finalization of a restore root | #76's, plus `restores/<op>` | #76's (the visible header keeps `restore` and `use`), plus `restores/<op>` |
| AcquireRestoreWriter | header, job, (with #13) `forgotten` absent for the node: ≤ 3 | header (epoch, target, phase), job: 2 |
| ReportRestoreProgress, ReportRestoreWriterClosed | header, job: 2 (a `superseded` answer reads the job only, R3-C2; an idempotent closure answer reads header and job, Nit; both write nothing) | job: 1 |
| BeginRestoreFinalize | header, job: 2 | header (phase), job: 2 |
| ReportRestoreChunkProof | header, job, the chunk record: 3 | job: 1 |
| CompleteRestore | header, job: 2 | header (phase Ready, target cleared), job: 2 |
| The abort mark | DeleteVolume's mark's (#73's, #52's reader record), plus the job | the mark's (#73's event included), plus the job |
| The abort's tombstone | DeleteVolume's | DeleteVolume's; its nodes also name the job's `writer_node` (R2-C2) |
| #76's revocation of a Creating restore root | #76's, plus the job | #76's, plus the job (`aborted`) |
| EnableRestore | `features/restore` absent: 1 | it: 1 |

**RPCs (R1-C1, R1-C8, R1-C10, R1-S2).** Messages in api/proto, generated in api/gen and mirrored
in the Rust metaclient; every method has its `auth.go` rule and `auth_test` rows.
- `EnableRestore()` -> revision. Writes `features/restore` once; a repeat answers its revision.
- `BeginRestore(operation_uuid,archive_ref,archive_digest,source_descriptor,name,geometry,
  rf,min_isr,failure_domain,policy)` -> destination_ref/job. Refused, before any write: an
  encrypted descriptor; `features/restore` absent (`FailedPrecondition` `restore is not
  enabled`); a source cluster equal to this one; once tenancy is active, a tenant-form name
  (`InvalidArgument`, #73 S3: give `--name`). Only the same job/digest/properties resumes.
- `AcquireRestoreWriter(operation_uuid,destination_ref,node,expected_job_revision)` -> volume,
  writer_epoch/generation,restore_ticket. New epoch, one bound writer; no source token and no
  ordinary target token: the ticket is the restore target's only capability (R2-S1).
- `ReportRestoreProgress(operation_uuid,destination_ref,writer_generation,bytes_done,bytes_total,
  error_code,error_object)`: the job's only writer of its error fields, which it refuses
  `InvalidArgument` outside the Records' bounds (R2-S7).
- `BeginRestoreFinalize(operation_uuid,destination_ref,writer_epoch/generation,
  expected_job_revision,expected_chunk_hash_root,chunk_count)` -> finalization_generation.
- `ReportRestoreChunkProof(operation_uuid,destination_ref,node,writer_generation,
  finalization_generation,chunk,isr_version,members,leaf,path,actual)`: one chunk.
- `ReportRestoreWriterClosed(operation_uuid,destination_ref,node,writer_generation,
  finalization_generation,instance,attach_gen,nonce)` -> `superseded` (bool). Decided in this
  order (R3-C2): (1) from the job record alone, read first: `superseded` true, writing nothing,
  when the job is terminal (its header may be gone) or its writer generation is above the
  request's; a request's writer generation above the job's is `FailedPrecondition` `restore OP
  has no writer generation G` (Nit). Step 1 answers any node's `io` that auth.go's node rule
  admits, before the `writer_node` compare: harmless, since an operation's terminal state and
  the existence of a later generation are not secret (Nit); (2) only then, for the job's
  current generation: the header (phase Finalizing) and
  `writer_node` are compared, and `finalization_generation` must equal the job's, else
  `FailedPrecondition` (`restore OP is not finalizing`, `node N is not restore OP's writer`);
  (3) a job whose `closure` already equals the request's answers false, writing nothing (an
  idempotent repeat, R3-C6); another closure is `FailedPrecondition` `restore OP already has a
  closure`; otherwise it writes `closure`.
- `CompleteRestore(operation_uuid,destination_ref,finalization_generation,
  expected_job_revision)` -> terminal outcome only if every chunk's proof and the closure persist.
- `AbortRestore(operation_uuid,destination_ref)` -> DeleteVolume's answer. Compares the job's
  operation and first epoch, then runs DeleteVolume's deletion of that volume (its mark, events,
  protection and tombstone); no writer closure is needed: #30's first epoch fences the old
  writer from any later incarnation, and the deletion's tombstone removes its replicas and,
  through the writer node's sweep, its attachment (R2-C2). While the job's `first_epoch` is 0
  (the root still Creating) it compares the job's `create_op` instead and runs #76's deletion of
  that incomplete operation, writing the job `aborted` in its revocation's transaction (R2-C6).
  DeleteVolume finds the job of a restore header by `header.restore.operation` (Nit).
- `GetRestore(operation_uuid)` -> the job; it holds no ticket or token.

| Method | Before activation (#73) | Once active (#73) | Event |
|---|---|---|---|
| `EnableRestore` | administrator name | `cluster-admin` | – |
| `BeginRestore`, `AcquireRestoreWriter`, `ReportRestoreProgress`, `BeginRestoreFinalize`, `CompleteRestore` | administrator name | `system-provisioner` | – |
| `AbortRestore` | administrator name | `system-provisioner` | #73's `delete.admitted`, `delete.committed` |
| `GetRestore` | administrator name | `system-provisioner`, `cluster-admin` | – |
| `ReportRestoreChunkProof`, `ReportRestoreWriterClosed` | the job's writer node's `io` | the same | – |

Insecure instances refuse every method of the table, as #73 refuses its new methods. The io rule
(R2-C1, R2-S6) is `auth.go`'s node rule: the request's `node` must be the certificate's node and
its component `io`, else `PermissionDenied`; the method's transaction then compares `node` with
the job's `writer_node`, else `FailedPrecondition` `node N is not restore OP's writer` (for
ReportRestoreWriterClosed only after its job-record step, so a moved-from node's old generation
is `superseded`, R3-C2). No
other identity or role reaches these methods, and no
"Reader" role exists (#73's roles replace it). Audit events for BeginRestore, the writer and
completion are a follow-up, to be filed; the restore's deletion is audited by #73's events.
Settled at approval, 2026-11-13: the proposed RPCs (see decisions.md, Settled at approval);
changed after approval in rounds 1 to 4 (R1-C1, R1-C3, R1-C8, R1-C10, R2-C1, R2-C2, R2-S1,
R2-S7, R3-C2, R3-C3, R3-C6, R4 Nits).

**A chunk's proof.** `members` is the chunk record's ISR at `isr_version`, compared in the
transaction; `actual` holds one SHA-256 per member, each equal to `leaf`; `path` takes `leaf` to
`expected_root`; `chunk` equals `cursor`, which then advances by one. A chunk below `cursor` with
the same values is an idempotent success; above it, or any mismatch, is `FailedPrecondition` and
writes nothing. Finalizing refuses ChangeIsr, MoveReplica, attachments and growth of the volume;
a return to Incomplete invalidates every proof (the cursor is cleared) and takes a new epoch.
Closure is `{generation, finalization_generation, instance, attach_gen, nonce}` after io drained
I/O and ended the device and target of the attachment through which a proof of that
finalization committed (R3-C3). That attachment is the generation's last: it held the node's
restore slot from before Finalizing began until its end, and any later restore-start reads
Finalizing and starts nothing. CompleteRestore checks the cursor and that closure; the next
ordinary attach gets a new epoch. Ordinary Attach/control/CSI shortcuts cannot expose a
non-Ready volume.

## Writer ticket, control and verification

**RestoreTicketV1 (R1-C11, R2-C3).** Signed by the metadata service's token key (Ed25519), as
`pkg/captoken`'s FLCT, with its own magic as the domain. Big-endian, then a 64-byte signature over
everything before it: `"FLRT"`; `u8` version 1; `u16` length then volume id (1–128 bytes); `u64`
first epoch; `u64` writer epoch (above the first epoch); `u16` length then node id (1–128 bytes);
`[16]` operation UUID; `[32]` archive digest; `u64` writer generation. `MaxLen` 401 bytes; length,
ids and signature are checked before any allocation or admission. The cluster UUID is cut: no
node could check it, and the key, which is the destination's own, already binds the ticket to
this cluster (R2-C3). Go `pkg/captoken/restore.go` and Rust `flaminio-net` share vectors in
`pkg/captoken/testdata/restore-ticket-v1.json`, checked by `make contracts`. It is not #52's FLBR.

**The restore target's only capability (R2-S1).** The ticket is the restore target's token, and
AcquireRestoreWriter issues no FLCT. Replicas accept it as a target token with FLCT's checks
(signature, volume, first epoch, the writer epoch as the epoch, the node equal to the target's
certificate) and the same fence. Ordinary attach (control `attach`, `Input::Attach`, the agent's
restart) takes only FLCT magic and answers `error: VOLUME: a restore ticket is not an attach
token` to FLRT. `restore-start` takes only FLRT. Under `--privsep` it refuses first (Direct mode
only, below). With #12, once a SIGTERM was requested it then refuses `error: VOLUME: the node is shutting
down` (#12's S4, R6-C2; by whichever of #12 and #54 lands second, R7-C1), read in the same acquisition of the registry's lock as `reserve`, as
#12's `begin_attach` reads it, so nothing is reserved after the signal. Otherwise it reserves,
under that lock, a #15 entry through `reserve`, counted
under `--max-volumes` (`error: VOLUME: limit: N volumes attached or attaching`; an existing
entry of the volume: its answer in A restore entry's lines, below, R4-C3), and the node's
restore slot (`error: VOLUME: restore slot taken`; the slot is freed when the entry ends,
R5-C3), then reads GetVolume (an open read, off the I/O path), and starts the target only if
`restore.phase` is Incomplete, `restore.operation` is OP, `epoch` is the ticket's writer epoch
and `target` is this node; otherwise `error: VOLUME: restore OP is not writable: PHASE`. In
Finalizing, after Ready, or once a later writer took the epoch, the ticket therefore attaches
nothing, and an attachment of an older generation is fenced by the later writer's epoch. No
offline expiry claim grants continued write authority.

Root-only local lines, strict IDs/UUID/u64/hex/base64, no shell/file path evaluation:
- `restore-start VOLUME OP_UUID WRITER_GEN TICKET_BASE64` -> `ok DEVICE INSTANCE ATTACH_GEN
  WRITE_EPOCH` only after typed matching target starts, under the checks above (R2-S1).
- `restore-close VOLUME OP_UUID WRITER_GEN` -> `ok` after write admission closes and every
  outstanding write/completion is drained; verification then remains read-only. It first opens
  the device `O_RDONLY|O_EXCL` (read-only, so its close sets off no udev re-probe, Nit) and
  closes it again, answering `error: VOLUME: device is open` while the
  device has an exclusive holder: a mount, another `O_EXCL` open, a dm or md claim (R1-C17).
  This does not see a plain `O_RDWR` opener (blkid, udev); the guarantees are admission
  closure, which refuses any later write, a writeback flush included, and the proofs, which
  fail on any stray write that reached the replicas before it (R3-C4). Round 2's `fsync` and
  held descriptor are cut. A repeat for a generation already closed answers `ok` (R2-C2); once
  that attachment has ended, a repeat answers `error: VOLUME: no restore attachment OP_UUID
  WRITER_GEN` (Nit). The helper retries `device is open` 10 times, 500 ms apart, then exits 1,
  the restore left Incomplete.
- `restore-verify VOLUME OP_UUID WRITER_GEN FIN_GEN CHUNK LEAF_HEX PATH_BASE64` -> `ok` once io
  has read CHUNK from every ISR member, found each equal to LEAF and its own
  ReportRestoreChunkProof has committed; otherwise `error: VOLUME: chunk N: ...` naming the member
  or the refusal. PATH is the leaf's Merkle siblings, bottom up, at most 16 × 32 bytes (N ≤
  65,536, #80) (R1-C11). Accepted only after restore-close, in the matching Finalizing
  generation; no ordinary arbitrary replica read is exposed. io records, on the attachment,
  the FIN_GEN of each proof that committed through it (R3-C3).
- `restore-detach VOLUME OP_UUID WRITER_GEN` (R2-C2, R2-S2), in any phase, for this node's
  attachment of that operation and generation: drains as restore-close and ends the device and
  target. Of a starting entry (a late start's, R6-C1) it answers #15's `detach` wording, `error:
  VOLUME is attaching`, and changes nothing. An attachment through which no proof committed answers `ok not-recorded` and sends
  nothing: a closure is recorded only in Finalizing (R3-C3). Otherwise io sends its own
  ReportRestoreWriterClosed with the latest FIN_GEN it recorded (R1-C1) and answers `ok` once
  that committed, an idempotent repeat included (R3-C6); `ok superseded` when it answered
  `superseded` (nothing recorded); otherwise `error: VOLUME: closure not recorded: ...`, the
  attachment ended and the closure kept in io's memory, which a repeat resends. With neither an
  attachment nor a kept closure: `error: VOLUME: no restore attachment OP_UUID WRITER_GEN`. The
  closure nonce is io's, sent only in that RPC; an io restart loses a kept closure, and the
  helper then takes a new writer generation and replays.
- `restore-list` (R3-C1) -> `ok none` or `ok VOLUME OP_UUID WRITER_GEN`: this node's restore
  attachment, at most one (the node's restore slot), in each of #15's states, starting included
  (R6-C1), under #74's 4096-byte cap. It is the
  helper's only view of restore attachments; #15's `list` and `status VOLUME` keep their forms.
  Under `--privsep` it answers `error: restore needs direct mode` (R5-C2).
  Unknown/old commands return explicit unsupported; no fallback.

**A restore entry's lines (R1-C15, R4-C3, R5-C1).** The restore attachment is a #15 entry,
reserved by restore-start through `reserve`, counted under `--max-volumes` and holding the node's
restore slot. It passes through #15's states (R5 Nit): starting from `reserve` until its target
and device are up, running until restore-detach, the sweep's `delete`, a deposition or a failure
stops it, then stopping until its device thread ends it (#15, S2). The restore slot is freed with
its `--max-volumes` count when the entry ends, under the registry's lock (R5-C3). Each line, for
a restore entry in any of these states:
- **`status`** (plain): never names it; it answers the first running ordinary volume or `none`,
  so an older plugin never reads `restoring` as a volume id.
- **`status VOLUME`:** `ok VOLUME restoring`, with no device fields, in every state.
- **`list`:** the pair `VOLUME restoring`, in every state. #15's grammar (`ok VOLUME STATE
  [...]`, `ok limit N [VOLUME STATE ...]`) and #48's arity are unchanged, `restoring` being one
  more STATE (R3-C1). `list` holds at most 16 entries; `restoring` makes #15's longest state 9
  bytes, and with one restore slot a node's longest `list` is 2,221 bytes with its newline,
  under 4096 (R5 Nit). `NodeGetInfo` reads only `limit N`.
- **`device-info VOLUME`:** `ok none`, in every state (R5-C1). The contract's revision 5 grammar
  (`95a1dba`, which #79 pins) is unchanged. No tuple of a restore device is given out, so no
  device check of the plugin passes on one.
- **`detach-device VOLUME ...`:** `ok absent` for any well-formed tuple, with no engine input
  and nothing changed (R5-C1); a malformed tuple fails the contract's grammar before any effect,
  as for every entry (Nit). No plugin was given the entry's lifetime, so no stage of it exists.
- **`detach VOLUME`:** `error: VOLUME is a restore attachment`, nothing changed.
- **`delete VOLUME`,** the tombstone sweep's line (R2-C2, R2-S2): reads GetVolume first. If the
  volume is absent or Deleting, or carries no restore or another restore operation, it ends the
  attachment as restore-detach does, with no closure report, then deletes the replicas (`ok N
  replicas deleted`); a starting entry gets restore-detach's `error: VOLUME is attaching`,
  nothing changed, and the sweep, which logs it, tries again at its next pass (R6-C1). Otherwise it answers `error: VOLUME is a restore attachment` and changes
  nothing.
- **An ordinary attach of the volume** (`attach VOLUME TOKEN`, `Input::Attach`, the agent's
  restart, the startup `--attach`, and #79's `attach-init` and `attach-operation` where #79 is
  in): refused by #15's `reserve`, `error: VOLUME is restoring here`, in every state, nothing
  changed (R4-C1: round 3's rule ending it is cut). Of another volume: counted with the restore
  entry, so `limit:` at the limit. An FLRT given to any of them: `error: VOLUME: a restore
  ticket is not an attach token`.
- **#79's `initialization-status VOLUME OP` and `attachment-status VOLUME OP`:** a restore entry
  is never of the asked operation or purpose. They answer it as #79 answers another operation's
  entry and bind nothing (a hand-off to #79, which owns that form).
- **`restore-start` of the volume** (`reserve`): a starting or running restore entry `error:
  VOLUME is restoring here`; a stopping entry, restore or ordinary, `error: VOLUME is stopping
  here`; any other ordinary entry `error: VOLUME is STATE here` with #15's state, #48's `kept`
  included; a resuming entry #48's own answer, `error: VOLUME is resuming` (Nit). restore-start
  never resumes a kept device.
- **A deposition** (the routed `Deposed`, from a later writer's fence): the device thread stops
  the entry as #15's does. With no detach flag set, the entry is marked stopped and keeps its
  count and the restore slot until restore-detach, the sweep's `delete` or an io restart ends it
  (the moved-from node; design, Risks).
- **A late start** (restore-start answered `no answer in time`, #6): the entry stays and counts;
  `restore-list` shows it, and restore-detach answers it `error: VOLUME is attaching` while it
  starts (R6-C1). The helper exits 1, and its `resume` ends the entry through `restore-list` and
  restore-detach, retrying `is attaching` (Ending restore attachments).
- **The restart scan** (#15, S10; #48's scan, its "#15's record" row) and `flaminio-io cleanup`
  (`--expired` too): the record is #15's with no #48 block, and the device has no recovery flag.
  A start or a cleanup ends or leaves it by #15's rule (a dead device deleted, any other logged
  and left), and never keeps or resumes it. `first_id` counts its id.
- **SIGTERM** (#12's S4, R6-C2; with #12, by whichever of #12 and #54 lands second, R7-C1):
  restore-start refuses after the signal (above). `restore-list`,
  restore-close, restore-verify and restore-detach are still served, so the running restore can
  finish and end its entry. The process waits for the restore entry as for an attached volume:
  #12's `should_exit`, read over #15's registry, is true only once no entry is left, a restore
  entry included (a hand-off to #15, by whichever of #12, #15 and #54 lands last, R7-C1); with #48
  it counts a running restore entry unrecoverable (below; a starting one does not hold the process
  under #48, and the scan ends its device, R7 Nit). Plain `status` still never names it, so #12's `status
  --none` reads a node mid-restore as idle (design, Risks).
- **#48's other paths,** direct mode (a hand-off to #48, by whichever lands second): the stamper
  skips a restore entry, which has no block; `should_exit` counts a running one unrecoverable (R7 Nit), so #12's S4
  holds whole on SIGTERM (the signal waits until the entry ends; restore-start refuses after it); the loop skips `restoring`;
  #48's `device()` meets `ok none`. A restore add is not #48's fallback: it neither stops #48
  asking for the flags nor sets `recovery` to `off-flags`.
- **The CSI plugin** (#15's S9; a hand-off to #15, by whichever lands second): `device()` gets
  `ok none`, calls `Attach`, and maps `error: VOLUME is restoring here` to `FailedPrecondition`,
  as it maps stopping. Kubelet retries with the same publish context, whose token works once the
  restore entry has ended (R5-C1). #79's plugin maps the same answer to its `attach-init` and
  `attach-operation` the same way (a hand-off to #79).

Where the limit is 1 (#15 forces it before Linux 6.9 where its 6.8 gate fails, or
`--max-volumes 1`), a restore attachment takes the node's only slot: restore-start there needs
no other entry, and no ordinary attach succeeds while it lives.

**Unrecoverable (R4-C2).** restore-start adds its device asking for neither
`UBLK_F_USER_RECOVERY` nor `UBLK_F_USER_RECOVERY_REISSUE`, and writes #15's record with no #48
block. An io restart, a kill included, therefore ends it: without `UBLK_F_USER_RECOVERY` the
kernel stops the device once its server has exited and fails its held requests, and START_USER_RECOVERY answers
only for a QUIESCED device, so no process can resume it. The next start ends or leaves its
record by #15's rule, never resumes it (R5 Nit), and no attach issues its writes again. The
helper's I/O fails and it exits 1; its `resume` takes a new writer generation and replays.

**Direct mode only (R5-C2).** Under `--privsep` the child cannot ask for an unrecoverable add:
#74's helper `add QUEUES DEPTH IO_BUF SIZE` has no flag field, and under #48 the helper asks for
both recovery flags, writes root's `recovery 2` block and keeps a `running` recoverable device at
the child's exit. So under `--privsep` `restore-list` answers `error: restore needs direct mode`
and `restore-start` `error: VOLUME: restore needs direct mode`, before any `reserve` or helper
request. The helper sends `restore-list` before its first metadata call in a restore and in
every `resume`, and again immediately before restore-start (Ending restore attachments, Nit),
so on such a node it exits 3 having called nothing; `cancel`, which runs
AbortRestore first, counts that answer as no attachment (none can exist there). The hazard by
mode:

| Node | The restore device |
|---|---|
| Direct mode, without #48 | No recovery flag, as every add today; stopped by the kernel at an exit; the next start ends or leaves it |
| Direct mode, with #48 | The same: no flag, no block; the scan applies #15's rule and never keeps it |
| Direct mode, limit 1 (before 6.9) | As direct mode |
| `--privsep`, with or without #48 | None: restore refused, so the helper never holds a restore device |
| A mode change across a restart | Already stopped by the kernel after the exit: no mode can resume it |

Restore under `--privsep` is deferred to #74 and #48 (a follow-up, to be filed): an unrecoverable
form of the helper's `add`, for which the helper writes no #48 block and which it deletes at the
child's exit, no `kept/<ID>` note, and a `--privsep` test.

Every answer fits #74's 4096-byte cap. #15 instance/generation routing and #79 device
ownership stay binding; failed I/O buffers remain pinned until kernel completion, even after job
cancellation.

Expected verification leaf is SHA256 of exactly chunk_bytes logical decoded bytes (zero holes
included), computed by the helper from the bytes it wrote. Merkle leaf digest=SHA256(UTF8
`flaminio.restore.chunk.v1` + chunk_index:u64LE + chunk_bytes:u64LE + leaf_hash32). Internal
digest=SHA256(UTF8 `flaminio.restore.node.v1` + left32 + right32); duplicate final odd leaf at
each level. Root binds total chunk count/size alongside digest in BeginRestoreFinalize. Unkeyed
hash is comparison, not malicious-replica MAC/freshness; all archive AEAD/keyed BLAKE3 checks
happen in the reader first, under #52 profile. No repository key is sent to the helper, node or
metadata. Membership freeze keeps durable proofs stable; interrupted verify may retry the same
frozen generation or start a new writer and reverify.

The helper replays the full nonzero 4 KiB image after writer/process restart, not an unproven
local skip cursor. All-zero blocks/extents remain logical holes on a fresh volume. Physical
destination clusters may include neighboring zeros; exact source page-table/allocation identity
is not claimed. It writes with `O_DIRECT|O_EXCL` from 4096-aligned buffers, then `fdatasync` and
closes the device before restore-close (R1-C17).

## Command/output and lab integration

**Two processes (R1-C1, R1-S1).** `flaminio-io restore-volume` is the helper: root on the
destination node, holding the destination's administrator pair (bound to `system-provisioner` once
active) and the control socket; it holds no repository key or store credential and parses no bucket
bytes. From its main thread, which lives as long as it does (R3-S2), it starts `flaminio-backup
restore-read` as the reader's own system user, `flaminio-restore-read` (R2-S3), refusing to
start it if that user is absent, is uid 0 or is #52's `flaminio-backup`: no supplementary
groups, empty environment, stdin `/dev/null`, stdout and stderr pipes to the helper, no other
descriptor; and, set between fork and exec (R2-S5),
`PR_SET_NO_NEW_PRIVS`, `PR_SET_PDEATHSIG` SIGKILL (the child exits if its parent is already not
the helper), its own session, `RLIMIT_AS` 2 GiB, `RLIMIT_NOFILE` 256 and `RLIMIT_CORE` 0. These
are set in a `pre_exec` closure, which Rust's `Command` runs after its `gid`/`uid` switch, since
the kernel clears the parent-death signal on a uid or gid change (R3-S2). The parent-death
signal follows the spawning thread, hence the main thread. The reader bounds its own
reservations, so its address space tracks its memory (R3-S3): `mallopt(M_ARENA_MAX, 2)` first
(its environment is empty), and a runtime of 2 worker threads and at most 4 blocking threads.
The helper kills the reader when it exits on an error. The reader reads, authenticates and
decodes the archive and holds no Flaminio pair: its configuration carrying one is refused,
exit 2, so #52's "never an administrator pair, never root" holds for every process that parses bucket
input. Its files are its user's alone: its config, key and credential in
`/var/lib/flaminio-restore` and its store client pair in `/etc/flaminio/store-client-pki` (both
`flaminio-restore-read`, 0700); it cannot read the helper's files, #52's
`/etc/flaminio/backup-pki` or `/var/lib/flaminio-backup`.

**Ending restore attachments (R2-C2, R2-S2).** Immediately before restore-start (the helper's
second read of `restore-list`; the first, before any metadata call, is Direct mode only's, Nit), in every
`resume` (a terminal job's included, R4-C1) and in `cancel` after AbortRestore, the helper reads
`restore-list` (R3-C1) and sends restore-detach
for the restore attachment on its node if it is of its own operation, any generation, or its job
(GetRestore) is terminal or has a later writer generation. `ok`, `ok superseded`, `ok
not-recorded` and `no restore attachment` (the sweep's `delete` or an io restart ended it
first, Nit) all count as ended; `closure not recorded` is retried as restore-close's `device is
open` is. restore-detach's `error: VOLUME is attaching` (a starting entry, a late start's, R6-C1)
is retried every 500 ms for up to 30 s, the same 30 s as #15's detach wait (R7 Nit), then the helper exits 1, the
restore left Incomplete (in `cancel`, aborted: the sweep's `delete` ends the entry). After its own restore-detach in that run, restore-start's `error: VOLUME is stopping
here` (#15 still ending the entry just detached, R4-C3), `error: VOLUME: limit: ...` and `error:
VOLUME: restore slot taken` (an entry just detached, of any volume, counted until it ends, R5-C3)
are retried every 500 ms for up to 30 s, #15's own detach wait, then the helper exits 1, the
restore left Incomplete. Without its own restore-detach in that run, each exits 1 at once.
restore-start's `error: VOLUME is restoring here` (another run's entry), `error: VOLUME: the node
is shutting down` (R6-C2) and `no answer in time` (a late start, R6-C1) exit 1 at once, never
retried, the restore left Incomplete. In a restore or a `resume`, `restore needs direct mode`
answered by `restore-list` or restore-start after the first metadata call (the node restarted
under `--privsep` between the reads) exits 1, not 3, the restore left Incomplete and its acquired
generation unused (Nit); `cancel` counts it as no attachment, as above. A crashed helper's resume, a writer moved away, a resume of a completed job and a cancel thus
end the attachments on the node they run on; the abort's tombstone ends the writer node's through the sweep's
`delete`.

`flaminio-io restore-volume --archive REF --destination-config FILE --operation-id UUID
--replicas N --min-isr N --failure-domain node|rack|zone --failure-domain-policy strict|bestEffort
[--name ID] [--output-format table|json]`; its config (root, 0600) supplies the destination
metadata address and administrator pair paths, the local control socket, and the reader's
config path and binary. REF is `REPOSITORY_UUID/BACKUP_UUID/VERSION_ID`: two UUIDs of 32
lowercase hex and the store's version id, 1–1024 bytes of 0x21–0x7e other than `/`; anything
else is exit 2 (Nit; #52's REF takes this text form, by whichever lands second).
`restore-volume status|resume|cancel --operation-id UUID --destination-config FILE` reuse exact
job identity; cancel is AbortRestore, then the local attachment's end, no generic force flag.

`flaminio-backup restore-read --config FILE --repository-key-file FILE --archive REF`, as #52's
worker options (Nit). Its config (`flaminio-restore-read`, 0600) supplies the repository
URL/bucket/prefix and, for the store (R1-C2, R1-S4): `store_ca`, the CA bundle that signs the
store's server certificate, a file of its own used only by the S3 client and never merged into
a Flaminio bundle; `store_identity`, required, as #52's, compared exactly whatever its trust
domain; the store client pair, issued by the store owner's CA, whose identity is no Flaminio
identity of either cluster (its URI is outside `flaminio/`, which `pki.ParseID` and
`flaminio-net`'s `Identity` refuse); and the credential file, a list/get-version-only S3
credential. Every secret file takes #52's checks (Secret files). No source-cluster identity,
metadata or network call, S3 mutation or KMS is needed for an unencrypted archive.

**The reader's stream (R1-S1).** Frames `[u8 kind][u32le body_len][body]`, body at most 1 MiB +
16 bytes:
- 1 Header, once, first: strict canonical JSON, at most 64 KiB: `{schema:1, archive_ref,
  archive_digest, source, logical_bytes, chunk_bytes, data_encoding, encrypted, backend_profile,
  immutable_guarantee, graph_checked, candidate_state?}`;
- 2 Extent: `offset:u64LE` then data; offset and length multiples of 4096, length 4096 to 1 MiB,
  data not all zero, offsets strictly increasing and nonoverlapping, within `logical_bytes`;
- 3 End, once, last: `extents:u64LE, bytes:u64LE`, equal to what was sent;
- 4 Error, once, last: strict JSON, at most 4 KiB: `{code, object_ref}`. The helper sends it to
  ReportRestoreProgress only as `error_code` = `code` if it matches `^[a-z0-9_]{1,64}$`, else
  `reader_error_invalid`, and `error_object` = `object_ref` with each byte outside 0x20–0x7e
  replaced by `?`, cut at 256 bytes (R2-S7).
The helper refuses a Header whose `archive_ref` is not `--archive REF` (Nit).
Anything else (an unknown kind, a length over its cap, a frame out of order, trailing bytes, end
of file before End or Error) ends the run, exit 1, the restore left Incomplete. The helper reads
nothing else from the reader; stderr lines, at most 1024 bytes with non-printable bytes replaced,
are forwarded as progress text. Fuzz target `restore_frames` (`dataplane/fuzz/`).

**#53's restore checks (R1-C12, R1-S7).** Before its Header frame, the reader runs #53's restore
checks: an immutable repository's schema-2 root, and the retention of the root, every page and
block version and the descriptor's (#53 S8, "graph size + 2 retention reads"), with #52's
content checks as each block is read. The Header carries #53's `backend_profile: {id, version,
qualified, source}`, `immutable_guarantee` and `graph_checked`, and under a candidate profile
`candidate_state`; restore-volume reports them in its JSON. `immutable_guarantee: true` only
for an exit-0, Ready restore of a covered archive under a qualified profile, as #53's `restore`.

JSON schema_version1: operation/archive/source/destination refs, phase, bytes_done/total,
writer generation, error_code/object_ref, completion/unsupported fields, and #53's fields above;
progress stderr, no secret/key/token output. Exit0 terminal success/status;1
error/incomplete/auth/corruption;2 usage;3 unsupported encoding/feature. Same completed job
returns outcome without replaying onto Ready. Missing source/object/space remains incomplete and
diagnosable, never auto-deletes or formats.

The lab's destination helper and restore check are the design's (Lab). Encrypted restore,
metadata snapshots/revision restore/disk-only reconstruction, Velero, force data-loss modes and
full ransomware/loss runbooks are absent; #71 adapter advertises only this owner capability after
it exists. General repository GC read-pin and encrypted key/escrow interfaces remain later gates,
not simulated as implemented here.
