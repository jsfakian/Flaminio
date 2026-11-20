# Issue #86 — plan review, round 1, code focus (intent, spec, design, adr-proposal)

Checked against the worktree at `/tmp/flaminio-plans-2026-11-12/86`. Paths below are under
`dataplane/crates/` unless stated. Nothing was built or run.

Verified and correct: every file:line cited in intent and design; the body formula
13+8N+8T, MAX_BODY 1,048,704, 1,048,589 at N=T=65,536 (115 spare); the two probe sizes
(524,317 and 1,048,717); prune-then-retain is equivalent to today's push-then-retain; the
coarse rule (clear only when max < p) never clears what the precise rule keeps; restart cover
is unchanged. #80 agrees on N <= 65,536, MAX_BODY, the `encode_checked` signature,
`SessionError::Encode` and the two probe sizes. #81 shares no bound or contract with this plan.

### R1-C1 — High

**Volume-wide sticky promotion is reached by ordinary workloads, not only by "attack or
fragmentation", and turns every later takeover into a whole-volume reconciliation.**

The budget counts raw records, and a write prunes only the chunk it writes
(`flaminio-repl/src/replica.rs:231-238`). `Settle` is sent only after reconciliation or a join
(`flaminio-repl/src/target.rs:2271,2579`). So a chunk that goes idle keeps the records of the
in-flight window at its last write: up to the queue depth, for sequential I/O. R is therefore
about (chunks touched) x (queue depth), and it never shrinks.

With the defaults (chunk size 256 MiB, `control/csi/controller.go:71`; ublk depth 128,
`flaminio-io/src/node.rs:134`), one sequential fill of a 128 GiB volume gives N=512 and
R about 65,536 > B-N. Under #80's largest RF=3 geometry (N=3,119) a mean of 21 records per
chunk is enough.

