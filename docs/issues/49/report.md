# Issue #49 — report

## Review rounds

### Plan review round 1 — dde1d79

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R1-C1 — High:** a later segment's first operation was not specified, so a write lost at the
  failover could pass (if every segment initializes, its first write overwrites the loss; if
  not, only a read that happens to be served first sees it). Evidence: `spec.md:33` (S2),
  `design.md:33-35`, `design.md:80`; the only mandatory read is the run's final one (`spec.md:58`,
  `:73-74`); protocol §9 (`docs/replication-protocol.md:352-353`, `sim/mod.rs:1385-1392`).
- **R1-C2 — High:** nothing made the kill land while history operations were in flight; with one
  burst per second and an unsynchronized SSH kill it almost never would, and the design's
  "outstanding throughout the kill-call interval" would fail most rounds. Evidence:
  `design.md:33-35,44,104-107`, `scripts/lab.sh:1455` (origin/main), `spec.md:71-72`.
- **R1-B1 — Medium:** two things were called the run manifest, and `chaos-heal`'s input had no
  format, location or validation. Evidence: `design.md:97,112-116`; `contracts.md:48-55` defines
  only the binary `FLRUN001`.
- **R1-S1 — Medium:** no rule said which volume the run may overwrite, nor checked that the device
  serves it: `VOL=pvc-… make chaos` would detach that volume and overwrite its first block.
  Evidence: `design.md:21-22`; `scripts/lab.conf:108`; `scripts/lab.sh:711-715,1431,1435`; #9's
  S7 and #84's own volumes.
- **R1-B2 — Medium:** the tracefs capture traced to no spec rule, was not cleaned up by S13 or
  `chaos-heal`, and its instance could clash with #84's. Evidence: `design.md:105-108,112-114`,
  `spec.md:93-97,142-143`, `origin/main:docs/issues/84/design.md:233-235`.
- **R1-C3 — Medium:** values were not unique across segments: each segment's client started its
  write IDs and op IDs with no allocation rule. Evidence: `design.md:21,25-27`,
  `contracts.md:27`.
- **R1-C4 — Medium:** who writes End and how a segment is sealed was unspecified, with two writers
  on one gapless sequence, late Returns changing the sealed hash, and End misaligned after a torn
  frame. Evidence: `contracts.md:17-34,41-44,49,52`; `design.md:48-51,80-81`.
- **R1-C5 — Medium:** S8's violation kind had no definition and no JSON field, and the faults had
  no data path to the report. Evidence: `spec.md:61-63,107-113`; `design.md:81-85`;
  `sim/check.rs:46-57`; `contracts.md:29`.
- **R1-C6 — Medium:** S10's two new failures (an error while the target was alive; no answer
  between the last heal and the kill) had no mechanism. Evidence: `spec.md:22,80-82,88`;
  `design.md:71,104`; `contracts.md:22`.
- **R1-C7 — Medium:** S16 "in CI" had no job that can run it: CI runs `cargo test` and `make race`
  separately, not `make test`. Evidence: `design.md:128`; `.github/workflows/ci.yml:40,50`;
  `Makefile:63-65,121-128`.
- **R1-C8 — Medium:** the lab plan did not say how it runs once #83 lands. Evidence:
  `origin/main:docs/issues/83/design.md:32-42,172-176`;
  `origin/main:docs/issues/84/design.md:224-227`; `design.md:137-142`.
- **R1-C9 — Low:** "std only" and "no new dependency" did not fit the CRC32C and SHA-256 the
  contract needs, and tests in an example do not run. Evidence: `contracts.md:9,17,49,55`;
  `dataplane/crates/flaminio-repl/Cargo.toml`; `design.md:61`.
- **R1-C10 — Low:** the Go tool's place (`cmd/`, `internal/`), lint coverage, `go.mod` and
  deployment did not match the repository. Evidence: `Makefile:88,119`; `go.mod:63`;
  `scripts/lab.sh:501-505`.
- **R1-C11 — Low:** header fields `volume_epoch` and `node` had no source or use, and `--run-id`'s
  encoding was unsaid. Evidence: `contracts.md:10-13`; `design.md:80`.
- **R1-C12 — Low:** the 100,000-operation cap failed long runs for no finding. Evidence:
  `design.md:44`; `Makefile:279`; `scripts/lab.sh:788-794`.
- **R1-C13 — Low:** a run could pass with the faults never touching the checked chunks. Evidence:
  `design.md:54-56`; `scripts/lab.sh:1256-1258,1264-1305`.
- **R1-C14 — Low:** "lacks a segment" (S7) was not decidable from the contract. Evidence:
  `contracts.md:48-49`; `scripts/lab.sh:1470`.
- **R1-C15 — Low:** the test plan named one file, and S18 had no test for `write-load.sh`'s
  default. Evidence: `design.md:126`; `scripts/lab.sh:753,773,1275,1291`.
