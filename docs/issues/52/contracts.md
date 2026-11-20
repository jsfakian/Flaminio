# Issue #52: snapshot-reader and repository contracts

Appendix to [design](design.md), shared with #53/#54/#58/#59. Settled with the plan, 2026-11-13;
changed after approval as decisions.md lists (Plan review rounds 1 to 6).

**Pending the coordinated byte review (round 1, R1-C12).** #58's settled decision fixes the bytes
#51, #52 and #58 share "in one coordinated review with #51 and #52" before implementation, and #51
(clear) holds its side pending it. Until that review, `SnapshotRefV1`, `Diff`, `SourceProofV1`
and the snapshot-reader/v1 bytes below are proposed, not final, and none of them is built before
it (deferred, not settled here). The repository format v1 and the rest of this file are settled
with the plan.

## Snapshot/root/diff interface

`SnapshotRefV1={cluster_uuid,volume_id,first_epoch,snapshot_name,cut_generation,
snapshot_operation_uuid,geometry:{size_bytes,chunk_bytes,logical_block_bytes},
data_encoding,encryption_descriptor}`. Numeric wire values are checked u64; IDs are validated.
**Where each field comes from (R1-C9).** `cluster_uuid`: the cluster record (Records, below).
`volume_id`, `first_epoch`, `chunk_bytes`: the header. `snapshot_name`, `cut_generation` and
`snapshot_operation_uuid`: #51's Committed catalog record (`name`, `generation`, `operation_id`).
`size_bytes`: the header's size when the grant is authorized, fixed in the grant for its life (a
resume uses the grant's ref, never a newer header's). #51's record holds no size, and a chunk
added by #70's growth after the cut reads as zeros in the snapshot (#51 S19), so the archive
equals the snapshot read at that size. `logical_block_bytes`: 4096, the only value
(`flaminio-io/src/ublk.rs:187`). `data_encoding`: `plain`, and `encryption_descriptor` absent:
the only values this increment admits (encrypted volumes are refused, spec S8).
The UUID/committed-operation reference is supplied by the #51 metadata cut. No physical root
byte ABI is invented here: `SnapshotView(ref,chunk)` is #51's committed layers below cut g.
Different chunk roots may lazily represent the same committed cut; caller-visible data is exact.

`Diff(ref_a,ref_b,chunk,cursor,max_extents)` returns sorted nonoverlapping cluster-aligned
`{offset_bytes,length_bytes}` candidates from generations [a,b), plus opaque next cursor.
Both refs must have identical cluster/volume incarnation/data identity, chunk and block sizes,
`ref_a`'s `size_bytes` at most `ref_b`'s (a chunk added since `ref_a` has all its changes in the
range, design: incremental map), and a<=b;
there is no current-data or uncommitted-snapshot endpoint for the external reader. Every actual
changed 4 KiB block is covered; false positives are allowed per #51 S20. Canonicalize the union
and expand intersections to 4 MiB volume-block indices, deduplicating indices across chunks.
A block spanning several chunks is assembled through separately authorized chunk reads.
Maximum 1024 extents/page and 1 MiB returned data per read; checked bounds precede allocation.
Snapshot read/diff must preserve explicit stored-zero/ciphertext encoding, not decrypt in worker.

