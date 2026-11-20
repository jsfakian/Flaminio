# Issue #52 — report

## Review rounds

### Plan review round 1 — c6efcd7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: a design-code reviewer (R1-C1 to R1-C16, one Nit) and a design-security reviewer
(R1-S1 to R1-S12, one Nit).

- **R1-C1 — High:** the five new RPCs and the backup identity have no rows in #73's method table
  and no rule in #73's Authorize order, so under #73 every call is refused. Evidence: #73 design,
  "Authorize, in order"; #73 S1; contracts.md:40-54, :35.
- **R1-C2 — High:** the revocation barrier has no delivery path to the sources; no grant ever
  closes and a backed-up volume's deletion blocks for good. Evidence: design.md:42-47,
  contracts.md:50-54; no poll in `flaminio-io`.
- **R1-C3 — High:** S4's source-completeness rule has no simulator check or injected bug, and the
  Go grant state machine is not tied to the Rust model. Evidence: AGENTS.md; design.md:165-167.
- **R1-C4 — Medium:** no etcd layout or transactions for the new records; closing on every source
  can exceed 120 operations; header writes unstated; registration's evidence unstated; closed
  records never collected. Evidence: contracts.md:40-60, design.md:29-31, 99-105;
  `meta/kv/etcd.go:91-92`; #51 contracts, Completeness evidence.
- **R1-C5 — Medium:** the DeleteVolume integration ignores #73's admission (a closing intent
  before a refused deletion), the unparseable-header branch, #73's operation budget and the
  blocked error's code. Evidence: design.md:49-53; #73 S5; `service.go:535-551`;
  batch-2026-11-12.md:222.
- **R1-C6 — Medium:** encrypted backups cannot happen in this increment (#50 and #51 refuse
  encrypted snapshots) but the plan promises them. Evidence: #51 S2 and contracts; #50 contracts;
  spec S8, intent 7 and 12, design.md:29-30, 99-105, 170, 184.
- **R1-C7 — Medium:** the reader port 7412 collides with #71's observer; no listener flag, no
  address rule. Evidence: #71 contracts:143-144; design.md:35, contracts.md:62; lab.conf:107.
- **R1-C8 — Medium:** reader store reads have no epoch rule and no incarnation check, so a
  failover breaks them. Evidence: `store.rs:756-773`, `:1901`; `replica.rs:173-183`;
  `flaminio-node/src/lib.rs:21`; #51 Offline seam.
- **R1-C9 — Medium:** `SnapshotRefV1`/`SourceProofV1` fields have no source (cluster UUID, size at
  the cut, logical block size, replica generation). Evidence: contracts.md:7-9, 24;
  `superblock.rs:28`; `ublk.rs:187`; #61 design.
- **R1-C10 — Medium:** the "separately established exclusion proof" is undefined, so a lost
  source makes a volume undeletable. Evidence: design.md:43-45, contracts.md:54, spec S3; #13 S19.
- **R1-C11 — Medium:** #73's audit hand-off is not taken. Evidence: batch-2026-11-12.md:222; #73
  S7; intent.md:151.
- **R1-C12 — Medium:** the shared byte contracts are called normative and accepted while #51 holds
  them pending #58's coordinated review. Evidence: contracts.md:3, design.md:152-158; #51
  contracts.md:3-9; batch-2026-11-12.md:290.
- **R1-C13 — Medium:** the RPC set and commands leave steps unspecified (admin commands, operation
  to grant, completion and progress RPCs). Evidence: contracts.md:161-163; design.md:99, 103, 120.
- **R1-C14 — Medium:** the lab plan is not runnable as written. Evidence: design.md:181-187;
  intent outcome 12.
- **R1-C15 — Low:** the test plan names no concrete tests; S11's age report has none. Evidence:
  design.md:163-177; spec.md:101.
- **R1-C16 — Low:** the worker's privileges are misstated (the open reads). Evidence:
  design.md:33; `auth.go:79`.
- **R1-S1 — High:** the plan backs up encrypted snapshots that #51 refuses to create and keeps
  keys through #50's deletion with no record. Evidence: #51 design.md:59-63, decisions; #50
  contracts.md:61-68, 167-169; design.md:99-105, 112-116.
- **R1-S2 — High:** no admin step creates the grant, so the worker would need administrator
  authority. Evidence: contracts.md:41-46, 161-163; design.md:32-33, 140-150; `auth.go:64-65`.
- **R1-S3 — Medium:** no #73 two-column rows for the new RPCs, and OverrideDelete bypasses the
  barrier. Evidence: #73 spec.md:54-68, contracts.md:26-80; design.md:49-52.
- **R1-S4 — Medium:** #73's audit hand-off is not taken. Evidence: batch-2026-11-12.md:222; #73
  spec.md:131-133; design.md:191; intent.md:151.
- **R1-S5 — Medium:** the barrier's two ends are unspecified: how a source learns of closing, what
  an exclusion proof is, what stops a new grant between the intent and the mark. Evidence:
  design.md:31, 42-51; spec S3; #13 S19.
- **R1-S6 — Medium:** the worker does not check that the reader server is the source node's data
  plane. Evidence: contracts.md:62, 78; #16 S15.
- **R1-S7 — Low:** the worker identity's lifecycle under #16 is unspecified. Evidence:
  contracts.md:35-37; `pki.go:222-240`; `identity.rs:12-22`; #16 S1, S6, S10.