- **R1-C16 — Low:** the issue's audit follow-up (#76–#84) was not recorded in full; #78, #79 and
  #82 were missing. Evidence: `design.md:141-143`; `origin/main:docs/issues/82/design.md:153-155`,
  `spec.md:93-95`.
- **R1-S2 — Low:** nothing said the kept and uploaded files hold no credential, though the
  harness handles the attach token. Evidence: `design.md:132,146`; `scripts/lab.sh:715-718`;
  `contracts.md:13`.
- **Nit:** `design.md:149-150` said Q1–Q5 remain proposed; they were settled.
- **Nit:** `spec.md:37` said "monotonic clock"; the design and contracts use `CLOCK_BOOTTIME`.
- **Nit:** `design.md:43` bounded a frame at 1 KiB; the contract fixes 136 bytes.

Changes/resolution:

- **R1-C1:** S2: only the run's first segment initializes; each later segment first reads every
  block, answered before it writes the block. S7 makes a later segment without those reads
  undecided; contracts (Opening reads) say how the checker recognizes them. Design: the opening
  read closes the old segment's deadlines. Tests: acknowledged write in segment k, older value
  read first in k+1 (lost); S16's lost write at a segment change.
- **R1-C2:** design, Kill phase and kill helper: `chaos.sh kill-node` on the target's VM signals
  the run's client (SIGUSR1), which runs its bursts back to back (at most 1,000) and creates
  `kill-ready` once one is issued; the helper then writes its kill mark and sends SIGKILL, with no
  SSH round trip between. Evidence from the history: `in_flight_at_kill` ≥ 1 per killed segment,
  or the run fails. Test with the controlled blocking backend: `kill-ready` only with an
  operation outstanding.
- **R1-B1:** contracts.md gains the heal ledger (format, table of kinds, VMs, unit patterns,
  fixed rule tag and qdisc handle, validation); its only copy is on the workstation ("remote"
  dropped). Design: `chaos-heal` checks `RUN_DIR` and refuses a bad ledger before any remote
  command; script test.
