# Issue #52 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`c78bb582d0b193f66cf9d3def25819dcc38a6ed3bb965074c9da73afdc8f9b33`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.
- Plan review round 1 (2026-11-18) fixes, made after approval: see below.
- Plan review round 2 (2026-11-18) fixes, made after approval: see below.
- Plan review round 3 (2026-11-18) fixes, made after approval: see below.
- Plan review round 4 (2026-11-18) fixes, made after approval: see below.
- Plan review round 5 (2026-11-18) fixes, made after approval: see below.
- Plan review round 6 (2026-11-18) fixes, made after approval: see below.
- Plan review round 7 (2026-11-18) fixes, made after approval: see below.
- Plan review round 8 (2026-11-18): clear; one wording Nit fixed after approval, see below.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **The worker, client and libraries:** as proposed, a Rust worker and library,
  `flaminio-backup`, in `dataplane/`, with a pinned `aws-sdk-s3` adapter and blake3, zstd,
  XChaCha20-Poly1305, HKDF-SHA256 and zeroize; later Go controllers call it over gRPC. Exact
  versions are chosen after licence and MSRV checks, before implementation. [spec.md](spec.md),
  Open questions; [design.md](design.md), Worker, grant and source proof;
  [adr-proposal.md](adr-proposal.md), Decisions proposed.
- **The repository's byte format:** as proposed, a versioned repository of 4 MiB
  content-addressed encrypted blocks with a complete, self-contained encrypted manifest per
  archive (no delta chains), completed by exact object versions, as
  [contracts.md](contracts.md) fixes. [spec.md](spec.md), Open questions;
  [design.md](design.md), Repository, crypto and publication;
  [adr-proposal.md](adr-proposal.md), Decisions proposed and Options.
- **The reader's authority:** as proposed, a distinct backup identity and a dedicated
  snapshot-reader/v1 protocol serving only a pinned committed snapshot under registered-source
  grants; the target's Hello and token are not reused. [design.md](design.md), Worker, grant and
  source proof; [adr-proposal.md](adr-proposal.md), Decisions proposed and Options.
- **Reader revocation against deletion's availability:** the proposed direction, an explicit
  registered-source closure barrier with no TTL: deletion waits, with a named error, while a
  registered source is unreachable. [spec.md](spec.md), Open questions; [design.md](design.md),
  Worker, grant and source proof; [adr-proposal.md](adr-proposal.md), Options.
- **The object store's TLS, profile and licence:** as proposed, a versioned, conditional,
  durable S3 profile that keeps mutual TLS 1.3 plus SigV4 signed authentication (a provider
  without it needs an exception ADR); the lab store is a pinned, licence-checked SeaweedFS
  (Apache-2.0) on a dedicated disposable volume, qualified in the lab. [spec.md](spec.md), Open
  questions; [design.md](design.md), Lab, docs, risks, ADRs and parallel work;
  [adr-proposal.md](adr-proposal.md), Decisions proposed and Consequences and open choices.
- **The seams with #50's keys and references:** as proposed, pins keep the snapshot's layers
  and required key references until completion or a durable abort, an archive's key references
  outlive the live volume, and the worker reads stored ciphertext, never a key; this increment
  claims no plaintext recovery of encrypted volumes. [spec.md](spec.md), Open questions;
  [adr-proposal.md](adr-proposal.md), Decisions proposed.
- **#51's snapshot and diff contract:** the semantic interface is settled as
  [contracts.md](contracts.md) proposes (`SnapshotRefV1`, `Diff`, `SourceProofV1`, with no
  physical root ABI invented here). The durable completeness and store-lifetime evidence that #51
  and #61 must agree before implementation: no recommendation in the plan. Settled 2026-11-13
  (user's instruction): see below.
  [spec.md](spec.md), Open questions; [contracts.md](contracts.md), Snapshot/root/diff interface.
- **False positives in the diff:** as proposed, a diff may name unchanged blocks, counted as extra
  read work; this explicitly corrects the intent's identical-answer wording.
  [design.md](design.md), Diff, resource limits and incremental map;
  [adr-proposal.md](adr-proposal.md), Consequences and open choices.

## Settled at the user's instruction of 2026-11-13

- **The durable completeness and store-lifetime evidence #51 and #61 must agree:** #52 uses
  #51's completeness evidence, a source replica holding the committed cut in full under #51's
  catch-up rule (each stage durably acknowledged, a join only once every committed cut is
  verified), and #61's store lifetime, the store UUID bound to each replica and to the node
  record. `SourceProofV1` carries both; #52's source proofs are built only once #51 and #61
  implement them, and a source without either is refused, never assumed. Recommended by Claude,
  no recommendation in the plan: it is the plan's own fail-closed rule (contracts.md: missing
  disk identity or completeness evidence refuses; no fake root), and it invents no evidence that
  #51 and #61 do not define.

