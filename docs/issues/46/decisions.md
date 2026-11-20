# Issue #46 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`d9b9a71648d65d9d42e649bb74c3c8026c7bcae3a5ac15af6305f5c6ace38016`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

Plan review round 1 (2026-11-16) fixes, made after approval: see below.
Plan review round 2 (2026-11-17) fixes, made after approval: see below.
Plan review round 3 (2026-11-17) fixes, made after approval: see below.
Plan review round 4 (2026-11-17) fixes, made after approval: see below.

## Settled at approval

- **Q1. Is the pass alone the first step:** yes. It is built and checked alone; the issue stays
  open for the repair. `spec.md`, Open questions Q1.
- **Q2. Do findings and the last pass's time survive a restart:** not yet; they are kept in
  memory only, with no format change. `spec.md`, Open questions Q2, S8 and S9.
- **Q3. Is the automatic pass on by default while nothing repairs:** on, one period after the
  start. `spec.md`, Open questions Q3 and S8.
- **Q4. The default rate:** 32 MiB/s, kept if the lab's results with a pass stay within the 10%
  spread the performance plan allows between runs, and lowered otherwise. `spec.md`, Open
  questions Q4 and S7; `design.md`, Reader, driver, throttle and schedule, and Measurement,
  docs, risks and ADRs.
- **Q5. The fault of Why 5:** filed now on its own issue, ahead of step 4. `spec.md`, Open
  questions Q5.
- **Q6. Metadata pages and the journal:** a later step of #46, after the repair. `spec.md`, Open
  questions Q6.
- **The proposed lab-only pin helper:** as designed, under the `lab-tools` feature and absent
  from release artifacts (`fixture pin`, `fixture release`); its feature boundary is reviewed
  before implementation, as the design asks. `design.md`, Reproducible corruption fixture and
  lab, and Measurement, docs, risks and ADRs.

## Plan review round 1 (2026-11-16): changes made after approval

Reviewed commit f6bb583; findings R1-C1 to R1-C12 and N1 to N5 (design-code), R1-S1 to R1-S9
and N1 to N3 (design-security). Nothing here is approved by this record, and no risk it names
is accepted.

**Spec rules narrowed, each because a finding shows the rule cannot hold as stated**

- **S7 (R1-C3, R1-S7):** "no session fails, no request times out and no lease lapses because
  of a pass" now holds on a device that serves the pass's one read beside its other I/O. A
  device that stalls its other I/O on a read is outside the rule; isolating classes of I/O is
  #75's. The engine never waits for the read, whatever the device does. Risk named, not
  accepted: a pass's read of a failing sector can cost a request, a session or a member.
- **S11 step 5 (R1-C3, R1-S7, R1-S5):** the restore is no longer unconditional. The saved
  block is written back only while the running node's lease says the block is unchanged;
  after a node restart or a change the check writes nothing and fails. Risk named, not
  accepted: the block then stays wrong in the check's own volume until the operator deletes
  the volume.
- **S3 (R1-C2, R1-S1):** "read again once every write to it that was in flight has committed"
  has no bound under steady writes, so a pass could not end (S5, S8). Now: a second read, and
  a block that a write, discard or deletion was applied to during either read is skipped. The
  invariant is unchanged: no block that matches its checksum is a finding.
- **The Finding term (R1-C10, R1-S8):** "its name when the node knows it" is cut. A replica
  keeps only the name's hash, so a finding names the volume by the store's id; docs/lab.md
  gives the rule from a name to its id.
- **S1 (R1-C9):** a pass reads the medium also on a `--buffered` node, through a descriptor
  of its own. A node whose device refuses direct reads runs no pass and says so. Round 2
  changed this: see R2-B1 below.

**Other spec changes**

- **S4 (R1-C4):** a failed read is read again block by block, so only a block whose own read
  fails is a finding. This restores the invariant; it narrows nothing.
- **S9 (R1-C2, R1-S8):** the status gains the blocks skipped as rewritten during their read,
  and carries no free text.