- **R1-S8 — Low:** "no general reader privileges" is contradicted by the open reads. Evidence:
  design.md:33, contracts.md:36; `auth.go:79-80`; #73 contracts.md:34.
- **R1-S9 — Low:** the worker URI is taken from a frame field. Evidence: contracts.md:63-64;
  `session.rs:114-147`.
- **R1-S10 — Low:** reader resources have no node-wide or grant-wide bound; records never
  collected. Evidence: contracts.md:47-49, 156-159.
- **R1-S11 — Low:** lab commands and secrets delivery are unspecified. Evidence: design.md:181-187;
  AGENTS.md Principles.
- **R1-S12 — Low:** creating and checking the repository key is unspecified. Evidence:
  contracts.md:91-92, 99.
- **Nit (code):** reader frames are big-endian while the replication frames are little-endian.
  Evidence: contracts.md:72; `frame.rs:16`.
- **Nit (security):** the AEAD's object-key bytes do not say whether the bucket prefix is
  included. Evidence: contracts.md:115.

Changes/resolution:
- R1-C6, R1-S1 (one fix): encrypted backups deferred, not settled, to the plan that lifts #51's
  refusal (intent outcome 7, S8, retained-key records, their tests, the encrypted lab check);
  AuthorizeSnapshotRead refuses an encrypted volume by #50's field 20 and header mark, with two
  tests; the intent's text unchanged, with a deferral note. S8 and S2's key-reference pin are
  narrowed (deferred), shown necessary by these findings.
- R1-C1, R1-S2, R1-S3 (one fix): #73-style rows for the six reader methods, a worker rule beside
  the node rules, insecure instances refusing all six; `flaminio-meta snapshot-read
  authorize|close` writes the grant file the worker takes; the worker refuses a non-`backup` pair;
  OverrideDelete runs the barrier; role tests per method and role.
- R1-C2, R1-C10, R1-S5 (one fix): an own-node `ListClosingSnapshotReads` poll at start and every
  5 s; #13's `forgotten` mark as the one exclusion proof; a closing volume refuses new grants and
  sources, the mark compares the reader record; tests for a source without session, a restarted
  source and a forgotten source; mutations `grant_under_closing`, `close_without_notify`.
- R1-C3: a reader model in `flaminio-repl` with archive-equality and closure checks, six named
  mutations, grant-lifecycle metadata vectors replayed against `meta/domain/snapread.go`.
