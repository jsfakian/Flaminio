# Issue #86 plan review — round 2 (incremental), security focus

Scope: `git diff 412bf8a..769b21e -- docs/issues/86/ ':!docs/issues/86/report.md'` in
`/tmp/flaminio-plans-2026-11-12/86`, checked against the code of that worktree. HEAD adds only
`report.md` on top of 769b21e.

## Earlier findings

- R1-S1: fixed. Spec S4 and design "Unsupported volume" place the mark outside `vols` and the
  core, refuse before the earlier-incarnation delete whatever the first epoch, leave chunks and
  fences alone, name the reason's surface and the two exits; `unsupported_volume_is_refused_and_kept`
  covers it.
- R1-S2: fixed. Coarsening is per chunk and not sticky; only the written chunk (or, for an add,
  the lowest-numbered chunk holding records) gets a cover; a cleared cover is precise again; the
  two triggers and the cost are in S9, the risks and the ADR; three tests pin it.
- R1-S3: fixed. An empty row is stated to carry a missing row's trust; the refusal half of the
  S7 test is dropped.
- R1-S4: fixed (the drop rule is in spec S3 and the design). Its stated rationale is wrong: see
  R2-S3.
- R1-S5: fixed. A reply to Hello must be a valid Fenced before `session.open` and
  `confirmed_until`; Stale deposes first; `hello_answered_ok_grants_nothing` added.
- R1-S6: fixed. Whichever of #80 and #86 lands first adds the entry point; consistent with #80's
  design lines 104-113.
- R1 Nit (mutation name): fixed.

What holds in the revision (not findings): a peer can coarsen only chunks of the volume its
current session is fenced on (`replica.rs:185-190` drops every other session), and only the
chunk it writes or, through a Hello or `BeginResync` create, the lowest-numbered chunk holding
records. Coarsen/clear cycles cost O(1) plus one free per record that a write had to allocate
first, so there is no amplification; arena slots stay at most B. With N<B the record set is
non-empty whenever N+R=B, so an add always frees a slot. Every chunk reaching `Ready` in a new
epoch is sent a Settle (`target.rs:2262-2271`), and a current session's epoch equals the fence
(`replica.rs:174-176,185`), so with the drop rule a cover cannot outlive the next takeover.
Honest targets send only their own epoch (`target.rs:1356-1363,2486-2489`). An invalid Fenced
gives a peer nothing beyond what not answering gives it. `MAX_UNCERTAIN` and `MAX_CHUNKS` are
already 65,536 (`frame.rs:18,20`). The store refuses a chunk above `MAX_CHUNK_SIZE` at replay
(`flaminio-store/src/state.rs:300-304`). The control socket's replica delete is refused only
while the volume is attached or attaching here (`engine.rs:696-704`), so the stated exit exists.

### R2-S1 — Low

**The over-budget Hello refusal is placed "before any store call", which the earlier-incarnation
delete contradicts.**

Design.md:63-64 says `hello()` refuses a Hello that would make N>B "before `core.handle` and any
store call"; spec S1 says "before any store or core change". The earlier-incarnation delete
(`flaminio-node/src/replica/mod.rs:194-212`) is a store call that runs first and changes N: it
removes the earlier volume's chunks and its `vols` entry. Read literally, the check runs before
it and counts the earlier incarnation's chunks. A new volume of the same name whose own set fits
is then refused when the earlier set plus the new one exceeds B, and since the refusal precedes
the delete, every retry is refused until someone runs the replica delete. The quarantine
paragraph deliberately says "before the earlier-incarnation delete", which makes the literal
reading the likely one. Placed after the delete, the check is right, but then the two sentences
are false.

Fix: state the position. After the quarantine check, the earlier-incarnation block and the
geometry-mismatch check; before `vol_id`, `core.handle`, any create and the fence; N counted on
what the delete left. Reword spec S1 to "before any create, fence or core change". Add to
`hello_over_budget_is_refused_before_create` a case where an earlier incarnation holds chunks
and a Hello with a higher `first_epoch`, whose set fits alone, succeeds.

### R2-S2 — Low

**The moved-chunk create at N=B has no node test, and a missed preflight is now durable harm.**

Design.md:54-56 says node adapters "preflight before a store create". The node row of the test
table covers only the Hello path. The other create is `create_moved_chunk`
(`mod.rs:268-292`), reached when the current target sends `BeginResync` for a chunk the node
lacks (`replica.rs:261`, `mod.rs:347-354`). It calls `store.create_chunk` and then
`core.add_chunk`. If the implementation checks only in `add_chunk`, the store ends with B+1
chunks while the core holds B, and under the new quarantine rule the next restart finds more
than B chunks and takes the whole volume out of service on that node. That needs a volume with
more than B chunks, which is the case the plan says it defends when #80's admission is bypassed
(spec S9).

Fix: name a node test, on `ReplicaService::with_budget(store, 4)`: with four chunks held, a
`BeginResync` for a fifth returns `Reply::Close`, `store.chunks()` is unchanged, and a restart
on that store does not quarantine the volume.

### R2-S3 — Low

**The reason given for the epoch check is wrong, and the test wording lets a weaker check pass.**

Design.md:87-89 and decisions.md (R1-S4) say that without the check "reports only over-cover".
The code says otherwise. A Write's prune or a Settle's bound with an epoch above the session's
clears restart cover (`replica.rs:235-237,251-253`) and every record of the chunk (`:234,250`),
so the next report under-covers; a Write whose version epoch is below its prune's is pruned as
it is recorded (`:232-234`). The sender is the authenticated current target, so this crosses no
trust boundary, and no simulator mutation is possible while the simulator's targets are honest:
unit tests and no mutation stand. But the check is also what enforces S3's "restart cover
clears only with a strictly newer epoch" against a faulty target, and a reader of the stated
purpose ("M never exceeds the fenced epoch") could keep only `version.epoch > m.epoch`. The S3
test row ("an epoch other than its session's is dropped and changes no report") passes with
that weaker check if it tries only a version.

Fix: correct the sentence in design.md and decisions.md: the check bounds M and also closes an
under-cover path from a forged prune or Settle epoch. List the unit cases: version, prune and
`below`, each lower and higher than the session's epoch; and a higher prune and a higher `below`
against a restart-covered chunk, whose row stays whole.

## Nits

- The design lists fixtures to change for empty rows but not for the drop rule. The node test
  helper `write()` hard-codes `Version { epoch: 1, .. }`
  (`flaminio-node/src/replica/tests.rs:88-100`) and is used on sessions at epochs 5 and 2
  (`tests.rs:169-170,318-320`); the second expects `Frame::Ok`. Say that the helper takes the
  session's epoch, so nobody relaxes the check to make these pass.

Reviewed commit: 344106d41ed47b944ae2d8c0d44c32e463c6aaca