- **R1-S1:** S9 bullet and design: `VOL` refused before anything else when invalid or `pvc-`,
  `bench-`, `bc-`; `node.sh attached` (#9's) must name `VOL` before each client and fio job.
  Script tests.
- **R1-B2:** the tracefs capture and its root prerequisite are dropped; the in-flight evidence is
  the history's (closed with R1-C2).
- **R1-C3:** design and contracts: the high 32 bits of `op_id` and a write's value ID are the
  segment ID; another segment's is malformed. Test: two segments' clients on one file.
- **R1-C4:** design: three named steps (client stopped; `flaminio-history end` under the file's
  `flock`, truncating a torn tail, appending End, fsyncing; `flaminio-history seal` writing
  `FLRUN001`). Contracts: End is the last frame; late Returns are not kept. Tests: torn tail then
  End decodes; End refused while the client holds the lock.
- **R1-C5:** design, Kinds: the four kinds defined on the history, any other violation reported as
  a stale read; `kind` in the verdict JSON; the faults come from the harness's marks on the
  target's clock, context only.
- **R1-C6:** the kill mark is in End (offset 40); the checker lists error Returns before it (or
  any, without a kill), and the harness fails the run on them. Progress: two reads of
  `flaminio-history status` on the target after the last heal, the second larger. Script tests.
- **R1-C7:** S16 runs in `scripts/dev/history-lies.sh` under `make contracts`; the Go lie test is
  behind the `historylies` build tag and fails, never skips, without the checker.
- **R1-C8:** design Lab and decisions.md: on the lab VMs before #83 lands, afterwards only on a host
  #83 admits, with no bypass.
- **R1-C9:** decoder, slicer and classification in `sim/history.rs` with tests, the example a thin
  `main`; CRC32C hand-written with RFC 3720 vectors; the sealed SHA-256 replaced by the sealed
  length (cut before adding), so `std` only holds.
- **R1-C10:** `control/cmd/flaminio-history`, `pkg/blockhistory`; `go.mod` makes `x/sys` direct;
  `history_build` and a copy as `meta_build`/`cmd_meta up`.
- **R1-C11:** `volume_epoch` dropped; `--node` flag (context only); `--run-nonce` as 64 lowercase
  hex digits.
- **R1-C12:** the per-run operation cap replaced by the 128 MiB artifact cap alone.
- **R1-C13:** S9 bullet and design: at each round's start every history block's chunk has its
  replicas on the target and the faulted node, or the round fails. Script test.
- **R1-C14:** contracts: segment IDs 1..n in order; the harness checks n = ROUNDS + 1 (+1 with
  the move back). Script test with a segment left out.
- **R1-C15:** the design's test table names a file and its cases per rule; `write-load.sh`'s
  default case in `scripts/tools/node-scripts-test.sh`.
- **R1-C16:** the intent's Out of scope lists #76–#84 as later steps of #49, with #82's
  live-partition against power-off test and #83's qualified host. The issue's comment itself
  could not be read here (the GitHub API lists none on #49); the list follows the review.
- **R1-S2:** S14: nothing kept or uploaded holds a token, key or journal excerpt; design says what
  is kept; script test greps the kept files for the fixture's token.
- **Nits:** all three fixed (wording).

Rules narrowed: none. Risks named, not accepted: see decisions.md, Changes after approval — plan
review round 1.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — b026fe8

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan reviewer, design-code and design-security focuses together.

- **R2-C1 — Medium:** the kill helper signalled the client's unit, whose main process under
  `run_detached` is a bash wrapper: SIGUSR1 ends the wrapper, the unit stops and the client takes
  SIGTERM, so `kill-ready` never appears; and a SIGKILLed client unit never writes `.rc`, so
  `run_detached`'s poll spins forever. Evidence: `design.md:199,214-215`;
  `origin/main:scripts/lib.sh:171-173,177-194`; `scripts/guest/write-load.sh:9`.
- **R2-C2 — Medium:** `in_flight_at_kill` counted an operation answered with success between the
  mark and the SIGKILL, and the Kill phase called "an instant" what are two fdatasync windows per
  burst with nothing outstanding. Evidence: `design.md:47-49,57-58,115-116,200-201`;
  `spec.md:157-158`; `decisions.md:114-115`.
- **R2-C3 — Low:** `--node` accepted only `^fl-[a-z]+$`, the lab's default names, though VM names
  are overridable and S17 names no lab node. Evidence: `design.md:128`;
  `origin/main:scripts/lab.conf:31,40,51,61`; `spec.md:126-128`; `contracts.md:11`.
- **R2-C4 — Low:** the 128 MiB per-run cap, the only bound on the workload, could not be enforced
  by a client, which sees only its own segment, and the checker did not refuse a run over it.
  Evidence: `design.md:34,66-68,126-128`.
- **R2-C5 — Low:** `chaos-history-test.sh` was not added to `make lint`, which runs script tests
  by name, so the S9–S15 tests would never run in CI. Evidence: `design.md:233-235,243`;
  `spec.md:149`; `origin/main:Makefile:92-94`.
- **R2-S1 — Low:** `chaos.sh slow on` still deletes whatever root qdisc wg0 has. Evidence:
  `design.md:185-187,216`; `contracts.md:90`; `origin/main:scripts/guest/chaos.sh:7`.
- **Nit:** the kinds' bullets overlap, so the kind was not deterministic. Evidence:
  `design.md:142-144`.
- **Nit:** a value whose write was invoked after the read was answered was called S6's foreign
  content, which S6 does not cover. Evidence: `design.md:146-147`; `spec.md:55-56`.
- **Nit:** ledger lines carry no sequence; how an `intend` matches its `healed` line was unstated.
  Evidence: `contracts.md:84`.

Changes/resolution:

- **R2-C1:** design: the client writes `client-pid` in its segment directory once it holds the
  history file's `flock`; `chaos.sh kill-node` takes the segment directory and signals that PID
  alone, after checking it is live, is the copied `flaminio-history` (`/proc/<pid>/exe`) and holds
  the file's lock (`/proc/locks`), else it refuses and the run fails. Given a unit name,
  `run_detached`'s poll also ends when the unit is inactive with no `.rc` (state read before
  `.rc`), returning 1, a failure unless the harness stopped that unit. Tests: `client-pid` only
  once the lock is held (recorder test); the fake wrapper survives SIGUSR1 and only the client's
  PID is signalled, a bad PID refused, a SIGKILLed unit without `.rc` ends the wait (script test).
- **R2-C2:** design: `in_flight_at_kill` counts operations invoked before the mark with no
  successful Return (none, or an error after the mark); the Kill phase names the two fdatasync
  windows per burst and that a mark in one fails the run. `sim/history.rs` cases: success after
  the mark gives zero, an error after it one. Risk named, not accepted (decisions.md).
- **R2-C3:** design: `--node` validated with `valid_id`'s grammar, at most 128 bytes;
  `main_test.go` accepts a name that is not `fl-…`.
- **R2-C4:** design: the harness passes each client `--max-bytes` (128 MiB less the earlier
  segments' sealed lengths and the other kept files); the client reserves a batch's Invokes,
  Returns, a Checkpoint and End before issuing it. Contracts (Run manifest): sealed lengths over
  128 MiB in total are undecided. Tests in the workload test, the script test and
  `sim/history.rs`.
- **R2-C5:** design, Touched: `lint` runs `scripts/tools/chaos-history-test.sh`.
- **R2-S1:** design: `slow on` adds or replaces the `49:` netem only over wg0's default root
  (handle `0:`) or that netem, else refuses, changing nothing, and the run fails. Script test: an
  unrelated root qdisc refused and kept.
- **Nits:** all three fixed (wording): the first matching kind bullet wins; the late-write value is
  reported as foreign content, an addition to S6; an `intend` is healed by the next `healed` line
  of the same kind, VM and unit.

Rules narrowed: none. Risks named, not accepted: see decisions.md, Changes after approval — plan
review round 2.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — ec75178

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R3-C1 — Low:** The SIGTERM before the move back still went to the client's unit, which kills
  the `run_detached` bash wrapper before it writes `.rc`, so the client's exit status (a
  recorder failure, answers outstanding at the end of its 30 s drain) was lost and could not fail
  the run. Evidence: `scripts/lib.sh:171-173` on `origin/main` (the unit's main process is the
  wrapper; bash exits on SIGTERM); design.md:82-84, 236-239; decisions.md, R2-C1.
- **R3-C2 — Low:** After a SIGKILL the client's unit is `failed`, not `inactive`, so a wait for
  "inactive" never ends and the backgrounded `run_detached` loops: the R2-C1 hang again; the
  script test's fake could pass by reporting `inactive`. Evidence: design.md:82, 236-237, 257;
  systemd marks a unit whose main process SIGKILL ended as `failed`; `chaos.sh kill-node` already
  runs `reset-failed` for `flaminio-node`.
- **R3-C3 — Low:** "A kill mark that lands in one [window] gives `in_flight_at_kill` zero" was
  false for most of each window: an Invoke is stamped before its batch's fdatasync, so a mark
  during that fdatasync counts an operation not yet issued. Evidence: design.md:49-50, 59-62,
  68-69, 124-127; decisions.md's round-2 risk repeated the claim.
- **R3-C4 — Low:** `--max-bytes` subtracted "the other kept files", whose sizes are not known
  when a segment starts, and the checker counts only the segment files, so the "total artifacts
  128 MiB" cap was not enforced. Evidence: design.md:70-74, 247-249, 259; contracts.md:72-73.

Changes/resolution:
- R3-C1: before the move back, `chaos.sh stop-client` (given the segment directory, validated as
  for `kill-node`) makes the kill helper's checks and sends SIGTERM to the `client-pid` PID alone,
  so the wrapper survives and writes `.rc`. The client exits non-zero when its recorder failed or
  an answer is still outstanding; unless SIGKILLed, a client's `.rc` must be 0. The unit-level
  signal and the poll's exit on a unit without `.rc` are kept for the SIGKILL alone. Script test:
  `stop-client` signals only the PID; a SIGTERMed client exiting non-zero fails the run.
- R3-C2: the wait at a kill and `run_detached`'s poll end on a unit not active (`inactive` or
  `failed`); the harness then runs `systemctl reset-failed` on it, as `kill-node` does for
  `flaminio-node`. The heal ledger's `client`/`fio` row (contracts) takes the same "not active"
  wording, the same root cause. The script test's fake reports `failed` after a SIGKILL.
