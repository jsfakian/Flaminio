# Issue #86 plan review — round 3 (incremental), security focus

Scope: `git diff 769b21e..3d89853 -- docs/issues/86/ ':!docs/issues/86/report.md'` in
`/tmp/flaminio-plans-2026-11-12/86`, checked against the code of that worktree. HEAD (8e355fe)
adds only `report.md` on top of 3d89853.

## Earlier findings

- R2-S1: fixed. Design.md:67-71 states the position (after the quarantine check, the
  earlier-incarnation block and the geometry-mismatch check; before `vol_id`, `core.handle`, any
  create and the fence), N counted on what the delete left; spec S1 and decisions.md agree. The
  added test case is too loosely worded to pin it: see R3-S2.
- R2-S2: fixed. `create_moved_chunk` asks the core before the store create (design.md:56-60);
  `moved_chunk_over_budget_is_refused_before_create` is named and matches the code path
  (`flaminio-node/src/replica/mod.rs:349-351` returns `Reply::Close`).
- R2-S3: fixed. The rationale is corrected in design, spec S3, the ADR and decisions.md; the six
  unit cases and the two restart-cover cases are listed; a simulator fault and the mutation
  `accept_foreign_epoch` are added. The corrected wording now claims slightly more than the rule
  gives: see R3-S1.
- R2 Nit (helper `write()`): fixed.

What holds in the revision (not findings):

- No honest request is refused. The target stamps its one epoch on the session, the version,
  the prune and the Settle bound (`flaminio-repl/src/target.rs:1196,1357,1361,2487`); a target
  never changes epoch (a newer view deposes it, `:1100`); the engine and the node convert these
  fields unchanged (`flaminio-node/src/engine.rs:2366-2405`, `replica/mod.rs:915-937`). A prune
  or `below` of an older epoch is never legitimate: sequence numbers restart at 0 in each
  target (`target.rs:486`), so a bound always speaks of the sender's own epoch.
- The silent drop cannot stall an honest target, which never triggers it. A faulty one sees the
  request time out and the session fail (`target.rs:811-827`, Settle included), the existing
  treatment of a protocol error (`replica.rs:280-282`). The drop holds nothing on the node
  (`mod.rs:359`, `Reply::Nothing`, no pending write).
- The earlier-incarnation delete before the budget check gives a peer nothing new. It is keyed
  on `first_epoch`, a signed token claim (`flaminio-net/src/token.rs:117-132`), and already runs
  before two other refusals (geometry mismatch `mod.rs:213-220`, a failed create `:252-254`).
  After it N is 0 for a new incarnation, so the refusal then needs an advertised set above B.
- With the mutation on, a forged prune of epoch + 1 prunes the write it carries and clears
  restart cover (`replica.rs:232-237`), while the shadow keeps both: the check can catch it.

### R3-S1 — Low

**The corrected rationale says the drop closes the under-cover path; the same-epoch half of
that path stays open, and the ADR sentence is false for it.**

Design.md:93-95 names two harms: a bound of a higher epoch clears records and restart cover,
and "a version below its own prune would be pruned as it is recorded". The epoch drop stops the
second only when the epochs differ. A Write with `version = (e, 5)` and `prune = (e, 100)` on a
session at epoch e passes the check, is written to the store (`Action::Write`) and is never
recorded: design.md:48-50 keeps "retain the new write iff Version≥prune", as
`replica.rs:232-234` does today. Likewise a Settle with `below = (e, u64::MAX)` clears every
record and every coarse cover of the chunk (M<p). The ADR says "a forged bound then cannot clear
records or restart cover": true for restart cover, false for records. The sender is the
authenticated current target, so no trust boundary is crossed, and a same-epoch Settle bound
cannot be checked by a replica at all. But the rule is now called a safety rule against a faulty
target, and a reader will take the stated harm as closed.

Fix: say what the rule gives. In design "Prune and restart proof", spec S3 and the ADR: the drop
makes every bound speak of the fenced epoch, so restart cover and records of other epochs cannot
be cleared by a forged epoch; a same-epoch bound stays the session holder's own claim and is
not defended. Add that as a line under Risks. Optionally drop a Write whose `version` is below
its own `prune` (honest targets never send one: `target.rs:1351` takes the oldest write in
flight, which is at most `seq`), with one more unit case; if not, say it is left as today.

### R3-S2 — Low

**The earlier-incarnation case of `hello_over_budget_is_refused_before_create` can pass with
the check in the wrong place.**

Design.md:203 says "with an earlier incarnation holding chunks, a Hello with a higher
`first_epoch` whose own set fits succeeds". If the new Hello advertises the indexes the earlier
incarnation holds (the natural fixture: `accepted(epoch, session, &[0, 1])` twice), a check
placed before the delete finds no missing chunk (`mod.rs:239-246` filters on the held set),
passes, and the test succeeds. The position R2-S1 asked for is then untested. The fixture
`accepted()` also describes a 4-chunk volume (`flaminio-node/src/replica/tests.rs:80-84`), so
with a budget of 4 no advertised set can exceed it until the helper takes a size.

Fix: state the case exactly: the earlier incarnation holds k chunks and the new Hello advertises
chunks it did not hold, with k plus the new set above the budget and the new set alone within it
(budget 4: held {0,1,2}, advertised {3,4} on a volume of at least 5 chunks). The Hello succeeds
and `store.chunks()` holds only the new chunks.

## Nits

- Spec S1 (spec.md:28-30) says the refused Hello comes "before any create, fence or core
  change" and "Neither deletes data". At the position the design now states, the
  earlier-incarnation block has already run `store.delete_volume` and `core.remove_volume`
  (`mod.rs:201-211`). Say that the refusal changes nothing of the volume the Hello names, and
  that an earlier incarnation's chunks and core state are removed first, as today; otherwise a
  test asserting an unchanged store pushes the check back before the delete.

Reviewed commit: 8e355fe6dbf4bb2f9dfdb78804d824b4d13d48d5
