# Issue #46 — report

## Review rounds

### Plan review round 1 — f6bb583

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

Findings, design-code (Nits as C-N1 to C-N5):

- **R1-C1 — High:** the pass's read path does not exist in the code, and the design does not say what it is. Evidence: design.md:16, :55-57; `flaminio-io/src/node.rs:344-376`, `flaminio-io/src/ublk.rs:782-787`, `:1293-1341`; `flaminio-store/src/device.rs:21,34`.
- **R1-C2 — Medium:** S3's recheck uses evidence coarser than the block (a per-chunk generation, admission, the touched sets), so a pass can retry without end. Evidence: design.md:36-46; `flaminio-store/src/store.rs:848-849`, `:872-881`.
- **R1-C3 — Medium:** the design says S7 and S11 step 5 cannot hold as written, and the spec is unchanged. Evidence: spec.md:73-75, :118; design.md:67-68, :113-115, :146-149.
- **R1-C4 — Medium:** S4 records every block of a failed 64 KiB read as a finding, against the invariant. Evidence: spec.md:28, :56-57; design.md:48-49, :57.
- **R1-C5 — Medium:** the lab fixture's pin has no mechanism, and the one the design points to is store-wide and unbounded; the restore does not check the mapping; the socket is not named. Evidence: design.md:34, :102-103, :111-112; `store.rs:723-725`, `:2043`; `flaminio-store/src/state.rs:239-245`.
- **R1-C6 — Low:** the `lab-tools` build has no path to the lab or to CI. Evidence: design.md:101-102, :127; `flaminio-io/Cargo.toml`; `scripts/guest/build.sh:6`; `scripts/lab.sh:241`.
- **R1-C7 — Low:** a store poisoned during a pass can yield a false finding. Evidence: design.md:49-50; `store.rs:743-752`, `:876-880`.
- **R1-C8 — Low:** the store has no layer removal; the design skips "deleted layers" and tests removing them. Evidence: design.md:30, :37, :123; `state.rs:283-471`.
- **R1-C9 — Low:** on a `--buffered` node a pass would check the page cache, not the medium. Evidence: `node.rs:41,403`; design.md:132; spec.md:20.
- **R1-C10 — Low:** off the target's node a finding names its volume only by a hash; the plan gives no way from it to a name. Evidence: spec.md:18-19; design.md:82; `flaminio-node/src/lib.rs:21-29`; `flaminio-node/src/replica/mod.rs:62-67`.
- **R1-C11 — Low:** the flags do not fit the node's parser, and the lab has no knob for them. Evidence: design.md:61, :70; `node.rs:150-207`; `scripts/guest/node.sh:53`.
- **R1-C12 — Low:** #44's note for #46 (blocks that differ under equal CRC-32C values) is not in the plan. Evidence: `origin/main:docs/issues/44/spec.md:185-189`; intent.md:88-90.
- **C-N1 — Nit:** "Q1–Q6 remain proposed/open" contradicts the settled lines. Evidence: design.md:6, :146; spec.md:4-6.
- **C-N2 — Nit:** "Lab procedure below"; it is above. Evidence: design.md:128.
- **C-N3 — Nit:** missing spaces make numbers misreadable ("at most64", "step1", "proposed0009"). Evidence: design.md, throughout.
- **C-N4 — Nit:** "completed passes" against S9's "passes ended". Evidence: design.md:86; spec.md:94.
- **C-N5 — Nit:** line references off. Evidence: design.md:10, :17; intent.md:14, :35.

Findings, design-security (Nits as S-N1 to S-N3):

