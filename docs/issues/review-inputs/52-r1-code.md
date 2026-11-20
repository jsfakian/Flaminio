# #52 plan review, round 1, focus design-code

### R1-C1 — High
**The new RPCs and the backup identity have no rows in #73's method table and no rule in #73's Authorize order. Under #73, every call is refused.**
- Evidence: #73 (clear) design, "Authorize, in order": with neither authorization record, "other new methods are refused". Once active, an identity that is neither an administrator nor allowed by a node, agent or target rule "is refused, with no store read". #73 spec S1 requires a role for every method outside the open reads, Attach and the node rules.
- #52 adds `AuthorizeSnapshotRead`, `RegisterSnapshotSource`, `ValidateSnapshotRead`, `CloseSnapshotRead` and `AckSnapshotReadClosed` (contracts.md:40-54), plus a non-admin, non-node identity `flaminio/backup/<id>` (contracts.md:35). The plan never mentions #73.
- So `RegisterSnapshotSource` and the worker's `CloseSnapshotRead` are refused once #73 is active. `AuthorizeSnapshotRead` is refused until activation, unless it is marked `inactiveRules`. Insecure instances are not covered either.
- Fix: add #51-style role rows to contracts.md. Give each RPC its rule before activation and once active (e.g. `cluster-admin` for Authorize and admin Close). Add a worker rule to #73's Authorize order that passes `backup` identities to the two worker methods with no store read; the handler then checks ownership. The two node self-rules can follow #73's existing node rules. State the insecure-instance behaviour, and say these rows are added by whichever of #52 and #73 lands second. Add S2 tests per role.

### R1-C2 — High
**The revocation barrier has no delivery path to the sources.**
- Evidence: design.md:42-47 and contracts.md:50-54 require every registered source to close admission and call `AckSnapshotReadClosed`. Sources are only "notify/poll"ed.
- No RPC lets a node's data plane list the grants that are closing for its node. `ValidateSnapshotRead` runs only when a session opens. A source registered by a worker that never connected has no session at all. Nothing in `flaminio-io` polls the metadata service for this.
- Result: no ack is ever sent, so no grant closes. Pins are held for good, and a `DeleteVolume` of any volume that was ever backed up blocks for good, even with every node up.
- Fix: specify the mechanism. For example, add `ListClosingSnapshotReads(node)` (own-node `io` rule, paginated, bounded), with its period, its caller (off the engine thread) and its restart behaviour. Test that a source with no session, and a restarted source, both ack.

### R1-C3 — High
**The source-completeness rule has no simulator check or injected bug, and the Go grant state machine is not tied to the Rust model.**
- Evidence: AGENTS.md Principles. The only simulator row (design.md:167) checks "no post-closure new read permit or unpinned source", and its injected bugs are missing-close and premature-pin-release.
- S4 is a new safety rule: read only from an ISR source proven complete, and invalidate on an ISR change, move or re-creation. It is checked only by unit tests ("partial/new store refused", design.md:165).
- The grant, close and ack transitions run in the Go service, but `make meta-vectors`/`contracts` vectors for them are not planned. The simulator's model therefore proves nothing about the Go code that releases pins.
- Fix: add a simulator checker that every block read for an archive equals the committed snapshot at g. Add injected bugs that the simulator must catch: accept a source outside the ISR, and skip the post-batch revalidation. Add metadata transition vectors for the grant lifecycle, generated from the Rust model and replayed by the Go service, as #51 does for the cut.

### R1-C4 — Medium
**No etcd layout or transactions for the new records, and the 120-operation bound is not shown.**
- Evidence: contracts.md:40-60 and design.md:29-31, 99-105 name grants, sources, acks, the reader-closing intent, the completion record and retained-key references. None has a key, a compare set or an operation count.
- Closing needs "every registered source ack". A volume can have chunks × R sources, so comparing them all can exceed 120 (`meta/kv/etcd.go:91-92`).
- The plan does not say whether grant or source writes touch the header. If they do, they move its revision, so ChangeIsr retries and #77's pinned Attach answers `Aborted` (as #51 contracts states for catalog writes).
- `RegisterSnapshotSource` does not say how it establishes #51's evidence: a #77-validated view with the snapshot committed and the node in the ISR at `isr_ver` (#51 contracts, Completeness evidence). decisions.md:58-61 describes catch-up stages instead.
- Closed grants and their source records are never collected.
- Fix: add a records and transactions table, as #51's contracts have:
  - keys under `/flaminio/v1/`;
  - each RPC's compares and writes;
  - a per-grant ack counter or cursor, so closing stays within 120 operations;
  - whether the header is written;
  - registration comparing the header, the chunk record, the catalog record and #61's node record;
  - deletion of closed records.

