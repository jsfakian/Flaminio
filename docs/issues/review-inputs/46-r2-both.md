# Plan review: issue #46 (Scrubbing and repair), round 2, focuses: design-code and design-security

Scope: `git diff f6bb583..1b2535e -- docs/issues/46/ ':!docs/issues/46/report.md'` in
`/home/jsfakian/Documents/src/flaminio-46`, and the code and clear plans the changed lines depend
on (#15, #74, #84 on `origin/main`). Paths are under `dataplane/crates/` unless they start with
`docs/`, `scripts/` or `Makefile`. Line numbers of `design.md` are at HEAD.

Counts: High 1, Medium 1, Low 4, Nit 4.

## Earlier findings

Design-code:
- R1-C1: fixed in direct mode (reader thread, `Input::ScrubDone`, ticket in the engine). What it opens: R2-B1, R2-B2.
- R1-C2: fixed (per-block watch marked by `State::apply`, at most three reads, no wait). See R2-B3.
- R1-C3: fixed (S7 and S11 step 5 amended and recorded; not stopping the node is argued and follows #84).
- R1-C4: fixed.
- R1-C5: fixed (per-cluster pin without `reads_in_flight`; `check`/`release` answers; the control socket). See R2-B4.
- R1-C6: fixed. See R2-B5.
- R1-C7: fixed (poison first).
- R1-C8: fixed.
- R1-C9: fixed for direct mode. See R2-B1, R2-B6.
- R1-C10: fixed.
- R1-C11: fixed.
- R1-C12: fixed.
- N1 to N5: fixed.

Design-security:
- R1-S1: fixed.
- R1-S2: fixed in direct mode. See R2-B1.
- R1-S3: fixed. See R2-B4.
- R1-S4: fixed. See R2-B5.
- R1-S5: fixed (conditional restore, `RESTORE=<run>`).
- R1-S6: fixed (five guards, scratch volume, pattern-bound lease).
- R1-S7: fixed.
- R1-S8: fixed.
- R1-S9: fixed (flag ranges tested; the clock and overflow rules cut with a reason the code bears out).
- N1 to N3: fixed.

## New findings

### R2-B1 — High

**Under #74's `--privsep`, the lab's default mode, the reader cannot open the descriptor the
design gives it, so the node runs no scrub.**
The reader "holds its own read-only descriptor of `--device`", opened `O_RDONLY | O_DIRECT`
(design.md:102-109), and "a device that refuses the open gives a node without scrub"
(design.md:112-113). #74 (clear, a fixed point) rules that out:
- **S2:** under `--privsep` the I/O process "never opens a disk or ublk device path ... and
  refuses the flags that would name one" (`origin/main:docs/issues/74/spec.md:32-36`); parse
  refuses `--device` and `--buffered` (`origin/main:docs/issues/74/design.md:153-157`); test
  `child_cannot_open_paths`.
- **One descriptor, once:** `disk` is answered once, the helper closes its copy, a second
  request gets `error already sent`, and a reply carries one descriptor
  (`74/design.md:54-56`, `74/spec.md:38,49`).
- **The default:** "The lab node runs under the helper by default" (`74/spec.md:63`).

So with #74 landed, every lab node (and S11's check, and S7's measurement) falls into the
design's "refuses the open" branch: no automatic pass, `scrub start` refused. The plan names
#74 nowhere. S1's narrowing ("a node whose device refuses direct reads runs no pass") was
recorded as a corner case; with #74 it is the default.

**Fix.** Say what the reader reads through in each mode, and which plan folds it in when the
second lands (as #74 does for #71's `--telemetry`). Two ways that keep #74's rules:
- a duplicate of the store's own descriptor (`File::try_clone`; `pread` shares no position)
  when that descriptor is `O_DIRECT`: no path, no second open, the same in both modes. A
  buffered store then has no pass, and S1's sentence on `--buffered` changes back;
- or a second read-only `O_DIRECT` descriptor from the helper: a change to #74's request
  grammar and its "once each", to be recorded as a hand-off to #74, not assumed.
State the lab check's and the measurement's mode (`PRIVSEP`), and add the test: a node under
the helper runs a pass.

### R2-B2 — Medium

**The driver issues only from `turn` and `handle`; #15's engine thread serves a deadline wake
with `flush`, so the wake does nothing and `next_wakeup` stays in the past.**
The design: "It issues from `turn` and from `handle`", `Engine::next_wakeup` "includes the
driver's next issue time", and "#15's engine thread, when it lands, waits on the same time"
(design.md:120-125); "when entries are left and no read is out, the driver asks for the next
turn at once" (design.md:131-133). Today that works only in `drive_until`, which calls `turn`
on every loop (`flaminio-io/src/node.rs:352-354`). `flush` is a separate entry that runs
`flush_local` only (`flaminio-node/src/engine.rs:748-750`).
- **#15's loop** is: at most 64 inputs, "`turn` if it is due, and `flush` otherwise", with
  `ppoll` timed by `next_wakeup` (`origin/main:docs/issues/15/design.md:256-275`). A wake at
  the driver's issue time, or "at once", runs `flush`: nothing is issued, `next_wakeup` still
  returns a past time, and the engine thread loops without sleeping, taking the engine's lock
  each time (the lock `submit` and `pump` take), until the 10 ms turn. During a pass that is
  most of every 10 ms.
- **An attached node today** has no carrier for "the next turn at once": queue threads never
  read `next_wakeup` (`flaminio-io/src/ublk.rs:581-598`, `:1178-1186`).

**Fix.** The driver also runs from `flush` (or a named `scrub_step` that both loops call), and
`next_wakeup` returns a scrub time only when a call at that time makes progress. Say which of
#15 and #46 folds this in when it lands second, with its test, as #15 does for #75's deadline
(`15/design.md:267-268`). Say that on an attached node today the cursor advances at the 10 ms
turn and at completions. Test: an engine driven as #15's loop (wake at `next_wakeup`, `flush`
unless the turn is due) issues the read, and no two consecutive wakes pass without progress.

### R2-B3 — Low

**S3's second read leaves one case undecided, and under one reading `ignore_marks` cannot
fail its test.**
Step 5 (design.md:89-91): a second ticket "planned from the map as it is then"; "still not
matching the same checksum, and unmarked: a mismatch". No watch exists between the first
ticket's finish and the second's issue (the throttle can put time between them), so a record
can change the entry unmarked. Not said: what the block is when the second ticket's physical
cluster or checksum differs from the first's (checked, skipped, read again?), and whether
"the same checksum" compares the two tickets. The counts are exact (S9) and the bound is three
reads, so an implementer must choose.
The choice decides the injected bug. A false finding under `ignore_marks` needs a
`DeferredWrite` to the same block applied during each of the two reads, with the device read
taken after it. If the tickets' checksums must be equal, it also needs the block's contents to
alternate (A, B, A, B). The property's generator is not described (design.md:302), and random
data over random offsets does not produce either. `MapUpdate`, `Discard` and `ChunkDelete`
never trip it: the second ticket is planned anew.

**Fix.** Carry the first ticket's physical cluster and checksum into the recheck: if the entry
differs when the second ticket is planned, the block is skipped (counted), with no read. A
mismatch is then "unmarked in both reads, against one checksum". Say that the property test
takes its device read after the interleaved apply, and add a directed schedule for
`ignore_marks`: a block written A, B, A, B, one write inside each read.

### R2-B4 — Low

**The pinned-cluster set loses the lease's pin when a scrub ticket reads the leased cluster,
which is what S11 step 4 does.**
"`State` keeps a small set of pinned clusters (the ticket's and the lab fixture's lease: at
most two)", and "a dropped pin only leaves the set" (design.md:71-77). In step 4 the pass on
CORRUPT plans a ticket for the cluster the lease holds: one cluster, two holders. When the
ticket finishes, a set keyed by cluster drops it for both, and the restore's argument "the pin
kept the cluster from reuse, so only the run's own volume can hold the old bytes"
(design.md:279-281, :342-345) no longer holds. The lease's watch still answers `changed`, so
the exposure is the window between guard 5 and the raw write.

**Fix.** Pins are per holder (two slots, or a count per cluster): a cluster stays blocked
while any holder remains. Add to the pin tests (design.md:304): a lease and a ticket on one
cluster, the ticket finished, the cluster freed and checkpointed: still not allocated.

### R2-B5 — Low

**The lint that keeps `lab-tools` out of a build is not specified enough to fail, and has
no failing case.**
The design: "no manifest lists it in `default` or in a dependency's `features` but for those
two forwards" (design.md:220-223), tested as "no manifest enables `lab-tools`" inside
`scrub-test.sh` (design.md:310); the Risks say nothing else guards a release build
(design.md:346-348). Not covered:
- a third feature that forwards to it (`x = ["flaminio-store/lab-tools"]`), in any member;
- `dataplane/fuzz/Cargo.toml`, a workspace of its own that depends on the crates by path and
  is linted with `--all-features` (`Makefile:86`);
- a build line that names it or takes all features: `Makefile:61` (`release`),
  `scripts/guest/build.sh:6`, the CI workflows. Only `deploy` is tested (design.md:310).
The check runs on the real manifests, which pass by construction: nothing shows the pattern
catches what it is for. Nor does any step tell a running fixture build from a default one but
`fixture status`; the measurement "on a default build" (design.md:324) does not ask it.

**Fix.** State the lint as a rule: in every `Cargo.toml` under `dataplane/`, the string
`lab-tools` appears only as the three declarations, with exactly the two forwards; no build
line in the Makefile, `scripts/` or `.github/` has it or `--all-features` for the main
workspace, but `deploy` under `LAB_TOOLS=1`. The script test runs the lint on doctored copies
(in `default`, in a dependency's `features`, a third forward, a build line) and expects each
to fail. The measurement's first step: `fixture status` answers the usage error on every node.

### R2-B6 — Low

**Two kernel behaviours the reader depends on are tested on a regular file, where they are not
the behaviours in question.**
Row S1 (design.md:306): "`scrub_io.rs` tests on a file: ... no `O_EXCL` conflict with the
store's descriptor. On a buffered node, in-place writes then a pass: no finding."
- `O_EXCL` without `O_CREAT` claims nothing on a regular file; the exclusive claim exists
  only on a block device (`flaminio-uring/src/uring.rs:125-127`). The test passes whatever the
  kernel does.
- Write-back before a direct read is a different code path for a block device than for a file
  on a filesystem. The risk says "the test must show it" (design.md:351-352); a file does not
  show it for `--device`, and the lab never runs `--buffered` (`scripts/guest/node.sh:45-58`).

**Fix.** Run both on a loop device, as a root-only kernel test (the idiom of #74's
`tests/kernel.rs`), or cite the kernel source and version for each claim as #74's decisions
do, and say the file test covers the reader's logic only. If R2-B1 is fixed with a duplicate
of the store's descriptor, both claims and the buffered case go.

## Nits

- **N1.** decisions.md:24 points to design.md's "Driver, strict throttle and schedule"; the round renamed it "Reader, driver, throttle and schedule" (design.md:100).
- **N2.** A node without scrub: `scrub status` is `error: scrub unavailable` in design.md:171, "the status `unavailable`" in design.md:113, and S1 says "its status says so". One answer.
- **N3.** Steps 4, 5 and 8 poll `scrub status` "until it ended" (design.md:267) with no limit; a read that never returns leaves the check waiting.
- **N4.** "Takes no volume ... from its caller" (design.md:206) beside `RESTORE=<run>` (design.md:273): say that RESTORE takes a run id checked against the pattern, and whether it needs `CONFIRM=yes`.

## The narrowings and the questions asked

- **Narrowings:** S7, S11 step 5, S3, the Finding term and S1 each follow from a round-1
  finding (R1-C3/S7; R1-C3/S5/S7; R1-C2/S1; R1-C10/S8; R1-C9), and spec, design and decisions
  state each the same way. Two notes for the user's decision: S1's "runs no pass" is the lab's
  default under #74 (R2-B1); intent's What we want 2 ("is read again") and 4 ("keep their
  timing") still state the want without the narrowing.
- **A lock on the steady-state path:** none added. The watch is marked inside `State::apply`
  on the store's owner; the reader's two channels are the existing idiom. The engine's
  existing mutex is held for the cursor's bounded scan.
- **A write between the read and the mark:** no false finding and no hidden one. The watch
  exists from the ticket's issue; `apply` marks before `write_many`
  (`flaminio-store/src/store.rs:876-881`), which is the only write to a mapped cluster. The
  open points are R2-B3's.
- **`O_EXCL`:** a non-exclusive read-only open beside the store's exclusive one is allowed on
  a block device. The obstacle is #74 (R2-B1).
- **The fixture:** ids, guards, the scratch volume's pattern, `unchanged`/`changed` and
  `RESTORE=<run>` hold against `confirm.sh`, `common.sh`, `flaminio-disk inspect` and
  `delete_volume`; the defects are R2-B4, R2-B5 and N3, N4.

Reviewed commit: 086b78963f8305826fbf761df367a46563fb7fd1
