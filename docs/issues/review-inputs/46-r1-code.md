# Plan review: issue #46 (Scrubbing and repair), round 1, focus: design-code

Scope: `docs/issues/46/` (intent, spec, design, decisions) in `/home/jsfakian/Documents/src/flaminio-46`,
checked against the code of that worktree. Paths below are under `dataplane/crates/` unless they
start with `docs/`, `scripts/` or `Makefile`. Clear plans read where this one touches them: #15,
#44, #74, #75, #85.

Counts: High 1, Medium 4, Low 7, Nit 5.

### R1-C1 — High

**The pass's read path does not exist in the code, and the design does not say what it is.**
The design says "Node/ublk already execute planned local reads asynchronously" (design.md:16) and
that the driver, "driven by engine turns", "emits background read work to the daemon's device
executor and accepts completions" (design.md:55-57). The code has no such executor:

- **A node without a volume** (the node S11 corrupts, and most replicas) runs the engine on the
  main thread in `drive_until`, a channel loop with no ring (`flaminio-io/src/ublk.rs:782-787`,
  `flaminio-io/src/node.rs:344-376`). Its replica reads are synchronous `store.read_batch` calls
  on the engine thread (`flaminio-node/src/replica/mod.rs:391-405`), and its commits are
  synchronous (`set_async_commits(false)`, `ublk.rs:1058`).
- **`BlockDevice` and `UringDevice` have no asynchronous read:** only `read_at` and `read_many`,
  which return when done (`flaminio-store/src/device.rs:21,34`, `flaminio-uring/src/uring.rs:516-537`).
- **The only asynchronous local reads** are submitted by a ublk tag task on its queue's ring
  (`local_read`, `ublk.rs:1293-1341`) and routed by op id to a waiting tag (`ublk.rs:603-618`).
  That ring exists only while a volume is attached; `log_output` drops `Output::LocalRead`
  otherwise (`node.rs:395`). A scrub read has no tag.
- **Turn cadence caps the rate.** With a volume attached, `turn` runs every 10 ms on queue 0 and
  every other pump calls `flush` (`ublk.rs:591-595,1178-1186`): one 64 KiB read per turn is
  6.4 MiB/s. Without one, `drive_until` is re-entered every 50 ms and `next_wakeup` knows only
  heartbeats, registration and targets (`ublk.rs:786`, `flaminio-node/src/engine.rs:621-633`):
  1.25 MiB/s, and 1,280 cursor entries a second over unmapped ranges. Both are far below S7's
  32 MiB/s default unless a completion wakes the engine and issues the next read, which the
  design does not say ("Timer is a wakeup hint", design.md:65).
- **The engine changes threads at attach and detach** (`ublk.rs:968-975,1053-1063`), nobody
  drives it while the device is added (up to 10 s, `ublk.rs:902-934`), and the queue rings end
  at detach. A read in flight on a ring that ends never completes. The store's read lifetime is
  one store-wide counter (`reads_in_flight`, `flaminio-store/src/store.rs:391,1913,1920`), so a
  leaked ticket keeps every cluster freed from then on out of reuse (`store.rs:723-725,2043`,
  `flaminio-store/src/state.rs:239-245`). The design's "process generation" on tickets does not
  cover this.

**Fix.** Name the executor and its interface. The existing idiom fits: one scrub I/O thread,
started with the node, with its own `O_DIRECT` descriptor and aligned buffer, taking read
requests on a channel and returning completions as a new `Input` on `Parts.sender` plus the
node's wake, as the network and metadata threads do (`node.rs:288-322`). Both drivers already
drain that channel (`node.rs:368-373`, `ublk.rs:586-590`), so a pass survives attach and detach.
Say that the next read is issued when the completion is handled (in `handle`, not only in
`turn`), and add the pass's next issue time to `Engine::next_wakeup` (#15's S3 folds it into its
engine thread's wait). In `flaminio-node` the executor is a trait or an `Output`/`Input` pair,
so that `tests/engine.rs` can hold a read. Add tests: a pass with a read in flight across an
attach and a detach (the ticket is released, the pass goes on); an idle node of each kind
reaches the configured rate.

### R1-C2 — Medium

**S3's recheck uses evidence coarser than the block, so a pass can retry without end.**
The design adds a per-chunk mutation generation "updated at write admission/commit", consults
"all Flying/Staged touched sets", retries when "the mapping identity or generation changes", and
forbids skipping on unrelated changes; "continual overlap can postpone completion"
(design.md:36-46). In the code, the medium and the map change together at one point only:
`execute_finish` applies the records (checksums into the map) and then writes the in-place data
synchronously, on the owner (`store.rs:872-881`). A batch in flight or staged changes neither:
"Nothing is applied until the batch is durable, so reads see the state before it meanwhile"
(`store.rs:848-849`). So admission and the touched sets are not evidence of a change, and a
per-chunk generation moves on every write to any block of the chunk.