- R1-C4: records and transactions tables (keys, compares, writes, all ≤ 120), a per-grant
  `pending` counter, no header writes, registration comparing header, chunk, catalog and #61's
  node record (#51's completeness evidence), collection of acked sources, closed grants and the
  volume's records.
- R1-C5: the barrier after #73's admission checks, the intent's compares, the mark's +1 compare
  (≤ 6), the unparseable header by listing `snapread/<v>/`, `FailedPrecondition` naming the node, a
  test of a refused protected deletion.
- R1-C7: `--reader-listen ADDR`, port 7413 in the lab, the address derived from `Node.address`.
- R1-C8: `Store::plan_view_read` without the session epoch check, refusing a chunk fenced below the
  first epoch; a test across a failover.
- R1-C9: a cluster record defined; `size_bytes` the grant's, a smaller base compatible;
  `logical_block_bytes` 4096; #61's `device_uuid` and `store_generation`.
- R1-C11, R1-S4 (one fix): a `docs/followups.md` item, "Audit events for snapshot-read grants",
  to be filed; the intent's audit line corrected.
- R1-C12: the shared bytes marked pending #58's coordinated review, nothing built before it.
- R1-C13: the admin commands, the grant file as the worker's input, `outcome`/`root` on
  CloseSnapshotRead; metadata operation progress cut; the retained-key reference deferred.
- R1-C14, R1-S11 (one fix): `make backup-store-up|down`, `make backup-check`, their scripts and
  ordered steps, secrets made in the VM, ids validated and quoted, a script test.
- R1-C15: tests named per rule, fuzz targets named, the restorable-point age test.
- R1-C16, R1-S8 (one fix): the worker keeps the open reads, and what they show it is stated.
- R1-S6, R1-S9: the client accepts only the source's `io`; the node takes the worker's identity
  from its certificate.
- R1-S7: issuance by `pki issue w`, renewal under #16 S10, pick-up per connection, no resumption.
- R1-S10: caps per node, grant and volume; idempotent registration; checks before the metadata
  call.
- R1-S12: `repo init` key creation and the checks on every secret file, each refusal tested.
- Nits: frames `[u32le len]`; the AEAD key relative to the repository prefix.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — ca86b81

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan review, design-code focus; plan review, design-security focus.

- **R2-C1 — Medium:** Two of the six injected bugs, `close_without_notify` and
  `grant_under_closing`, harm only progress, so neither named check (archive equality, closure)
  catches them. The simulator also has no volume deletion, re-creation or node forgetting.
  Evidence: design.md:176-183, 226; contracts.md:184-188; `flaminio-repl/src/sim/mod.rs:7-14`
  (the chaos list), `:500-533` (the liveness checks); `meta.rs` has no deletion and no
  forgotten set.
- **R2-C2 — Medium:** Nothing clears a deletion's closing intent, so a volume whose deletion
  stops after the intent can never be backed up again; the named risk said the opposite.
  Evidence: contracts.md:89, 93, 114, 185; design.md:289-290; decisions.md (Risks).
- **R2-C3 — Low:** The acks have no bound on the data plane's single metadata-client thread, so
  up to 1024 of them can queue ahead of a target's ChangeIsr. Evidence:
  `flaminio-metaclient/src/lib.rs` (one call at a time); `flaminio-node/src/engine.rs:89-91`;
  contracts.md:131-133, 169-171.
- **R2-C4 — Low:** No node is started with `--reader-listen`, neither in the lab nor under #12's
  DaemonSet. Evidence: `scripts/guest/node.sh:41-54`; `scripts/lab.sh:679` (`node_env`);
  #12 design.md:96-105; design.md:203-205.
- **R2-C5 — Low:** Two lab steps cannot run as written. `pki.sh` cannot sign the store's pair
  inside the VM, and the restore has no device on fl-w. Evidence: design.md:251-252, 256-257,
  270-271; `scripts/guest/pki.sh:2-5`; `scripts/lab.sh:548-553` (`issue`).
- **R2-C6 — Low:** Collection deletes `snapread/<v>/<fe>/` on paths that know no first epoch,
  and it orders by a create revision that `kv.Entry` does not carry. Evidence:
  contracts.md:100-104, 191; `meta/service/service.go:441, 527-534`; `meta/kv/kv.go:11-16`;
  #51 contracts (Deletion).
- **R2-C7 — Low:** ListClosingSnapshotReads cannot answer from the read it names: grant state is
  not in the source record, and `kv.List` has no limit. Evidence: contracts.md:91, 131-133;
  `meta/kv/kv.go:39-40`.
- **R2-C8 — Low:** A forgotten node that is still running never closes its reader sessions.
  Evidence: contracts.md:174-177; #13 design.md:233-235.
- **R2-C9 — Low:** The grant lifecycle had two Rust copies (`reader.rs` and `meta.rs`), so the
  vectors might not come from the model the simulator checks. Evidence: design.md:173-186;
  `examples/meta_vectors.rs:15`.
- **R2-C Nit:** spec.md:44 said the worker takes its identity from its certificate.
- **R2-C Nit:** design.md:266-267: `node-fault` stops fl-b's node, not fl-a's.
- **R2-S1 — Medium:** ValidateSnapshotRead could not check the worker, the ref or the current
  proof, and the grant generation was undefined. With caller-chosen operation ids and grant
  collection, an old token could validate under another worker's grant, and a token superseded by
  re-registration stayed valid. Evidence: contracts.md:91, 100, 124, 127-130, 204, 342-344;
  #73 contracts (Identifiers: service-drawn operation ids).
- **R2-S2 — Low:** The reader connection caps had no rule for unauthenticated connections and no
  deadlines, so stalled connections could take every slot. Evidence: contracts.md:224-227;
  `flaminio-node/src/net.rs:168-200`.
- **R2-S3 — Low:** The lab store's TLS identity, and what the worker checks of it, were
  unspecified, so any lab-CA workload certificate with fl-w's IP would pass as the store.
  Evidence: design.md:252; `scripts/guest/pki.sh:12-17`; `control/cmd/flaminio-ca/main.go:90`;
  `pkg/pki/pki.go:222-240`; contracts.md:263; `scripts/lab.sh:577-578`.
- **R2-S4 — Low:** On fl-w the worker must run as root to read `/etc/flaminio/pki`, which also
  holds `admin.key` and `token.key`. "Never holds an administrator pair" was therefore only a
  configuration check. Evidence: `scripts/lab.sh:578, 589-592`; `scripts/guest/pki.sh:8-10`;
  #16 design.md:144-153; contracts.md:71-74, 265-268.
- **R2-S5 — Low:** `closing` was never cleared, so a volume whose deletion stops after its
  intent refuses every backup permanently, against the named risk. Evidence: contracts.md:93,
  188; design.md:289; decisions.md:166-167; `meta/service/service.go:581`.
- **R2-S Nit:** AckSnapshotReadClosed should require the grant to be `closing`.
- **R2-S Nit:** "an invalid id or URI" should require a `backup` identity of the instance's
  trust domain in `pki.ParseID(..).String()` form.

Changes/resolution:
- R2-C2, R2-S5 (one fix): a new administrator RPC, `ReopenSnapshotReads`
  (`flaminio-meta snapshot-read reopen VOLUME`), clears `closing`. It is refused while the
  header is deleting or does not parse and while `open` > 0. It compares the header and the
  reader record, so a racing deletion's mark fails and the barrier runs again. Its role row,
  transaction row and `TestReopenSnapshotReads` (an abandoned deletion, then a reopen and a new
  grant) are added. Authorize's closing refusal now names the way out. S3 gains the exception
  "unless an administrator reopens it after its deletion stopped (no grant open, the volume not
  marked deleting)": without it the rule bars every later backup. The named risk is corrected
  (design, decisions).
- R2-S1: a grant's `generation` is the reader record's `next`, raised at each claim, so no grant
  reference is reused even when an operation id is reused after collection. The source record
  keeps the issued token's SHA-256. ValidateSnapshotRead takes the token's worker, proof and
  digest and compares them, and the generation, with the grant and the record; a
  re-registration replaces the digest. A non-identical repeat of an operation id is
  `AlreadyExists`, and `--operation-id` is only for retrying the same request. Three
  `TestValidateSnapshotRead` cases are added.
