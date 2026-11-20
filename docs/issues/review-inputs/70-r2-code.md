# Issue #70 — plan review, round 2, code focus (design-code)

## Earlier findings (round 1, code reviewer)
- R1-C1: fixed (online resize cut; no session refresh text remains).
- R1-C2: fixed (no pending growth; every step is a complete volume).
- R1-C3: fixed (XFS, use-state, local intent cut; NodeExpand only reads #79 records).
- R1-C4: fixed as a settled choice (flag + documented rollout order; an automatic check stays open).
- R1-C5: fixed (labels, modes, probes cut; grep finds no remnant in spec, design, ADR, decisions).
- R1-C6: fixed (no header field; flag default off; JSON-keys test in `TestExpandAppendsChunks`).
- R1-C7: fixed (use-state protocol cut).
- R1-C8: fixed for the data-plane rule (`grown_view_keeps_prefix_isr_changes`, `drop_grown_view`); I judge the bug catchable (see below).
- R1-C9: fixed (`ExpandVolumeRequest {id, size_bytes}`; #80 signed-range helper).
- R1-C10, C11, C12, C13, nit: fixed (legacy test; `resize2fs` from the pinned base's e2fsprogs; lab on the 7.0 VMs with lab.sh/meta.sh/Makefile in Files touched; decisions.md exists).

Cut text: gone from spec, design, ADR, decisions and intent (grep for online/UPDATE_SIZE/pending/use_state/growth-v1/RESERVED/expansion-mode found only statements that these are out of scope).

Checked and holding: 2 compares + at most 65 ops per step against `kv.MaxTxnOps=120` (`meta/kv/kv.go:33`); every chunk-record writer compares the header (`service.go:774-790`, `:907`); `Attach` only bumps the epoch under OnlyIfTarget at NodeStage (`control/csi/node.go:70`), so a restage gets a new epoch and a Hello above the fence; Hello always returns `Action::Fence` when epoch >= fence (`flaminio-repl/src/replica.rs:170-184`), so the Fence arm is the right place for the size raise; `apply_view` with `take(len)` removes the panic at `target.rs:2510`; the injected bug makes the scripted scenario's ISR change never apply, so it settles never; `auth.go:64` is the admin case; the cited line numbers are right.

## Findings

### R2-C1 — Medium
**The change to #80's engine refresh guard has no file and no test for its refusal half.**
Design "Engine refresh" says the engine path is "unchanged" and then says "#80's refresh guard passes a same-epoch view larger by whole chunks ... a smaller one stays refused". #80's design (`80/design.md:78-86`) makes the guard match the immutable attached size and exact record count and says #70 needs "its explicit validated growth transition". That transition is code in `flaminio-node/src/engine.rs` (the `MetaWait::Refresh` arm, `engine.rs:1544-1579`, plus #80's validator), but `engine.rs` is not in Files touched, only `tests/engine.rs`. The test table has `a_grown_view_leaves_the_running_target_alone` (larger passes) but nothing for the other half: smaller view, a larger one with different chunk size/RF/minISR, or a larger one whose record count is not size/C.
**Fix:** name `engine.rs` (and #80's validator module) in Files touched; state that the guard keeps the attached size in `TargetRt` and only compares, never adopts, the larger one; add engine tests for smaller, not-whole-chunk and changed-geometry views (refused, not installed, apply_view not called).

### R2-C2 — Medium
**"csi-resizer with default flags" cannot start.**
Design "CSI" (deploy): the sidecar is added "with default flags". csi-resizer's `--csi-address` defaults to `/run/csi/socket`; this pod's socket is `/csi/csi.sock` (`driver.yaml:67-68,76-77`), and every other sidecar passes `--csi-address`, `--leader-election`, `--leader-election-namespace`, `--timeout=60s`. Its `--timeout` default is 10 s, shorter than a multi-step growth (up to 80 steps, each with a gate read, a node list and a transaction), so the resizer would cancel calls and retry.
**Fix:** specify the args as for the others (`--csi-address=/csi/csi.sock --leader-election --leader-election-namespace=flaminio-system --timeout=60s`) and `securityContext: *restricted`, `socket-dir` mount; say the lab check waits for the resizer pod Ready.

### R2-C3 — Medium
**The replica rule "raise the known size only after chunk creation and the store fence succeed" has no test that can fail, and the slot-exhaustion path is described wrongly.**
S5 and design "Replica Hello" make the ordering the safety point (R1-S8). The tests listed (`a_larger_hello_raises_the_size`, `a_stale_larger_hello_changes_nothing`, `a_smaller_hello_is_refused`, `recovery_keeps_the_largest_size`, `a_full_replica_refuses_the_grown_volume`) never make `create_chunk` or `store.fence` fail after a larger Hello passes the size check, so an implementation that raises the size before the Fence arm passes them all. `a_full_replica...` does not reach it either: with #80's `preflight_chunks` (80/design.md "Add Store read-only preflight_chunks ... Call before Hello's old-incarnation cleanup") slot exhaustion is refused before the Fence arm, yet the design (Replica Hello, "Slots exhausted") says the Fence arm fails the session "as today (:252-254)".
**Fix:** add `replica/tests.rs` `a_failed_creation_keeps_the_old_size` (fault-injected store write failing after one suffix chunk: known size, fence and a following smaller-size Hello at the old epoch behave as before; a retry with the larger size succeeds and creates the rest). Say that slot exhaustion is refused by #80's preflight and that the Fence-arm failure is the injected-fault case. Also cover the local replica path (`engine.rs:1955`) in `a_full_replica_refuses_the_grown_volume`, since S5 says local and remote follow the same rules.

### R2-C4 — Low
**Several stated rules have no case in the test table.**
- Metadata step 4 (`Aborted` on exhausted attempts, `Unavailable` KV error, `Canceled`/`DeadlineExceeded`, "committed steps stay in every case") is untested; `interleaveKV` (`meta/service/service_test.go:619-670`) can run one hook and count writes but injects no error, and its hook runs once, so exhaustion and a KV error need a different wrapper that the design does not name.
- `TestNodeExpand` omits the pin mismatch of step 3 (`FailedPrecondition`, no effect), the stage not in phase `ready`, a publication leading to its stage, the child being killed at context end, and the 4 KiB output cap.
- `TestControllerExpand` has no case where `Meta.ExpandVolume` returns a size above the requested T (a concurrent larger request) or where V > limit with an unchanged V.
**Fix:** add those cases, and name the failing-KV wrapper (fail the nth `Txn`, return a canceled context) in `expand_test.go`.

### R2-C5 — Low
**`TestExpandSteps` relies on a hook that cannot fail a later step as described.**
Design test table: "+150 chunks is three transactions, the second failed by `interleaveKV`". A compare failure is retried up to `retries=32` and then succeeds; only a hook that makes the second step fail for good (for example, one after the first `volumes/<id>` write that drains every node, giving `ErrPlacement`, or deletes the volume) does what the sentence needs, and the test must say which.
**Fix:** state the hook (after the first applied write of `volumes/<id>` via `txnKey`, drain all candidates) and the expected `FailedPrecondition` with +64 committed and readable.

### R2-C6 — Low
**The simulator work lists less than it needs.**
Design "Files touched" and the test row name `Config::grow_pct`, `Metadata::grow`, `Report::growths`, but a grown volume in the sim also needs the replicas to create the suffix chunks at the next attach (`sim/mod.rs:392-396` creates chunks only at build) and clients and moves to draw from the grown count (`:1049`, `:1262` use `cfg.chunks`); without that the "failovers onto the larger size" half of the sweep exercises nothing beyond the prefix.
**Fix:** say which of the three the sweep changes, and that `Report::growths > 0` is paired with a check that a post-growth failover target serves a suffix chunk (a write to it completes and a later read returns it).

## Nits
- `adr-proposal.md` Options still says "(the first draft)" about the cut design; a reader of an ADR will not know which draft. Name the cut alternative without the draft history.
- `design.md` facts say the Hello refusal is "`replica/mod.rs:213–220`, `:234`"; `:234` is the `Action::Fence` match arm, not the refusal. Cite `:213-220` and `:235-258`.

Reviewed commit: 0138cbeb6bbff98488c6dc0336204ed79f4fe007 (HEAD; only report.md differs from 4057eb9 in docs/issues/70)