A damaged block in a chunk or cluster under steady writes (S11 runs fio on the same volume) is
then re-read for as long as the writes last. The pass never ends, so S8's next automatic pass
(end + period) never starts and every operator start answers `already-running`. S5 has no outcome
for it, and no test covers it.

**Fix.** Take the evidence per block, at the apply. At most one ticket is out: mark its block
range when `State::apply` applies a `MapUpdate`, `DeferredWrite`, `Discard` or `ChunkDelete`
that covers it (`state.rs:322-470`). A marked block was rewritten or removed: S2 lets the pass
skip it. An unmarked block is decided by its second read. That is at most two reads per block,
and no wait on batches in flight. Keep whatever identity must survive a deletion outside
`Chunk`, which `ChunkDelete` drops (`state.rs:324-326`). Add a property test: one damaged block,
continuous writes to the other blocks of its cluster and chunk; the block is reported and the
pass completes within a bounded number of reads.

### R1-C3 — Medium

**The design says two spec rules cannot hold as written, and the spec is unchanged.**
- **S7:** "No session fails, no request times out and no lease lapses because of a pass"
  (spec.md:73-75). The design: this "universal promise against every storage stall needs that
  qualification documented in Risks", and "S7 cannot guarantee unchanged timing for arbitrary
  slow/hung devices" (design.md:67-68,146-147).
- **S11 step 5:** "it puts the saved block back, whatever failed before" (spec.md:118). The
  design: no restore after the node's process is lost; stop the node and keep the saved block
  (design.md:113-115,147-149).

The spec points to decisions.md for accepted risks (spec.md:4); decisions.md records neither.

**Fix.** Amend S7 (a pass adds at most one read in flight and its rate; a device that stalls is
outside the rule, and queue isolation is #75's) and S11 step 5 (restore only while the pin and
the same process hold; otherwise the check stops the node, keeps the saved block and fails).
Record both in decisions.md as changes after approval.

### R1-C4 — Medium

**S4 records good blocks as findings, against the invariant.**
The invariant: "A block whose stored data matches its checksum is never a finding" (spec.md:28).
A finding includes an unreadable block (spec.md:18), and S4 records "each block of the failed
read" (spec.md:56-57; design.md:48-49). With 64 KiB reads (design.md:57), one bad sector makes
16 findings, 15 of them blocks that read and match. The counts are called exact (spec.md:97-98)
and are what step 5 feeds to disk health (intent.md:79-81). A transient error on one read is
counted the same way.

**Fix.** A failed read is read again block by block (4 KiB each, charged to the rate), and only a
block whose own read fails is recorded; the others are checked as usual. Test: a device that
fails one block inside a 64 KiB read gives one unreadable block and 15 checked.

### R1-C5 — Medium

**The lab fixture's pin has no mechanism, and the one the design points to is store-wide and
unbounded.**
A ticket "pins planned physical clusters using the existing read-lifetime mechanism"
(design.md:34), and the fixture's ticket "pins that cluster until restore/release"
(design.md:103). The existing mechanism is the counter `reads_in_flight`: while it is not zero,
a checkpoint quarantines every data cluster freed since the last one, and `guard` lifts the
quarantine only at zero (`store.rs:723-725,2043`; `state.rs:239-245`). Quarantined clusters
cannot be allocated (`flaminio-store/src/bitmap.rs:62,112`). Held for a check of three passes
(16 GiB at 32 MiB/s is 8.5 minutes each), every cluster that fio's cluster-covering writes free
(`state.rs:398-400`) stays out of reuse: free space only shrinks, and the replica can run out,
which fails S11 step 4. Also:
- the pin does not stop a write in place to the pinned block; restoring the saved bytes over a
  block rewritten since would corrupt it, and the design restores without checking the mapping
  or its checksum (design.md:111-112);
- "a local diagnostic socket pair" (design.md:102) does not say whether `fixture pin` and
  `fixture release` are lines of the control socket or of another socket.

**Fix.** Specify the pin per cluster: for example a set of pinned clusters that
`release_pending` leaves pending (allocated, not reusable) until released, with no use of
`reads_in_flight`. Have `fixture release` (or a `fixture check`) answer whether the leased
mapping and checksum are unchanged, and restore only then. Say which socket carries the lines.
Add to the script test: a restore refused after the mapping changed.

### R1-C6 — Low

**The `lab-tools` build has no path to the lab or to CI.** The fixture is built "only under
explicit `lab-tools` feature, absent from release artifacts" (design.md:101-102), and a test
checks that the "production build refuses fixture commands" (design.md:127). `flaminio-io` has
no features (`flaminio-io/Cargo.toml`); the lab builds `cargo build --release "$@"`
(`scripts/guest/build.sh:6`, `Makefile:61`); lint's clippy runs without features
(`scripts/lab.sh:241`). As written, `make test lint` never compiles the fixture, and S11 needs a
binary the deploy does not build. **Fix:** name the feature and the crates that carry it, the
lab variable that deploys it (and that the check refuses a node without it), and the
`--features lab-tools` runs added to `make test` and `make lint`.