- **R1-S1 — Medium:** a write load on the chunk starves a real finding, and the pass never ends. Evidence: design.md:35-46, :96-97; `control/csi/controller.go:71`.
- **R1-S2 — Medium:** the executor of the pass's read does not exist where the pass mostly runs; implemented with what exists, the read blocks the engine thread. Evidence: design.md:16, :55-57, :130-132; `node.rs:344-376`; `replica/mod.rs:399`.
- **R1-S3 — Medium:** "pins that cluster" is not what the read-lifetime mechanism does; a long pin quarantines the disk. Evidence: design.md:34, :103; `store.rs:391`, `:723-725`, `:1913-1922`, `:2043`; `flaminio-store/src/bitmap.rs:65-83`.
- **R1-S4 — Medium:** no path gets a `lab-tools` build into the lab; the boundary's only test cannot test it. Evidence: design.md:99-101, :127; `scripts/guest/build.sh:4-6`; `Makefile:426`; `flaminio-net/Cargo.toml:22-26`.
- **R1-S5 — Low:** the restore can put old bytes over a newer write, and a dead harness has no way back. Evidence: design.md:111-115; `store.rs:872-881`.
- **R1-S6 — Low:** the raw write's guards (device identity, offset, ids) are not listed. Evidence: design.md:102-109; `scripts/guest/common.sh:11-16`; `scripts/guest/confirm.sh:19-21`.
- **R1-S7 — Low:** the design contradicts S7 and S11 step 5 and leaves the spec as it was. Evidence: spec.md:73-75, :118; design.md:67-68, :113-115.
- **R1-S8 — Low:** the status line's encoding, sources and limit are unspecified. Evidence: design.md:78-82; `scripts/guest/common.sh:21-30`; `flaminio-io/src/control.rs:37`.
- **R1-S9 — Low:** four behaviours the design adds have no named test (flag ranges, a backward clock, a count overflow). Evidence: design.md:61, :64-65, :70, :84, :125-126.
- **S-N1 — Nit:** the same stale "Q1–Q6" text as C-N1. Evidence: design.md:6, :146.
- **S-N2 — Nit:** "Refuse uncertain clock/profile assumptions; this test does not qualify #83 timing" names no state and no check. Evidence: design.md:95-96.
- **S-N3 — Nit:** #44's note is not in the plan's folder (as R1-C12).

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 1 (2026-11-16): changes made after approval"):
- **R1-C1, R1-S2:** the design names the read path: a reader thread of the node (`flaminio-scrub`, `flaminio-io/src/scrub_io.rs`) with its own read-only `O_DIRECT` descriptor, the trait `ScrubReader` in `flaminio-node`, completions as `Input::ScrubDone` with the node's wake. The ticket stays in the engine across attach and detach. The next read is issued when the completion is handled; `Engine::next_wakeup` carries the next issue time; reads are paced so that both kinds of node reach the rate. Tests added: a read held across attach and detach, a held read on a replica-only node with heartbeats answered, the rate reached on each kind of node. `BlockDevice` and `flaminio-uring` are not touched.
- **R1-C2, R1-S1:** evidence is per block: one watch on the ticket out, marked by `State::apply`. A marked block is skipped and counted (S2); an unmarked mismatch is read once more. At most three reads a block, no wait on any write, so a pass ends. The per-chunk generation and the use of admission and the touched sets are cut. S3 is reworded; S9's status gains the skipped count. Tests: a damaged block under continuous writes to the rest of its chunk is reported and the pass ends; a second injected bug (`ignore_marks`) must fail the property test. The lab check's fio now writes in the block's chunk.
- **R1-C3, R1-S7:** S7 and S11 step 5 are amended, and both narrowings are in decisions.md with their risks named, not accepted. Not taken from R1-C3: the check does not stop the node when it cannot restore; with a volume of its own, the operator's `vol-delete` removes the block (decisions.md says why).
- **R1-C4:** S4 and the design: a failed read is read again block by block; only a block whose own read fails is unreadable. Test: one failing block in a 64 KiB read gives one unreadable and 15 checked.
- **R1-C5, R1-S3:** a per-cluster pin in `State` (the ticket's cluster and the lease's), without `reads_in_flight`; other freed clusters are released as today. `fixture check` and `fixture release` answer `unchanged` or `changed`; the lines are on the node's control socket. Tests: free space returns while a pin is held; a restore refused after a change.
- **R1-C6, R1-S4:** the feature is named in three crates, forwarded, and enabled nowhere else (a lint); `LAB_TOOLS=1 make deploy` builds it; a default build answers the usage error, which stops the check before it creates anything; S7's measurement runs on a default build; `make test`, `make lint` and `lab.sh lint` gain runs with the feature; a `control.rs` test under default features.
- **R1-C7:** `finish_scrub` checks poison first: the pass ends failed and a pending mismatch is dropped. Test added.
- **R1-C8:** layer removal is dropped from the design and its tests; a layer's index is its identity.
- **R1-C9:** the reader's descriptor is `O_DIRECT` whatever `--buffered` says; a device that refuses it gives a node without scrub (S1 amended). Tests added.
- **R1-C10, R1-S8:** a finding names the volume by the store's id only (the Finding term is narrowed); docs/lab.md gives the rule (S12); the check matches the finding to the id in the fixture's answer. The status is hand-written JSON with hex ids and fixed tokens only, error text goes to the log, and the socket's wait is bounded by `SCRUB_LIMIT` (15 s). Tests added.
- **R1-C11:** `--scrub-rate-mib N` and `--scrub-period-hours N|off`, in `USAGE`; `SCRUB_RATE_MIB` and `SCRUB_PERIOD_HOURS` through `node_env` and `node.sh`, with the script test.
- **R1-C12, S-N3:** intent, Out of scope, step 2 carries #44's note.
- **R1-S5:** the restore is conditional on the lease (`unchanged` before the write and at the release) and can be run again from the saved artifact (`make scrub-check RESTORE=<run>`). Script tests for both.
- **R1-S6:** five guards before each raw write (ids, the node's process and device, the disk UUID against the superblock, the offset's alignment and range, the lease), each with a script test. The check works on a volume it creates and takes none from its caller (#84's S7); the fixture leases only a volume with the check's pattern.
- **R1-S9:** the flags' range checks are in the test rows. The backward-clock rule and the count-overflow rule are cut instead of tested: the engine's `Instant` is monotonic, and u64 counts of blocks saturate.
- **C-N1, S-N1:** the stale text is gone from design.md and spec.md's header.
- **C-N2:** "above".
- **C-N3:** the spaces are restored.
- **C-N4:** "ended passes (complete, stopped or failed)".
- **C-N5:** the references are corrected in design.md and intent.md.
- **S-N2:** the sentence is dropped.

Nits left: none.

Rules narrowed (each because a finding shows it cannot hold as stated): S7, S11 step 5, S3, the Finding term, S1. Risks named, not accepted: a device that stalls on a pass's read; a block left wrong in the check's volume when the restore is refused; a raw write beside a live store; the fixture reaching a build; a leaked lease; a buffered node's dependence on write-back before a direct read; a block rewritten during its read skipped in that pass.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — 086b789

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R1-C1 to R1-C12, R1-S1 to R1-S9 and round 1's Nits fixed; what they opened is R2-B1 to R2-B6.

- **R2-B1 — High:** under #74's `--privsep`, the lab's default mode, the reader cannot open the descriptor the design gives it, so the node runs no scrub. Evidence: design.md:102-113; `origin/main:docs/issues/74/spec.md:32-36`, `:38`, `:49`, `:63`; `74/design.md:54-56`, `:153-157`.
- **R2-B2 — Medium:** the driver issues only from `turn` and `handle`; #15's engine thread serves a deadline wake with `flush`, so the wake does nothing and `next_wakeup` stays in the past. Evidence: design.md:120-125, :131-133; `flaminio-node/src/engine.rs:748-750`; `origin/main:docs/issues/15/design.md:256-275`; `flaminio-io/src/ublk.rs:581-598`.
- **R2-B3 — Low:** S3's second read leaves one case undecided (the entry changed between the two tickets), and under one reading `ignore_marks` cannot fail its test. Evidence: design.md:89-91, :302.
- **R2-B4 — Low:** the pinned-cluster set loses the lease's pin when a scrub ticket reads the leased cluster, which S11 step 4 does. Evidence: design.md:71-77, :279-281, :342-345.
- **R2-B5 — Low:** the lint that keeps `lab-tools` out of a build is not specified enough to fail, and has no failing case. Evidence: design.md:220-223, :310, :324, :346-348; `Makefile:61`, `:86`; `scripts/guest/build.sh:6`.
- **R2-B6 — Low:** two kernel behaviours the reader depends on (a non-exclusive open beside `O_EXCL`, write-back before a direct read) are tested on a regular file, where they are not the behaviours in question. Evidence: design.md:306, :351-352; `flaminio-uring/src/uring.rs:125-127`.
- **R2-N1 — Nit:** decisions.md:24 points to a section the round renamed.
- **R2-N2 — Nit:** three wordings for the answer of a node without scrub. Evidence: design.md:113, :171; spec.md S1.
- **R2-N3 — Nit:** steps 4, 5 and 8 poll `scrub status` with no limit. Evidence: design.md:267.
- **R2-N4 — Nit:** "takes no volume from its caller" beside `RESTORE=<run>`. Evidence: design.md:206, :273.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 2 (2026-11-17): changes made after approval"):
- **R2-B1:** the reader reads through a duplicate of the store's own descriptor: no path is opened and nothing is asked of #74's helper, so it is the same in direct mode and under `--privsep`. #74 is followed and not changed. A node whose store runs buffered has no scrub and its status says so (S1 amended; round 1's "a device that refuses direct reads" is replaced). The lab check and S7's measurement run in the lab's default mode; the check's process guard takes `confirm.sh`'s rule as #74 leaves it. Test added, by whichever of #46 and #74 lands second: a node under the helper runs a pass.
- **R2-B2:** one `Engine::scrub_step`, called by `turn`, by `flush` and after a completion. Its deadline is in `next_wakeup` only while a pass runs with no read out, and after a step it is absent or later than now; a scan that reaches its bound sets it 1 ms later ("the next turn at once" is cut). The design says what an attached node does today, and that whichever of #15 and #46 lands second lists the deadline among the engine thread's. Test added: driven as #15's loop, the read is issued and no two consecutive wakes pass without progress.
- **R2-B3:** the first ticket's physical cluster and checksum are carried to the second's planning; an entry that differs is skipped and counted, with no read. A mismatch is a block unmarked in both reads that failed one checksum twice. The property test's device takes each read after the interleaved records; `ignore_marks` gets a directed schedule (A, B, A, B).
- **R2-B4:** pins are per holder (a slot for the ticket, a slot for the lease); a cluster stays blocked while either names it. Test added.
- **R2-B5:** the lint is a rule in a script of its own (`scripts/tools/lab-tools-lint.sh`): the string occurs in the manifests under `dataplane/`, the fuzz workspace's included, only as the three declarations; no build line of the Makefile, `scripts/` or `.github/` has it or `--all-features` for the main workspace, but the listed ones. The script test runs it on doctored copies, each of which must fail. The measurement's first step wants the usage error from `fixture status` on every node.
- **R2-B6:** both kernel claims, their tests and the buffered case are cut with R2-B1's fix; the reader's file test covers its logic only.
- **R2-N1:** the reference names the section as it is now.
- **R2-N2:** one answer: `scrub start` and `scrub stop` give `error: scrub unavailable`; `scrub status` gives `ok JSON` with `state` `unavailable` and `reason` `buffered`.
- **R2-N4:** `RESTORE` takes a run id checked against the pattern and needs `CONFIRM=yes`.

Nits left: R2-N3 (a limit on the check's wait for a pass to end): it adds a behaviour and its test, more than a change of wording.

Rule narrowed (because findings show it cannot hold as stated): S1, for a node whose store runs buffered (R2-B1 with R1-C9 and R2-B6); it replaces round 1's narrowing of S1. Risks named, not accepted: a buffered node has no scrub; a running node tells a fixture build from a default one only by `fixture status`; round 1's other risks stand, less the buffered node's dependence on write-back.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — be8f4dd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R2-B1 to R2-B6 and R2-N1, R2-N2, R2-N4 fixed; R2-N3 left on purpose.

- **R3-B1 — Low:** the reader's three calls (`BorrowedFd` from a raw descriptor, `fcntl`, `pread`) need `unsafe` in `flaminio-io`, which forbids it, and the design says `flaminio-uring` is not touched. Evidence: design.md:121-128, :369-371; `dataplane/Cargo.toml:39-40`; `flaminio-uring/Cargo.toml:19-23`; `flaminio-uring/src/uring.rs:287-291`.
- **R3-B2 — Low:** the lint's build-line rule fails on the tree it is added to (its own name and lines carry the string), and it shuts the feature's tests out of CI. Evidence: design.md:262, :269-273, :361-362; `.github/workflows/ci.yml:40`.
- **R3-B3 — Low:** the measurement's first step has a script test but no script. Evidence: design.md:362, :377-383; spec.md:178-180.
- **R3-N1 — Nit:** "no two consecutive wakes pass without progress" fails on correct code (a completion wake, then a turn before `due`). Evidence: design.md:357; spec.md:164-165.
- **R3-N2 — Nit:** `child_cannot_open_paths` is not part of `flaminio-io/tests/privsep.rs`'s harness, whose node blocks before the reader starts. Evidence: design.md:358; `origin/main:docs/issues/74/design.md:288`.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 3 (2026-11-17): changes made after approval"):
- **R3-B1:** `flaminio-uring` gains one safe method, `UringDevice::try_clone_fd() -> io::Result<OwnedFd>`, and no new `unsafe`; the mode comes from `UringDevice::is_direct()`; the read is `FileExt::read_at` on a `File` made from the duplicate, into an `AlignedBuf`. `flaminio-io` gains no `unsafe`. Touched now lists `flaminio-uring/src/uring.rs`, and "not touched" is corrected. A unit test of `try_clone_fd` is added.
- **R3-B2:** the lint is `scripts/tools/fixture-feature-lint.sh`, whose path lacks the string; it matches the exact string and exempts by path exactly itself and `scripts/tools/scrub-test.sh`. `ci.yml`'s `rust-test` job gains the step `cd dataplane && cargo test --locked --features flaminio-io/lab-tools`, which is among the rule's exceptions, in Touched and in the S11 (fixture) row. The doctored workflow case stays on a `cargo build` line.
- **R3-B3:** `lab.sh scrub-build` and `make scrub-build` ask every data node for `fixture status`, pass when each answers the usage error and fail otherwise, naming the node. The measurement's procedure in docs/lab.md starts with it, and `scrub-test.sh` tests that command.
- **R3-N1:** the test is stated as the design's rule: after every step `next_wakeup` is later than now, and every wake at `due` issues a read or scans (design and spec).
- **R3-N2:** the pass test under the helper needs a real PKI directory and a `disk` answer opened with `O_DIRECT`; the other test's name is dropped.

Nits left: none from this round (R2-N3 is still left).

No rule narrowed. Risk named, not accepted: the measurement's runs are the existing bench targets, so its first step, `make scrub-build`, is the operator's to run.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — 8a39a89

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-B1 to R3-B3, R3-N1 and R3-N2 fixed; R2-N3 left on purpose.

- **R4-1 — Low:** `read_at` is one `pread`: it may return short (an interrupted read, the device's end), and the design says nothing of that nor of `O_DIRECT`'s need for an aligned offset and length. A short read taken as a full block would give a false mismatch. Evidence: design.md, "The read"; `flaminio-uring/src/aligned.rs:81`.
- **R4-2 — Low:** the new CI step is a second raw `cargo test` in `rust-test`, against docs/ci.md's rule that every job is a make target; it runs the whole suite twice, and docs/ci.md is not among the files to change. Evidence: design.md, "CI"; `docs/ci.md:5-13`; `.github/workflows/ci.yml:40`.
- **R4-3 — Low:** the lint exempts `scripts/tools/scrub-test.sh` as a whole file, so a real build line added there goes unlinted. Evidence: design.md, "Exempt, by path, exactly two files".

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 4 (2026-11-17): changes made after approval"):
- **R4-1:** the reader uses `FileExt::read_exact_at`: an interrupted read is retried, and a short read or one that ends at the device's end is an I/O error, reported as any read error is and never as a CRC result. It asserts that offset and length are multiples of 4,096 before the read; a release build answers a request that is not as a read error. Tests added to the reader's row, now S1, S3, S4: a read past the end of the file returns an error, not CRCs; a misaligned request is answered as an error with nothing read. The spec's S4, S5 test line gains the short read.
- **R4-2:** a make target, `make test-fixture`, runs `cargo test --locked --features flaminio-io/lab-tools -p flaminio-store -p flaminio-node -p flaminio-io` (the three crates that declare the feature). `make test` calls it, and CI's `rust-test` job gains the step `make test-fixture` instead of a raw `cargo test`. `docs/ci.md` (its CI row) and the Makefile's help are in Touched. The target's name lacks the feature's string, so the lint has no exception under `.github/`.
- **R4-3:** the lint exempts only itself. `scripts/tools/scrub-test.sh` is linted as any script: it composes the string at run time and writes its doctored manifests and build lines with `printf` into a temporary tree the lint is pointed at. Script-test case added: a build line with the feature in any other `scripts/tools/*.sh` fails the lint.

Nits left: none from this round (R2-N3 is still left).

No rule narrowed; no new risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — ee3aa88

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R4-1 to R4-3 fixed; R2-N3 left on purpose.

- **R5-1 — Low:** the script test is linted as any script, but only the feature's string is composed at run time. The lint also fails `--all-features` on a main-workspace build line, and the test writes such a doctored line, so the lint would fail on the script test. Evidence: design.md, "Build lines" and the S8, S11 test row.
- **R5-2 — Low:** the reader "asserts" alignment while "a release build answers it as a read error": in a debug build, which `make test` runs, the misaligned-request test would panic the reader thread. Evidence: design.md, "Aligned".

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 5 (2026-11-17): changes made after approval"):
- **R5-1:** the script test composes `--all-features` from parts at run time too, so no line of it has the feature's string or `--all-features`; the "passes on the tree" case says so.
- **R5-2:** the alignment check is a checked condition in every build: a misaligned request is answered as a read error (`InvalidInput`) with nothing read, and no build panics on it.

Nits left: none from this round (R2-N3 is still left).

No rule narrowed; no new risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 6 — d3cf93dcaa07819d444ce9794974a206559e62e7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