### R1-C5 — Medium
**The DeleteVolume integration ignores #73's admission and today's unparseable-header branch.**
- Evidence: design.md:49-53 records a "reader-closing intent" before the deletion mark.
- #73 (clear) S5: "A refused deletion writes no mark, and nothing else". Its protection check runs in the mark's transaction. A closing intent written before that check would close a protected volume's backups on a deletion that is then refused.
- `meta/service/service.go:535-551` deletes a header that does not parse without a mark. The plan does not say what happens to its grants.
- The batch's #73 hand-off (batch-2026-11-12.md:222) asks plans that change DeleteVolume to keep #73's compares within 120 operations. The plan does not.
- The "named blocked-reader error" has no gRPC code. CSI retries on it.
- Fix:
  - Write the intent only after #73's admission checks, carrying their compares.
  - Define the unparseable-header case.
  - Give the operation count with #73's extra compares and writes.
  - Name the code (e.g. `FailedPrecondition` naming the source).
  - Test a refused, protected deletion that leaves the grants untouched.

### R1-C6 — Medium
**Encrypted backups cannot happen in this increment, but the plan promises them.**
- Evidence: #51 (clear) S2 and contracts RequestSnapshot refuse a snapshot of an encrypted volume ("no snapshot until its keys are pinned"). #50 (clear) contracts, "Data and retained references": "No encrypted snapshots, clones or backups are admitted until their catalog pins this descriptor".
- #52 nevertheless has, for encrypted volumes:
  - S8 (spec.md:78-84);
  - intent outcome 7;
  - pinning key references at the grant (design.md:29-30, 99-105);
  - retained-key records that outlive the volume, with no layout;
  - ciphertext tests (design.md:170);
  - the lab check "an encrypted volume's backup" (intent.md:121-122, design.md:184).
- Fix: state that encrypted volumes have no snapshot until #51's catalog pins #50's descriptor. Mark S8, the retained-key records, their tests and the encrypted lab check as conditional on that later step, or move them to it. The grant then refuses an encrypted SnapshotRef.

### R1-C7 — Medium
**The reader port 7412 collides with #71's observer.**
- Evidence: #71 (clear) contracts:143-144 has the node observer listening on `:7412` by default, and the CLI uses `--observer-port 7412`. #52 puts its "dedicated replica listener 7412" on every replica node (design.md:35, contracts.md:62).
- The plan also gives no `flaminio-io node` flag for the listener, and does not say how the worker finds its address. `Node.address` holds the replica port (lab `NODE_PORT=7411`, `scripts/lab.conf:107`).
- Fix: pick a free port (e.g. 7413), add a `--reader-listen` flag and say how the address is derived from or published in the node record.

### R1-C8 — Medium
**Reader reads through the store have no epoch rule and no incarnation check.**
- Evidence: every store read takes an epoch, and `check_epoch` refuses `epoch < fenced_epoch` (`flaminio-store/src/store.rs:756-773`; `plan_read` :1901). Each failover's Hello fences a higher epoch (`flaminio-repl/src/replica.rs:173-183`).
- The reader holds no epoch, so the epoch it passes is unspecified. The commit epoch fails after a failover, which breaks outcome 4, "through a failover".
- The store keys a chunk by name hash (`flaminio-node/src/lib.rs:21`). The plan does not say that the node refuses a chunk whose `fenced_epoch` is below `SnapshotRefV1.first_epoch`, as #51's offline reader does (#51 contracts, Offline seam).
- Fix: specify the store entry point the reader uses: a view read admitted by the grant, with no epoch check, or an explicit "at least first_epoch" check. Add the earlier-incarnation refusal and a test reading across a failover.

### R1-C9 — Medium
**`SnapshotRefV1`/`SourceProofV1` fields have no source.**
- Evidence: contracts.md:7-9, 24.
- `cluster_uuid`: the metadata service has no cluster identity. The only one is the per-disk superblock `cluster_uuid` (`flaminio-format/src/superblock.rs:28`), NIL in tests, and no lab or agent path sets it. #54's contracts assume #52 defines it.
- `geometry.size_bytes` at the cut: #51's `Snapshot` record carries no size. After #70 growth, the current header's size is not the snapshot's. The ref changes between grant and resume, and a restore is not "equal to the snapshot".
- `logical_block_bytes` exists nowhere (ublk fixes 4096, `ublk.rs:187`).
- `replica_generation` is defined by neither #51 nor #61. #61 has `Node.store_generation` and ISR `ReplicaRef{node,device_uuid}`.
- Fix:
  - Define the cluster UUID record (created once, conditionally, by whom), or cite #54.
  - Record the size at the cut, or state the rule a pre-growth snapshot is read and restored under.
  - Fix `logical_block_bytes` to 4096.
  - Replace `replica_generation` with #61's fields (ISR `ReplicaRef`).