### R1-C7 — Low

**A store poisoned during a pass can yield a false finding.** The design covers only "an already
poisoned/unusable store" (design.md:49-50; test row S3-S5: "existing poison terminates Failed").
`execute_finish` puts the new checksums in the map (`store.rs:876-878`) before the in-place
write (`store.rs:880`); if that write fails, the store is poisoned (`store.rs:743-752`) with the
map ahead of the medium. A scrub read of that block then mismatches on both reads. **Fix:** the
owner's validation checks poison first: the pass ends failed (S5) and the pending mismatch is
dropped. Test: an in-place write that fails while a scrub read of its block is in flight gives
a failed pass and no finding.

### R1-C8 — Low

**The store has no layer removal.** The design skips "deleted layers" and its tests "remove
layers" (design.md:30,37,123). The journal's records are `ChunkCreate`, `ChunkDelete`, `Fence`,
`LayerCreate`, `MapUpdate`, `DeferredWrite` and `Discard` (`state.rs:283-471`); a layer is only
pushed (`state.rs:360`). The test cannot be written. **Fix:** drop it; say that layers only
grow, that a layer's index is its identity, and that the issue that adds layer removal extends
the cursor.

### R1-C9 — Low

**On a `--buffered` node a pass would check the page cache, not the medium.** The store's device
is opened without `O_DIRECT` under `--buffered` (`node.rs:41,403`), and the design reuses the
"existing ... device APIs" (design.md:132). A finding "says the medium does not hold what the
map records" (spec.md:20). **Fix:** the pass reads through its own `O_DIRECT` descriptor
whatever the node's mode, or does not run on a buffered node and the status says why; one test.

### R1-C10 — Low

**Off the target's node, a finding names its volume only by a hash.** The spec gives "the
store's id for it, and its name when the node knows it" (spec.md:18-19; design.md:82). The id is
the first 16 bytes of SHA-256 over `flaminio-volume:NAME` (`flaminio-node/src/lib.rs:21-29`);
the replica service keys volumes by it and keeps no name (`flaminio-node/src/replica/mod.rs:62-67,150-170`).
Only an attached target knows a name, so S11's corrupt node reports an id the check and an
operator cannot match to `vol1` as the plan stands. **Fix:** say in S9 and S12 how the id maps
to a name (the rule in docs/lab.md; the fixture's answer returns the id, or the check computes
it), and cover the match in the script test.

### R1-C11 — Low

**The flags do not fit the node's parser, and the lab has no knob for them.** The design gives
`--scrub-rate-mib=32` and `--scrub-period=336h|off` (design.md:61,70). `parse` takes
`--flag VALUE` and refuses anything else as an unknown argument (`node.rs:150-207`), and no flag
takes a duration with a unit (`--poll-us`, `--gather-us`, `--commit-target-mib`). The lab passes
node flags from environment variables in `scripts/guest/node.sh:53`; S12 wants "the automatic
pass off for a measurement" in docs/lab.md, but the design's touched files name no such
variable. **Fix:** `--scrub-rate-mib N` and `--scrub-period-hours N|off` (or name the duration
parser), in `USAGE` (`node.rs:34-85`); `SCRUB_RATE_MIB` and `SCRUB_PERIOD` in `node.sh`, with
its script test.

### R1-C12 — Low

**#44's note for #46 is not in the plan.** #44 (clear) settled that blocks that differ under
equal CRC-32C values are "accepted for now, and noted on #46 (scrubbing), which compares
replicas the same way" (`origin/main:docs/issues/44/spec.md:185-189`, `decisions.md:24`). #46's
plan records step 2 as out of scope (intent.md:88-90) without it, and the cached issue has no
such comment. **Fix:** one line in intent's step 2 bullet: a comparison of digests cannot tell
such blocks apart (#44, question 5); step 2's plan starts from it.

## Nits

- **N1.** design.md:6 ("The spec's Q1–Q6 remain proposed choices") and design.md:146 ("Open
  Q1–Q6 remain") contradict the settled lines beside them; spec.md:4-6 still says each choice
  awaits a human.
- **N2.** design.md:128: "Lab procedure below"; the procedure is above (design.md:92-115).
- **N3.** Missing spaces make numbers misreadable throughout design.md: "at most64", "at
  most16,384/s", "bound262,144", "accepted1…1024", "exactly4 KiB", "after1024", "step1",
  "proposed0009".
- **N4.** design.md:86 counts "completed passes"; S9 counts "passes ended" (spec.md:94), which
  includes stopped and failed ones.
- **N5.** Line references off: design.md:17 `control.rs:49` is the `remove_file`, the mode is
  set at `:51`; design.md:10 `store.rs:353–374` and intent.md:14 `store.rs:362` for
  `ReadPlan::verify`, which is `:366-379`; intent.md:35 `replica/mod.rs:391` for `Reply::Close`,
  which is `:402-404`.

Reviewed commit: f6bb5830547e1c3a385f0a7308dc27b85a0c959f