`SourceProofV1={node_id,device_uuid,store_generation,chunk_index,isr_version,snapshot_ref}`
binds a complete #51 snapshot view to that exact source lifetime (R1-C9: `device_uuid` is the
source's #61 `ReplicaRef{node,device_uuid}` in the chunk's ISR and `store_generation` #61's
`Node.store_generation`; they replace `store_uuid` and `replica_generation`, which no plan
defines). The completeness evidence is #51's (#51 contracts, Completeness evidence): a view with
#51's snapshot Committed and the node in the chunk's ISR at `isr_version`, established by
RegisterSnapshotSource's transaction, which compares the header, the chunk record, the catalog
record and #61's node record at the revisions read (Transactions, below; R1-C4). Only a current
ISR source holding the committed cut in full may obtain it; missing disk identity/completeness
evidence refuses. Revalidate source membership/version around bounded read batches; a changed proof
invalidates the batch. A moved/partially copied/new store is never accepted from name/epoch alone.
#51/#61 must agree durable completeness/lifetime evidence before implementation; no fake root
or “empty means zeros” shortcut is accepted. #58 consumes the same semantic diff/read interface.
Settled 2026-11-13 (user's instruction): #52 uses #51's completeness evidence and #61's store UUID as their plans settle them; its source proofs are built only once both are implemented, and a source without either is refused (see decisions.md).

## Reader grant/RPC/lifecycle

Proposed worker URI `spiffe://<trust-domain>/flaminio/backup/<worker_id>` in Go/Rust parsers
(`pkg/pki` `ParseID`, `flaminio-net` `Identity`; `<worker_id>` takes the `pkg/pki` segment
syntax). It is no administrator, node or `io` identity. Only explicit grants admit replica
snapshot-reader sessions; the existing target accept path continues node/io-only and rejects
this role. The worker keeps the three open reads (`meta/service/auth.go:79`, #73's "any identity"
row): `GetVolume`, which it uses to revalidate around read batches, `ListNodes` and
`ListVolumes`. They show it what they show any authenticated identity, every volume's record
included (#50's wrapped key envelope, never a key); no row of #73 changes (R1-C16, R1-S8). Its
only other methods are RegisterSnapshotSource and CloseSnapshotRead of its own grants.
Settled at approval, 2026-11-13: the distinct backup identity (see decisions.md, Settled at approval).

**Identity lifecycle (R1-S7).** The lab's `pki issue w` issues the pair on fl-w, its key made in
the VM: #16's `identities VM` gains a `backup` line for fl-w, added by whichever of #16 and #52
lands second, so #16 S10's renewal renews it. The worker is a run-and-exit command that loads its
pair at each new connection, metadata and reader alike, through #16's reloading resolver, so a
long backup picks up a renewed pair at its next handshake (#16 S1); a pair that expires mid-run
fails the next handshake and the run stops, resumable. Neither the reader client nor the reader
listener resumes a TLS session (#16 S6). The worker refuses to start, exit 1, when its configured
pair is not a `backup` identity (an administrator, `io` or other pair) (R1-S2).
**Where it runs (R2-S4).** The worker runs as a system user of its own, `flaminio-backup`, never
as root. Its pair is in `/etc/flaminio/backup-pki` (owned by `flaminio-backup`, 0700; its first
key 0600, owner and mode kept by #16's renewal), not in `/etc/flaminio/pki` (root, 0700) beside
fl-w's `admin.key` and `token.key`. `pki.sh` gains that directory for the `backup` name, and
#16's list of directories (#16 design) the same entry, added by whichever of #16 and #52 lands
second. The repository key and the S3 credential the worker reads are that user's files (Secret
files, below). The directory also holds its own `ca.crt`, the CA it checks the metadata service,
the nodes and the store against; `pki.sh` makes the user, and the store's `backup-store` name
and directory, on fl-w only, as the design's Lab states (R3-B1, R4-B1). `identities VM` also
gains a fl-w `backup-store` line, so #16 S10's renewal renews the store's pair too (R4-C4).

**Records (R1-C4).** Under `/flaminio/v1/`, JSON with `"schema":1`. `<fe>` is the first epoch in
20 zero-padded decimal digits, as #51's catalog; `<op>` the operation UUID, 32 lowercase hex;
`<chunk>` 8 decimal digits, as chunk keys (`service.go:65`). No record is in the header, and no
call below writes the header, so no grant or source write moves its revision: ChangeIsr,
MoveReplica and #77's pinned Attach are not disturbed by them.

| Key | Value | Written | Deleted |
|---|---|---|---|
| `cluster` | `{cluster_uuid}`: 16 random bytes, hex | the first AuthorizeSnapshotRead, if absent | never |
| `snapread/<v>/<fe>/reader` | `{closing, open, held, next}` | Authorize, Close, the last ack or exclusion, deletion's intent, Reopen | with the volume |
| `snapread/<v>/<fe>/grants/<op>` | `{generation, worker, ref, state, pending, sources, outcome, root}` | Authorize, Register, Close, acks, exclusion | collected by Authorize; with the volume |
| `snapsrc/<node>/<v>/<fe>/<op>/<chunk>` | `{device_uuid, store_generation, isr_version, token_digest}` | Register | its ack or its exclusion |

`closing` is set by a deletion's intent and cleared only by ReopenSnapshotReads (R2-C2,
R2-S5); `open` counts the volume's grants not closed (at most 4); `held` its grant records (at
most 64); `next` the generation the next grant takes, from 1. A grant's `generation` is `next`
when Authorize claims it, and the claim raises `next`. The reader record lives as long as the
incarnation, so no grant reference is used twice, even for an operation id reused once its
record is collected (R2-S1). A source's `token_digest` is the SHA-256 of the FLBR token issued
for it. A grant's `state` is `active`, `closing` or `closed`; `pending`
counts its source records not yet acked or excluded; `sources` counts registrations (at most
chunks × (RF + 2)); `outcome` is `completed` with `root` = `{repository_uuid, backup_uuid,
version_id}`, or `aborted`. `cluster` gives `SnapshotRefV1.cluster_uuid`; #54 reads it.
**Collection.** An ack or an exclusion deletes its source record, so a closed grant (`pending`
0) holds none. A closed grant record stays, for its operation's idempotence and its outcome,
until an AuthorizeSnapshotRead finding `held` = 64 deletes the oldest closed one (the lowest
`generation`, since `kv.Entry` carries no create revision, R2-C6; with at most 4 open, one always
is) in its own transaction. After the transaction that deletes the volume's header,
`deleteChunks` also deletes `snapread/<v>/`, every incarnation's records, in batches of at most
120, each conditional on the header being absent, as #51's catalog (`snapshots/<volume>/`).
DeleteVolume's header-absent branch and a creation's claim, which know no first epoch
(`service.go:441, 527-534`), delete the same prefix (R2-C6). No `snapsrc/` record remains by
then: the mark needs `open` = 0.

New metadata RPCs, messages generated in api/proto/gen and mirrored in Rust metaclient. A grant
reference is `{volume, first_epoch, operation, generation}`.
- `AuthorizeSnapshotRead`: request operation UUID, volume, snapshot name, worker URI; response
  the grant reference and its `SnapshotRefV1`. Claims the grant conditionally on the current
  header, the Committed catalog record and the reader record (Transactions); a repeated identical
  operation returns its grant, and the same operation id with any other field different is
  `AlreadyExists` (R2-S1). Refused before any write: an invalid id or URI (`InvalidArgument`;
  the worker URI must parse as a `backup` identity in the instance's trust domain and equal its
  `pki.ParseID(..).String()` form, as #73 requires of bound IDs); a volume unknown, being created
  or deleting, or a snapshot unknown (`NotFound`); a snapshot not Committed, the volume closing
  (`volume <v> is closing for a deletion: re-run it, or snapshot-read reopen`), 4 grants open
  (`FailedPrecondition`); an encrypted volume: #50's `Volume.encryption` (field 20) with a profile
  other than `none`, or #50's mark in the header's `id` (`FailedPrecondition` `volume <id> is
  encrypted: no backup until its keys are pinned`; whichever of #50 and #52 lands second adds this
  refusal and its two test cases, as #51 does for RequestSnapshot) (R1-C6, R1-S1).
- `RegisterSnapshotSource`: request grant reference, chunk, node; response `SourceProofV1` and a
  signed source-scoped FLBR token. The service first signs the token, then one transaction
  commits the source record with the token's digest, and the token is returned only after that
  commit (R3-S1); a lost compare signs again, and a token whose transaction did not commit is
  never returned. The transaction's compares: grant active at that generation, the node in the
  chunk's ISR at the chunk record's `isr_ver`, #61's `ReplicaRef` and node record, #51's Committed
  record, the node not in #13's `forgotten`. The record keeps the issued token's `token_digest`.
  Its first check of the grant is its worker: a grant of another worker answers `NotFound`,
  whatever its state, revealing nothing (R1-S10, R5-S1). Only then is a grant not active
  refused, before any signing, `FailedPrecondition` `grant <op> is closing` (or `is closed`), a
  message for people: the worker matches no message (CLI, below; R4-C1, R5-C3).
  Idempotent per (grant, chunk, node): a repeat replaces the record's proof and digest in place, so
  a token issued before it no longer validates (R2-S1), and `sources` grows only for a new pair
  (`ResourceExhausted` past its cap).
- `ValidateSnapshotRead`: request node, grant reference, chunk, and from the verified token its
  worker URI, its `SourceProofV1` and its digest (R2-S1); a linearizable read, no write.
  Mandatory on new/restarted reader connections. Refused: grant missing, closing or closed; a
  `generation` other than the grant's; a worker other than the grant's `worker`; a proof whose
  `snapshot_ref` is not the grant's `ref` or whose `device_uuid`, `store_generation` or
  `isr_version` differ from the source record's; a digest other than its `token_digest`; no
  source record for (node, grant, chunk); the node forgotten (#13 S19). No offline
  signature-only admission.
- `ListClosingSnapshotReads` (R1-C2): request node, cursor; response at most 1024 of the node's
  source records whose grant is closing, each `{grant reference, chunk}`, and the next cursor
  (the last key returned). No write. Its reads (R2-C7): one linearizable `List` of
  `snapsrc/<node>/` (`kv.List` has no limit, so each page lists the node's whole prefix: at
  most 4 open grants × the node's replicas, a closed grant holding no source record), then one
  linearizable `Get` of each distinct grant on the page (≤ 1024). Reads at different revisions
  are safe: a grant's state only advances, and an ack of a record gone answers OK. A forgotten
  node is answered `FailedPrecondition` `node <n> is forgotten` (R2-C8, The barrier).
- `CloseSnapshotRead`: request grant reference, `outcome` (`completed` with `root`, or `aborted`;
  an administrator closes `aborted` only); response state, `pending`, one unacked source's node,
  and the recorded `outcome` and `root`. A grant reference whose `generation` is not the grant
  record's answers `FailedPrecondition` with the fixed message `grant <op> generation <g> is not
  current`, `<g>` the request's, and writes nothing (R3-S2, R4-C2). Active -> closing (closed at
  once when `pending` = 0), recording outcome and root; no new source registers. **A repeated close
  (R4-C2)** keeps the first recorded outcome and root and answers the state with them, never an
  error, whatever outcome it asks; a grant a deletion's intent made closing, which has no outcome
  yet, takes its first close's outcome and root, whether still closing or already closed by its
  acks (R5-C2). Repeated on a closing grant, it first excludes its sources on
  forgotten nodes (below). Records completion (R1-C13): there is no other progress or
  key-reference RPC.
- `AckSnapshotReadClosed`: request node, grant reference, chunk; the node's own source only, after
  it closed admission, drained or cancelled outstanding read permits and closed sessions of that
  grant. A grant reference whose `generation` is not the grant record's answers
  `FailedPrecondition`, with the close's fixed message, and deletes nothing, so a delayed ack of
  an earlier grant of a reused operation id never removes the current one's source record
  (R3-S2). Deletes the source record and lowers `pending`; the ack that reaches 0 makes the grant
  closed and lowers the reader record's `open`. An ack of an absent record answers OK (a retry);
  one of an `active` grant's record answers `FailedPrecondition`, so only a closing grant is
  acked closed.
- `ReopenSnapshotReads` (R2-C2, R2-S5): request volume; an administrator's. Clears the current
  incarnation's `closing`, so that a volume whose deletion stopped after its intent (an `Aborted`
  deletion not retried, a caller that gave up on `FailedPrecondition`, a #73 mark refused) can
  be backed up again. Refused: the volume unknown (`NotFound`); its header deleting or not
  parsing, or `open` above 0, grants still closing (`FailedPrecondition`). Not closing: OK, no
  write. Its transaction compares the header and the reader record at the revisions read: a
  deletion that marks first fails it, and a deletion that read the record before it fails its
  mark's compare of that record and runs the barrier again from a new read, so a grant
  authorized after the reopen still fails the mark.

**Roles (#73's "Methods and roles", R1-C1, R1-S2, R1-S3).** Whichever of #52 and #73 lands
second adds these rows, the worker rule and the compares below. In #73's Authorize,
AuthorizeSnapshotRead, ReopenSnapshotReads and an administrator's CloseSnapshotRead join the
existing methods (`inactiveRules` before activation), as #51's RequestSnapshot; the node methods
pass by a node rule, as #13's, `id.Component == "io" && id.Node == req.node`; and a **worker rule**
beside the node rules passes a `backup` identity to RegisterSnapshotSource and CloseSnapshotRead
with no store read, the handler then requiring the grant's `worker` to equal the caller's URI
(`NotFound` otherwise) before any other check of the grant (R5-S1). Every other method refuses a
`backup` identity, the open reads apart.

| Method | Before activation | Once active | Event |
|---|---|---|---|
| `AuthorizeSnapshotRead` | administrator name | `cluster-admin` | – (follow-up) |
| `RegisterSnapshotSource` | the grant's worker | the grant's worker | – |
| `ValidateSnapshotRead`, `ListClosingSnapshotReads`, `AckSnapshotReadClosed` | the node's data plane, for itself | the node's data plane, for itself | – |
| `CloseSnapshotRead` | the grant's worker; administrator name | the grant's worker; `cluster-admin` | – (follow-up) |
| `ReopenSnapshotReads` | administrator name | `cluster-admin` | – (follow-up) |

Insecure instances (`--insecure`) refuse all seven (`FailedPrecondition`, `reader grants need
mutual TLS`), before and after activation: the handlers cannot check a worker or a node, and
the reader listener is mutual TLS only. Audit (R1-C11, R1-S4): no event in this step;
`docs/followups.md` gains "Audit events for snapshot-read grants" (AuthorizeSnapshotRead,
CloseSnapshotRead, ReopenSnapshotReads, a deletion's closing intent; #73 S7: the issue that adds
them owns the hook), issue to be filed.

**The barrier.** Closing reaches a source by its own poll (R1-C2): each node's data plane calls
`ListClosingSnapshotReads` for itself at start and every 5 s, as #13's sweep, from its metadata
client's thread, paging by cursor. Its pages, its acks and the reader sessions'
ValidateSnapshotRead calls (R4-S1) share #13's sweep rule (#13 design, The sweep): one reader
call outstanding, none queued behind it, each sent after the previous answer, so a ChangeIsr on
the same client (`MetaClient` runs one call at a time) waits behind at most one reader call
(R2-C3). A ReaderHello that needs a validation while a reader call is outstanding is `Refused`
(`retry`); a poll or an ack waits for the outstanding call's answer, so validations never
starve the barrier. For each listed source the engine closes
that grant's admission (new and queued requests refused), cancels or drains its permits, closes
its reader sessions, then acks. A source with no session acks at the next poll; a restarted
source acks at its first, with nothing to drain. The engine's closed set is a cache: the durable
refusal is ValidateSnapshotRead's, so no RAM-only blacklist or expiry is relied on.
**Exclusion (R1-C10, R1-S5).** The only exclusion proof is #13's `forgotten` mark (S19): a node
marked gone for good never registers again, and every reader method refuses it. A forgotten
node that still runs (#13 does not stop its data plane) learns it from its poll's
`FailedPrecondition` `node <n> is forgotten`: it then closes every reader session and refuses
new ones for the rest of its run, so no session outlives its sources' exclusion (R2-C8). A
CloseSnapshotRead of a closing grant and a deletion waiting on the barrier first exclude that
grant's sources on forgotten nodes, in batches comparing the mark (naming the node) and the grant
at the generation the call names (a deletion's, the one it read). The worker retries its
CloseSnapshotRead until the grant is `closed` (CLI, below), so a source whose node is forgotten
after the worker's first close is still excluded (R3-C2); and a grant a deletion's intent made
closing reaches the worker through its reader sessions, closed at the barrier or lost with their
node, and its registrations then refused, which end the run with its abort close, retried the
same way, so that grant too is excluded and closes, even after its barrier was abandoned (CLI,
below; R4-C1, R5-C1, R5-C3). Nothing else releases an unacked
source: a down node that is not forgotten blocks, as settled.
**Deletion (R1-C5, R1-S3, R1-S5).** Every path to a header's deleting mark runs the barrier after
its admission and before the mark: DeleteVolume after #73's checks (unassigned, wrong kind,
protected; today, none), #73's OverrideDelete after its own, and the deletion of a header that
does not parse (`service.go:535-551`), which #73 counts as published. So a refused deletion
writes nothing and closes no grant (#73 S5). With a reader record holding open grants, the call
writes the closing intent (`closing`, each active grant `closing`), excludes forgotten sources,
and answers `FailedPrecondition` `volume <v>: reader grant <op> waits for node <n>` while `open`
is above 0; CSI's provisioner retries it. With `open` = 0, the mark's transaction also compares
the reader record at the revision read, absent included, so a grant authorized after the read
fails it and the loop reads again; a closing record refuses AuthorizeSnapshotRead, and a closing
grant RegisterSnapshotSource. The intent stays until that deletion completes, the records going
with the volume, or until ReopenSnapshotReads clears it: a deletion that stops after its intent
leaves the volume's backups refused until then (R2-C2, R2-S5). A header that does not parse
gives no first epoch: the call lists `snapread/<v>/` (each incarnation's reader record and at most
64 grants; earlier incarnations keep none past their deletion) and applies the barrier to every
reader record found. A header being created has no reader record (Authorize refuses it), so #76's
rollbacks compare nothing new. Later #51 snapshot deletion refuses a snapshot that a grant not
closed names (the grant record is the pin). A timeout or an offline node is a blocked deletion, not
permission to release pins. Earlier-authorized bytes may arrive later or already be copied by the
worker; revocation cannot erase disclosed data.

**Transactions (120 compares and 120 operations, `meta/kv/kv.go:33`).** No transaction compares
every source: closing advances by the grant's `pending` counter. Lost compares retry at most 32
times with the service's backoff, then `Aborted`.

| Call | Compares | Writes |
|---|---|---|
| AuthorizeSnapshotRead | header, catalog record, `reader` (absent included), `grants/<op>` absent, `cluster` (absent included); under `inactiveRules` `tenant/authorization` absent; at `held` = 64 the collected grant: ≤ 7 | the grant, `reader`, `cluster` if absent, the collected grant's deletion: ≤ 4 |
| RegisterSnapshotSource | header, chunk record, catalog record, #61's node record, `forgotten`, the grant, the source record (absent included): 7 | the source record, the grant: 2 |
| CloseSnapshotRead | the grant (read at the request's generation); `reader` when it closes: ≤ 2 | the grant; `reader` when it closes: ≤ 2 |
| AckSnapshotReadClosed | the grant (read at the request's generation), the source record; the last ack `reader`: ≤ 3 | the source's deletion, the grant; the last `reader`: ≤ 3 |
| An exclusion batch | `forgotten`, the grant (at the call's generation); the last batch `reader`: ≤ 3 | ≤ 100 source deletions, the grant; the last `reader`: ≤ 102 |
| A deletion's closing intent | header, `owners/<v>` (absent included), under `inactiveRules` `tenant/authorization` absent, `reader`, each grant not closed (≤ 4): ≤ 8 | `reader`, each active grant: ≤ 5 |
| ReopenSnapshotReads | header, `reader`, under `inactiveRules` `tenant/authorization` absent: ≤ 3 | `reader`: 1 |
| #73's mark, the override's mark | #73's (header, owner record, tail, a tenant's policy, under `inactiveRules` the authorization record: ≤ 5; today's: the header) plus the reader record, absent included: ≤ 6 | unchanged |
| An unparseable header's deletion | #73's (or today's) plus each reader record listed under `snapread/<v>/` (one per incarnation left, absent included) | unchanged |

## Snapshot-reader protocol

Separate replica listener, `flaminio-io node --reader-listen ADDR` (R1-C7): no reader listener
without the flag; the lab passes port 7413 (7411 is the replica's, 7412 #71's observer). The
worker dials the host of the source's `Node.address` at its configuration's `reader_port`,
default 7413. TLS1.3 mutual authentication, no session resumption, protocol `snapshot-reader/v1`.
**Identities (R1-S6, R1-S9).** The reader client accepts a server only if its leaf's single URI
SAN is `spiffe://<td>/flaminio/node/<n>/io` with `<n>` the `SourceProofV1.node_id` it registered
(as #16 S15 does for `meta`); another node's `io`, a `meta`, an administrator or a backup
certificate is refused. The node takes the worker's identity from the peer certificate, never a
frame, and refuses a ReaderHello whose URI or token worker differs from it.
**Caps (R1-S10, R2-S2).** A node serves at most 16 authenticated reader sessions, and 2 per
(grant, node); only a session past its ReaderHello's checks counts. It checks the peer
certificate, the FLBR signature and the token's worker against the peer before any metadata
call, so a refused connection costs no ValidateSnapshotRead. Over a cap: `Refused` (`retry`),
connection closed, nothing queued. Liveness bounds, not safety rules (proposed, to be measured):
at most 32 connections not yet authenticated, a new one over that closing the oldest; the TLS
handshake and ReaderHello within 5 s; an authenticated session idle for 60 s closed, ending with
`Refused` (`retry`) (R6-C2); at most 2
ValidateSnapshotRead calls per (grant, node, chunk) in any 10 s; and, for the node's total, a
node-wide token bucket of validations, a burst of 16 (the session cap) refilled at 4 a second
(R4-S1). Over either, a Hello is `Refused` (`retry`). A Hello carries one source's token, so
a backup validates each source once at its session, again only at a restart, and a worker
walking every chunk at its read rate is not refused (R3-S3). So stalled TCP connections or
handshakes hold no slot an authorized worker needs; a worker cycling connections costs at most
2 validations per registered source in 10 s; and the node sends at most 16 + 4 a second
validations in all, one at a time (The barrier), whatever the workers do.
**Reads (R1-C8).** The reader holds no epoch. It reads through a store entry point beside
`plan_read`, `Store::plan_view_read(key, View::Before(g), offset, buf, first_epoch)`, that skips
the session epoch check (`check_epoch`, `store.rs:756`): data below a committed cut does not
change with a fence (#51 S12, S15), so a failover's higher `fenced_epoch` does not stop it. It
refuses a chunk whose `fenced_epoch` is below `SnapshotRefV1.first_epoch` (an earlier volume of
the name: #30 S5, as #51's `snapshot-read`) and an absent chunk, which it never creates.
Target port 7411 and target Hello are unchanged. First frame ReaderHello carries worker URI,
grant/source token, SnapshotRef and SourceProof. Accepted methods only ReadSnapshot/DiffSnapshot/
DigestSnapshot/Close, each bound to grant generation and admitted immutable snapshot. No target
heartbeat/write/fence/ISR/move/current Read/command passthrough is accepted. Request IDs bind
retries, not authority. Token signature domain/magic is distinct (`FLBR`, v1), signed by metadata,
and binds worker URI, operation/grant UUID+generation, source/device/store-generation/chunk proof
and ref. There is deliberately no safety expiry claim. New node/network protocol handshake
rejects old peers explicitly; replica nodes upgrade together per the existing intent's scoped
rollout.

Frames are `[u32le body_len][body]`, little-endian as the replication frames
(`flaminio-net/src/frame.rs`), maximum body 1 MiB+16 KiB. Body is protobuf v1 with
oneof Hello=1, Read=2, Diff=3, Digest=4, Close=5 and replies Accepted=64, Data=65, Ranges=66,
Digests=67, Closed=68, Refused=69. `Refused` carries a `reason` (R5-B1): `retry` (a cap, a
liveness bound, the idle close, the validation rate or bucket, an outstanding reader call, a
ValidateSnapshotRead failing with `Unavailable`, `DeadlineExceeded`, `Cancelled` (the codes the
node's metadata client takes for no answer) or `Aborted`, or not sent with that client down: the
same token may succeed later; R6-C2, R7-C2) or `closed` (the grant closing or closed, or the
token refused by a check or by a refusal ValidateSnapshotRead answers: only a new registration's
token can).
Each request repeats grant generation/request ID; unknown variants/fields, duplicates, overflow
or out-of-range data refuse before I/O. Exact nested schema and tagged golden vectors are
generated together, not reused as target replication frames.
`Read(offset,len)` is 4 KiB aligned/nonempty/<=1 MiB/within one granted chunk. Digest returns
checksums under #51's snapshot semantics; worker integrity checks do not call CRC authentication.

## Repository v1 objects and crypto

Propose mandatory versioned bucket and conditional writes. Server profile must provide durable
successful PUT, exact-version GET/HEAD, list-versions, read-after-write and If-None-Match support;
qualify actual backend behavior. [S3 conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html)
and [version IDs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Versioning.html) are separate;
an ETag/current version is not content or immutable-version proof. No delete/overwrite permission
is needed. Client identity initially has list/get-version/put only; #53 adds retention rights.
Settled at approval, 2026-11-13: the versioned, conditional-write store profile (see decisions.md, Settled at approval).

Worker inputs: repository URL/bucket/prefix, credential-file, TLS CA/client cert/key paths,
`store_identity`, repository-key-file. **The store's identity (R2-C5, R2-S3):** `store_identity`
is required, with no default: a `spiffe://` URI that the single URI SAN of the store's leaf
certificate must equal, or a DNS name that certificate must carry under the configured CA. Any
other certificate is refused before a request is sent, even one the CA signed for the store's
address (fl-w's `io` or `meta`, an administrator's). In the lab it is
`spiffe://<td>/flaminio/node/fl-w/backup-store`, issued for server use only (design, Lab). No
secrets in argv values beyond file paths; key buffers are zeroized.
**Secret files (R1-S12).** Every secret file (repository key, store credential, private keys) is
opened `O_NOFOLLOW` and refused, naming the file and the check, unless it is a regular file owned
by the running user with no group or other mode bits; the repository key file must be exactly 32
bytes. `repo init` generates K from the OS CSPRNG and creates its file with
`O_CREAT|O_EXCL|O_NOFOLLOW`, mode 0600, writes the 32 bytes, and fsyncs the file and its
directory; an existing file is refused, never overwritten. All Flaminio connections remain mTLS;
proposed store profile also requires TLS1.3/mTLS plus SigV4 (native endpoints lacking mTLS are
unsupported absent a separately approved external-auth exception). No redirect/downgrade or
credential-bearing URL is accepted. Lab candidate SeaweedFS is [Apache-2.0](https://github.com/seaweedfs/seaweedfs/blob/master/LICENSE);
its actual version/conditional/version/TLS behavior must be pinned and qualified before use.
Settled at approval, 2026-11-13: the proposed TLS profile; SeaweedFS as the lab store (see decisions.md, Settled at approval).

Repository master K is 32 random bytes, independently escrowed off-cluster by administrator;
repository_uuid is random. HKDF-SHA256 with salt=repository_uuid and exact UTF8 info strings
`flaminio.backup.v1/names`, `/blocks`, `/manifests` derives separate 32-byte keys. The names key
feeds keyed BLAKE3 over `kind:u8 || plaintext_len:u64LE || plaintext`; digest hex names content.
Block plaintext is <=4 MiB stored snapshot bytes, final block exact remaining length. Page/root
plaintext is canonical strict JSON (sorted keys, no duplicates, u64 decimal strings).

Object keys: `repository`, `blocks/<first2hex>/<64hex-id>`, `pages/<64hex-id>`,
`roots/<backup_uuid>`. Backup root identity is `(repository_uuid,backup_uuid,version_id)`;
list all root versions, never silently choose current. Store LastModified/version are separate
from untrusted manifest timestamps. Each root version is independently authenticated/identified.

Envelope header is 64 bytes: magic `FLBO`[4], version:u16LE=1, kind:u8 (repository=0/block=1/
page=2/root=3), codec:u8 (raw=0/zstd=1), plaintext_len:u64LE, repository_uuid[16], nonce[24],
ciphertext_len:u64LE. XChaCha20-Poly1305 random 24-byte nonce per new encryption, tag16 included
in ciphertext_len. Block key is derived /blocks; other kinds /manifests. AEAD associated data
is UTF8 `flaminio.backup.envelope.v1` + header + u32LE(key length) + exact object-key bytes,
the key relative to the repository prefix (so a repository copied to another prefix still reads).
Compression precedes encryption; use zstd only when smaller than raw. Decrypt/authenticate before
bounded decompression; output must equal declared length. Block cap4 MiB, page/root cap1 MiB;
encoded length <= plaintext cap+80. Unknown versions/codecs, malformed lengths, trailing bytes,
wrong kind/repo/path or hash fail before use. Decoder fuzz corpus includes decompression bombs.

Repository descriptor plaintext: `{schema:1,repository_uuid,block_bytes:4194304,
envelope_version:1,versioned:true,immutable:false,crypto_profile}`; initialized once conditionally,
then its exact version is recorded in worker config/archive roots. #53 immutable repository is
new at setup; never mutate an unlocked descriptor into a locked-history claim.
Root plaintext: `{schema:1,repository_uuid,backup_uuid,operation_uuid,descriptor_version,
source:SnapshotRefV1,block_bytes:4194304,logical_bytes,data_encoding,encryption_descriptor,
pages:[{first_index,last_index,count,page_id,version_id}],base_archive?,created_at,statistics}`.
`base_archive` is informational only: no base manifest is needed to restore. Page plaintext
`{schema:1,entries:[{block_index,block_id,version_id,plaintext_len}]}` sorted/unique/in range.
Pages are nonoverlapping complete sparse maps. Missing indices mean exact stored zeros, not
“look in parent”. All-zero blocks omit data objects; restore must zero their output range.
All object references name exact version IDs; page IDs hash their plaintext. Root/page/key
metadata are encrypted; header sizes, equality and timing leak by design, not payload/name/key.

## Object client/worker/resume seams

Rust crate `flaminio-backup` exposes `ObjectStore::{list_versions,head_version,get_version,
put_if_absent}` and serverless test implementation; proposed production adapter aws-sdk-s3.
Universal request interceptor `before_attempt(context,request)->Admission` runs on EVERY actual
HTTP attempt, including SDK retries/list/head/get/put and future multipart/control requests;
`on_wire_observation(context,counters)` reports the transport boundary actually measured. #59
may deny/defer new attempts and apply a qualified per-run external wire shaper; payload tokens
alone never certify TCP/TLS rate or zero post-window traffic. Existing connections cannot bypass
admission on a new request. Disable unmanaged SDK retry paths or prove the interceptor sees them.
Settled at approval, 2026-11-13: aws-sdk-s3 as the production adapter (see decisions.md, Settled at approval).

Worker operation state is source/ref/grant UUID + repository UUID + operation UUID, in its
private local checkpoint only; the metadata service holds the grant, and completion is recorded
by CloseSnapshotRead (R1-C13). Persist/fsync immutable map/upload progress;
revalidate every previously referenced object/version on resume. If-None-Match conflict requires
GET exact existing version, AEAD/hash/length verification before reuse. PUT followed by exact
GET verifies bytes; store durability still relies on the qualified server contract, not an ETag.
Publish root only after all block/page versions validate. Lost root
reply resolves operation UUID/root version; a different root for that operation is conflict.
Earlier roots remain untouched. Orphans remain allocated; GC/pruning is outside this increment.

Snapshot reads are asynchronous, one <=128 KiB background dispatch per engine turn, <=2 pending
reads/source and proposed 32 MiB/s/source; foreground traffic wins scheduling. Worker has <=4 reads
and <=4 object requests in flight, bounded buffers and separate backpressure; S3 never holds
engine work. Values are initial Proposed limits, to be measured rather than called free overhead.

CLI (R1-C13, R1-S2). An administrator authorizes with `flaminio-meta snapshot-read authorize
VOLUME SNAPSHOT --worker URI --grant-file FILE [--operation-id UUID]` (the operation id drawn at
random unless given, only to retry the same request: R2-S1), which writes the grant reference
and its `SnapshotRefV1` as JSON to FILE (created exclusive, mode 0600), and closes with
`flaminio-meta snapshot-read close GRANT-FILE --abort`; `flaminio-meta snapshot-read reopen
VOLUME` calls ReopenSnapshotReads (R2-C2). **Closing is retried (R3-C2, R4-C2).** `backup`
(its `completed` close or its abort close), `verify --compare-source` and `snapshot-read close`
call CloseSnapshotRead again every 5 s, the wait doubling to 60 s, while it answers `closing` or
fails with `Unavailable`, `Aborted` or `DeadlineExceeded`, until it answers `closed`. Any other
error ends it at once, exit 1, naming the error: the generation refusal says the grant file's
grant was closed and collected (only a closed grant is) and its operation id reused (its fixed
message is matched only to word this: the exit is 1 either way); `NotFound`, that it was
collected or deleted with its volume; the insecure refusal and a role refusal, themselves.
**An unusable grant ends with its close (R4-C1, R5-C3, R6-C1).** Only a failure that leaves the
grant unusable ends `backup` with its abort close, CloseSnapshotRead(aborted), retried as above:
the registration end rule below, which a grant closing or closed always reaches (a `Refused` of
reason `closed`, and 3 tries in a row with no `Accepted`, are each followed by a registration,
which such a grant refuses; R7-C1). Every other failure
(the store, an interrupt before its close, the metadata service unavailable, a lost root reply)
exits 1, incomplete, with the grant active and its checkpoint kept, so a rerun with the same
grant file resumes (S7) and can reach `completed`; a grant a deletion's intent made closing
meanwhile is closed by that rerun, through the end rule, or by `snapshot-read close --abort`.
Once its root is published, only its `completed` close is left. No error message is matched to
get there: a registration failing with `Unavailable`, `Aborted` or `DeadlineExceeded` is retried
as the close is, and any other refusal
makes the worker re-read the volume (GetVolume) and try each ISR member of that chunk once; a
chunk none of whose members registers while its `isr_ver` stays the same ends the run. A grant
closing or closed refuses every registration, so it ends the run that way. **Sessions (R4-C1,
R5-C1, R5-B1, R6-C2).** The worker registers a chunk again (the same source or another ISR
member) before any retry of its reads only after a `Refused` of reason `closed`, after a session
past its `Accepted` ends without the worker's Close (the barrier closed it, or its node went
down), or after the tries before `Accepted` below, so a closing grant reaches it even when every
source is already registered. A `Refused` of reason
`retry` it retries with the same token after a jittered backoff, 1 s doubling to 10 s (proposed,
to be measured); the backoff is per chunk, keeps doubling across a reselection and resets at an
`Accepted`, and the count of tries below is per (chunk, node). A connection that ends before its
`Accepted` with no `Refused` (the node down, TCP or TLS refused, a liveness bound before TLS
completes) counts as a `retry` toward the same node. After 3 tries in a row toward a node with no
`Accepted`, each a `Refused` (`retry`),
whatever its cause, or such a connection (proposed, to be measured), the worker re-reads the
volume and registers another ISR member of that chunk (the same one if it is the only one), as
S4's reselection, so a node that answers `retry` to every Hello (cut off from the metadata
service) does not stall its chunk (R6-C2, R7-C1). `verify --compare-source` registers sources
as `backup` does and ends the same way, matched or not, with its grant's abort close, retried as
above (it records no completion), so its grant file is one no running `backup` uses. **Exit (R4-C2,
R5-C2).** `backup` exits 0 only when its close answers `closed` with outcome `completed` and its
own root; another recorded outcome or root (the first close's, kept) it prints and exits 1, its
root published but not the grant's recorded completion. Its abort path always exits 1,
incomplete, naming the failure that ended the run, whatever the close answers. `verify
--compare-source` exits 0 only when every compared block matched and its close answered
`closed`. `snapshot-read close --abort` exits 0 once the grant is `closed`, whatever outcome it
recorded, which it prints. While it waits, progress on stderr
names the unacked node. Interrupted, the command exits 1 naming that node, the archive already
published if its root was; a rerun with the same grant file resumes at the close (its
checkpoint holds the outcome). The worker:
`flaminio-backup repo init|list`, `backup --grant GRANT-FILE`,
`verify --archive REF [--compare-source --grant GRANT-FILE]`, `restore --archive REF --output
PATH --confirm-output IDENTITY`; common `--config FILE --repository-key-file FILE --output-format
table|json`. The worker never authorizes; every metadata answer is checked against the grant file,
and the service checks the grant itself. REF is typed repository/root/version identity, not
newest-name shorthand. `--compare-source` requires a live reader grant of its own; ordinary
list/verify/restore need no source metadata.
Output identity binds an exclusively created file or opened block device rdev/diskseq/size,
never a symlink/mounted/in-use device. Refuse encryption plaintext-output claims; ciphertext
restore is explicitly labelled. New file is sparse/size-set; block output explicitly writes or
zeroes every hole, fsyncs and optionally read-verifies. Failure reports incomplete destination.
Exit0 success;1 validation/auth/store/corruption/incomplete;2 usage;3 unsupported profile. JSON
schema_version=1 has operation/archive IDs, source/ref, full/incremental reason, read/upload/reused
counts, byte counters, completion/ciphertext-only capability and errors without secrets.
`verify` reports `restorable_point_age_seconds`, now less the store's LastModified of the root
version it verified; `list` reports each archive's age with `verified: false` (S11, R1-C15). Print
no successful partial JSON; progress is stderr. A listed encrypted archive is not certified as
plaintext-restorable without #54's key/escrow/recovery evidence.

#53 retention hooks are metadata-only checks/extensions of these EXACT object versions; no
reference to “current version” is introduced. #54/#58/#59 consume this contract only after its
own implementation/approval. #51 root/completeness decisions remain prerequisites, not assumed
implemented from a proposed interface. Encrypted archives, their retained-key records and #50's
data-identity seam are deferred to the plan that lifts #51's refusal of encrypted snapshots
(round 1, R1-C6, R1-S1; deferred, not settled).
Settled 2026-11-13 (user's instruction): #52 uses #51's completeness evidence and #61's store UUID as their plans settle them; its source proofs are built only once both are implemented, and a source without either is refused (see decisions.md).