## Plan review round 1 (2026-11-18), changes made after approval

Reviewed commit c6efcd7; findings R1-C1 to R1-C16 (code) and R1-S1 to R1-S12 (security), and one
Nit each. Each change below was made after approval, in this round; nothing in it is
implemented, measured or approved anew.

- **Encrypted volumes: deferred, not settled** (R1-C6, R1-S1, one fix, the orchestrator's
  decision): #50 and #51 (clear) admit no snapshot of an encrypted volume until its keys are
  pinned, so none can be backed up now. AuthorizeSnapshotRead refuses one (`FailedPrecondition`),
  by #50's field 20 and by its header mark, as #51's RequestSnapshot; whichever of #50 and #52
  lands second adds the refusal and its two tests. Intent outcome 7, S8, the retained-key
  records, their tests and the encrypted lab check move to the plan that lifts #51's refusal; the
  intent's text is unchanged (a note marks the deferral). **A rule narrowed, shown necessary by
  R1-C6 and R1-S1:** S8, and S2's pin of key references, cannot hold while no encrypted snapshot
  can exist; they are deferred, not removed. This supersedes the Settled entry "The seams with
  #50's keys and references" for this increment. [spec.md](spec.md), S2, S8;
  [design.md](design.md), Repository, crypto and publication; [contracts.md](contracts.md).
- **Roles, the worker's grant and its privileges** (R1-C1, R1-S2, R1-S3, R1-C16, R1-S8, one
  fix): contracts gain #73-style rows for the six reader methods (AuthorizeSnapshotRead and an
  administrator's CloseSnapshotRead: administrator name, once active `cluster-admin`, as #51's
  RequestSnapshot; the node methods: the node's `io` for itself; RegisterSnapshotSource and the
  worker's CloseSnapshotRead: a new worker rule with no store read, the handler checking the
  grant's worker). Insecure instances refuse all six. Whichever of #52 and #73 lands second adds
  them. An administrator authorizes with `flaminio-meta snapshot-read authorize`, which writes a
  grant file; the worker takes it, never holds an administrator pair and refuses to start with a
  pair that is not a `backup` identity. The worker keeps the open reads (GetVolume for its
  revalidation); the plan no longer says it has no reader privileges. #73's OverrideDelete runs
  the barrier, as DeleteVolume does.
- **The barrier's two ends** (R1-C2, R1-C10, R1-S5, one fix): closing reaches a source by a new
  own-node `ListClosingSnapshotReads` poll, at start and every 5 s, off the engine thread (the
  reviewer's recommended RPC, rather than a heartbeat field: the heartbeat is the agent's, the
  sessions the data plane's). The one exclusion proof is #13's `forgotten` mark (S19), compared
  in the exclusion's transaction; every reader method refuses a forgotten node. A closing volume
  refuses new grants and sources, and the mark compares the reader record. Simulator mutations
  `grant_under_closing` and `close_without_notify` join the catalogue. S3's "separately
  established exclusion proof" is now defined as that mark; no other proof exists in the code or
  a clear plan.
- **Simulator and vectors** (R1-C3): a reader model in `flaminio-repl` with an archive-equality
  check and a closure check; six mutations (`missing_close`, `premature_pin_release`,
  `source_outside_isr`, `skip_batch_revalidation`, `grant_under_closing`,
  `close_without_notify`); grant-lifecycle transitions in `meta/testdata/isr-vectors.jsonl`,
  generated from the Rust model and replayed against `meta/domain/snapread.go`.
- **Records and transactions** (R1-C4): keys under `/flaminio/v1/` (`cluster`,
  `snapread/<v>/<fe>/{reader,grants/<op>}`, `snapsrc/<node>/...`), each call's compares and
  writes, all within 120; a per-grant `pending` counter so closing never compares every source;
  no grant or source write touches the header; registration compares the header, the chunk
  record, the catalog record and #61's node record, which establishes #51's completeness evidence
  as #51's contracts define it (superseding the catch-up wording of the user-instruction entry
  above); acks and exclusions delete source records, closed grants are collected at 64, and the
  volume's records go after its header.
- **Deletion under #73** (R1-C5): the barrier runs after #73's admission checks, so a refused
  deletion writes nothing; the intent compares the header, the owner record and, under
  `inactiveRules`, the authorization record; the mark gains one compare (≤ 6); a header that does
  not parse is covered by listing `snapread/<v>/`; the blocked deletion answers
  `FailedPrecondition` naming the waiting node.
