# Issue #86 — plan review round 3 (incremental), focus: design-code

Diff reviewed: `git diff 769b21e..3d89853 -- docs/issues/86/ ':!docs/issues/86/report.md'`.
The reviewer wrote no file; this is its returned report, saved by the orchestrator.

## Earlier findings

All five round-2 findings are addressed.

## What fits (checked against dataplane/crates)

- Mutation switch: `replica::Bugs` (`flaminio-repl/src/replica.rs:93`) is `#[cfg(test)]`, wired in by `with_bugs` (`sim/mod.rs:446-449`); `tests.rs:369` has a `replica(|b| ..)` helper. `accept_foreign_epoch` fits.
- Fault default: like `local_read_fail_pct`, draw from the rng only when the percentage is above 0, so pinned seeds stay fixed. `Config::random` must not set `forge_epoch_pct`.
- Where to forge: `Sim::send` (`sim/mod.rs:648`).
- Mutation catch: the mutant's `retain(w >= p)` with `p` = (epoch+1, ..) clears every record (`replica.rs:231-238`); a forged Settle bound clears records and `restarted_at` (`:250-253`); a later Hello under-covers against the shadow: `uncertainty not covered`.
- Clean-run recovery: a dropped Write or Settle leaves an inflight entry; `tick_inner` raises `SessionFailed` after `request_timeout` and `fail_peer` reconnects (`target.rs:811-830,1214`).
- Hello check order and the `create_moved_chunk` preflight fit `flaminio-node/src/replica/mod.rs` (195-221, 268, 349-352).

## Findings

### R3-C1 — Medium

The forged-epoch fault is not restricted to chaos. `try_seed` (`sim/tests.rs:230`) panics when a seed fails with no bug, so a clean run with `forge_epoch_pct` on must never fail, liveness included. A dropped Write times out and the peer leaves the ISR; if forging continues through the settle phase, runs can stall on `liveness:` and the mutation search panics instead of counting a catch. `local_read_fail_pct` is limited to chaos for this reason (`sim/mod.rs:746-757`). The design does not say whether the fault is, nor what happens when the target's own replica (`from == to`) is forged against. Fix: "forged only while `self.chaos`, drawn only when the percentage is above 0"; and a clean campaign with the fault on (for example 200 seeds at 20%) as a named test, since today "clean runs pass" is only a risk note.

### R3-C2 — Medium

The node test volume is too small for the new tests. `accepted()` (`flaminio-node/src/replica/tests.rs:60-86`) sets `size: 4 * CHUNK`: chunk indices 0 to 3. `moved_chunk_over_budget_is_refused_before_create` needs a fifth chunk beyond budget 4; `create_moved_chunk` returns `out of range` first (`mod.rs:277`), so the test passes for the wrong reason. The "5 chunks of A" setup of `unsupported_volume_is_refused_and_kept` has the same constraint. Fix: name a helper with a volume of at least 6 chunks (or a size parameter) and assert the refusal reason is the budget error.

### R3-C3 — Low

Spec S1 says the refusal happens "before any create, fence or core change", but the design runs the check after the earlier-incarnation delete, which changes the store and the core (`mod.rs:195-210`). The design's order is right. Fix: reword S1 to "before any create or fence, and after discarding an earlier incarnation's chunks"; add the test case (over-budget Hello with a higher `first_epoch` refuses, the earlier incarnation's chunks are gone, no chunk of the new volume is held).

### R3-C4 — Low

The design's "Mutations" row does not say how `accept_foreign_epoch` is made catchable: it needs a test-config closure with `forge_epoch_pct` above 0 (for example `|s| Config { forge_epoch_pct: 20, ..Config::random(s) }`) as the `mutation!` config argument, and a seed budget recorded after a real search. Fix: name the closure in the design.

Reviewed commit: 8e355fe6dbf4bb2f9dfdb78804d824b4d13d48d5