- R2-C1: the simulator gains reader-configuration events: delete the volume (the barrier),
  re-create it at a new first epoch, stop a deletion and reopen, and forget a node (#13's
  `forget()`). Two new checks: exclusion (no grant or source after the closing intent, unless
  reopened) catches `grant_under_closing`, and reader progress (in settle, closing grants close
  and deletions not stopped complete) catches `close_without_notify`. A table names the check
  that catches each of the six mutations.
- R2-C9: the metadata side of the lifecycle lives once, in `flaminio-repl/src/meta.rs`. The
  simulator drives it and the vectors are emitted from it. `src/reader.rs` keeps only the node
  and worker sides.
- R2-C3: the poll's pages and its acks share #13's sweep rule: one reader call outstanding, none
  queued behind it. A test, `reader_calls_one_outstanding`.
- R2-C4: `READER_PORT` (7413) in `lab.conf`, `READER_LISTEN` in `node_env` and `node.sh`, and a
  `node-scripts-test.sh` case. Whichever of #12 and #52 lands second adds
  `--reader-listen "$HOST_IP:7413"` and `hostPort` 7413 to #12's DaemonSet; #12 is not edited.
- R2-C5, R2-S3 (one fix): `lab.sh backup-store up` issues the store's pair from the host
  through `issue`, as `spiffe://<td>/flaminio/node/fl-w/backup-store`, server use only. The
  worker requires a configured `store_identity` (an exact URI SAN, or a DNS name under its CA)
  and refuses any other certificate. A test, `store_identity_required`.
- R2-C5 (restore): the archive is restored to a file on fl-w. That file (data only) is streamed
  through the host onto the new volume on fl-a, and the SHA-256s are compared; cleanup deletes
  the file.
- R2-C6: every deletion path deletes `snapread/<v>/`, as #51 does `snapshots/<volume>/`.
  Collection takes the lowest-generation closed grant. Test cases are added for each path.
- R2-C7: ListClosingSnapshotReads does one List of the node's prefix (its cost stated), then one
  Get per distinct grant on the page (≤ 1024). States only advance, so reads at different
  revisions are safe. A paging test with mixed states.
- R2-C8: when the poll answers `node N is forgotten`, the node closes every reader session and
  refuses new ones. A test, `forgotten_node_closes_reader`.
- R2-S2: the caps count authenticated sessions only. Liveness bounds are added: 32 connections
  before authentication, the oldest closed past that; 5 s for the handshake and Hello; 60 s
  idle; 2 validations per (grant, node) in 10 s. All are proposed and to be measured. Tests
  `stalled_connections_do_not_block` and `validation_rate_bounded`. A residual handshake-delay
  risk is named, not accepted.
- R2-S4: the worker runs as `flaminio-backup`, with its pair in `/etc/flaminio/backup-pki`
  (0700, key 0600). `pki.sh` gains that directory, and #16's list the same entry, added by
  whichever of #16 and #52 lands second. The repository key and the S3 credential belong to
  that user.
