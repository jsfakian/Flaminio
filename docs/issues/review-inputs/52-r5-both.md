# #52 plan review, round 5, focus design-code and design-security

Scope: `git diff a25d9f9..e12f857 -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
`flaminio-metaclient/src/lib.rs` (one thread, FIFO), `meta/service/service.go` (error codes),
origin/main's `scripts/lab.sh` (`cmd_pki`, `issue`, `put_file`) and `scripts/guest/pki.sh`, and
clear #16's and #13's designs (origin/main).

## Earlier findings
- R4-C1: fixed in the CLI (a closing Register answer ends `backup` with its abort close, and a refused or closed session is followed by a re-registration). Not fixed in the worker model, which lacks the session rule (R5-C1).
- R4-C2: fixed in part. The first outcome is kept and answered, and the retry set and terminal errors are listed. New gaps are in the exit rules (R5-C2).
- R4-B1: fixed (`file` never builds the two directories; there is a test). The `ca.crt` re-put after `csr backup` is untested (R5-C4).
- R4-C3: fixed (the owners are given, with tests).
- R4-C4: fixed (the fl-w `backup-store` line, skipped by `pki issue` and renewed by #16; there is a test that it is listed).
- R4-S1: fixed. There is a node-wide bucket, and validations join the one-outstanding rule. The refusal it adds now triggers a re-registration (R5-B1).
- Nit (`--user-group`): fixed.

### R5-C1 — Low
**The worker model reaches a closing grant only through its next registration, so a grant with every source already registered still never closes after an abandoned barrier.**
- Evidence:
  - The model "registers the next source of each grant it holds" and closes `aborted` only when "a registration [is] refused because its grant is closing" (design.md:215-219).
  - The CLI also re-registers after "a reader session the node refuses or closes" (contracts.md:447-450). That is the only route once no source is left to register.
  - R4-C1's sequence needs N's source to be unacked, so N goes down before its poll. N's sessions then break. With every source of G already registered (small volumes do this within a few 5 s steps), the model never registers again. G stays closing, and "every closing grant closes" fails on a clean seed.
  - `reader_progress_after_abandoned_barrier` (design.md:291) does not say whether G has a source left.
- Fix:
  - Give the model the CLI's rule: a session that breaks or is refused is followed by a registration of that chunk.
  - Script the sim test with every source of G registered before the intent.

### R5-C2 — Low
**The close's exit rules give an aborted `backup` exit 0, ignore the root, and do not cover a grant that is already `closed` with no outcome.**
- Evidence:
  - The rule is "`closed` with the caller's outcome exits 0" for `backup`, its abort and `snapshot-read close` (contracts.md:442-443). So a `backup` that ended with its abort close (R4-C1 path) exits 0. That contradicts "Exit ... 1 validation/auth/store/corruption/incomplete" (contracts.md:465).
  - Only the outcome is compared. A `completed` answer carrying another root, which is possible since the first root is kept (contracts.md:173-176), exits 0.
  - An intent-closed grant whose sources all ack reaches `closed` with no outcome (contracts.md:184-185). Register then answers `is closed` (contracts.md:146-147). Only `is closing` is given a reaction (contracts.md:446). Whether a close of a closed grant with no outcome records the caller's outcome is unstated (contracts.md:175-176 covers "closing" only). Then "caller's outcome / the other one" has a third case.
  - `verify --compare-source` also registers sources under a live grant (contracts.md:457-458). It gets no R4-C1 rule, so its grant can stay closing the same way.
- Fix:
  - `backup` exits 0 only when the answer is `completed` with its own root. Its abort path always exits 1 (incomplete), naming the reason.
  - A closed grant with no outcome takes its first close's outcome, as a closing one does.
  - `is closed` ends `backup` like `is closing`.
  - Apply the same end to `verify --compare-source`.
  - Add cases to `close_retried_until_closed`, `closing_grant_aborted` and `TestSnapshotReadClosure`.

### R5-B1 — Low
**Every throttle refusal now costs a Register write, because the worker cannot tell "retry later" from closing.**
- Evidence:
  - R4-S1 adds two reasons to answer `Refused (retry later)`: the node-wide bucket, and any Hello while a reader call (a poll page, an ack, another validation) is outstanding (contracts.md:228-229, 304-306).
  - R4-C1 makes the worker follow any refused or closed session with a RegisterSnapshotSource before it retries (contracts.md:447-449).
  - `Refused=69` carries no reason (contracts.md:331). So each such refusal becomes a signing plus a 7-compare etcd write (contracts.md:277), a new TLS handshake (`Refused` closes the connection, and there is no resumption), and a token replacement.
  - A worker opening its 16 sessions at once serializes through refusals. No backoff is given for "retry later".
- Fix:
  - `Refused` carries a reason: `retry` (caps, bucket, outstanding call) or `closed` (grant closing or closed, token invalid).
  - The worker re-registers only after `closed` or a session the barrier closed. It retries a `retry` refusal with the same token after a jittered backoff (proposed, for example 1 s doubling to 10 s).
  - Cases in `validation_rate_bounded` and `closing_grant_aborted`.

### R5-C3 — Low
**A Rust-side match of a Go error message is the only route from a closing Register to the abort close, and nothing ties the two literals together.**
- Evidence:
  - The worker "matches" `grant <op> is closing` inside a `FailedPrecondition` (contracts.md:146-147). The same code also carries Register's per-source refusals (not in the ISR, stale `isr_ver`, forgotten node, #61 mismatch) and the insecure refusal.
  - `TestSnapshotReadClosure` tests the Go text, and `closing_grant_aborted` tests the worker against its own fake (design.md:289, 293). A wording drift on either side passes both tests and silently reopens R4-C1.
  - The close's generation-refusal match (contracts.md:439-441) is only diagnostic: the command exits 1 either way, so that one is robust.
  - Clear #13 carries a machine-readable cause in a field (`MoveReplicaResponse.reason`), not in the message.
- Fix (either):
  - State that every terminal failure of `backup` after its grant is authorized ends with its abort close, retried until `closed`. The match then only shortens the path.
  - Or carry the refusal's kind structurally (a `google.rpc.ErrorInfo` reason), with one test vector both sides read.

### R5-S1 — Low
**The new closing refusal of RegisterSnapshotSource is not ordered after the worker check.**
- Evidence:
  - "A grant not active is refused before any signing" (contracts.md:146) is not placed relative to "a grant of another worker answers `NotFound`, revealing nothing (R1-S10)" (contracts.md:150-151).
  - Checking state first tells another `backup` identity that holds an operation id whether that grant is closing or closed.
  - `TestRegisterSnapshotSource` has "another worker `NotFound`" but no case of it on a closing grant (design.md:288).
- Fix:
  - The worker check runs first: another worker gets `NotFound` whatever the grant's state.
  - Add that case.

### R5-C4 — Low
**The `ca.crt` re-put that R4-B1 now depends on has no test.**
- Evidence:
  - With `file` skipping missing backup directories, the worker gets its CA only from "`pki issue` puts `ca.crt` on fl-w again after its `backup` line" (design.md:333). `cmd_pki issue` puts `ca.crt` before any `issue` (lab.sh, `cmd_pki` `issue)` branch).
  - The extracted `cmd_pki` test checks only `backup-store` listed and not issued (design.md:392-393). Without the re-put, the worker would start with no `ca.crt`, which only the lab would show.
- Fix: in the extracted `cmd_pki` test, assert a `file ca.crt` on fl-w after `csr backup` and its `install`, and none on another VM after it.

## Nits
- contracts.md:171-172: say which `<g>` `grant <op> generation <g> is not current` names (the request's), and contracts.md:443-444's "`NotFound`, that it was collected" should also cover "or deleted with its volume".

Reviewed commit: f4a3f1c1fd89e06cecdcdbdd9e9a4c403a8a1a5f
