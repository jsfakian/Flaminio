# #52 plan review, round 4, focus design-code and design-security

Scope: `git diff b2f2b77..8ce0f0b -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
the worktree's `flaminio-repl` (examples, sim), origin/main's `scripts/lab.sh` (`metactl`, `issue`,
`put_file`, `cmd_pki`) and `scripts/guest/pki.sh`, and clear #16's design (origin/main, Installing).

## Earlier findings
- R3-C1: fixed. The sim stops at the mark, keeps one incarnation, and the end-state checks are unchanged. All six mutations keep a check that can fire, and no S-rule loses its simulator check.
- R3-C2: fixed for grants the worker closes. A grant that a deletion's intent closed is still never re-closed in the model (R4-C1). Both halves of `grant_under_closing` can now fire.
- R3-B1: fixed in part. The names, users, sweep patterns, `ca.crt` and the grant-file hand-off are in. New gaps: `file ca.crt` on every VM (R4-B1), the store user's files (R4-C3) and the renewal of `backup-store` (R4-C4).
- R3-S1: fixed. The order is sign, commit with the digest, return; a lost compare re-signs; there is a test.
- R3-S2: fixed. The ack, the close and the exclusion compare the generation, with a test.
- R3-S3: fixed for the throttle. The new key no longer bounds the total number of validations (R4-S1).
- Nit (reopen retried): fixed (design.md:206-208).

### R4-C1 — Low
**Reader progress can still fail on a clean seed, for a grant that an abandoned barrier closed.**
- Evidence:
  - Exclusion runs only in a CloseSnapshotRead of a closing grant and in a deletion waiting on the barrier (contracts.md:230-232).
  - The worker model "retries each of its CloseSnapshotRead calls" (design.md:211-212). It never issues one for a grant that a deletion's intent made closing. Its Register on that grant is refused, and nothing says it then closes the grant.
  - One barrier in four is abandoned (design.md:205-206). The forget event may follow it (design.md:213-215).
  - That gives this sequence: the intent closes grant G, whose source is on node N, which is up. The barrier is abandoned. N goes down and is forgotten. Nobody excludes N's source, so G stays closing. The retried ReopenSnapshotReads is refused while `open` > 0 (design.md:206-208). So "every closing grant closes" and "every reopen succeeds" fail with no bug present.
  - The real CLI has the same gap unless `backup`, refused because its grant is closing, ends with its abort close. The text says "a closing volume's backups end at once" but does not say how.
- Fix:
  - On a Register or Close answer that its grant is closing, the worker, and its model, calls CloseSnapshotRead(aborted) and retries it until `closed` (contracts, CLI).
  - Add a model case and a `failure_matrix.rs` case: an intent-closed grant whose source's node is forgotten after an abandoned barrier reaches `closed`.

### R4-C2 — Low
**The close retry loop has no rule for a repeated close with a different outcome, or for errors that will not clear, so it can loop forever.**
- Evidence:
  - The loop ends only on `closed`, `NotFound` or a generation refusal (contracts.md:421-424; design.md:286).
  - A repeat on a closing grant "first excludes" (contracts.md:169-170). Nothing says what a repeat with another outcome does.
  - Now that both sides retry, such repeats are realistic:
    - the worker's `completed`, root R, retried after an administrator's `--abort`;
    - `snapshot-read close --abort` retried after the worker's `completed`;
    - a `completed` close of a grant that an intent closed with no outcome.
  - If the service refuses these, both commands loop until interrupted. If it overwrites, the recorded outcome and root are lost (R1-C13 makes the grant the completion record).
  - The generation refusal is a `FailedPrecondition`, the same code as the insecure refusal and other refusals. No message is given that would let the CLI tell them apart (contracts.md:166-168, 208-209).
- Fix:
  - A repeated close keeps the first recorded outcome and answers the state with that outcome, never an error. An intent-closed grant takes the outcome of its first close.
  - The CLI retries only the `closing` answer, `Unavailable`, `Aborted` and `DeadlineExceeded`. Any other error exits 1 and names the error.
  - The generation refusal gets a fixed message that the CLI matches.
  - Add cases to `close_retried_until_closed` and `TestSnapshotReadClosure`.

### R4-B1 — Low
**`file ca.crt` into `backup-pki` and `backup-store-pki` would build both directories and both users on every VM.**
- Evidence:
  - The plan adds "`file ca.crt` also to both directories", and building either directory makes its user (design.md:323-333; decisions.md:187-196).
  - Under #16, `file` builds a missing directory (16/design.md, Installing: "for `file`, which has no `RUN`, its pid instead").
  - `cmd_pki issue` runs `put_file "$vm" ca.crt` on every VM of the cluster, first of all (lab.sh:576-577).
  - So `flaminio-backup` and `flaminio-backup-store`, and their 0700 directories, appear on fl-a, fl-b and fl-c. Only fl-w runs the worker and the store.
  - Neither the users nor the directories are removed afterwards.
- Fix:
  - `file ca.crt` writes into a backup directory only if that directory exists. Its `csr` makes it, and `backup-store up` and `pki issue w` already put `ca.crt` after issuing.
  - Alternatively, only on fl-w.
  - Add a `backup-scripts-test.sh` case: `file ca.crt` with neither directory present makes no user and no directory.

### R4-C3 — Low
**Once the store runs as `flaminio-backup-store`, it cannot read its own S3 credential or write its volume, as specified.**
- Evidence:
  - design.md:307-316 now runs the store as `flaminio-backup-store`.
  - The credential files are created as root, 0600, and only "the worker's copy" is given an owner (`flaminio-backup`). The store's copy stays root 0600.
  - The 2 GiB file-backed volume under `/var/lib/flaminio-backup-store` is set up as root and gets no owner. A loop mount needs root, and SeaweedFS writes to that directory.
  - The tests cover the pairs and the grant file, not these files (design.md:368-373).
- Fix:
  - The store's copy of the credential is created owned by `flaminio-backup-store`, 0600.
  - The mounted volume's root is chowned to `flaminio-backup-store`, 0700.
  - Add `backup-scripts-test.sh` cases for both owners.

### R4-C4 — Low
**The new `backup-store` name is in `pki.sh`'s list but not in `identities VM`, so #16's renewal never renews it.**
- Evidence:
  - #16's renewal runs `pki.sh due` only over `identities VM`'s names (16/design.md, `lab.sh pki renew`).
  - The plan adds `backup` to fl-w's identities (contracts.md:72-73). `backup-store` is issued only by `backup-store up` (design.md:317-322).
  - #16's lifetimes run from 30 days down to 30 minutes in its rotation runs (`CERT_LIFE=30m`). After that, the store's certificate expires.
  - The worker then refuses the store (`store_identity_required`), until someone runs `backup-store up` again by hand.
- Fix: add a fl-w line `backup-store CA ID --server --ip`, whichever of #16 and #52 lands second (`csi-controller` is already a line that `pki issue` skips). Or state that the store is outside renewal and must be re-issued, with that limit in docs/lab.md.

### R4-S1 — Low
**Keying the validation rate per (grant, node, chunk) removes any total bound on the validations that a node's ValidateSnapshotRead calls send to the metadata service.**
- Evidence:
  - A worker that cycles sessions across chunks now validates at the rate its sessions turn over. Turnover is bounded only by the session caps (2 per grant, 16 per node), not by the rate.
  - For a 4096-chunk volume that is up to about 2 × 4096 / 10 s ≈ 800 linearizable reads per second per grant and node, against 0.2 before.
  - "at most 2 validations per registered source in 10 s" (contracts.md:291-296) is true, but it is no longer a defence against exhaustion.
  - These calls run on the node's MetaClient, which runs one call at a time. The one-outstanding rule names only the poll's pages and acks (contracts.md:217-220). So a ChangeIsr can wait behind many queued validations, which is the case R2-C3 bounded.
  - Only an authorized `backup` identity with registered sources gets this far (contracts.md:286-287), so it is liveness, but on the write path's metadata client.
- Fix:
  - Keep the per-chunk key, and add a per-(grant, node) token bucket sized to a legitimate walk (for example a burst of 2 and a few per second; proposed, to be measured).
  - State that ValidateSnapshotRead is a reader call under the one-outstanding rule: a Hello that would queue a second one is `Refused` (retry later).
  - `validation_rate_bounded`: a worker cycling sessions over many chunks is refused past the bucket, and a ChangeIsr waits behind at most one validation.

## Nits
- design.md:349-351 and decisions.md:195 install with `-g flaminio-backup`, but the `useradd` line (design.md:330-331) relies on the distribution's USERGROUPS_ENAB default for that group. Pass `--user-group`.

Reviewed commit: a25d9f9b394b63223552a1b1672674c48ea7af8e