- Nits: all four fixed (spec S2 wording; "as `node-fault` does for fl-b"; the ack refuses an
  active grant; the worker URI's form).

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — b2f2b77

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R3-C1 — Medium:** The simulator's delete and re-create events have no incarnation in the
  model they drive, and the plan does not say what a deletion does to the target, the replicas,
  #13's sweep or the run's end state. Evidence: design.md:193-210 re-creates VOL "at a new first
  epoch"; `flaminio-repl/src` has no incarnation, its replicas hold chunks by `(VOL, c)`
  (`sim/mod.rs:388-395`) read by the final divergence check (`:1355-1364`); `run_mut` and `check`
  expect the volume (`:497-540`, `:1346`); #13's sim sets `deleted_since` false since "the sim
  deletes no volume".
- **R3-C2 — Low:** "Reader progress" can fail on a clean seed: a closing grant whose source's node
  is forgotten after the worker's one Close closes only on another Close, which nothing makes;
  and `grant_under_closing`'s grant half needs the worker model to authorize during a barrier.
  Evidence: contracts.md:220-222, design.md:139-141, :200-206, :211-213.
- **R3-B1 — Low:** The lab's `flaminio-backup` and `backup-store` setup cannot run under #16's
  clear `pki.sh`, and the worker cannot read its CA or its grant file. Evidence: #16's `pki.sh`
  refuses an unknown name (16/design.md, Installing) and installs `ca.crt` only to `pki` and
  `io-pki`; no step creates `flaminio-backup`; `metactl` runs under `sudo` (`lab.sh:529-534`), so
  the grant file is root's 0600 (contracts.md:402-404).
- **R3-S1 — Low:** RegisterSnapshotSource's order contradicts the digest it stores. Evidence:
  contracts.md:137 "registers the source BEFORE the token exists" against contracts.md:140 "keeps
  the issued token's `token_digest`".
- **R3-S2 — Low:** Acks and exclusions do not compare the grant's generation, while a reused
  operation id shares source keys across generations. Evidence: contracts.md:96, :101-104,
  :165-170, :253.
- **R3-S3 — Low:** The validation rate is keyed per (grant, node), but each session validates one
  chunk's token, so a normal backup is throttled to 0.2 sources per second per node. Evidence:
  contracts.md:144-146, :277-279, :287-292.
- **Nit:** design.md:202-203 "a ReopenSnapshotReads follows" is refused while `open` > 0.

Changes/resolution:
- R3-C1: the simulator never writes a deletion's mark and deletes no volume: its deletion event
  runs the barrier up to the mark (one model step finding `open` = 0), one in four abandoned after
  its intent, then a reopen. The re-create event is cut, so the run keeps one incarnation, the
  end-state checks run unchanged and #13's `deleted_since` stays false. The mark and the records'
  deletion stay `TestDeleteVolumeReaderBarrier`'s. Closure and reader progress restated for the
  barrier's mark; each of the six mutations keeps its check.
- R3-C2: the worker and its model retry CloseSnapshotRead every 5 s (to 60 s in the CLI) until
  `closed`, each retry excluding sources forgotten since; `NotFound` or a generation refusal ends
  it; an interrupted wait exits 1 and a rerun resumes. The worker model authorizes during a
  barrier and after a reopen and registers sources of a closing grant. Tests: a
  TestSnapshotReadClosure case, `close_retried_until_closed`.
- R3-B1: #16's `pki.sh` list gains `backup` and `backup-store` (their directories and users, made
  idempotently before the `chown`; `ca.crt` into both; the sweep's temporary-directory names), by
  whichever of #16 and #52 lands second. `backup-store.sh up` runs the store as
  `flaminio-backup-store`, makes `/var/lib/flaminio-backup` and refuses without both pairs;
  `backup.sh` hands the grant file to `flaminio-backup` with `sudo install -o flaminio-backup -m
  0600`; `backup-scripts-test.sh` cases.
- R3-S1: sign the token, commit the record with its digest in one transaction, return the token
  only after the commit; a lost compare re-signs. contracts.md corrected; a test case.
- R3-S2: AckSnapshotReadClosed, CloseSnapshotRead and the exclusion refuse a grant reference of
  another generation (`FailedPrecondition`, nothing written); a TestSnapshotReadClosure case.
- R3-S3: the rate keyed per (grant, node, chunk), 2 in 10 s, proposed and to be measured;
  `validation_rate_bounded` asserts a worker walking N chunks is not refused.
- Nit: the model retries the reopen every 5 s until the grants close.

No rule narrowed. Risk named, not accepted: a source down and not forgotten keeps the worker's
`backup` waiting on its close. Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — a25d9f9

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one plan-reviewer, design-code and design-security focuses together.

- **R4-C1 — Low:** Reader progress could still fail on a clean seed, for a grant that an abandoned
  barrier closed: the worker never closed a grant a deletion's intent made closing, so once its
  source's node was forgotten nobody excluded it, the grant stayed closing and the reopen was
  refused. Evidence: contracts.md:230-232 (exclusion only in a closing grant's close and a
  waiting deletion); design.md:205-215 (the worker model retries only its own closes; one
  barrier in four abandoned; forget may follow); design.md:206-208 (reopen refused while `open`
  > 0); the CLI's "a closing volume's backups end at once" says not how.
- **R4-C2 — Low:** The close retry had no rule for a repeated close with another outcome, nor for
  errors that will not clear, so it could loop forever. Evidence: contracts.md:421-424 and
  design.md:286 (ends only on `closed`, `NotFound` or a generation refusal); contracts.md:169-170
  (a repeat only "first excludes"); the generation refusal shares `FailedPrecondition` with the
  insecure and other refusals, with no message to tell them apart (contracts.md:166-168, 208-209).
- **R4-B1 — Low:** `file ca.crt` into `backup-pki` and `backup-store-pki` would build both
  directories and both users on every VM. Evidence: design.md:323-333 and decisions.md:187-196;
  #16's `file` builds a missing directory (16/design.md, Installing); `cmd_pki issue` runs
  `put_file "$vm" ca.crt` on every VM first (lab.sh:576-577); only fl-w runs the worker and store.
- **R4-C3 — Low:** Once run as `flaminio-backup-store`, the store could not read its own S3
  credential or write its volume. Evidence: design.md:307-316 (only the worker's credential copy
  given an owner; the volume set up as root with no owner); the tests cover neither
  (design.md:368-373).
- **R4-C4 — Low:** `backup-store` was in `pki.sh`'s list but not in `identities VM`, so #16's
  renewal never renewed it and the store's certificate expired. Evidence: 16/design.md (`lab.sh
  pki renew` runs `pki.sh due` over `identities VM`'s names only); contracts.md:72-73 adds only
  `backup`; design.md:317-322 (`backup-store` issued only by `backup-store up`).
- **R4-S1 — Low:** Keying the validation rate per (grant, node, chunk) removed any total bound on
  a node's ValidateSnapshotRead calls, which run on the node's one-call MetaClient outside the
  one-outstanding rule, so a ChangeIsr could wait behind many. Evidence: contracts.md:291-296
  (bound per registered source only); contracts.md:217-220 (the one-outstanding rule names only
  the poll's pages and acks); about 2 × 4096 / 10 s ≈ 800 linearizable reads per second per grant
  and node for a 4096-chunk volume.
- **Nit:** the grant file's `install -g flaminio-backup` relied on the distribution's
  `USERGROUPS_ENAB` default for the group (design.md:330-331, 349-351; decisions.md:195).

Changes/resolution:
- R4-C1: RegisterSnapshotSource refuses a grant not active with the fixed message `grant <op> is
  closing` (or `is closed`); `backup`, refused with it, ends with its abort close,
  CloseSnapshotRead(aborted), retried until `closed`, and a reader session the node refuses or
  closes is followed by a registration for that chunk before any retry, so a closing grant
  always reaches the worker. The worker model does the same. Tests:
  `reader_progress_after_abandoned_barrier` (sim) and `closing_grant_aborted`
  (`failure_matrix.rs`); a `TestSnapshotReadClosure` case for the refusal.
- R4-C2: a repeated close keeps the first recorded outcome and root and answers the state with
  them, never an error; an intent-closed grant takes its first close's outcome; the response
  carries the recorded outcome and root. The CLI retries only `closing`, `Unavailable`, `Aborted`
  and `DeadlineExceeded`; any other error exits 1 naming it (the generation refusal, now with the
  fixed message `grant <op> generation <g> is not current` on the close and the ack, `NotFound`,
  the insecure and role refusals). `close --abort` answered `completed` exits 0; `backup`'s
  `completed` answered `aborted` exits 1. Cases in `close_retried_until_closed` and
  `TestSnapshotReadClosure`.
- R4-B1: `pki.sh file ca.crt` writes into the two backup directories only where they exist and
  never builds them; only `csr backup` and `csr backup-store`, run on fl-w alone, make them and
  their users. `pki issue` puts `ca.crt` on fl-w again after its `backup` line, and `backup-store
  up` after issuing. The reviewer's first option. A `backup-scripts-test.sh` case: neither
  directory present, no user and no directory made.
- R4-C3: `backup-store.sh up` creates the store's credential copy owned by
  `flaminio-backup-store`, 0600, and chowns the mounted volume's root to it, 0700, before starting
  the store. `backup-scripts-test.sh` cases for both owners.
- R4-C4: #16's `identities VM` gains the fl-w line `backup-store CA ID --server --ip`, skipped by
  `pki issue` as `csi-controller` is, so #16's renewal renews it; added by whichever of #16 and
  #52 lands second (#16 unchanged). If the pinned SeaweedFS does not take a renewed pair without a
  restart (qualified with its profile), the renewal restarts it on fl-w, added the same way. A
  `backup-scripts-test.sh` case.
- R4-S1: the per-(grant, node, chunk) key stays; a node-wide token bucket (burst 16, the session
  cap, refilled at 4 a second; proposed, to be measured) bounds the node's total, and
  ValidateSnapshotRead joins the one-outstanding reader-call rule (a Hello that would need a
  second one `Refused`, retry later), so a ChangeIsr waits behind at most one validation. A
  node-wide bucket rather than the reviewer's per-(grant, node) one, since only it bounds the
  node's total. Cases in `validation_rate_bounded` and `reader_calls_one_outstanding`.
- Nit: `useradd --system --user-group`. No Nits left.
- No rule narrowed. New risk named, not accepted: a ReaderHello can be refused, to be retried,
  by the node-wide bucket or the outstanding reader call (liveness only).

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — f4a3f1c

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R5-C1 — Low:** The worker model reached a closing grant only through its next registration,
  so a grant with every source already registered never closed after an abandoned barrier.
  Evidence: design.md:215-219 (the model closes `aborted` only on a refused registration);
  contracts.md:447-450 (the CLI also re-registers after a refused or closed session, which the
  model lacked); `reader_progress_after_abandoned_barrier` (design.md:291) did not fix whether G
  had a source left.
- **R5-C2 — Low:** The close's exit rules gave an aborted `backup` exit 0, ignored the root, and
  did not cover a grant already `closed` with no outcome, nor `verify --compare-source`.
  Evidence: contracts.md:442-443 against 465 (exit 1 for incomplete); 173-176 (the first root
  kept); 184-185 and 146-147 (an intent-closed grant reaching `closed` with no outcome, Register
  answering `is closed`); 457-458 (`verify --compare-source` registers under a live grant).
- **R5-B1 — Low:** Every throttle refusal cost a Register write, since `Refused` carried no
  reason and the worker re-registered after any refused session. Evidence: contracts.md:228-229,
  304-306 (the bucket and the outstanding call answer `Refused`); 447-449 (re-registration after
  any refusal); 331 (`Refused=69` with no reason); 277 (Register's 7-compare write).
- **R5-C3 — Low:** A Rust match of a Go error message was the only route from a closing Register
  to the abort close, and nothing tied the two literals together. Evidence: contracts.md:146-147
  (the worker matches `grant <op> is closing` inside a `FailedPrecondition` shared with
  per-source refusals); design.md:289, 293 (each side tested against its own text or fake).
- **R5-S1 — Low:** RegisterSnapshotSource's closing refusal was not ordered after its worker
  check, so another `backup` identity holding an operation id could learn the grant's state.
  Evidence: contracts.md:146 against 150-151; `TestRegisterSnapshotSource` (design.md:288) had no
  case of another worker on a closing grant.
- **R5-C4 — Low:** The `ca.crt` re-put on fl-w that R4-B1 depends on had no test. Evidence:
  design.md:333 (`pki issue` puts `ca.crt` on fl-w again after its `backup` line); origin/main
  `scripts/lab.sh` `cmd_pki` `issue)` (puts `ca.crt` before any `issue`); design.md:392-393 (the
  extracted `cmd_pki` test checks only the `backup-store` line).
- **Nit:** contracts.md:171-172 did not say which `<g>` the generation refusal names, and
  contracts.md:443-444's `NotFound` "that it was collected" omitted a grant deleted with its
  volume.

Changes/resolution:
- R5-C3 (closing R5-C1's root cause in the CLI and R5-C2's `is closed` and `verify` cases):
  every failure that ends `backup` after it read its grant file and before its root is
  published ends with its abort close, retried until `closed`. No message is matched: a
  registration failing with `Unavailable`, `Aborted` or `DeadlineExceeded` is retried; any other
  refusal makes the worker re-read the volume and try each ISR member of that chunk once; a chunk
  none of whose members registers while its `isr_ver` stays the same ends the run. So a grant
  closing or closed ends it whatever the message says, and `grant <op> is closing` stays a
  message for people. This is the reviewer's first option, chosen over a structured `ErrorInfo`
  reason because it adds no field or contract between the two sides. `closing_grant_aborted`
  uses a fake whose `FailedPrecondition` message matches nothing.
- R5-C1: the worker model follows a session the node refuses as `closed`, closes at the barrier
  or loses with its node with a registration for that chunk, as the CLI does. It closes
  `completed` a grant it has assembled, and ends a grant by the same unchanged-`isr_ver` rule.
  `reader_progress_after_abandoned_barrier` is scripted with every source of G registered and its
  sessions open before the intent, and N down before its poll.
- R5-C2: `backup` exits 0 only on `closed` with `completed` and its own root. Another outcome or
  root exits 1 and is printed. The abort path always exits 1 (incomplete), naming the failure. A
  grant with no outcome takes its first close's outcome and root, whether still closing or
  already closed. `verify --compare-source` always ends with its grant's abort close, retried, so
  it runs under a grant file of its own (lab step 5 says so). It exits 0 only when matched and
  closed. There are cases in `close_retried_until_closed`, `closing_grant_aborted` and
  `TestSnapshotReadClosure`.
- R5-B1: `Refused` carries a reason, `retry` (caps, liveness bounds, the validation rate or
  bucket, an outstanding reader call) or `closed` (the grant closing or closed, a token refused).
  The worker registers again only after `closed` or after a session past `Accepted` ends without
  its Close. It retries `retry` with the same token after a jittered backoff of 1 s doubling to
  10 s (proposed, to be measured). There are cases in `validation_rate_bounded` and
  `closing_grant_aborted`.
- R5-S1: Register checks the grant's worker first, so another worker gets `NotFound` whatever
  the state. The worker rule puts the handler's worker check before any other check of the
  grant, Close's generation refusal included. There are cases in `TestRegisterSnapshotSource`
  (closing and closed) and `TestSnapshotReadClosure`.
- R5-C4: the extracted `cmd_pki` test, with `run_on_vm` and `ca_tool` faked, asserts a `pki.sh
  file ca.crt` on fl-w after its `csr backup` and `install backup`, and none on another VM after
  that VM's own lines.
- Nit: fixed. `<g>` is the request's, and `NotFound` covers "or deleted with its volume".
- No rule narrowed. Two risks are named, not accepted: a `retry` refusal waits up to its backoff
  (10 s at most) before the next try; a `backup` whose chunk has no registrable ISR member at an
  unchanged `isr_ver` ends with its abort close, and backing up that snapshot then needs a new
  grant.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 6 — eb95fa0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R6-C1 — High:** Under round 5's "every failure ends with its abort close", a backup that hits
  an ordinary failure (a store outage, an interrupt, a metadata error, a lost root reply) can no
  longer be resumed, which S7 and lab step 4 need. Evidence: the abort rule covered every failure
  before publication (contracts.md:452-455); an aborted grant is closed and refuses every
  registration (contracts.md:458); re-authorizing with the same operation id returns the closed
  grant (contracts.md:127-128) and a new one fails the checkpoint's binding (design.md:176);
  against S7 (spec.md:83-86), lab step 4 (design.md:385-387) and the failure matrix's resume
  (design.md:303); a lost root reply then aborted can leave an archive with its grant recorded
  `aborted` (design.md:180).
- **R6-C2 — Low:** The session rule covers only a `Refused` and a session past `Accepted`, so a
  connection ending some other way has no defined reaction, and some end in a wasted
  registration. Evidence: "only after" a `Refused` (`closed`) or a session past `Accepted`
  (contracts.md:460-462); a connection failing before `Accepted` (node down, TLS failure, the
  pending eviction or 5 s handshake bound, contracts.md:306-307) cannot reselect, stalling a chunk
  against S4 (spec.md:63-64) and reopening R5-C1 for that window (design.md:224-225); the 60 s
  idle close costs a registration (contracts.md:307, against design.md:86-87); a transient
  ValidateSnapshotRead failure had no reason and fell under `closed` (contracts.md:337-338).
- **Nit:** `--compare-source` "requires a live registered reader grant" (contracts.md:483), but
  `verify` registers its own sources under a grant of its own.

Changes/resolution:
- R6-C1: the reviewer's fix. `backup` closes `aborted` only on a failure that leaves its grant
  unusable: the registration end rule (no ISR member of a chunk registers at an unchanged
  `isr_ver`), which a grant closing or closed always reaches, since a `Refused` (`closed`) is
  followed by a registration such a grant refuses; no Go error message is matched. Every other
  failure (the store, an interrupt before the close, the metadata service unavailable, a lost
  root reply) exits 1 with the grant active and its checkpoint kept, so a rerun with the same
  grant file resumes and can reach `completed`. A grant a deletion's intent made closing
  meanwhile is closed by that rerun or by `snapshot-read close --abort` (a risk named, not
  accepted). The worker model and `verify --compare-source` keep their abort close. New
  `store_fault_resumes` in `failure_matrix.rs` (a store fault mid-run, then a rerun with the same
  grant file that completes; the same after a lost root reply) and a closing grant's `Refused`
  (`closed`) followed by a refused registration that aborts, in `closing_grant_aborted`
  (contracts.md CLI; design.md, The barrier, Resume, test plan and risks). decisions.md's
  round-5 R5-C3 entry now says it was narrowed in round 6, and why.
- R6-C2: the reviewer's fix. The node ends a session it closes idle with `Refused` (`retry`), and
  a ValidateSnapshotRead answering `Unavailable` (or not sent with the node's metadata client
  down) answers `retry`. The worker counts a connection ending before `Accepted` with no
  `Refused` as a `retry` toward the same node and, after 3 tries in a row (proposed, to be
  measured), re-reads the volume and registers another ISR member of that chunk. Cases in
  `validation_rate_bounded` and `closing_grant_aborted`; the worker model's "loses (its node
  down)" names the CLI's tries.
- Nit: fixed ("a live reader grant of its own").
- Rule narrowed: round 5's contract rule "every failure ends with its abort close" (R6-C1: it
  cannot hold beside S7). S1 to S11 unchanged.
- Risks named, not accepted: a `backup` that fails other than by an unusable grant leaves the
  grant active, pinning its snapshot and counted among the volume's 4 open grants, until a rerun
  or `snapshot-read close --abort`, and an intent-closed grant records no outcome until then; a
  source failing before `Accepted` costs its chunk 3 tries before reselection (liveness only).

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 7 — ccc2082

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R7-C1 — Medium:** A source that keeps answering `Refused` (`retry`) is retried forever,
  with no revalidation or reselection; a node cut off from the metadata service answers `retry`
  to every Hello, so its chunk stalls against S4, and a closing grant whose last needed source is
  that node never reaches the end rule. Evidence: contracts.md:472-478 (a `retry` registers
  nothing; only a connection ending before `Accepted` counts toward the 3 tries), :337-339 (the
  transient-validation `retry`), :52 and design.md:110 (revalidation only around read batches),
  spec.md:62-64 (S4), contracts.md:455-457.
- **R7-C2 — Low:** Only `Unavailable` and the metadata client down make a ValidateSnapshotRead
  failure `retry`; `DeadlineExceeded`, `Cancelled` and `Aborted` fall under its refusal, `closed`,
  costing a registration per Hello. Evidence: contracts.md:337-340;
  `flaminio-metaclient/src/lib.rs:139-143` and :146-153 (its timeout is `DeadlineExceeded`, and
  `Unavailable`, `DeadlineExceeded`, `Cancelled` are no answer); the CLI's transient set,
  contracts.md:446, 462-463.
- **Nit:** design.md:231-232's "loses (its node down; the CLI after its tries toward that node)"
  reads as if a session lost past `Accepted` were tried before its registration.

Changes/resolution:
- R7-C1: the 3 tries in a row toward a node with no `Accepted` now count each `Refused`
  (`retry`), whatever its cause, as well as each connection ending before `Accepted`; after the
  third the worker re-reads the volume and registers another ISR member (the same one if it is
  the only one), so the stalled chunk reselects and a closing grant reaches the end rule
  (contracts.md, CLI's Sessions and the end rule's "always reaches"; design.md's summary and the
  worker model). The worker cannot tell a `retry`'s cause without a new reason on the wire, so
  every `retry` counts, the simpler option than the reviewer's validation-only count; a cap's
  `retry` then costs one idempotent registration per 3 in a row. `closing_grant_aborted` gains a
  node answering `retry` for its validation and one over a cap, each tried 3 times, then another
  member registered, and on a closing grant `aborted`. Risk named, not accepted: a worker held by
  a node's caps for 3 tries costs a GetVolume and a registration and may move its chunk.
- R7-C2: a ValidateSnapshotRead failing with `Unavailable`, `DeadlineExceeded`, `Cancelled` or
  `Aborted`, or not sent with the metadata client down, answers `retry`; only a refusal the
  service answers is `closed` (contracts.md, `Refused`). The codes added to
  `validation_rate_bounded`.
- Nit: fixed; the worker model scopes the 3 tries to a node with no `Accepted`, apart from a
  session lost past `Accepted`.
- No spec rule narrowed; decisions.md records the round.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 8 — c4b750a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Reviewed commit f4915b9 (diff ccc2082..c4b750a). R7-C1, R7-C2 and the Nit fixed. Nothing of Low
or worse. One Nit, wording, fixed in 48d4d82: contracts.md:476-479 now say the 3-try count is per
(chunk, node) and the backoff is per chunk, doubling across a reselection and reset at an
`Accepted`.

Status: clear. Approval remains separate; no approval is inferred.