- **Reader port** (R1-C7): `flaminio-io node --reader-listen ADDR`, 7413 in the lab (7412 is
  #71's observer); the worker dials the host of `Node.address` at its configured reader port, so
  no node-record field is added.
- **Reads without an epoch** (R1-C8): `Store::plan_view_read` skips the session epoch check and
  refuses a chunk fenced below the first epoch (#30 S5, as #51's `snapshot-read`), with a test
  reading across a failover.
- **SnapshotRefV1 and SourceProofV1 fields** (R1-C9): the cluster UUID record is defined here
  (written once, conditionally, by the first AuthorizeSnapshotRead; #54 reads it); `size_bytes`
  is the header's at the grant, fixed for its life, a base of smaller size staying compatible
  (growth); `logical_block_bytes` is 4096; `device_uuid` and `store_generation` are #61's fields,
  replacing `store_uuid` and `replica_generation`.
- **Audit** (R1-C11, R1-S4, one fix): no event in this step; `docs/followups.md` gains "Audit
  events for snapshot-read grants", issue to be filed (cut before adding, as #51 did); the
  intent's out-of-scope line now names #73's audit log.
- **Shared byte contracts pending** (R1-C12): `SnapshotRefV1`, `Diff`, `SourceProofV1` and the
  snapshot-reader/v1 bytes are pending #58's coordinated review with #51 and #52, nothing built
  before it (deferred, not settled), as #51 marks its side; the repository format stays settled.
  This supersedes "Settled at approval" on #51's snapshot and diff contract for those bytes.
- **Flow steps** (R1-C13): the administrator's commands; the grant file as the worker's input
  (the reviewer's second option, which needs no new RPC); completion and abort as
  CloseSnapshotRead's `outcome` and `root`; the design's "conditional metadata operation
  progress" is cut (local checkpoints only), and the retained-key reference is deferred with S8.
- **The lab** (R1-C14, R1-S11, one fix): `make backup-store-up|down` and `make backup-check`
  through `lab.sh`, `scripts/guest/backup-store.sh`, `scripts/checks/backup.sh` with ordered steps
  per outcome 12 check, secrets made in the VM and never on an ssh command line, ids validated and
  quoted, a script test under `make lint`.
- **Tests** (R1-C15): named per rule, with fuzz targets and the restorable-point age test.
- **TLS checks** (R1-S6, R1-S9): the reader client accepts only the source node's `io`; the node
  takes the worker's identity from its certificate, never a frame.
- **Identity lifecycle** (R1-S7): issued by `pki issue w`, renewed under #16 S10 (whichever of #16
  and #52 lands second adds the `backup` line), picked up at each new connection, no TLS
  resumption on the reader port.
- **Resource bounds** (R1-S10): 16 reader connections a node and 2 per grant, 4 open grants and 64
  grant records a volume, chunks × (RF + 2) source records a grant, registration idempotent, the
  certificate and token checked before any metadata call.
- **Secret files** (R1-S12): `repo init` generates the key and creates its file exclusive,
  no-follow, 0600, fsynced; every secret file is checked (regular, owner, no group or other bits,
  the key exactly 32 bytes).
- **Nits:** the reader frames are `[u32le len]`, as the replication frames; the AEAD's object key
  is relative to the repository prefix. None left.
- **Risks named, not accepted:** a source node that is down and not forgotten blocks the deletion
  of its volume (as settled, now with a named error); a volume holds at most 4 open grants and 64
  grant records; a deletion's closing intent is not undone if that deletion then fails for
  another reason, so its backups must be authorized again. (Corrected in round 2, R2-C2, R2-S5:
  they could not be; see below.)

## Plan review round 2 (2026-11-18), changes made after approval

Reviewed commit ca86b81; findings R2-C1 to R2-C9 (code) and R2-S1 to R2-S5 (security), and two
Nits each. Each change below was made after approval, in this round; nothing in it is
implemented, measured or approved anew.

- **A stopped deletion's closing intent** (R2-C2, R2-S5, one fix): nothing cleared `closing`, so
  a volume whose deletion stopped after its intent could never be backed up again. A new
  administrator RPC, `ReopenSnapshotReads` (`flaminio-meta snapshot-read reopen VOLUME`), clears
  it, refused while the header is deleting or does not parse and while `open` is above 0; its
  transaction compares the header and the reader record, so a deletion's mark that read the
  record before it fails and the barrier runs again. Chosen over Authorize clearing `closing`
  (the reviewer's other option): that would let backups keep postponing a deletion still being
  retried, against S3's "no new grant once closing begins"; over making `closing` final (R2-S5's
  first option): the volume could then not be backed up again. Its row is #73's
  existing-methods rule (administrator name, once active `cluster-admin`); insecure instances
  refuse all seven methods. **A rule changed, shown necessary by R2-C2 and R2-S5:** S3's "Once
  closing begins, no new grant or source is admitted" gains "unless an administrator reopens it
  after its deletion stopped (no grant open, the volume not marked deleting)": as stated, it
  bars every later backup of a volume whose deletion stopped. Its safety (no read after the
  barrier, no mark with a grant open) is unchanged. The named risk is corrected.
- **Token, grant and operation binding** (R2-S1): a grant's `generation` is the reader record's
  `next`, raised at each claim, so no grant reference is used twice within an incarnation, even
  for an operation id reused after its record is collected (the reviewer's first option: it
  keeps the CLI's idempotent retry, which a service-drawn id would cut). The source record keeps
  the issued token's SHA-256; ValidateSnapshotRead takes the token's worker, proof and digest and
  compares them, with the generation, to the grant and the record; a re-registration replaces
  the digest. The same operation id with another field is `AlreadyExists`.
- **The simulator** (R2-C1, R2-C9, one fix of the model): the metadata side of the grant
  lifecycle is kept once, in `flaminio-repl/src/meta.rs`, driven by the simulator and emitted by
  the vectors; `src/reader.rs` holds only the node and worker sides. New chaos events: delete the
  volume (the barrier), re-create it at a new first epoch, stop a deletion and reopen, forget a
  node (#13's `forgotten` and `forget()`, whichever of #13 and #52 lands second adds them as #13
  states). Two new checks: **exclusion** (no grant or source created after its volume's closing
  intent, unless reopened), which catches `grant_under_closing`, and **reader progress** (in
  settle, every closing grant closes and every requested deletion not stopped completes), which
  catches `close_without_notify`. Each of the six mutations names the check that catches it.
- **Reader calls on the metadata client** (R2-C3): the poll's pages and its acks share #13's
  sweep rule, one reader call outstanding and none queued behind it (the reviewer's first
  option, which adds no batch format), so a ChangeIsr waits behind at most one; a test.
- **The reader listener in the lab** (R2-C4): `READER_PORT` in `lab.conf`, `READER_LISTEN` in
  `node_env` and `node.sh`, a `node-scripts-test.sh` case. #12 (clear) is not changed: whichever
  of #12 and #52 lands second adds `--reader-listen "$HOST_IP:7413"` and `hostPort` 7413 to its
  DaemonSet.
- **The lab store's identity** (R2-C5, R2-S3, one fix): `lab.sh backup-store up` issues
  `spiffe://<td>/flaminio/node/fl-w/backup-store` from the host, server use only, through
  `issue`; the worker requires a configured `store_identity` (an exact URI SAN, or a DNS name
  under its CA) and refuses any other certificate; a test.
- **The lab restore** (R2-C5): to a file on fl-w, then streamed, data only, onto the new volume
  on fl-a and compared by SHA-256.
- **Collection** (R2-C6): every path deletes `snapread/<v>/` (no first epoch on the header-absent
  branch or at a creation's claim), as #51 deletes `snapshots/<volume>/`; the grant collected at
  `held` = 64 is the lowest-generation closed one, since `kv.Entry` has no create revision.
- **ListClosingSnapshotReads' reads** (R2-C7): one List of the node's prefix (its cost stated),
  then one Get per distinct grant on the page (≤ 1024); a paging test with mixed states.
- **A forgotten node still running** (R2-C8): the poll's `node N is forgotten` makes it close
  every reader session and refuse new ones; a `flaminio-node` test.
- **Connections before authentication** (R2-S2): the caps count authenticated sessions only;
  liveness bounds (32 pending, the oldest closed past it; 5 s for the handshake and Hello; 60 s
  idle; 2 validations per (grant, node) in 10 s), proposed and to be measured; tests.
- **The worker on fl-w** (R2-S4): it runs as `flaminio-backup`, its pair in
  `/etc/flaminio/backup-pki`, away from `admin.key` and `token.key`; `pki.sh` gains that
  directory, and #16's list the same entry, by whichever of #16 and #52 lands second (the
  reviewer's isolation option, not the statement of no isolation).
- **Nits:** spec S2 says the node takes the worker's identity from the worker's certificate; the
  lab's failover step says "as `node-fault` does for fl-b"; AckSnapshotReadClosed refuses an
  active grant; the worker URI must be a `backup` identity of the instance's trust domain in
  `pki.ParseID(..).String()` form. None left.
- **Risks named, not accepted:** a deletion that stops after its intent leaves the volume's
  backups refused until it is re-run to completion or an administrator reopens the volume; a
  flood of connections not yet authenticated can still delay a worker's handshake (liveness
  only).

## Plan review round 3 (2026-11-18), changes made after approval

Reviewed commit b2f2b77; findings R3-C1 (Medium), R3-C2, R3-B1, R3-S1, R3-S2 and R3-S3 (Low), and
one Nit, from one reviewer on the design-code and design-security focuses together. Each change
below was made after approval, in this round; nothing in it is implemented, measured or approved
anew.

- **The simulator's deletion** (R3-C1, and the Nit): the simulator never writes a deletion's mark
  and deletes no volume. Its deletion event runs the barrier up to the mark (the intent, the
  exclusion, `FailedPrecondition` while `open` is above 0, retried every 5 s), one model step
  finding `open` = 0 where the mark would commit; one barrier in four is abandoned after its
  intent; either way a ReopenSnapshotReads follows, retried every 5 s while `open` is above 0
  (the Nit). The re-create event is cut, so the run keeps one incarnation, the end-state checks
  (final reads, divergence, full ISR) run unchanged, the history is not cut, and #13's sweep keeps
  `deleted_since` false as #13 states. Chosen over an incarnation model in the sim (the
  reviewer's other option, more than the reader model needs: what a deletion does to the target
  and the replicas is today's DeleteVolume's and #13's, and the mark's compare and the records'
  deletion are `TestDeleteVolumeReaderBarrier`'s). Closure checks a pin's release at a grant's
  close and at a barrier's mark; reader progress expects every closing grant closed, every
  barrier not abandoned at its mark and every reopen succeeded in settle. Each of the six
  mutations keeps its check: none of them needed the re-creation.
- **Reader progress on clean seeds** (R3-C2): the worker, and its model, retry CloseSnapshotRead
  every 5 s (doubling to 60 s in the CLI) until the grant is `closed`, each retry excluding
  sources forgotten since; `NotFound` or a generation refusal ends it, since only a closed grant
  is collected; an interrupted wait exits 1 naming the node and a rerun resumes at the close. The
  reviewer's first option, chosen over a new exclusion path in AuthorizeSnapshotRead: it adds no
  call to a method, only a retry. The worker model authorizes every 5 s while it holds fewer than
  4 open grants, during a barrier and after a reopen too, and registers sources of a closing grant,
  so both halves of `grant_under_closing` can fire. Tests: TestSnapshotReadClosure (a grant closed
  before its source's node is forgotten reaches `closed`), `close_retried_until_closed`.
- **The lab's backup users and files** (R3-B1): #16's `pki.sh` list gains `backup`
  (`/etc/flaminio/backup-pki`, `flaminio-backup` 0700, a first key 0600) and `backup-store`
  (`/etc/flaminio/backup-store-pki`, `flaminio-backup-store` 0700, a first key 0600), `file
  ca.crt` into both, each user made idempotently (`id -u || useradd --system`) before its
  directory's `chown`, and both temporary-directory names in #16's sweep: added by whichever of
  #16 and #52 lands second (#16 is clear and unchanged here). `backup-store.sh up` runs the store
  as `flaminio-backup-store`, makes `/var/lib/flaminio-backup` (`flaminio-backup`, 0700) and
  refuses without both pairs, naming `make pki-issue`. `backup.sh` hands the root-written grant
  file to the worker with `sudo install -o flaminio-backup -g flaminio-backup -m 0600`, refusing
  an existing destination. `backup-scripts-test.sh` cases.
- **Registration order** (R3-S1): the service signs the token, commits the source record with its
  digest in one transaction, and returns the token only after the commit; a lost compare signs
  again, and an uncommitted token is never returned. The contradicting "BEFORE the token exists"
  is corrected. A TestRegisterSnapshotSource case.
- **Generation on acks, closes and exclusions** (R3-S2): AckSnapshotReadClosed,
  CloseSnapshotRead and the exclusion they run refuse (`FailedPrecondition`, nothing written) a
  grant reference whose generation is not the grant record's; the reviewer's first option, which
  keeps the source keys as they are. Close joins them because the worker now retries it (R3-C2).
  A TestSnapshotReadClosure case: an earlier generation's ack and close under a reused operation
  id.
- **The validation rate** (R3-S3): keyed per (grant, node, chunk), still 2 in 10 s, so a backup,
  which validates each source once per session, is never refused, and a cycling worker costs at
  most 2 validations per registered source in 10 s; the values stay proposed, to be measured.
  `validation_rate_bounded` asserts a worker walking N chunks is not refused.
- **No rule narrowed.** S1 to S11 are unchanged; the simulator's coverage of the deletion stops
  at the mark, which the Go tests check.
- **Risks named, not accepted:** a source that is down and not forgotten keeps the worker's
  `backup` waiting on its close, as it already blocks the volume's deletion.
- **Nits:** the reopen retried until the grants close (above). None left.

## Plan review round 4 (2026-11-18), changes made after approval

Reviewed commit a25d9f9; findings R4-C1, R4-C2, R4-B1, R4-C3, R4-C4 and R4-S1 (Low), and one Nit,
from one reviewer on the design-code and design-security focuses together. Each change below
was made after approval, in this round; nothing in it is implemented, measured or approved
anew.

- **A grant a deletion's intent closed** (R4-C1): RegisterSnapshotSource refuses a grant not
  active with the fixed message `grant <op> is closing` (or `is closed`); `backup`, refused with
  it, ends with its abort close, CloseSnapshotRead(aborted), retried until `closed`, and a reader
  session the node refuses or closes is followed by a registration for that chunk before any
  retry of its reads, so a closing grant always reaches the worker that way. The worker model
  does the same, so a grant whose barrier was abandoned and whose source's node is forgotten
  later is excluded and closes. The reviewer's fix; tests `reader_progress_after_abandoned_barrier`
  (sim) and `closing_grant_aborted` (`failure_matrix.rs`).
- **A repeated close and the end of its retry** (R4-C2): a repeated close keeps the first
  recorded outcome and root and answers the state with them, never an error; a grant an intent
  made closing takes its first close's outcome. The response carries the recorded outcome and
  root. The CLI retries only `closing`, `Unavailable`, `Aborted` and `DeadlineExceeded`; any other
  error exits 1 naming it, so no retry loops forever. The generation refusal of the close and the
  ack gets the fixed message `grant <op> generation <g> is not current`, which the CLI matches;
  `NotFound` (collected) now exits 1 too, since the outcome is no longer recorded. `close --abort`
  answered `completed` exits 0; `backup`'s `completed` answered `aborted` exits 1. The reviewer's
  fix; cases in `close_retried_until_closed` and `TestSnapshotReadClosure`.
- **The backup directories on fl-w only** (R4-B1): `pki.sh file ca.crt` writes into
  `backup-pki` and `backup-store-pki` only where they exist and never builds them, so only `csr
  backup` and `csr backup-store`, run on fl-w alone, make them and their users; `pki issue` puts
  `ca.crt` on fl-w again after its `backup` line, and `backup-store up` after issuing. The
  reviewer's first option, chosen over limiting the put to fl-w in `lab.sh`: `file ca.crt` writes
  every directory of its list at once, so that would need a second file name. Either way the
  directories and users exist on fl-w only. A `backup-scripts-test.sh` case.
- **The store's own files** (R4-C3): `backup-store.sh up` creates the store's credential copy
  owned by `flaminio-backup-store`, 0600, and chowns the mounted volume's root to it, 0700, before
  starting the store. The reviewer's fix; `backup-scripts-test.sh` cases.
- **The store's renewal** (R4-C4): #16's `identities VM` gains the fl-w line `backup-store CA ID
  --server --ip`, which `pki issue` skips as it skips `csi-controller`, so #16's renewal renews
  the store's pair; added by whichever of #16 and #52 lands second (#16 is clear and unchanged
  here). Whether the pinned SeaweedFS takes a renewed pair without a restart is qualified with its
  profile; if not, the renewal restarts the store on fl-w after renewing it, added the same way.
  The reviewer's first option, rather than leaving the store outside renewal.
- **A total bound on validations** (R4-S1): the per-(grant, node, chunk) key stays, and a
  node-wide token bucket (a burst of 16, the session cap, refilled at 4 a second; proposed, to be
  measured) bounds the node's total; ValidateSnapshotRead joins the poll's one-outstanding reader
  call rule, a Hello that would need a second one `Refused` (retry later), so a ChangeIsr waits
  behind at most one validation. A node-wide bucket rather than the reviewer's per-(grant, node)
  one: a per-grant bucket bounds a grant, and a node's total would still grow with its grants,
  while the finding asks for a total bound; one bucket is also the simpler.
  `validation_rate_bounded` and `reader_calls_one_outstanding` cases.
- **Nit:** `useradd --user-group`, so the `install -g flaminio-backup` of the grant file does not
  rest on the distribution's `USERGROUPS_ENAB`. None left.
- **No rule narrowed.** S1 to S11 are unchanged.
- **Risks named, not accepted:** under many sessions a ReaderHello can be refused, to be retried,
  by the node-wide validation bucket or the one outstanding reader call (liveness only).

## Plan review round 5 (2026-11-18), changes made after approval

Reviewed commit f4a3f1c; findings R5-C1, R5-C2, R5-B1, R5-C3, R5-S1 and R5-C4 (Low), and one Nit,
from one reviewer on the design-code and design-security focuses together. Each change below
was made after approval, in this round; nothing in it is implemented, measured or approved
anew.

- **Every run ends with its close, with no message matched** (R5-C3, closing R5-C1 and R5-C2's
  `is closed` and `verify` cases with it): every failure that ends `backup` after it read its
  grant file and before its root is published ends with its abort close, retried until
  `closed`; after publication only its `completed` close is left. The worker matches no Go error
  message: a registration failing with `Unavailable`, `Aborted` or `DeadlineExceeded` is
  retried, any other refusal makes it re-read the volume and try each ISR member of that chunk
  once, and a chunk none of whose members registers while its `isr_ver` stays the same ends the
  run, so a grant closing or closed (which refuses every registration) ends it whatever the
  message says. `grant <op> is closing` stays a message for people. The reviewer's first
  option, chosen over a structured `ErrorInfo` reason with shared vectors: it adds no field and
  no contract between the two sides, and covers every terminal failure, not only the closing
  one. The bound on reselection (the unchanged `isr_ver`) is what makes the refusal terminal; it
  keeps S4's reselection without restarting, since a stale `isr_ver` changes between rounds.
  **Narrowed in round 6 (R6-C1):** "every failure ends with its abort close" broke S7's resume
  and lab step 4, since an aborted grant is closed, refuses the rerun's registrations, and a new
  operation id fails the checkpoint's binding. Only a failure that leaves the grant unusable
  (the registration end rule) now aborts; the rest exit 1 with the grant active (round 6,
  below). The no-message-matched part stands.
- **The worker model's sessions** (R5-C1): as the CLI's, a session the node refuses as `closed`,
  closes at the barrier, or loses with its node is followed by a registration for that chunk;
  the model closes `completed` a grant it has assembled, and ends a grant (abort close, retried)
  by the same unchanged-`isr_ver` rule, whatever the refusal. So a grant whose sources are all
  registered still learns it is closing. `reader_progress_after_abandoned_barrier` is scripted
  with every source of G registered and its sessions open before the intent, N down before its
  poll. The reviewer's fix.
- **Exit codes and the root** (R5-C2): `backup` exits 0 only when its close answers `closed`
  with `completed` and its own root; another outcome or root exits 1, printing it; its abort
  path always exits 1 (incomplete), naming the failure that ended the run. A grant with no
  outcome (intent-closed) takes its first close's outcome and root whether still closing or
  already closed by its acks. `verify --compare-source` registers sources as `backup` does and
  always ends, matched or not, with its grant's abort close, retried until `closed` (it records
  no completion), so it runs under a grant file of its own (the lab's step 5 says so); it exits
  0 only when matched and closed. `snapshot-read close --abort` exits 0 once `closed`, printing
  the recorded outcome. The reviewer's fix; cases in `close_retried_until_closed`,
  `closing_grant_aborted` and `TestSnapshotReadClosure`. Always closing `verify`'s grant rather
  than only on failure: one end for every run, and no grant left open to pin its snapshot.
- **`Refused` carries a reason** (R5-B1): `retry` (a cap, a liveness bound, the validation rate
  or bucket, an outstanding reader call) or `closed` (the grant closing or closed, the token
  refused by a check or by ValidateSnapshotRead). The worker registers again only after
  `closed` or a session past its `Accepted` that ended without its Close; a `retry` it retries
  with the same token after a jittered backoff, 1 s doubling to 10 s (proposed, to be measured),
  registering nothing. The reviewer's fix; cases in `validation_rate_bounded` and
  `closing_grant_aborted`.
- **Register's worker check first** (R5-S1): RegisterSnapshotSource checks the grant's worker
  before its state, so another worker gets `NotFound` whatever the state; the worker rule says
  the handler's worker check comes before any other check of the grant, so CloseSnapshotRead's
  generation refusal follows it too. The reviewer's fix, stated once in the rule; cases in
  `TestRegisterSnapshotSource` (closing and closed) and `TestSnapshotReadClosure`.
- **The `ca.crt` re-put tested** (R5-C4): the extracted `cmd_pki` test, with `run_on_vm` and
  `ca_tool` faked, asserts a `pki.sh file ca.crt` on fl-w after its `csr backup` and `install
  backup`, and none on another VM after its own lines. The reviewer's fix.
- **Nit:** the generation refusal's `<g>` is the request's; `NotFound` at the close says the
  grant was collected or deleted with its volume. None left.
- **No rule narrowed.** S1 to S11 are unchanged.
- **Risks named, not accepted:** a `Refused` (`retry`) ReaderHello waits up to its backoff, 10 s
  at most, before it is tried again (liveness only); a `backup` whose chunk has no registrable
  ISR member at an unchanged `isr_ver` ends with its abort close, and a new grant is needed to
  back up that snapshot.

## Plan review round 6 (2026-11-18), changes made after approval

Reviewed commit eb95fa0; findings R6-C1 (High), R6-C2 (Low) and one Nit, from one reviewer on
the design-code and design-security focuses together. Each change below was made after
approval, in this round; nothing in it is implemented, measured or approved anew.

- **Only an unusable grant aborts** (R6-C1): `backup` closes `aborted` only on a failure that
  leaves its grant unusable, the registration end rule (no ISR member of a chunk registers at an
  unchanged `isr_ver`), which a grant closing or closed always reaches: a `Refused` of reason
  `closed` is followed by a registration, which such a grant refuses. No Go error message is
  matched. Every other failure (the store, an interrupt before the close, the metadata service
  unavailable, a lost root reply) exits 1 with the grant active and its checkpoint kept, so a
  rerun with the same grant file resumes and can reach `completed`, as S7 and lab step 4 need,
  and a lost root reply resolves by operation id at the rerun. A grant a deletion's intent made
  closing meanwhile is closed by that rerun (through the end rule) or by `snapshot-read close
  --abort`. The worker model, which has no store failures, keeps its abort rule; `verify
  --compare-source`, which records no completion and needs no resume, keeps its close at every
  end. The reviewer's fix; `store_fault_resumes` (a store fault mid-run, then a rerun with the
  same grant file that completes; the same after a lost root reply) and a closing grant's
  `Refused` (`closed`) followed by a refused registration that aborts, in `closing_grant_aborted`.
  This narrows round 5's contract rule (R5-C3, above), not a spec rule: the finding shows it
  cannot hold beside S7.
- **Sessions that end some other way** (R6-C2): the node ends a session it closes idle with
  `Refused` (`retry`), and a ValidateSnapshotRead that fails transiently (`Unavailable`, or the
  node's metadata client down) answers `retry`, so neither costs a registration. A connection
  that ends before `Accepted` with no `Refused` counts as a `retry` toward the same node; after 3
  tries in a row (proposed, to be measured) the worker re-reads the volume and registers another
  ISR member of that chunk (the same one if it is the only one), so a down source does not stall
  its chunk (S4) and a closing grant still reaches the worker. The reviewer's fix; cases in
  `validation_rate_bounded` and `closing_grant_aborted`; the worker model's "loses (its node
  down)" names the CLI's tries.
- **Nit:** `--compare-source` requires "a live reader grant of its own". None left.
- **Rules narrowed:** round 5's "every failure ends with its abort close" (R6-C1, above). S1 to
  S11 are unchanged.
- **Risks named, not accepted:** a `backup` that fails other than by an unusable grant leaves
  that grant active, pinning its snapshot and counted among the volume's 4 open grants, until a
  rerun completes or aborts it or `snapshot-read close --abort` closes it; a grant a deletion's
  intent made closing meanwhile records no outcome until then (R6-C1). A source whose
  connection keeps failing before `Accepted` costs its chunk 3 tries before reselection
  (liveness only, R6-C2).

## Plan review round 7 (2026-11-18), changes made after approval

Reviewed commit ccc2082; findings R7-C1 (Medium), R7-C2 (Low) and one Nit, from one reviewer on
the design-code and design-security focuses together. Each change below was made after
approval, in this round; nothing in it is implemented, measured or approved anew.

- **Every `retry` counts toward reselection** (R7-C1): round 6's 3 tries in a row toward a node
  now count each `Refused` (`retry`), whatever its cause, as well as each connection that ends
  before `Accepted`; an `Accepted` resets the count. After the third the worker re-reads the
  volume and registers another ISR member of that chunk (the same one if it is the only one), so
  a node cut off from the metadata service, which answers `retry` to every Hello, does not stall
  its chunk (S4's reselection and revalidation), and a closing grant still reaches the
  registration end rule and closes `aborted`. The reviewer counted only a transient validation's
  `retry`; the worker cannot tell the causes of a `retry` apart without a new reason on the wire,
  so this counts them all, the simpler option: a cap's or a rate's `retry` then costs one
  registration per 3 refusals in a row, an idempotent repeat for the same pair (`sources` grows
  only for a new pair, at most chunks × (RF + 2)). Cases in `closing_grant_aborted` (a node
  answering `retry` for its validation, and one over a cap, each tried 3 times, then another
  member registered; on a closing grant, `aborted`); the worker model does the same.
- **The transient validation set** (R7-C2): a ValidateSnapshotRead failing with `Unavailable`,
  `DeadlineExceeded` or `Cancelled` (what the node's metadata client takes for no answer,
  `flaminio-metaclient/src/lib.rs`), or `Aborted`, or not sent with that client down, answers
  `retry`; only a refusal the service answers is `closed`. The reviewer's fix; the codes added to
  `validation_rate_bounded`.
- **Nit:** the worker model's "loses (its node down; the CLI after its tries toward that node)"
  now scopes the 3 tries to a node with no `Accepted`, separate from a session lost past
  `Accepted`. None left.
- **Rules narrowed:** none. Round 6's "a throttle, an idle close or a transient
  ValidateSnapshotRead failure costs no registration" becomes "costs none before the third
  `retry` in a row" (contract level). S1 to S11 are unchanged.
- **Risks named, not accepted:** a worker held by a node's caps or rate for 3 tries in a row
  costs a GetVolume and a registration, and may move its chunk to another ISR member, though the
  first would have admitted it later (metadata load and liveness only, R7-C1).

## Changes after approval — plan review round 8 (2026-11-18)

Nothing of Low or worse; the plan clears. The Nit, fixed by the orchestrator: contracts.md's
Sessions now say the backoff is per chunk, keeps doubling across a reselection and resets at an
`Accepted`, and the count of tries is per (chunk, node), so the 10 s bound is reachable.