- R3-C3: the reviewer's first option, the simpler (no new record): an Invoke is stamped when its
  batch is formed, before its fdatasync and issue, and S9's in flight means invoked in the
  history; `in_flight_at_kill` counts such an operation issued or not. The window sentence is
  corrected (zero only between a drain's last Return and the next Invoke stamps) and the round-2
  risk restated in decisions.md. `sim/history.rs` cases: stamped before the mark and issued after
  it, unanswered, gives one; stamped after the mark gives zero.
- R3-C4: the reviewer's first option, by cutting the term whose value cannot be known:
  `--max-bytes` is 128 MiB less the earlier segments' sealed lengths; 128 MiB is the segment
  files' cap, the contract's rule (its wording now says so). Every other kept file has one stated
  bound together, 8 MiB, checked by the harness after its last write (the run fails, files kept).
  Script-test cases updated and added; the checker's refusal is unchanged.

Rules narrowed: none in the spec's text; R3-C3 states S9's in flight as invoked in the history.
Risks named, not accepted: a mark during a batch's fdatasync counts operations not yet issued, so
`in_flight_at_kill` of at least one does not show the SIGKILL cut an issued operation (R3-C3); a
run whose other kept files pass 8 MiB fails though its history may be sound (R3-C4).
Nits left: none (no Nit raised).

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — 32b9784

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R4-B1 — Medium:** Round 3's "in flight means invoked in the history" narrowed S9, whose
  operations in flight at the kill are issued and unanswered (intent gap 4: reconciliation of
  in-flight writes). Of a back-to-back cycle's four phases only the burst's I/O has a write
  issued; a kill in the other three passed the check with no write at the device, while
  decisions.md said no rule was narrowed. Evidence: intent.md:46-49; spec.md:73-74;
  design.md:59-63, 132-135; decisions.md, round 3's Rules narrowed and risk.
- **R4-C2 — Medium:** The claim that zero comes only between a drain's last Return and the next
  Invoke stamps was false, and the likely case was zero: the mark (a `flaminio-history clock`
  process) and the kill (`sudo systemctl kill`, an exec, `sudo` and a D-Bus call) were tens of
  milliseconds apart, longer than a burst, so operations stamped before the mark were answered
  with success before the SIGKILL, and a sound build failed the run. Evidence: design.md:63-65,
  134-135; `scripts/guest/chaos.sh`:13 on `origin/main`; decisions.md, round-3 risk.
- **R4-C3 — Low:** After `stop-client`'s SIGTERM nothing bounded the harness's wait: a client
  that does not exit (an uncancellable request on a stalled ublk device, a bug) kept
  `run_detached` polling forever, and S13's "named and fails the run" had no trigger. Evidence:
  design.md:87-94; `scripts/lib.sh`:177-193 on `origin/main` (a poll with no deadline).
- **Nit 1:** the `sim/history.rs` case "stamped before the mark and issued after it" cannot be
  expressed in a history that records no issue time. Evidence: design.md:267.
- **Nit 2:** "Only after that SIGKILL does the harness's wait end on a unit that wrote no `.rc`"
  was wrong: the poll ends that way in every case; only after a SIGKILL is it not a failure.
  Evidence: design.md:92-93, 247-249.
- **Nit 3:** the 8 MiB check runs after the report is written, so a run it fails keeps a report
  that does not say why. Evidence: design.md:258-259.

Changes/resolution:
- R4-B1: the reviewer's first option, which keeps S9 as written. After submitting a batch, and
  before reaping any of its completions, the client appends one Issued frame per operation (type
  5, its issue time sampled after the submission returned, no fsync of its own); contracts.md
  defines it (136 bytes, CRC32C as every frame) and the decoder's checks (an Issued for an
  unknown op, of another kind or block, a duplicate, after its Return, or a Return with no
  Issued: malformed). `in_flight_at_kill` counts writes issued before the kill with no successful
  Return; reads, checkpoint reads included, and unissued operations do not count. The budget
  reserves the Issued frames (128 MiB is now about 300,000 operations, some 4 h). Round 3's
  meaning and risk are withdrawn and marked so in decisions.md. Tests: `sim/history.rs` (only
  unissued operations or reads unanswered at the kill give zero; the Issued malformations), the
  workload and recorder tests (the Issued frames' order).
- R4-C2: the reviewer's fix. `chaos.sh kill-node` reads `flaminio-node`'s MainPID (`systemctl
  show`, a nonzero decimal) and runs `sudo flaminio-history kill-at --pid PID --exe … --unit
  flaminio-node.service` (root: the node runs as root). In one process `kill-at` checks, as round
  2 checks `client-pid`, that the PID is live, that `/proc/<pid>/exe` is `node.sh`'s
  `flaminio-io` and that `/proc/<pid>/cgroup` places it in the unit, sending nothing otherwise;
  then it reads `CLOCK_BOOTTIME`, sends SIGKILL with `kill(2)` and reads the clock again. End
  holds both samples (contracts, offsets 40 and 80). `in_flight_at_kill` counts writes issued
  before the first sample with no Return, or an error Return after the second; S10's live-target
  errors are those earlier than the second. Risk text corrected. Tests: `sim/history.rs` with the
  samples further apart than every operation's latency; `main_test.go`, `kill-at`'s one-process
  order and refusals; the script test, the helper's order and a refused MainPID.
- R4-C3: the reviewer's fix. After `stop-client` the harness waits up to 45 s for `.rc`; past
  it, it SIGKILLs the client's unit as at a kill, names the client and fails the run (S13).
  Script test: a fake client that ignores SIGTERM.
- Nit 1: the case is replaced by R4-B1's test.
- Nit 2: reworded: the wait can end on a unit with no `.rc`, returning 1; only after the SIGKILL
  at a kill is that not a failure.
- Nit 3: a run the 8 MiB check fails gets that reason appended to its report.

Rules narrowed: none; R4-B1 restores S9's in flight (issued and unanswered), which round 3's
meaning had narrowed. Risks named, not accepted: a kill while no write is issued and unanswered
(three of a cycle's four phases) gives zero and fails a sound run, how often unknown until a lab
run (R4-B1); `kill-at`'s checks and `kill(2)` are separate steps, a narrower PID-reuse window,
not closed (R4-C2); an error between the two samples fails the run as a live-target error
(R4-C2); `kill-at` runs as root from the copied binary, no wider than the lab user's `sudo`
(R4-C2); the 128 MiB cap now holds about 4 h of paced workload, not 6 h (R4-B1); a SIGTERMed
client needing more than 45 s to exit fails the run (R4-C3). See decisions.md, round 4.
Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — 659b0e0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R5-C1 — High:** Three of each kill-phase cycle's four phases give `in_flight_at_kill` zero,
  so a sound build fails most runs; round 4 named this as a risk instead of designing it away.
  Evidence: design.md:66-70 and decisions.md (round-4 risk 1); with in flight meaning an issued,
  unanswered write, a kill during the burst's Invoke fdatasync, the checkpoint read's fdatasync
  or I/O counts zero; the lab step's 4 rounds must each count one (p^4 per run).
- **R5-C2 — Medium:** The Issued frame assumes a submit/reap I/O model, but the client issues
  blocking O_DIRECT `Pread`/`Pwrite`, so "sampled after the submission returned: never earlier
  than the issue" cannot hold, and the literal reading counts nothing. Evidence:
  contracts.md:49-52, design.md:22, :49-52, :62-63, and the workload and recorder tests
  (design.md:284-285).
- **R5-C3 — Low:** Two decoder rules have no test: an Issued whose kind or block differs from
  its Invoke's, and an End with one kill sample 0 or the second earlier than the first.
  Evidence: contracts.md:53-55, :67-68; the `sim/history.rs` list (design.md:286).
- **Nit 1:** `sudo flaminio-history kill-at` will not find the tool (sudo's `secure_path`), and
  the copy's path is never named. Evidence: design.md:25, :240.
- **Nit 2:** the record table does not say that `value_run_nonce`, `write_value_id` and
  `value_block_index` are zero for Issued. Evidence: contracts.md:28, :31, :52.
- **Nit 3:** `kill-at` as root should refuse a `--pid` that is not a strict positive decimal
  greater than 1 before any `/proc` read, match the unit's whole cgroup path, and could use a
  pidfd, which closes the PID-reuse risk. Evidence: design.md:241-246; decisions.md (round 4).

Changes/resolution:
- R5-C1: the reviewer's fix. `kill-at` takes `--history`, the segment's history file (validated
  as the helper's segment directory; opened read-only with `O_NOFOLLOW`; a regular file owned by
  `SUDO_UID`, since it runs as root). It reads the header, then polls the last 64 KiB of whole,
  CRC-good frames until some Write has an Issued frame and no Return, at most 10 s (past it, it
  sends nothing and the run fails); then it samples, signals and samples. Round 4's risk is
  marked replaced; round 5 names what remains (a write the gate saw answered before the kill
  takes effect; no outstanding write within 10 s; root reading a lab-user file). Tests:
  `main_test.go` (no outstanding write waits and refuses at the bound with nothing sent; one
  outstanding write kills; a torn or CRC-bad last frame is not an Issued; `--history` refusals)
  and the script test's helper order includes the gate.
- R5-C2: the reviewer's fix, not `io_uring`. After its batch's fdatasync each operation's
  goroutine samples the clock, appends its Issued frame through the recorder and enters
  `Pread`/`Pwrite` directly; the stamp is the last point before the syscall. The contract says
  an Issued operation may not have reached the device for that gap, a risk named, not accepted.
  The workload and recorder tests check Issued before its operation's syscall, after its batch's
  fdatasync, and before its Return; `kill-ready` follows the first back-to-back burst's Issued
  frames.
- R5-C3: the `sim/history.rs` list adds an Issued of another kind or block than its Invoke, an
  Issued with a nonzero field it must leave zero, and an End with one kill sample 0 and not the
  other or the second earlier than the first, each undecided.
- Nit 1: the copy is `$HOME/flaminio-history`; every remote call, `sudo` included, names it by
  that absolute path.
- Nit 2: the table's offsets 40, 48 and 80 say "zero for Issued"; the Issued paragraph lists the
  fields it leaves zero.
- Nit 3: `kill-at` refuses a `--pid` that is not a strict positive decimal greater than 1 (at
  most `PID_MAX_LIMIT`) before any `/proc` read, compares the whole cgroup line
  `0::/system.slice/<UNIT>`, and uses `pidfd_open` before the checks and `pidfd_send_signal` for
  the kill; round 4's PID-reuse risk is marked withdrawn. `main_test.go` refuses `--pid` 0, -1,
  1, `+5`, `05`, a non-number, and a cgroup line holding the unit only as a substring.

Rules narrowed: none. Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 6 — a08bad2

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R6-B1 — High:** the gate accepted only a history owned by `SUDO_UID`, but the plan never
  said which uid runs the client, and a client run as root through `sudo` (as fio is) writes a
  root-owned history, so every gated kill would be refused and every killed round fail.
  Evidence: design.md:256-258, decisions.md:330; `run_detached` starts the unit as the lab user
  (`--uid=$VM_USER`, lib.sh:171), `/dev/ublkbN` is `root:disk` and the lab reaches devices
  through `sudo` (write-load.sh:9, bench-writes.sh:99); no test used the real client's
  ownership.
- **R6-S1 — Low:** `kill-at` opened the lab user's path as root before checking its type, so a
  FIFO there would block it outside the 10 s bound, and `O_NOFOLLOW` guarded only the last
  component. Evidence: design.md:258-262, decisions.md:381.
- **R6-C1 — Low:** the tool's copy was never made executable (`cat >` creates 0644), so every
  `sudo "$HOME/flaminio-history" …` would fail with EACCES. Evidence: design.md:25-28;
  `cmd_meta up` works only because `meta.sh` runs `sudo install -m 755` (meta.sh:13).
- **R6-C2 — Low:** the gate relied on each Return being appended as soon as its syscall returns,
  which the design did not require; a buffering recorder would make answered writes look
  outstanding. Evidence: design.md:56-58, :259-262; decisions.md:370-375.
- **R6-C3 — Low:** `kill-ready`'s test still asserted "an operation outstanding at the backend",
  which no longer states the round-5 condition and a correct client can violate. Evidence:
  design.md:74-76, :307; contracts.md:50-56.
- **Nit 1:** "the last 64 KiB of whole frames" is not a multiple of 136. Evidence: design.md:260,
  decisions.md:331.
- **Nit 2:** under `sudo`, `~` and `$HOME` are root's, so "`SUDO_UID`'s `~/.flaminio-lab/…`"
  was ambiguous. Evidence: design.md:256.
- **Nit 3:** PIDs are below `pid_max`, so the bound is 4,194,303. Evidence: design.md:249.

Changes/resolution:
- R6-B1: design.md states the client runs as root (its unit runs `sudo "$HOME/flaminio-history"
  run …`, as `write-load.sh` runs `sudo fio`), so its history, `client-pid` and `kill-ready` are
  root's; `kill-at`'s gate accepts only a regular file owned by uid 0 (the simpler of the two
  rules offered), and says why that is safe (no symlink in any component, protected hardlinks,
  no device node, nothing printed from the file). `end`, `status`, the kill helper's `client-pid`
  read, checks and SIGUSR1, `stop-client`'s SIGTERM and the collection (`sudo cat`) run with
  `sudo`. Tests: `main_test.go` accepts a uid-0 file and refuses `SUDO_UID`'s or another uid's
  (owner injected); the script test asserts the client's unit runs it with `sudo`, and the other
  commands too.
- R6-S1: `kill-at` opens with `openat2`, `O_RDONLY|O_NONBLOCK|O_NOCTTY|O_CLOEXEC` and
  `RESOLVE_NO_SYMLINKS`, then `fstat`; `main_test.go` refuses a FIFO and a symlinked directory
  component at once. Round 5's "a hostile file can at most…" risk is marked corrected and
  replaced by round 6's.
- R6-C1: the copy is `cat > ~/flaminio-history.tmp && chmod 0755 … && mv …`; the script test's
  fake `vssh` asserts the `chmod 0755`.
- R6-C2: each goroutine appends its Return right after its syscall returns, before anything
  else; the recorder buffers nothing (each Issued and Return one `write(2)`). `recorder_test.go`:
  a Return is in the file, read through another fd, before the next batch's Invoke fdatasync
  and with no later append. Round 5's append-latency risk is marked narrowed.
- R6-C3: `workload_test.go`'s case now reads: in the kill phase, `kill-ready` appears only after
  SIGUSR1, once every Issued frame of the first back-to-back burst is in the file.
- Nits 1–3 fixed: the last 481 whole frames (65,416 bytes) at `header_end + 136k`, and a header
  with a bad magic, CRC or payload length over 4,096 refused; the run tree's home from
  `SUDO_UID`'s passwd entry (missing, not a number, 0 or no entry refused); `--pid` at most
  4,194,303 (`PID_MAX_LIMIT` - 1).
- No rule narrowed. Risks named, not accepted (decisions.md, round 6): the client, `end` and
  `status` run as root from the lab user's copy; a root-owned file of the run tree moved to the
  checked path between the helper's check and `kill-at`'s open can at most make it kill or
  refuse; `fs.protected_hardlinks` is systemd's default, not checked by `kill-at`.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 7 — 1e3fba0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R6-B1, R6-S1, R6-C1, R6-C2, R6-C3 and the three Nits fixed.

- **R7-C1 — Low:** nothing said who creates the segment directory; with the client root, a
  directory made by `run` or a `sudo mkdir` is root's, the helper's `kill-mark` write fails and
  every kill round fails. Evidence: design.md:72, :294; `lib.sh:170` makes only `~/.flaminio-lab`.
- **R7-S1 — Low:** the `SUDO_UID` refusals "not a number" and "no passwd entry" had no test.
  Evidence: design.md:272-273 against the `main_test.go` list.
- **Nit:** a blank line ended the test table after the S9/S10 row. Evidence: design.md:338.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R7-C1, the harness
creates the segment directory as the lab user without `sudo` before the client's unit, and
`run` creates only its files and refuses a missing parent, with a script-test and a
`main_test.go` case; R7-S1, `main_test.go` refuses an empty, non-decimal or unknown `SUDO_UID`;
the Nit, the blank line removed. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 8 — c73bc7d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R7-C1, R7-S1 and the Nit fixed; R7-C1's fix does not conflict with S17 (its
temporary file is the device, not the history).

- **R8-B1 — Low:** `run`, as root in a directory the lab user owns, relies on exclusive creation
  of `history`, `client-pid` and `kill-ready`, and no test checks it; an open without `O_EXCL`
  would follow a planted symlink or stale file. Evidence: design.md:73-74, :337, :339.
- **Nit:** nothing created the run directory `~/.flaminio-lab/chaos-<h>/`, and `mkdir -m 0700`
  makes only the segment. Evidence: design.md:73; `lib.sh:170`.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R8-B1, each of the three
files is created with `O_CREAT|O_EXCL`, and `recorder_test.go` refuses an existing file or a
symlink at each name, the target unchanged; the Nit, the harness makes the run directory as the
lab user with `mkdir -p` before the first segment, checked by the script test. No rule narrowed;
no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 9 — 23ade95

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R8-B1 fixed; the round-8 Nit fixed in part, the rest is R9-B1.

- **R9-B1 — Low:** the run directory was made once, "before the first segment", but after each
  failover the next segment runs on another VM, where the segment's `mkdir -m 0700` would fail.
  Evidence: design.md:72-75; spec.md:11, :71-73; the S9/S10 test fakes one VM.
- **R9-B2 — Low:** `kill-ready` is created only after SIGUSR1; nothing said what the client does
  when `O_EXCL` refuses it, and the test read as a check at start. Evidence: design.md:91-92,
  :75-77, :340.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R9-B1, before each
segment, on that segment's VM, `mkdir -p` the run directory and then `mkdir -m 0700` the
segment, as the lab user, with a two-VM script test; R9-B2, a failed `kill-ready` creation stops
issuing and exits non-zero, and `recorder_test.go` sends SIGUSR1 with the name present (a file,
then a symlink). No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 10 — 6911634

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R9-B1 fixed; R9-B2 fixed as worded, its run-level result is R10-B1.

- **R10-B1 — Low:** with a planted `kill-ready`, the helper still went on to the kill, and at a
  kill the harness ignored the client's `.rc`, so "the run fails" on a refused `kill-ready` was
  never enforced or tested. Evidence: design.md:93, :120-122, :263, :342.

Changes/resolution (fixed by the orchestrator, the reviewer's fix): the helper refuses a
`kill-ready` present before SIGUSR1 and, after its wait, requires a regular file owned by uid 0;
at a kill a present, non-zero `.rc` fails the run; the script test covers both refusals and the
non-zero exit. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 11 — e208909

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R10-B1 fixed for the case it named.

- **R11-B1 — Medium:** round 10's "at a kill, a present non-zero `.rc` fails the run" can fail a
  correct kill: the kill's I/O errors may make the client exit before its unit's SIGKILL, and
  the plan left the client's behaviour after its first failed operation open. Evidence:
  design.md:93, :120; `ublk.rs:906-919` (no `UBLK_F_USER_RECOVERY`); `lib.sh:172-173`.
- **R11-B2 — Low:** both new `kill-ready` checks follow symlinks (`sudo test -e`, and an unnamed
  type/owner check), so a dangling symlink, then one to a root-owned file, passes them.
  Evidence: design.md:264-265.
- **Nit:** "either check fails" covered three conditions; the pre-signal refusal "sends nothing
  more" should read "sends nothing". Evidence: design.md:265-266.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R11-B1, after
`kill-ready` the client never exits on its own (it stops issuing, keeps its answers and the
`flock`, and waits for a signal), and at a kill any present `.rc` fails the run, with tests in
`recorder_test.go` and the script test; R11-B2, `test -e || test -L` before SIGUSR1 and
`stat -c '%F:%u'` without `-L` after the wait, with script-test cases; the Nit, reworded. No rule
narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 12 — 33d9433

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R11-B1 fixed in the rule, its test weak (R12-B2); R11-B2 and the Nit fixed.

- **R12-B1 — Medium:** with the client never exiting after `kill-ready`, a kill-phase recorder
  failure no longer failed the run, and an unrecorded Return could fake `in_flight_at_kill`.
  Evidence: design.md:64, :93-96, :118, :164-166, :314.
- **R12-B2 — Low:** the new script-test case used a non-zero exit at any time after SIGUSR1, so
  it would pass under round 10's rule and did not prove "any value". Evidence: design.md:351;
  decisions.md round 11.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R12-B1, a recorder failure
is the one exception, the client exiting non-zero at once so its `.rc` fails the run, with tests
in `recorder_test.go` and the script test; R12-B2, the case is a client that creates
`kill-ready` and exits 0 between `kill-at`'s SIGKILL and its unit's. No rule narrowed; no risk
named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 13 — 435e159

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R12-B1 and R12-B2 fixed.

- **R13-B1 — Low:** the recorder-failure test did not cover the waiting state after the kill's
  first failed operation, where the Returns of the other outstanding operations are appended; a
  client that stops watching recorder errors there would pass. Evidence: design.md:96-98, :351.
- **Nit:** the colon at design.md:93-98 made the first failed operation read as the exception.

Changes/resolution (fixed by the orchestrator, the reviewer's fix): R13-B1, two
`recorder_test.go` cases, while issuing and while waiting, each exiting non-zero at once; the
Nit, reworded. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 14 — 70f1816

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Reviewed commit 70f1816 (diff 435e159..70f1816). R13-B1 and the Nit fixed; nothing found,
Nits included.

Status: clear. Approval remains separate; no approval is inferred.
