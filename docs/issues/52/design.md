# Issue #52: Incremental backups to S3 — design

[Spec](spec.md), S1–S11; shared [contracts](contracts.md) (the bytes #51 and #58 share pending
#58's coordinated review); [Proposed ADR](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review rounds 1 to 6
(decisions.md).
Manual single-snapshot backup/verify/raw-output restore only; the full issue remains open.

The facts it starts from:
- No backup worker, S3 client/repository or backup RPC exists. Current metadata issues target
  tokens only (`meta/service/service.go:Attach`, `pkg/captoken`), and target Hello takes a session
  and fences (`flaminio-net/src/session.rs:106–155`, `flaminio-repl/src/replica.rs`). Reusing it
  would disrupt the live target. Existing tokens have no expiry/revocation scope for snapshots.
- Store layers/snapshot/digests/mapped exist (`flaminio-store/src/store.rs:1288–1449`), without
  snapshot reads/diff above them. #51's draft S19/S20 supplies layers<g and candidate diff[a,b),
  not a stable physical-root byte ABI. Its snapshots/catch-up are dependencies, not code here.
- A store is opened O_EXCL; the worker cannot open a running disk a second time. Reads must
  use bounded asynchronous work in the node. Ciphertext/key identity is #50's scoped contract.
- Proposed audit ADR0009/0011 require retained references, durable publication and exact object
  identities. They are read as proposals, not accepted new policy. #54 owns broader recovery.

## Worker, grant and source proof (S1–S4, S8)

Propose Rust crate/binary `flaminio-backup` in dataplane/, sharing flaminio-net and Rust metaclient;
future Go schedules call a narrow gRPC job interface, not Rust framing. Object adapter is pinned
aws-sdk-s3; crypto uses blake3, zstd, chacha20poly1305/XChaCha20Poly1305, HKDF-SHA256 and zeroize.
These new dependencies/identity/format are ADR choices with licence/MSRV checks before coding.
A serverless ObjectStore fake runs in make test; actual store enforcement is a later lab gate.
Settled at approval, 2026-11-13: the proposed worker and dependencies, versions pinned after those checks (see decisions.md, Settled at approval).

An administrator runs `flaminio-meta snapshot-read authorize` (contracts, CLI), which calls
AuthorizeSnapshotRead and writes the grant file the worker takes; the worker never holds an
administrator pair and refuses to start with anything but a `backup` identity (R1-S2). The
service claims the grant in one conditional transaction on the header, #51's Committed catalog
record and the volume's reader record (contracts, Records and Transactions; R1-C4), and refuses
deleting/unknown/uncommitted volumes and snapshots, a closing volume, a fifth open grant and an
encrypted volume (R1-C6, R1-S1). Grant and source records live outside the header, so no grant
write moves its revision. For each allowed source the service signs a source-scoped FLBR token,
commits the source record with its digest in one transaction, and returns the token only after
that commit (R3-S1). The worker never receives a target token or calls Attach; besides its
own grants' RegisterSnapshotSource and CloseSnapshotRead it keeps only the open reads, GetVolume
for its revalidation among them, which show it every volume's record as they show any identity
(R1-C16, R1-S8). Each new RPC has its row in #73's table and its `auth.go` rule: the node methods
a node self-rule, the worker's a new worker rule with no store read, the administrator's (with
ReopenSnapshotReads, which clears a stopped deletion's closing intent: R2-C2, R2-S5) the
existing-methods rule; insecure instances refuse all seven (contracts, Roles; R1-C1, R1-S3).

The node serves a dedicated listener, `flaminio-io node --reader-listen ADDR` (port 7413 in the
lab; 7412 is #71's observer, R1-C7), and a snapshot-reader/v1 session only accepts Read/Diff/Digest
of the admitted committed snapshot. The node takes the worker's identity from the peer
certificate and the worker accepts only the source's `io` identity (R1-S6, R1-S9). Validate the
signature domain, complete SourceProof, range/size/grant generation and, only after the
certificate and token checks, an online ValidateSnapshotRead at each new/restarted session,
which compares the token's worker, ref, proof and digest with the grant and the source record.
A grant's generation is never reused, so an operation id reused after collection validates no
earlier token (R2-S1). Caps on authenticated sessions per node and per grant, and separate
bounds on connections not yet authenticated and on validations, bound the cost (R1-S10, R2-S2):
validations per source, and in all a node-wide token bucket, sent one at a time as the poll's
calls are, so a ChangeIsr waits behind at most one of them (R4-S1).
Require #51's completeness evidence and #61's store lifetime (`ReplicaRef` device UUID,
`store_generation`; R1-C9); if absent, refuse instead of inventing a root or treating a new/partial
replica as zero data. Reads go through `Store::plan_view_read` with no session epoch, refusing a
chunk fenced below the first epoch, so a failover does not stop them and an earlier volume of the
name is never read (R1-C8). Only the owning worker can register/read its operation; node/io
validates/acks only its own source.

Reader revocation is a barrier, not a TTL: mark the grant closing, disallow new source
registration; every source node polls `ListClosingSnapshotReads` for itself every 5 s and at
start, off the engine thread, closes the grant's admission, cancels/drains its permits and
sessions, then acks (R1-C2, R1-S5), one reader call outstanding as #13's sweep, so a ChangeIsr
waits behind at most one (R2-C3). A source with no session acks at its next poll, a restarted
one at its first. Acks advance the grant's `pending` counter, so closing never compares every
source and stays within 120 operations (R1-C4). Lost/offline sources block pin release/deletion
unless their node is forgotten (#13 S19), the one exclusion proof: the closing call excludes such
sources in transactions comparing #13's mark (R1-C10); a forgotten node still running closes
every reader session once its poll is refused (R2-C8). A fresh restarted source must validate
metadata before using an old token, so a RAM-only blacklist is never the durable revocation
mechanism. The worker retries its CloseSnapshotRead until the grant is `closed` (contracts, CLI),
and each retry excludes the sources whose node was forgotten since, so a grant closes without an
administrator's call (R3-C2). A `backup` whose grant is unusable ends with its abort close,
retried the same way, and no error message is matched to get there: a grant a deletion's intent
made closing reaches the worker through its sessions, closed at the barrier or lost with their
node, after which every registration is refused and the run ends, so it closes too after an
abandoned barrier, even with every source already registered (R4-C1, R5-C1, R5-C3). Every other
failure (the store, an interrupt before the close, the metadata service unavailable, a lost root
reply) exits 1 with the grant active and its checkpoint kept, so the rerun resumes (S7; R6-C1).
`Refused` says whether the same token may succeed later (`retry`, retried after a backoff) or
only a new registration's (`closed`), so a throttle, an idle close or a transient
ValidateSnapshotRead failure costs no registration at once; after 3 tries in a row toward a node
with no `Accepted` (each a `retry` or a connection lost before `Accepted`), another ISR member is
registered (R5-B1, R6-C2, R7-C1). A
repeated close keeps the first outcome and answers it, `backup` exits 0 only on `completed` with
its own root, and the close's retry ends on any error that will not clear (contracts,
CloseSnapshotRead and CLI; R4-C2, R5-C2). Acks, closes and exclusions refuse a grant
reference of another generation, so a delayed call for an earlier grant of a reused operation
id touches nothing of the current one (R3-S2). Data already authorized/disclosed can be
delivered later or copied; the promise concerns new success permits, not erasure.

Integrate every path to the deleting mark (DeleteVolume, #73's OverrideDelete, the deletion of a
header that does not parse) after its admission checks and BEFORE the mark: with open grants,
write the reader-closing intent while the volume stays externally present, exclude forgotten
sources, and answer `FailedPrecondition` naming the waiting source until the barrier completes;
then the mark, whose transaction gains one compare of the reader record (contracts, Deletion and
Transactions; R1-C5, R1-S3). A refused deletion writes nothing; one that stops after its intent
leaves it until it is re-run to completion or an administrator reopens the volume
(`snapshot-read reopen`; R2-C2, R2-S5). Later #51 snapshot deletion
uses the same grant records as pins. Timeout/liveness is no substitute.
No normal client I/O metadata read is added, and targets' epochs/leases/ISRs remain unchanged.

A source is accepted only with current chunk ISR version and complete immutable snapshot view;
revalidate metadata before/after bounded read batches. Membership/store/root-generation change
invalidates that batch and source, then select another complete ISR member. Keep already verified
repository objects. A failover can change targets but not the source SnapshotRef. An interrupted
cut or partial catch-up is never eligible merely because its replica exists or CRC matches.

## Diff, resource limits and incremental map (S1, S4, S7)

Diff unions layer candidates [base_cut,new_cut), with checked sorted cluster extents and paginated
responses. Cover every actual changed block; false positives are allowed by #51 and counted as
extra read work, correcting the intent's stronger identical-answer expectation explicitly.
Map candidate ranges to 4 MiB volume indices, deduplicate and read full touched blocks in <=1 MiB
chunk-scoped requests. Assemble cross-chunk blocks without assuming chunk size >=4 MiB.

Find the newest base matching cluster/FirstEpoch/geometry/encryption identity whose source cut
is retained and whose complete archive map validates. Full scan/list limits fail explicitly;
no partial scan claims newest/no base. If no eligible base, report full-backup reason. Start
from the base's entire map; replace dirty block references, remove blocks now all-zero and keep
unchanged immutable references. New manifest is self-contained, not a parent chain.
Geometry is compatible when chunk and block sizes are equal and the base's `size_bytes` is at
most the new one's (R1-C9): blocks past a smaller base's size are absent from its map, so zeros
unless the diff names them (a chunk added by #70 after the base's cut has every write in layers
at or above it), and the block holding the base's old end is read again.

Node scheduler dispatches <=128 KiB snapshot work/turn with <=2 asynchronous reads/source and
proposed 32 MiB/s/source, foreground first. Keep store read pins until completion. Worker <=4
reads and <=4 S3 requests in flight with bounded buffers; S3 stalls are outside engine state.
Measure these defaults and adjust explicitly; no claim of zero application cost is made.

## Repository, crypto and publication (S5–S8)

Implement repository v1 exactly as contracts.md: versioned immutable object references; 4 MiB
stored-byte blocks; keyed BLAKE3 names; bounded zstd then XChaCha20-Poly1305; separate HKDF keys
for naming/block/manifest domains; nonce24 from CSPRNG; encrypted root/page/descriptor objects.
Authenticate header/path/repository/kind/length. Public headers, sizes, equality and timing are
not hidden; plaintext data, volume names and keys are not public bucket metadata.

Strict envelope/manifest decoders bound input, decompressed output, page counts and geometry
before allocation; reject duplicate keys/indices, overlap, overflow, bad tags/hashes/versions and
trailing bytes. Root points to a complete sparse page graph with exact version IDs. Absent block
indices mean stored zeros; no other backup root is needed. Dedup shares content objects/pages,
not the authority or manifest of a base. All future restore/retention clients use that same ABI.

Write object with If-None-Match. On conflict, resolve an exact version and GET/authenticate/hash/
length-check before reuse; ETag/name alone does not prove content. After PUT, exact-version GET
verifies bytes and identity. Qualified store durability remains a server contract; a readback
alone does not certify power-loss durability. Version IDs are recorded for data, pages and roots.

Before root publication, ensure every referenced object/page/version is durable and verified,
conditionally PUT the unique root, read it back and resolve lost replies by operation/root ID.
Record completion with the exact root version by `CloseSnapshotRead(completed, root)`, which
closes the grant; its pins go once every source acks (R1-C13). A durable abort is
`CloseSnapshotRead(aborted)`. Retained-key references, and their records outliving the volume,
are deferred with S8 (R1-C6, R1-S1).

List all authenticated root versions, identified by (repo UUID, backup UUID, version ID), using
server LastModified separately from manifest time. Never use mutable newest-version semantics.
An interrupted upload has no published root; completed earlier roots remain untouched. This
increment has no prune/GC. #53 can lock/check/extend EXACT versions without data reupload.

**Encrypted input: deferred after approval (round 1, R1-C6, R1-S1), not settled.** #50 and #51
(clear) admit no snapshot of an encrypted volume until its keys are pinned, so AuthorizeSnapshotRead
refuses one, by #50's field 20 and by its header mark, as #51's RequestSnapshot (contracts). The
plan that lifts #51's refusal brings S8 here: #50's stored representation only, never a DEK
request; its public wrapped key/source/version and data identity copied; retained-key records
(key, schema, bound, writer, releaser) and #50's DeleteVolume change; ciphertext verify/restore;
plaintext recovery/key escrow #54's. XTS/CRC has no malicious integrity/replay guarantee.

## Resume, command and output restore (S7, S9, S10)

Persist private local checkpoints by fsync+atomic rename; the metadata service holds only the
grant and its outcome (R1-C13). A checkpoint binds SnapshotRef/grant/repository/operation;
never resume onto a different snapshot/base. Revalidate arrived object versions before skipping
uploads. SDK failures, process crashes and lost responses converge on the same root or a
controlled conflict, with no overwrite.
Durable abort closes source grants but cannot assume an uncertain root never existed. Only an
unusable grant aborts (contracts, CLI; R6-C1): a store failure, an interrupt before the close or
a lost root reply leaves the grant active and the checkpoint kept, and the rerun resumes and
resolves the root.

ObjectStore adapter provides universal attempt admission and byte-observation hooks, covering
SDK retries, list/head/get/put and future multipart. #59 can defer requests/apply a qualified wire
shaper. Logical payload counters do not claim TLS/TCP bandwidth or zero in-flight tail. Pin SDK
behavior and test the actual request path rather than instrumenting only outer method calls.
S3 profile requires qualified durable/version/conditional APIs and mTLS/TLS1.3+SigV4; native S3
endpoints without client-certificate auth need a separately approved external-auth exception.

CLI grammar and JSON/exit schema are appended. repo init/list/backup/verify/restore take private
credential/key files, never secret argv values. Default restore creates a new exclusive file;
raw device requires validated identity/size/no mounted holders, exclusive handle and exact
output confirmation. Keep descriptors through writes, zero every sparse hole on nonzero devices,
fsync and report incomplete output on failure. It is not a supported restore over an exposed PVC;
metadata-marked restore/CSI integration is #54. Never mkfs, open the running store or invoke SSH.

## Simulator and metadata vectors (S2–S4, R1-C3, R1-S5, R2-C1, R2-C9)

**One model of the grant lifecycle (R2-C9).** `flaminio-repl/src/meta.rs`'s `Metadata` holds
the reader records and their transitions (authorize, register, close, list-closing, ack,
exclude, reopen, the deletion barrier and the records' deletion), beside #13's `forgotten` set
and `forget()` (#13 design, the reference model; whichever of #13 and #52 lands second adds them
as #13 states). The simulator drives that model, and `examples/meta_vectors.rs` emits its
transitions into `meta/testdata/isr-vectors.jsonl`, replayed against the pure functions of
`meta/domain/snapread.go` by `meta/domain/domain_test.go` (`make meta-vectors`, checked by `make
contracts`), as #51 does for the cut. `src/reader.rs` holds only the node side (the poll,
admission, permits, sessions, the ack) and the worker side (source selection under #51's
completeness evidence, batch revalidation, assembly), sans I/O.

**Events (R2-C1, R3-C1).** In a reader configuration (`Config.reader`, in the tests and in
`make sim`'s campaign), the chaos phase gains, beside cut, move and failover:
- *a deletion's barrier:* DeleteVolume at `META` up to its mark: the intent, the exclusion and
  `FailedPrecondition` while `open` is above 0, retried every 5 s as CSI retries, until one
  model step finds `open` = 0, where the mark would commit. The simulator never writes the mark
  and deletes no volume (R3-C1): what a deletion does to the target and the replicas is today's
  DeleteVolume's and #13's sweep's, not the reader model's, and the mark's compare of the reader
  record and the records' deletion are `TestDeleteVolumeReaderBarrier`'s. One barrier in four
  is abandoned after its intent, its caller giving up. Either way a ReopenSnapshotReads follows,
  which the model retries every 5 s while it answers `FailedPrecondition` (`open` above 0) until
  the grants close and it succeeds;
- *the worker model:* every 5 s while it holds fewer than 4 open grants it authorizes a new one,
  during a barrier and after a reopen too, and it registers the next source of each grant it
  holds, a closing one included, reads it and closes `completed` a grant it has assembled; it
  retries each of its CloseSnapshotRead calls every 5 s until the grant is `closed` (R3-C2); a
  session of its that the node refuses as `closed`, closes at the barrier, or loses past
  `Accepted` (its node down), and 3 tries in a row toward a node with no `Accepted` (each a
  `Refused` (`retry`) or a connection lost before `Accepted`; R6-C2, R7-C1), are each followed
  by a registration for that chunk, as the CLI's, so a grant whose sources are
  all registered still learns it is closing (R5-C1); a chunk none of whose ISR members registers
  while its `isr_ver` stays the same ends that grant, and the model closes it `aborted`, retried
  the same way, whatever the refusal (R4-C1, R5-C3);
- *forget a node:* at most one a run, a node down at that moment and running no target:
  `meta.forget` (#13 S19). It stays down to the end, the simulator moves each of its replicas off
  (as a drain), and the settle phase's full-ISR check counts only the nodes not forgotten.

**After a deletion's barrier (R3-C1).** The volume stays live, one incarnation for the whole run,
so the end-state checks (the final reads through the settled target, the divergence check, the
full-ISR check) run unchanged on it; the volume's history is not cut; #13's sweep keeps
`deleted_since` false, as #13 states, since the sim still deletes no volume. The re-create event
of round 2 is cut: with no deletion there is no second incarnation to model.

**Checks** (`sim/check.rs`). A new `reader` field of `sim::Bugs` holds six mutations, each caught
on a seed that passes clean:
- **archive equality:** every block the worker model assembles for an archive equals the
  model's committed snapshot at *g* of the volume;
- **closure:** no read permit is granted after its grant's closure, and no pin goes (grant
  closed, or a deletion's barrier at its mark) while a registered source is neither acked nor
  forgotten;
- **exclusion,** at each commit: no grant or source record is created after its volume's closing
  intent committed, unless a reopen cleared that intent first;
- **reader progress,** in the settle phase beside the existing liveness checks
  (`sim/mod.rs:500-533`): once healed, every closing grant closes (the worker's retried close
  excludes a source forgotten after its first close, and its sessions bring a closing grant to
  its abort close, R5-C1), every barrier requested and not abandoned
  reaches its mark, and every reopen requested succeeds, within `settle_limit`.

| Mutation | Caught by |
|---|---|
| `missing_close` (a node acks without closing the grant's admission) | closure |
| `premature_pin_release` (a pin goes with a source unacked) | closure |
| `source_outside_isr` (a source outside the ISR accepted) | archive equality |
| `skip_batch_revalidation` | archive equality |
| `grant_under_closing` (a grant or source admitted while the volume closes) | exclusion |
| `close_without_notify` (closing never reaches a source) | reader progress |

## Files touched and contracts

- New `dataplane/crates/flaminio-backup/{src,tests}`, workspace deps/lockfile: S1/S5–S11 worker,
  repository/crypto/serverless adapter, CLI and universal request hooks.
- `pkg/pki`, `flaminio-net/src/{identity,reader}.rs`, new bounded reader protocol/types/tests:
  S2–S4 distinct identity/session/auth; no target-token reuse.
- `api/proto/flaminio/meta/v1/meta.proto`, generated Go, `meta/domain/snapread.go`,
  `meta/service/{service,snapread,auth}.go` and tests, `meta/kv` tests,
  `meta/cmd/flaminio-meta` (`snapshot-read authorize|close|reopen`),
  `flaminio-metaclient/src/{pb,lib}.rs`: S2–S3 operation/pin/source/close/ack/reopen, the records
  and the deletion barrier.
- `flaminio-repl` grant lifecycle (`src/meta.rs`), node and worker model (`src/reader.rs`), sim
  and vectors; `flaminio-io` (`--reader-listen`, the poll);
  `flaminio-node` reader sessions, caps and scheduler; `flaminio-store` `plan_view_read`:
  S2–S4/S11 read authority, scheduler and complete-source interface shared with #51/#58.
- Fuzz targets/regressions; `scripts/lab.sh`, `scripts/guest/{backup-store,node,pki}.sh`,
  `scripts/checks/backup.sh`, `scripts/tools/{backup-scripts,node-scripts}-test.sh`,
  `scripts/lab.conf`, Makefile and docs: S5–S11 format/behavior and the lab (below).

Proposed SnapshotRef/diff/grant, separate framed protocol, repository envelope/map, client hook
and CLI contracts are in [contracts.md](contracts.md); the bytes shared with #51 and #58 wait for
#58's coordinated review (R1-C12). Agree #51 root/completeness ABI before coding. All new RPCs are
per-method authorized/default-denied (#73's rows, contracts Roles); worker read scope never
grants attach/mutation. `make proto/contracts` covers generated and handwritten
Rust/client/crypto vectors. Reader barrier requires no new replica on-disk record: metadata pin
state plus mandatory fresh post-restart validation prevents reuse of stale grants.
Settled at approval, 2026-11-13: the proposed contracts (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): #52 uses #51's completeness evidence and #61's store UUID as their plans settle them; its source proofs are built only once both are implemented, and a source without either is refused (see decisions.md).

## Tests

Named per rule (R1-C15); each can fail without its change.

| Rules | Tests |
|---|---|
| S1, S4 | `flaminio-backup/tests/incremental.rs`: `diff_covers_every_change` (property, sparse generations, false positives counted, cross-chunk 4 MiB, cluster sizes), `newest_compatible_base`, `no_base_reasons` (first, other incarnation, other cluster, base gone, `geometry`), `incomplete_scan_refuses`, `grown_volume_keeps_base`. `flaminio-store` tests: `view_read_ignores_fence` (a read across a failover's higher fence returns the snapshot), `view_read_refuses_earlier_incarnation`, `view_read_absent_chunk`. `flaminio-node` reader tests: `partial_or_new_store_refused`, `batch_invalidated_on_isr_change`, `foreground_first`, `reader_caps` (16 per node, 2 per grant), `no_target_session_change`. |
| S2 | `meta/service/snapread_test.go`: `TestAuthorizeSnapshotRead` (each refusal of contracts in order, an encrypted volume by field 20 and by the header mark, idempotent operation, a non-identical repeat `AlreadyExists`, a worker URI of another trust domain or not in `ParseID(..).String()` form, generations rising, the 64th record collecting the lowest-generation closed one), `TestRegisterSnapshotSource` (the token returned only after its record committed, carrying the committed digest, a lost compare re-signing and never returning the first token, not in the ISR, stale `isr_ver`, snapshot not Committed, forgotten node, #61 mismatch, another worker `NotFound`, another worker on a closing and on a closed grant `NotFound` before any state refusal (R5-S1), idempotent per (grant, chunk, node), the source cap), `TestValidateSnapshotRead` (a token from before a re-registration, a collected grant's token under a new grant of the same operation id, another worker, another ref, a proof differing from the record: each refused). `meta/service/auth_test.go` `TestSnapshotReadRoles`: each method by an administrator, each #73 role, the grant's worker, another worker, the node's `io` for itself and another node, an agent, an unbound identity; inactive and active; insecure refuses all seven; the worker's AuthorizeSnapshotRead refused. `flaminio-net` reader tests: `client_accepts_only_source_io` (another node's `io`, a `meta`, an administrator, a backup certificate refused), `hello_identity_from_certificate` (a valid token from another backup identity refused), `no_session_resumption`, `frames_roundtrip_and_refuse`. `flaminio-node` reader tests: `stalled_connections_do_not_block` (stalled TCP connections and stalled handshakes, up to and past the pending bound, do not keep an authorized worker out), `validation_rate_bounded` (a worker walking N chunks of one grant, a session each, at its read rate, is not refused; a worker cycling one source is refused past 2 in 10 s; a worker cycling sessions over many chunks is refused past the node-wide bucket; a Hello while a reader call is outstanding is refused, R4-S1; each of these refusals, a cap's, a session closed idle and a Hello whose ValidateSnapshotRead fails with `Unavailable`, `DeadlineExceeded`, `Cancelled` or `Aborted` (or whose node's metadata client is down) carry `retry`, R6-C2, R7-C2; and a Hello on a closing grant, a re-registered source's old token or a token ValidateSnapshotRead refuses carries `closed`, R5-B1). `flaminio-backup/tests/identity.rs`: `refuses_non_backup_pair`, `picks_up_renewed_pair`, `store_identity_required` (an `io`, `meta` or administrator server certificate at the store's address refused). `meta/cmd/flaminio-meta/main_test.go` `TestSnapshotReadCommands`. |
| S3 | `snapread_test.go` `TestSnapshotReadClosure`: close, `ListClosingSnapshotReads` paging with grants in mixed states (active, closing, closed, collected), ack, an ack of an active grant refused, an ack and a close of an earlier generation under a reused operation id refused (`FailedPrecondition`) with the current grant's source records and `pending` unchanged, the last ack closing and lowering `open`, an ack retried, a forgotten source excluded, a grant closed before its source's node is forgotten reaching `closed` at the worker's next retried close, no `snapsrc/` record left; a repeated close asking the other outcome keeping the first outcome and root and answering them, no error; an intent-closed grant taking its first close's outcome and root, still closing and already closed by its acks (R5-C2); another worker's close of a closing grant at another generation `NotFound` (R5-S1); the generation refusal's fixed message on the close and the ack; RegisterSnapshotSource on a closing grant answering `grant <op> is closing` (R4-C1, R4-C2). `TestDeleteVolumeReaderBarrier`: a refused protected deletion leaves every grant active; the intent closes grants and refuses a new grant and source; `FailedPrecondition` naming the waiting node; a forgotten source unblocks deletion; a header that does not parse; an `interleaveKV` Authorize between the read and the mark fails the mark; OverrideDelete blocked by an unacked source; every incarnation's records deleted after the header, on the header-absent branch and at a creation's claim; `TestReopenSnapshotReads`: an abandoned deletion followed by a reopen and a new grant, refused while the header is deleting and while `open` is above 0, an `interleaveKV` reopen between a deletion's read and its mark failing the mark; `TestSnapshotReadTransactions` (each transaction's compares and operations at its largest, ≤ 120). `flaminio-node`/`flaminio-io` tests: `source_without_session_acks`, `restarted_source_acks_at_first_poll`, `closing_drains_permits`, `reader_calls_one_outstanding` (a ChangeIsr waits behind at most one reader call, ValidateSnapshotRead calls included, R4-S1), `forgotten_node_closes_reader` (the poll's `node N is forgotten` closes every reader session and refuses new ones). |
| S2–S4 | The simulator (above), with its four checks and the new events: `sim/tests.rs` `reader_progress_after_abandoned_barrier` (a scripted run: every source of grant G registered and its sessions open before an intent closes G, G's source's node N down before its poll, the barrier abandoned, N then forgotten, and G reaching `closed` by the worker model's abort close after its session on N broke and its registrations were refused, R4-C1, R5-C1), `catches_reader_missing_close`, `catches_reader_premature_pin_release`, `catches_reader_source_outside_isr`, `catches_reader_skip_batch_revalidation`, `catches_reader_grant_under_closing`, `catches_reader_close_without_notify`; the vectors replayed by `meta/domain/domain_test.go`. |
| S5, S6 | `flaminio-backup/tests/repository_golden.rs` (envelope/manifest golden vectors; tampered tag/path/repo/kind/version; duplicate/out-of-bounds pages; exact-version substitution); fuzz targets `dataplane/fuzz/fuzz_targets/{backup_envelope,backup_page,backup_root,reader_frame}.rs` with `regressions/<target>/` inputs, bombs included. |
| S6, S7 | `flaminio-backup/tests/failure_matrix.rs`: a fault at each put/checkpoint/root/close step; `store_fault_resumes` (a store fault mid-run exits 1 with no close, the grant left active and the checkpoint kept, and a rerun with the same grant file completes, reusing what arrived, and closes `completed`; the same after a lost root reply, the rerun resolving the root by operation id, R6-C1); `close_retried_until_closed` (a closing answer, `Unavailable`, `Aborted` and `DeadlineExceeded` retried every 5 s until `closed`; the generation refusal, `NotFound`, the insecure refusal and a role refusal exit 1 at once naming the error; a `completed` close answered `aborted`, or `completed` with another root, exits 1 and `close --abort` answered `completed` exits 0, each printing the recorded outcome and root; `backup`'s abort close answered `closed` exits 1 naming the failure that ended the run; `verify --compare-source` closing its grant matched or not, exiting 0 only when matched and closed; an interrupted wait exits 1 naming the node, and a rerun with the grant file resumes at the close, R4-C2, R5-C2); `closing_grant_aborted` (a fake service refusing every registration of a closing grant, and of a closed one, with a `FailedPrecondition` whose message matches nothing: `backup` re-reads the volume, tries each ISR member once, closes `aborted` and retries until `closed`, its source's node forgotten after an abandoned barrier; the same for `verify --compare-source`; a `Refused` of reason `closed` and a session the node closed or lost each followed by a registration; a `Refused` of reason `retry` retried with the same token after its backoff, with no registration before the third in a row; a node answering `retry` to every Hello for its validation (its metadata client down), and one over a cap, each tried 3 times, then another ISR member registered, and on a closing grant that registration refused and the run closed `aborted`, R7-C1; a closing grant's `Refused` (`closed`) followed by a refused registration closing `aborted`; a connection ending before `Accepted` with no `Refused` retried toward the same node 3 times, then another ISR member registered; R4-C1, R5-C2, R5-C3, R5-B1, R6-C1, R6-C2); `early_root_mutation_fails`; old roots still read; resume reuses verified arrived versions only. |
| S8 | Deferred with S8 (R1-C6, R1-S1); here only the encrypted refusal of `TestAuthorizeSnapshotRead`. |
| S9, S10 | `flaminio-backup/tests/restore.rs` (bucket+key-only list/verify/restore, a corrupted or missing version named, output identity/symlink/mount/size/hole-zero, partial output); `tests/secrets.rs` (secrets absent from logs and argv; each secret-file refusal: symlink, not regular, other owner, group/other bits, key not 32 bytes; `repo init` refusing an existing file); `tests/client_hooks.rs` (every SDK attempt, retries and control calls, through the hooks; bounded queue and memory; a dead store does not stall the engine). |
| S11 | `flaminio-backup/tests/report.rs`: `verify_reports_restorable_point_age` (a fake clock and the store's LastModified), `list_marks_unverified`. Later qualified S3/lab cost/restoration evidence; no serverless test is described as Object Lock or physical-durability certification. |

Before the pull request: `make proto meta-vectors`, then `make lint test race hygiene contracts
sim`. Actual object store tests are opt-in, never implicit lab. No implementation, runtime tests
or measurements ran during drafting.

## Lab, docs, risks, ADRs and parallel work

Proposed lab store is a pinned/licence-checked SeaweedFS profile on a dedicated disposable
volume with qualified TLS/version/conditional behavior; keep test data <=1 GiB rather than fill
fl-w's 8 GiB disk. Restore altered objects/test data, stop only test store/workloads, delete test
files/credentials; never format shared disks. #53 lock tests require separate retention-aware
cleanup and consent.
Settled at approval, 2026-11-13: the pinned, licence-checked SeaweedFS profile (see decisions.md, Settled at approval).

**Commands (R1-C14, R1-S11).** `make backup-store-up` / `make backup-store-down` (`lab.sh
backup-store up|down`) and `make backup-check` (`lab.sh backup-check`), each in the Makefile's
help and `docs/lab.md`:
- `scripts/guest/backup-store.sh up|down|stop|start` runs on fl-w. `up` installs the SeaweedFS
  release pinned in `scripts/lab.conf` (version and SHA-256, checked before use), on a 2 GiB
  file-backed volume under `/var/lib/flaminio-backup-store` on fl-w's root disk (never the data
  disk), run as the store's system user `flaminio-backup-store`; points the store at its TLS
  server pair and `ca.crt` in `/etc/flaminio/backup-store-pki` (below); makes the worker's
  directory `/var/lib/flaminio-backup` (`flaminio-backup`, 0700) for its configuration, key file,
  checkpoints and grant files; refuses, naming `make pki-issue`, unless both pairs are there (as
  #16's `meta.sh up` stops without `meta.crt`); generates the S3 credential in the VM into files
  created exclusive and no-follow, mode 0600, the worker's copy owned by `flaminio-backup` and the
  store's by `flaminio-backup-store`; loop-mounts the volume as root and chowns its root to
  `flaminio-backup-store`, 0700, before starting the store, so the store reads its credential and
  writes its data as its own user (R4-C3); creates the versioned bucket. `down` stops the store
  and deletes the volume file and the credentials.
- **The store's identity (R2-C5, R2-S3).** `lab.sh backup-store up` first issues the pair from
  the host, as `cmd_pki` issues every name: `issue "$VM_W" backup-store "$dir/flaminio"
  "spiffe://$TRUST_DOMAIN/flaminio/node/$VM_W/backup-store" --server --ip "$ip"` (`dir` and `ip`
  as `cmd_pki` sets them; the key made in the VM by `pki.sh csr`, signed by the workstation's
  CA; server use only, never `--client` and never an `admin` id), then `put_file` of `ca.crt`.
  The worker's `store_identity` is that URI (contracts, Worker inputs).
- **Its renewal (R4-C4).** #16's `identities VM` gains the fl-w line `backup-store CA ID --server
  --ip` with that ID, added by whichever of #16 and #52 lands second (#16 is clear and unchanged
  here). `pki issue` skips it, as it skips `csi-controller`: `backup-store up` issues it. #16's
  renewal then renews it like any other name (a pair not yet issued is `absent`, never `due`).
  Whether the pinned SeaweedFS takes a renewed pair without a restart is part of qualifying its
  profile; if it does not, the renewal, after renewing `backup-store`, restarts the store
  (`backup-store.sh stop`, then `start`) on fl-w, beside its CSI Secret step, added the same way.
- **`pki.sh`'s names (R3-B1).** #16's `pki.sh` refuses a name not in its list, so the list gains,
  added by whichever of #16 and #52 lands second (#52 alone, on today's `pki.sh`, adds the same
  two names, their directories and users): `backup`: `/etc/flaminio/backup-pki`,
  `flaminio-backup` 0700, a first key `flaminio-backup` 0600; `backup-store`:
  `/etc/flaminio/backup-store-pki`, `flaminio-backup-store` 0700, a first key
  `flaminio-backup-store` 0600; `file ca.crt` also to both directories, but only to one that
  exists (R4-B1): `file` never builds these two, so only `csr backup` and `csr backup-store`,
  which run on fl-w alone, make them and their users, and no other VM gains either. `pki issue`
  puts `ca.crt` on fl-w again after its `backup` line, and `backup-store up` after issuing.
  Building either directory first makes its user, idempotently, as #16 makes its group with
  `groupadd -f --system`: `id -u USER >/dev/null 2>&1 || useradd --system --user-group
  --no-create-home --shell /usr/sbin/nologin USER` (its group of the same name explicit, for the
  `install -g` below), before the directory's `chown`. #16's sweep of old temporary
  directories (`install`'s step 6) gains `backup-pki.[0-9a-z]*.new` and
  `backup-store-pki.[0-9a-z]*.new`. #16 is clear and unchanged here.
- The worker's pair: `pki issue w` with #16's `backup` line, in `/etc/flaminio/backup-pki`
  with its `ca.crt` (contracts, Identity lifecycle; R2-S4, R3-B1). The worker, `flaminio-backup
  repo init` and the repository key file run as and belong to `flaminio-backup`, never root. No
  secret is ever on an ssh command line or leaves the VM.
- **The reader listener (R2-C4).** `lab.conf` gains `READER_PORT` (7413); `lab.sh`'s
  `node_env` passes `READER_LISTEN=$(vmvar "$vm" WG):$READER_PORT`, and `node.sh up` adds
  `--reader-listen "$READER_LISTEN"` when it is set, with a `node-scripts-test.sh` case. Under
  Kubernetes, #12's DaemonSet command gains `--reader-listen "$HOST_IP:7413"` and its ports
  `{containerPort: 7413, hostPort: 7413}`, added by whichever of #12 and #52 lands second (#12
  is clear and unchanged here).
- `scripts/checks/backup.sh` runs from the host, as `resync.sh`. It validates every volume,
  snapshot, operation and bucket id before any ssh (ids `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`, the
  bucket S3's lowercase grammar) and passes them quoted (`printf %q`). Its steps: (1) refuse
  unless the store is up, and create its own volume `bk-<UTC stamp>` (refusing one that exists),
  RF 2, attached on fl-a, under `scripts/guest/write-load.sh` writing a known marker; (2)
  snapshot, authorize, full backup: `metactl` runs `flaminio-meta` under `sudo`, so the grant
  file it writes on fl-w (a fresh root path, exclusive, 0600) is handed to the worker with `sudo
  install -o flaminio-backup -g flaminio-backup -m 0600 -- SRC DST` into
  `/var/lib/flaminio-backup`, refusing a DST that exists, then root's copy removed (R3-B1);
  (3) a second snapshot and backup, incremental, uploading no
  more blocks than the diff touched (counted from the JSON); (4) a backup with the store stopped
  mid-run and started again: the rerun completes, reusing what arrived, and no partial archive is
  listed meanwhile; (5) a failover of the target mid-backup (fl-a's node stopped, as `lab.sh
  node-fault` does for fl-b), then `verify --compare-source` under a grant of its own, which it
  closes (R5-C2); (6) on a separate archive, one
  referenced block version's bytes altered at the store's backend by its administrator path (never
  the worker's credential; if the pinned profile has none, that version deleted, and the check says
  which): `verify` exits 1 naming the object; (7) a restore to a new file on fl-w, of an archive
  `verify --compare-source` passed; fl-w runs no data node, so that file (data only, no secret) is
  then streamed through the host onto a new volume `bk-<stamp>-r` of the same size, attached on
  fl-a, and the device's SHA-256 equals the restored file's (R2-C5); (8) the marker absent from the
  store's data on fl-w; (9) by the performance plan's rules, alternating rounds of the load alone
  and with a backup: the backup's MiB/s and the load's IOPS and latency each way; (10) cleanup: the
  load stopped, grants closed, the restored file and only its own volumes deleted. The
  encrypted-volume check is deferred with S8 (R1-C6, R1-S1).
- `scripts/tools/backup-scripts-test.sh` (the id checks, quoting, refusals) joins `make lint`'s
  script tests, with fake `sudo`, `id` and `useradd` (R3-B1): `pki.sh csr backup` and `csr
  backup-store` accepted, each directory's owner and mode, each user made once and not again
  when `id` finds it, `file ca.crt` in both directories, and `file ca.crt` with neither present
  making no user and no directory (R4-B1); `backup-store.sh up` refusing without either pair,
  the store's credential owned by `flaminio-backup-store` 0600 and the worker's by
  `flaminio-backup`, the mounted volume's root `flaminio-backup-store` 0700 (R4-C3); `backup.sh`'s
  grant file installed owned by `flaminio-backup`, 0600, an existing DST refused; and, with
  `identities` and `cmd_pki` extracted as #16's script test extracts them, `backup-store` listed
  for fl-w only and not issued by `pki issue` (R4-C4), and, with `run_on_vm` and `ca_tool` faked,
  a `pki.sh file ca.crt` on fl-w after its `pki.sh csr backup` and `install backup`, and none on
  another VM after that VM's own lines (R5-C4).

Docs: repository v1/reader protocol, commands/rate limits/store prerequisites,
docs/lab/followups/README/Makefile and measured benchmarks. `docs/followups.md` gains "Audit
events for snapshot-read grants" (R1-C11, R1-S4; issue to be filed) and keeps encrypted backups
open with S8. Say manual archive increment only; schedules/retention/PVC/encrypted backups/WAN
remain open.

[Proposed ADR](adr-proposal.md) leaves worker/deps/format/auth/store licence/profile/revocation
availability tradeoff open. Risks named, not accepted: unreachable sources that are not forgotten
block deletion (`FailedPrecondition` until the node returns or is forgotten); storage failure may
prevent complete archives; a volume's 64 grant records and 4 open grants bound backups at once
(round 1, R1-S10); a closing volume's backups end at once (their sessions close at the
barrier, their registrations are refused, and the worker closes each grant `aborted`, R4-C1,
R5-C3); a `backup` that fails otherwise leaves its grant active, pinning its snapshot and counted
among the volume's 4 open grants, until a rerun completes or aborts it or `snapshot-read close
--abort` closes it, and a grant a deletion's intent made closing meanwhile records no outcome
until then (R6-C1); and a deletion that stops after its
intent leaves the volume's backups refused until it is re-run to completion or an administrator
reopens the volume (R2-C2, R2-S5); a flood of connections not yet authenticated can still delay
a worker's handshake on the reader port, a liveness loss only (R2-S2); a source that is down
and not forgotten keeps the worker's `backup` waiting on its close (R3-C2); under many
sessions a ReaderHello can be refused, to be retried with the same token after a backoff, by
the node-wide validation bucket or the one outstanding reader call, a liveness loss only (R4-S1,
R5-B1). Accepted ADR0001
fits Rust worker/Go future controllers; no accepted fencing/ISR rule is relaxed. Audit0008/9/11
remain Proposed; wrong-base recovery or key erasure cannot be justified by a backup count.
Parallelize repository worker and Go/Rust grant/protocol only after contracts settle; security,
source completeness, simulator/mTLS and exact-version restore integration must join before release.
Settled at approval, 2026-11-13: worker, dependencies, format, authority, store profile and revocation as proposed; the ADR is accepted with the plan (see decisions.md, Settled at approval).
