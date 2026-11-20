# #52 plan review, round 3, focus design-code and design-security

Scope: `git diff ca86b81..96351c0 -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
the worktree's `flaminio-repl` (sim, meta.rs), `meta/service`, `pkg/pki`, origin/main's
`scripts/lab.sh` and `scripts/guest/pki.sh`, and the clear plans #13, #16 and #73 (origin/main).

## Earlier findings
- R2-C1: fixed in part. The events, the exclusion and reader-progress checks and the mutation table are in place. The new events have no incarnation model in the simulator, and reader progress can fail clean (R3-C1, R3-C2).
- R2-C2: fixed (ReopenSnapshotReads; S3 amended; risk text corrected; TestReopenSnapshotReads).
- R2-C3: fixed (one reader call outstanding, as #13's sweep; `reader_calls_one_outstanding`).
- R2-C4: fixed (`READER_LISTEN` in `node_env`/`node.sh`, a script test, #12 hand-off).
- R2-C5: restore fixed. Store pair: issued from the host now, but `pki.sh` under #16 refuses the name (R3-B1).
- R2-C6: fixed (`snapread/<v>/` on all three paths; the lowest generation).
- R2-C7: fixed (List, then one Get per grant; the cost stated; mixed-state paging test).
- R2-C8: fixed (the poll's `node N is forgotten` closes sessions; a test).
- R2-C9: fixed (one model in `meta.rs`; `reader.rs` holds the node and worker sides).
- R2-S1: fixed in part. Digest, generation `next`, `AlreadyExists` and ValidateSnapshotRead's compares are in, but the registration order contradicts the digest (R3-S1), and acks do not compare the generation (R3-S2).
- R2-S2: fixed (authenticated-only caps, pending bound, deadlines, validation rate). The rate's key throttles normal backups (R3-S3).
- R2-S3: fixed for the worker's check (`store_identity`, test); the lab issuance is R3-B1.
- R2-S4: fixed in part. `flaminio-backup` user and `backup-pki` are named, but the user, the CA and the grant file are not provided for (R3-B1).
- R2-S5: fixed (as R2-C2).
- Nits: spec S2 wording fixed; `node-fault` wording fixed; ack of an active grant refused, fixed; worker URI form, fixed.

### R3-C1 — Medium
**The simulator's delete and re-create events have no incarnation in the model they drive, and the plan does not say what a deletion does to the target, the replicas, #13's sweep or the run's end state.**
- Evidence:
  - design.md:193-205 deletes VOL at `META`, re-creates it "at a new first epoch", and checks archive equality "of the incarnation its grant names" (design.md:209-210).
  - `flaminio-repl` has no first epoch outside `Metadata::create_volume`. `grep first_epoch|incarnation` in `flaminio-repl/src` finds nothing. The sim's replicas hold chunks keyed by `(VOL, c)` with no fence by incarnation (`sim/mod.rs:388-395`). So after a re-creation, the earlier incarnation's copies still answer `replica.block(VOL, c, b)`, which the final divergence check reads (`sim/mod.rs:1355-1364`).
  - `run_mut` and `check` take `meta.volume(VOL).expect("volume")` and do final reads through the settled target (`sim/mod.rs:497-540`, `:1346`). A deletion that reader progress requires to complete in settle (design.md:211-213) leaves no volume, unless re-creation also runs in settle, and the plan does not say that.
  - Clear #13's sim sets the sweep's `deleted_since` to false because "the sim deletes no volume" (13/design.md, The simulator). After #52's events that no longer holds, and the plan does not say how `release::may_delete` sees a deletion.
  - Nothing says whether the target is stopped or detached before DeleteVolume, or what its in-flight ChangeIsr and the replicas' sessions do afterwards.
- Fix: in Events, state:
  - deletion detaches and stops the target first (as CSI does);
  - the earlier incarnation's copies are removed through #13's sweep, with `deleted_since` true across the deletion, or the sim's `Replica` gains the first-epoch fence the store has;
  - history and the divergence check are per incarnation;
  - a deletion is requested only during chaos, and its re-creation is scheduled even in settle, so that the final reads and `check()` run on a live incarnation.

### R3-C2 — Low
**"Reader progress" can fail on a clean seed. A closing grant whose source is on a node forgotten after the worker's Close closes only when someone calls Close again, and neither the worker nor its model does.**
- Evidence:
  - Exclusion runs only in "a CloseSnapshotRead of a closing grant and a deletion waiting on the barrier" (contracts.md:220-222).
  - The worker closes once (design.md:139-141).
  - The forget event takes a node that is down (design.md:204-206), so it can hold a source of a grant its worker has already closed.
  - In settle nothing re-runs Close. The grant stays closing, and the check "every closing grant closes" (design.md:211-213) fails without any bug.
- In the real system the same grant keeps `open` raised and its snapshot pinned until an administrator re-runs `snapshot-read close`.
- A related gap: `grant_under_closing`'s grant half fires only if the worker model calls Authorize while a barrier is open or after a reopen. The model authorizes only after a re-creation (design.md:200-201).
- Fix:
  - The worker, and its model, retries CloseSnapshotRead with backoff until `closed`. Or a node's ForgetNode-independent path, e.g. AuthorizeSnapshotRead, excludes the closing grants' forgotten sources.
  - The worker model also authorizes during a barrier and after a reopen.
  - Test: a grant closed before its source's node is forgotten reaches `closed`.

### R3-B1 — Low
**The lab's `flaminio-backup` and `backup-store` setup cannot run under #16's clear `pki.sh`, and the worker cannot read its CA or its grant file.**
- Evidence:
  - #16's `pki.sh` "knows the names and files it writes and refuses any other (`unknown name`)" (16/design.md, Installing). The plan adds #16 entries for `backup` only (contracts.md:79-85). design.md:297-301 runs `issue "$VM_W" backup-store ...`, and `pki.sh csr backup-store` is refused.
  - #16 installs `ca.crt` to `/etc/flaminio/pki` and `io-pki` only. `/etc/flaminio/pki` is root 0700 (`pki.sh:8-10`). So the worker, running as `flaminio-backup`, cannot read the CA it needs for the metadata service, the nodes and the store. The plan installs no `ca.crt` into `backup-pki`.
  - No step creates the `flaminio-backup` system user. #16's `chown` of `backup-pki`, and `backup-store.sh`'s chown of the credential, need it (#16 uses `groupadd -f --system` for its group).
  - `metactl` runs `flaminio-meta` under `sudo` (`lab.sh:529-534`), so the grant file is created exclusive 0600 by root (contracts.md:402-404), and the worker cannot open it.
- Fix:
  - Add #16 entries, whichever lands second: `backup-store` (its directory owned by the store's user), and `ca.crt` into `backup-pki`.
  - Create `flaminio-backup` idempotently (`useradd --system`) in `pki.sh` before its first `chown`, or in `backup-store.sh up`, run before `pki issue w`.
  - `backup.sh` hands the grant file to `flaminio-backup` (`sudo install -o flaminio-backup -m 0600`).
  - Add `backup-scripts-test.sh` cases.

### R3-S1 — Low
**RegisterSnapshotSource's order contradicts the digest it now stores.**
- Evidence: contracts.md:137 says "One transaction registers the source BEFORE the token exists". contracts.md:140 says "The record keeps the issued token's `token_digest`". A digest cannot be written before its token exists.
- An implementer following the first rule cannot satisfy R2-S1's binding. One who signs after the commit needs a second write, and that write leaves a window where the record holds no digest.
- Fix:
  - The service signs the token, commits the record with its digest in that one transaction, and returns the token only after the commit.
  - A lost compare re-signs, and a token whose transaction did not commit is never returned.
  - Correct line 137.

### R3-S2 — Low
**Acks and exclusions do not compare the grant's generation, while a reused operation id now shares source keys across generations.**
- Evidence:
  - Source keys are `snapsrc/<node>/<v>/<fe>/<op>/<chunk>`, with no generation (contracts.md:96).
  - An operation id may be reused after collection, giving a new generation under the same `<op>` (contracts.md:101-104).
  - AckSnapshotReadClosed refuses only an `active` grant (contracts.md:165-170). Its compares are the grant and the source record "at the revision read" (contracts.md:253), not the request's generation. The exclusion batch is the same.
- A delayed or retried ack for generation g can therefore delete generation g+k's source record, if that grant is closing, when that node never closed g+k's admission. It lowers `pending`, and the grant can close, releasing its pins, while that node still serves g+k's session.
- Fix:
  - Ack and exclusion refuse (`FailedPrecondition`) a request whose generation is not the grant record's, or the source key carries the generation.
  - Add a `TestSnapshotReadClosure` case: an ack of an earlier generation under a reused operation id.

### R3-S3 — Low
**The validation rate is keyed per (grant, node), but each session validates one chunk's token, so a normal backup is throttled to 0.2 sources per second per node.**
- Evidence:
  - ReaderHello carries one source-scoped token (contracts.md:287-292), and ValidateSnapshotRead is per chunk, mandatory per new session (contracts.md:144-146).
  - "at most 2 ValidateSnapshotRead calls per (grant, node) in any 10 s, over which a Hello is `Refused`" (contracts.md:277-279).
  - An incremental backup diffs every chunk. A 1 TiB volume of 256 MiB chunks (ARCHITECTURE §Chunk) means 4096 sessions. Over 3 nodes that is about 6800 s spent only on admission, and a full backup is capped near 51 MiB/s per node.
- Fix:
  - Key the bound per (grant, node, chunk), which still bounds a cycling worker per source.
  - Or let a session present successive source tokens, each validated once.
  - `validation_rate_bounded` asserts that a worker walking N chunks is not refused.

## Nits
- design.md:202-203: "a ReopenSnapshotReads follows" is refused while `open` > 0, which is just after the intent. Say that the model retries it until the grants close.

Reviewed commit: b2f2b77ecc4b81187343a63041a51c8cd7decefc