### R1-C10 — Medium
**The "separately established exclusion proof" that frees a dead source is undefined.**
- Evidence: design.md:43-45, contracts.md:54, spec.md S3. #13 (clear) S19 already has the mark for a node gone for good: forgotten, never registers again.
- Without a defined proof, a source node lost during a backup makes the volume undeletable forever. Its tombstone never comes, and its space and quota are never released.
- Fix: name #13's forgotten mark (and #61's store replacement, if it applies) as the exclusion proof. Compare it in the closing transaction, and add a test where a forgotten source unblocks deletion.

### R1-C11 — Medium
**#73's audit hand-off is not taken.**
- Evidence: batch-2026-11-12.md:222 asks #50-#53 to audit key, snapshot and backup operations through #73's event append, or to record that as a follow-up. #73 S7: "snapshot or backup deletions are audited by the issues that add them".
- intent.md:151 still says the audit log "does not exist yet", and the spec and design record no follow-up.
- Fix: either commit #73 events for AuthorizeSnapshotRead and Close (within the transaction budget), or add a `docs/followups.md` item, "Audit events for backup operations", in the docs list, as #51 did.

### R1-C12 — Medium
**The shared contracts are called normative and accepted, while #51 (clear) holds them pending #58's coordinated review with #52.**
- Evidence: contracts.md:3 says "Normative … Accepted with the plan", and design.md:152-158 says "Exact … contracts". #51 contracts.md:3-9 says the bytes #52 and #58 read are "proposed, not final" until #58's review. #58's settled decision covers #52's reader contract, and batch-2026-11-12.md:290 says "fixed in one coordinated review with #51 and #52".
- Fix: mark `SnapshotRefV1`, `Diff`, `SourceProofV1` and the snapshot-reader/v1 bytes as pending that review, with nothing built before it, and keep the repository format as settled.

### R1-C13 — Medium
**The RPC set and the commands leave steps of the flow unspecified.**
- Evidence:
  - The administrator's `AuthorizeSnapshotRead` and `CloseSnapshotRead` have no `flaminio-meta` command (contracts.md:161-163 lists only `flaminio-backup`).
  - `backup --operation-id` has no RPC that resolves an operation to its grant.
  - "Conditional metadata operation progress" (design.md:120), "Record completion with the exact root version" (design.md:103) and the "retained-key reference" persisted before publication (design.md:99) have no RPC, fields or `auth.go` rule.
- Fix: add the admin commands, and either a `GetSnapshotRead(operation)` RPC or the grant id in the worker's input. Add the completion and abort fields to `CloseSnapshotRead`, or add new RPCs, each with its `auth.go` rule.

### R1-C14 — Medium
**The lab plan is not runnable as written.**
- Evidence: design.md:181-187 names no lab command, `scripts/guest`/`scripts/checks` file, Makefile target, store install step, certificate issuance for the worker's identity, or ordered steps. Intent outcome 12 requires the store "by a lab command" and lists seven checks.
- Fix:
  - Name the targets: e.g. `make backup-store-up`/`backup-check` through `scripts/lab.sh`.
  - Name the scripts and their steps for each of outcome 12's checks: incremental count, store stop and resume, failover, altered object, restore equality, measured load.
  - Give the SeaweedFS install and teardown, and the worker certificate step.

### R1-C15 — Low
**The test plan names no concrete tests, and one rule has none.**
- Evidence: design.md:163-177 describes proofs but names no test files, functions or fuzz targets. S11's "actual last verified restorable-point age" report (spec.md:101) has no test.
- Fix: name the test files and functions, and the `dataplane/fuzz` targets (envelope, page, root, reader frames). Add a test for the reported age.

### R1-C16 — Low
**The worker's privileges are misstated.**
- Evidence: design.md:33 says the worker's role has "no … general reader privileges". `meta/service/auth.go:79` admits any authenticated identity to GetVolume, ListNodes and ListVolumes, and the worker needs GetVolume for its revalidation (design.md:55-57).
- Fix: state that the worker keeps the open reads and uses GetVolume for its revalidation.

## Nits
- contracts.md:72: "u32 big-endian" frames, while the replication frames are `[u32le len][u8 tag]` (`flaminio-net/src/frame.rs:16`, #51 contracts Wire-v2). Say the choice is deliberate, or use little-endian.

Reviewed commit: c6efcd778d462d865277c99094a692c1bc7ee76c
