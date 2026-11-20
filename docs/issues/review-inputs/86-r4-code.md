# Issue #86 — plan review round 4 (incremental), focus: design-code

Diff reviewed: `git diff 3d89853..3f063b8 -- docs/issues/86/ ':!docs/issues/86/report.md'`.

## Earlier findings
- R3-C1: fixed. The fault runs only while `self.chaos` and draws from the rng only above 0 (design.md "Simulator check", as `local_read_fail_pct`, `sim/mod.rs:746-757`); `forged_epochs_leave_runs_clean` (200 seeds, no bug, liveness included) is named.
- R3-C2: fixed. `accepted_in` with 6-chunk volumes; refusals asserted by text (`out of range` is `replica/mod.rs:278`, the condition at :277); `create_moved_chunk` wraps its reason as `create chunk N: ...`, so `more than 4 chunks` matches.
- R3-C3: fixed. S1 reworded; case (b) and (c) pin the check after the earlier-incarnation delete (`mod.rs:195-210`), and (c) asserts the earlier chunks are gone.
- R3-C4: fixed. `forged` is a `fn(u64) -> Config`, matching `Configure` (`sim/tests.rs:213`) and the `mutation!` four-argument arm (`sim/tests.rs:386`).

## Changed lines checked
- Forge in `Sim::send` (`sim/mod.rs:648`): `Body::ToReplica` carries `epoch` and `Request::{Write{prune}, Settle{below}}`, so "session's epoch + 1" is available there; self-sends (`from == to`) pass through `send`.
- Refusal texts and test substrings are consistent between design.md (:68, :77, :82), decisions.md and the test rows; spec S1/S3/Tests agree with design.
- Own-epoch bound left unchecked is stated in the ADR, spec, design, decisions and risks consistently.

## Findings
None.

Reviewed commit: 3f063b8 (checkout HEAD f685343f892f2a1f123ec985821a019422f9a640 as reported by git log in the worktree)