- **S11 and intent, What we want 7 (R1-S6, with #84's S7):** the check works on a volume it
  creates, not on vol1, and takes no volume from its caller. S11 also says that the check
  needs the lab-only build (R1-C6, R1-S4).
- **S12 (R1-C10, R1-C11):** docs/lab.md gains the id rule and the check's build.

**Design choices**

- **The read path (R1-C1, R1-S2):** a reader thread of the node, `flaminio-scrub`, with its
  own read-only `O_DIRECT` descriptor (round 2, R2-B1: a duplicate of the store's descriptor
  instead); completions come back as an `Input`, and the ticket
  stays in the engine across attach and detach. Chosen over a synchronous read, which would
  hold heartbeats behind a failing disk, and over an asynchronous read in `UringDevice`, which
  would change `BlockDevice` and `flaminio-uring` for one caller. The rate is paced so that
  both kinds of node reach it; it is a ceiling, and the lab records what is reached.
- **Per-block evidence (R1-C2, R1-S1):** a watch on the one ticket out, marked by
  `State::apply`. The per-chunk mutation generation and the use of admission and of the
  Flying/Staged touched sets are cut. A block is read at most three times.
- **A per-cluster pin (R1-C5, R1-S3):** tickets and the fixture's lease pin single clusters
  and do not use `reads_in_flight`, so neither a long pass nor a lease held for a check
  quarantines the disk.
- **Poison first (R1-C7):** a poisoned store ends the pass failed before any pending mismatch
  is reported.
- **Layers (R1-C8):** the store has no layer removal; the test of it is cut, and a layer's
  index is its identity.
- **Flags (R1-C11):** `--scrub-rate-mib N` and `--scrub-period-hours N|off`, with
  `SCRUB_RATE_MIB` and `SCRUB_PERIOD_HOURS` in the lab.
- **The status (R1-S8):** hand-written JSON whose strings are hex ids and fixed tokens; error
  text goes to the log; the socket's wait is bounded by `SCRUB_LIMIT` (15 s).
- **Cut instead of tested (R1-S9):** the rule for a backward or failed clock reading (the
  engine's `Instant` is monotonic) and the rule that a count overflow fails the pass (u64
  counts of blocks cannot overflow; they saturate). The flags' range checks get tests.
- **The fixture (R1-C5, R1-C6, R1-S3, R1-S4, R1-S5, R1-S6):** four lines on the node's
  control socket under the `lab-tools` feature of three crates; `LAB_TOOLS=1 make deploy`
  builds it; a default build answers the usage error, which stops the check before it
  creates anything. The fixture leases only a volume with the check's pattern. Guards before
  each raw write, a conditional restore and a restore that can be run again are in the
  design. This specifies the helper settled at approval; its feature boundary is still
  reviewed before implementation.
- **No node is stopped (R1-C3):** the reviewer proposed that the check stop the node when it
  cannot restore. It does not: with a volume of its own, an unrestored block is removed by the
  operator's `vol-delete`, and stopping a node would cost the lab's other volumes a member.
  This follows #84, whose check starts and stops no node.
- **#44's note (R1-C12, security N3):** recorded in intent, Out of scope, step 2.
- **Nits:** all fixed (the stale "Q1–Q6 open" text, "below", the missing spaces, "ended"
  passes, the line references, the #83 sentence dropped).

**Risks named, not accepted:** the two above, and those listed in `design.md`, Measurement,
docs, risks and ADRs: a raw write beside a live store, the fixture reaching a build, a leaked
lease, a buffered node's dependence on write-back before a direct read, and a block rewritten
during its read being skipped in that pass.

## Plan review round 2 (2026-11-17): changes made after approval

Reviewed commit 086b789; findings R2-B1 to R2-B6 and Nits R2-N1 to R2-N4 (design-code and
design-security, one agent). Nothing here is approved by this record, and no risk it names is
accepted.

**Spec rule narrowed, because findings show it cannot hold as stated**

- **S1 (R2-B1, with R1-C9 and R2-B6):** round 1 gave the reader a descriptor of its own,
  opened by path. #74 (clear) forbids the I/O process any disk path under `--privsep`, the
  lab's default, so that design left every lab node without scrub. Now the reader reads
  through a duplicate of the store's descriptor: no path, nothing asked of #74's helper, the
  same in both modes. A pass must read the medium (R1-C9), so a node whose store runs buffered
  runs no pass, and its status says so. Round 1's narrowing ("a device that refuses direct
  reads") is replaced by this one ("a buffered store"); a node under the helper runs passes.
  Chosen over a second descriptor from the helper, which would change #74's request grammar
  and its "once each" for one caller.

**Design choices**

- **#74 (R2-B1):** neither plan changes for the other. Whichever of #46 and #74 lands second
  adds the test that a node under the helper runs a pass. The lab check and S7's measurement
  run in the lab's default mode; the check's process guard takes `confirm.sh`'s rule as #74
  leaves it.
- **The driver's step (R2-B2):** one `Engine::scrub_step`, called by `turn`, by `flush` and
  after a completion. Its deadline `due` is in `next_wakeup` only while a pass runs with no
  read out, and after a step it is absent or later than now. A scan that reaches its bound
  sets `due` 1 ms later; round 1's "the next turn at once" is cut. Whichever of #15 and #46
  lands second lists `due` among the engine thread's deadlines and runs the test against that
  thread.
- **The second read (R2-B3):** the first ticket's physical cluster and checksum are carried to
  the second's planning. An entry that differs then is skipped and counted, with no read. A
  mismatch is a block unmarked in both reads that failed one checksum twice. The injected bug
  `ignore_marks` gets a directed schedule (A, B, A, B), since random writes do not produce it.
- **Pins per holder (R2-B4):** a slot for the ticket and a slot for the lease; a cluster stays
  blocked while either names it.
- **The lint (R2-B5):** stated as a rule over every manifest under `dataplane/` and every
  build line of the Makefile, `scripts/` and `.github/`, in a script of its own that the
  script test runs on doctored copies. The measurement's first step asks every node for
  `fixture status` and wants the usage error.
- **Kernel claims cut (R2-B6):** with the duplicate descriptor, the design no longer depends
  on a non-exclusive open beside the store's `O_EXCL`, nor on write-back before a direct
  read. Both claims, their tests and the buffered case are gone; the reader's file test
  covers its logic only.
- **Nits:** R2-N1 (the section's name above), R2-N2 (one answer for a node without scrub) and
  R2-N4 (`RESTORE` takes a run id checked against the pattern, with `CONFIRM=yes`) are fixed.
  R2-N3 (a limit on the check's wait for a pass to end) is left: it adds a behaviour and its
  test, which is more than a change of wording.

**Risks named, not accepted:** round 1's list stands but for the buffered node's dependence on
write-back, which is gone. Added: a buffered node has no scrub; a running node tells a fixture
build from a default one only by `fixture status`.

## Plan review round 3 (2026-11-17): changes made after approval

Reviewed commit be8f4dd; findings R3-B1 to R3-B3 and Nits R3-N1 and R3-N2 (design-code and
design-security, one agent). Nothing here is approved by this record, and no risk it names is
accepted. No intent or spec rule is narrowed.

- **No `unsafe` in `flaminio-io` (R3-B1):** round 2's path (`AsRawFd`, `F_GETFL`, `pread`)
  needs `unsafe`, which the workspace forbids there. Now `flaminio-uring` gains one safe
  method, `UringDevice::try_clone_fd`; the mode comes from `UringDevice::is_direct()`; the read
  is `FileExt::read_at` on a `File` made from the duplicate (round 4, R4-1:
  `read_exact_at`). No new `unsafe` in any crate.
  `flaminio-uring` is touched after all, by that one method.
- **The lint's own lines, and CI (R3-B2):** the lint is `scripts/tools/fixture-feature-lint.sh`,
  whose path lacks the string; it exempts by path exactly itself and its script test. CI's
  `rust-test` job gains `cargo test --locked --features flaminio-io/lab-tools`, an exception
  of the rule, so the fixture's tests run on every pull request. Round 4 changed both: the
  lint exempts only itself (R4-3), and CI calls `make test-fixture` (R4-2).
- **The measurement's first step (R3-B3):** `lab.sh scrub-build` and `make scrub-build` ask
  every data node for `fixture status` and pass only on the usage error. docs/lab.md's
  procedure starts with it, and the script test tests that command. Risk named, not accepted:
  the measurement's runs are the existing bench targets, so the step is the operator's to run.
- **Nits:** R3-N1 (the wake test stated as the design's rule: after every step `next_wakeup`
  is later than now, and a wake at `due` reads or scans) and R3-N2 (the pass test under the
  helper needs a real PKI directory and an `O_DIRECT` file; the other test's name is dropped)
  are fixed. None left from this round; R2-N3 is still left.

## Plan review round 4 (2026-11-17): changes made after approval

Reviewed commit 8a39a89; findings R4-1 to R4-3, all Low (design-code and design-security, one
agent). Nothing here is approved by this record, and no risk it names is accepted. No intent
or spec rule is narrowed.

- **All of a read or an error (R4-1):** the reader uses `FileExt::read_exact_at`, which
  retries an interrupted read and makes a short read, or one that ends at the device's end,
  an I/O error. It is reported as any read error is, never as a checksum result, so it
  cannot become a false mismatch. The reader checks that offset and length are multiples of
  4,096 before the read (round 5: a checked error, not an assertion). Test added: a read
  past the end of the file returns an error.
- **One make target for the fixture's tests (R4-2):** `make test-fixture` runs
  `cargo test --locked --features flaminio-io/lab-tools` for `flaminio-store`,
  `flaminio-node` and `flaminio-io`, the three crates that declare the feature. `make test`
  and CI's `rust-test` job call it; the workflow gains no second raw `cargo test`, and the
  whole suite is not run twice. `docs/ci.md` and the Makefile's help are among the files
  that change. The target's name lacks the feature's string, so the lint needs no exception
  under `.github/`.
- **No whole-file exemption (R4-3):** the lint exempts only itself. The script test is linted
  as any script: it composes the string at run time and writes its doctored lines with
  `printf` into a temporary tree. Test added: a build line with the feature in any other
  `scripts/tools/*.sh` fails the lint.

## Plan review round 5 (2026-11-17): changes made after approval

Reviewed commit ee3aa88; findings R5-1 and R5-2, both Low (design-code and design-security,
one agent). Nothing here is approved by this record, and no risk it names is accepted. No
intent or spec rule is narrowed.

- **The script test passes its own lint (R5-1):** the lint also fails `--all-features` on a
  main-workspace build line, and the script test writes such a doctored line. It composes
  `--all-features` from parts at run time as it composes the feature's string, so no line of
  it has either.
- **A misaligned request is an error in every build (R5-2):** the reader checks the offset
  and the length and answers a request that is not a multiple of 4,096 as a read error
  (`InvalidInput`) with nothing read. It is not an assertion, so the test holds in a debug
  build and the reader thread does not panic.