Design "One bounded arena" and spec S2 then cover every held chunk and stay coarse until the
process restarts. After that each written chunk stays whole-covered (its last write is never
pruned), so the next target failover digests the whole volume instead of a few extents per
chunk. Safety holds, but design "Lab, docs, risks" ("sticky imprecision after
attack/fragmentation") and the ADR's Consequences misstate the trigger, and S8 bounds none of
this work.

Fix, one of:
- Coarsen per chunk, on demand: when N+R would exceed B, replace only the written chunk's
  records by its cover; a chunk returns to precise when its cover clears (a cleared cover
  represents no record, so no extra proof is needed). N+R <= B still holds and work stays
  amortized O(1) per record.
- Stop the idle residue at its source, e.g. let a write's prune drop same-epoch records below
  it on every chunk of the volume. This is a new replica rule and needs its own simulator
  check and mutation.
- Keep the design, but state the real trigger and cost in spec S2/S9, the design's risks and
  the ADR, with the numbers above, and add a test that a sequential fill at the default depth
  on a default-geometry volume does or does not promote.

### R1-C2 — Medium

**The unsupported-volume quarantine has no mechanism and no test; nor do two other
design-only checks.**

Spec S1/S4 and design "One bounded arena" (last paragraph) say an over-limit legacy volume is
"marked unusable" by the constructor, the node keeps serving the others, and disk bytes are
preserved. Not specified:
- where the mark lives (`ReplicaService::new` is infallible and builds `vols` and the core
  together, `flaminio-node/src/replica/mod.rs:127-146`);
- what a `Hello` for that volume gets, and whether the check runs before or after the
  earlier-incarnation delete at `mod.rs:194-212`;
- whether `ReplicaService::delete_volume` (`mod.rs:813`) still works on it, which is the only
  existing cleanup path, and how the volume leaves quarantine;
- whether the volume's fenced epoch is still kept.

The rule-to-test table has only `replica.rs` unit tests for S1/S4. Nothing tests the
constructor quarantine, serving a second volume beside it, the preserved bytes, the
`Engine::start_target` count check, or that `UNCERTAINTY_BUDGET`, `MAX_UNCERTAIN` and
`MAX_CHUNKS` agree ("agree by contract" has no assertion).

Fix: specify the four points above; add node tests (a MemDevice store holding B+1 chunks of
one volume plus a supported volume: refused Hello, the other volume served, inventory
unchanged, delete still works), an engine test for the `start_target` refusal, and a
compile-time assertion in `flaminio-net` that the three constants are equal.

### R1-C3 — Medium

**"Preserve #13 NotHeld / #61 bootstrap refusal" names behaviour the code does not have, and
its test cannot be written on this base.**

Spec S7, design "Report construction" and the ADR say to preserve a missing-holder refusal.
No such refusal exists: `hello()` creates every advertised chunk it does not hold, with no
ISR information available (`flaminio-node/src/replica/mod.rs:239-257`), and `NotHeld` appears
nowhere in the data plane. #13 and #61 are unimplemented drafts in other worktrees. The S7
row of the test table ("explicit known-fresh empty rows versus missing-holder bootstrap
refusal") therefore has nothing to exercise.

Fix: say that this issue leaves today's create-on-Hello unchanged, so an empty row carries
exactly the meaning a missing row has today; that #13/#61 must define how `NotHeld` or a
bootstrap refusal interacts with the required-row rule when they land; and drop the refusal
half of the S7 test, or mark it as owned by #13/#61.

### R1-C4 — Low

**How the engine and the simulator obtain the captured Hello chunk set is not specified, and
a Target accessor alone does not work for the simulator.**

`Request::Hello` is a unit variant (`flaminio-repl/src/msg.rs:32`); the engine recomputes the
list from its own `rt.view` (`flaminio-node/src/engine.rs:1927-1934`). Design "Report
construction" says only "Target exposes the same read-only set to engine/simulator". The
simulator delivers packets later (`sim/mod.rs:596,696`), when the sending target may have
failed the session or been replaced by a new epoch's target, so the replica adapter cannot
ask the Target at delivery time.

Fix: carry the set in the message (`Request::Hello { chunks }`), which the engine copies into
`Frame::Hello`/`Accepted` and the simulator's `Replica::handle` uses to create the missing
chunks; or state that the adapter reads an accessor keyed by (peer, session id) at send time
and stores the set with the packet.

### R1-C5 — Low

**#80 and #86 each name the other as prerequisite, and neither says which one introduces
`encode_checked`.**

Spec S6 and design "Checked codec" say "extend #80's" `Frame::encode_checked`, `EncodeError`
and `SessionError::Encode`; none exists (`flaminio-net/src/session.rs:30-39,59-63`). #80's
design (lines 111-114) says #86 "must land before" its guarantee is complete, while spec S9
and design "even without #80" require #86 to work alone.

Fix: state the landing order, or that whichever lands first adds `encode_checked`,
`EncodeError`, `SessionError::Encode` and the `Conn::send` change with the agreed signature,
and the other only adds its frame's preflight.

### R1-C6 — Low

**Two state transitions of the core are left open.**

- `ReplicaCore::restart()` (`replica.rs:136-145`) models a process crash in the simulator,
  but the design does not say whether it resets Coarse mode. A real restart builds a fresh
  core (`mod.rs:127`), so a sticky mode across `restart()` makes the model differ from the
  node and stops the simulator from exercising precise state after the first promotion.
- When adding a chunk triggers promotion, "for every held chunk ... covered:true" and "Added
  empty chunks start known-empty" (design "One bounded arena") disagree on the new chunk.
  The S7 test expects an explicit empty row for a newly created chunk.

Fix: state that `restart()` returns the volume to precise mode with an empty arena, and that
promotion runs before the insertion, so the new chunk starts known-empty; name both cases in
the S1/S2/S4 test row.

### R1-C7 — Low

**The mutation harness would pass over the new checker's failures.**

`try_seed` counts a failure as a catch only if it contains "not linearizable" or "replicas
diverge" (`flaminio-repl/src/sim/tests.rs:238`); anything else is skipped.
`bypass_uncertainty_budget` can only fail the new state checker, and `drop_on_coarsen` /
`coarse_prune_early` are meant to be caught by the coverage shadow. The design's Mutations
row does not say the catch classes are extended, so these mutations would report "not
caught". `ReplicaCore` also derives `Default` (`replica.rs:101`), which would give a budget
of 0 once the budget is a field the simulator can set to 8 or 16.

Fix: name the new error strings and add them to `try_seed`'s catch classes; say how the
budget is configured (a constructor or field with an explicit `Default` of 65,536).

## Nits

- Spec Tests names the mutation `bypass_budget`; the design's Mutations row names it
  `bypass_uncertainty_budget`. Use one name.
- Missing spaces before numbers change the reading in places: design "Checked codec" `tag64`
  (a one-byte tag of value 64, `frame.rs:141`, not a 64-bit tag), spec S5 `body13+8N+8T`,
  design "Lab, docs" `audit0007/#83`. Write `tag (u8, 64)`, `body 13+8N+8T`, `ADR 0007`.

Reviewed commit: 412bf8ae49616116170f5b125d6c0d63d5f10488
