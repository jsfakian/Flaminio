# Issue #68 — independent model checks

Proposed review revision, 2026-11-20; unapproved. This is a normative part of the
[design](design.md), confined to the isolated hydration facade and simulator.

## Client operands and register transition (S4, S5, S10)

Use a bounded `ModelBlock = [u64; 4]`: four symbolic lanes of a logical 4 KiB block.
This does not change production `types::Value` or claim byte-accurate physical-cluster coverage.
The driver records `HistOp { incarnation, epoch, chunk, block, invoke, result, input }`
at client invocation, with `input = Read | Replace(ModelBlock) | Patch { mask: u8,
lanes: ModelBlock }`. A Patch has a nonzero four-bit mask; only selected lanes are operands.
History input is immutable: never replace Patch with the target's merged value, a fetch result
or replica contents. Reads record the entire returned block; hydration is no client operation.

The separate `sim::hydration::check` linearizability search starts from the immutable test
fixture's base block, not an actor's installed/fetched value. Its own transition for Replace
sets the register to the original operand. For Patch, independently set each selected lane to
its original operand and retain every unselected lane from the checker's preceding register.
No implementation merge helper is shared with this checker. Search respects invocation/return
order and explores overlapping writes, memoizing `(linearized_set, ModelBlock)` as today.
Successful writes are required; unanswered writes are optional once, with the existing
later-epoch-completion deadline. Definitely rejected, effect-free writes are omitted; an
indeterminate write retains its original operand as optional. Failed reads add no value claim.

Deterministic cases start at `[11, 22, 33, 44]`, patch lane 1 to 99 and then require
`[11, 99, 33, 44]`; a subsequent read of an adjacent remote block must retain its distinct
nonzero base. Overlap a lane-1 patch with a full replacement `[51, 52, 53, 54]`: a read after
both return permits exactly `[51, 52, 53, 54]` or `[51, 99, 53, 54]`, according to serialization.
Also overlap two disjoint partial writes, then delay hydration across their completion and
failover. Each clean scenario passes. `partial_update_zero_fill` makes the implementation
zero unselected lanes; the unchanged independent checker must reject the first scenario.
Seeded runs generate original masks/operands before scheduling and use this same oracle.

## Destination/base pin lifecycle (S9, S10)

The driver keeps immutable requested `Origin[dest_incarnation] = { base: BaseId, geometry }`
from the client create input. Metadata actors own durable `Pins` keyed by the exact
`(dest_incarnation, BaseId)` and durable admission/Complete/deletion commits. A recreated
destination has a fresh incarnation even when its name and base match. These pins are separate
from #52's source-reader grants. Restart reloads modeled durable pins/commits; it never invents
an acquisition from the continued availability of the grow-only repository.

An independent monitor records actual actor events `PinAcquire`, `HydratingCommit`,
`CompleteCommit`, `DeleteCommit` and `PinRelease`, each carrying the exact destination/base
key; only a successful modeled durable transition emits a commit event. It retains its own
sets of admitted and terminal incarnations and acquisition/release history, initialized empty.
After every model transition, including restart, check:

- Every admitted, nonterminal destination has exactly its requested pin in durable `Pins`.
- Every Hydrating exposure/commit requires that exact pin already durable, or atomically
  acquired by the same transition; a wrong base or previous incarnation cannot satisfy it.
- Every release event has a preceding successful Complete or deletion commit for that exact
  requested destination/base. Sending a request, local all-Local state, a rejected/stale CAS,
  outage or a lost reply without commit never supplies terminal evidence.

The monitor compares its event-derived expectation with the actor's durable pin state; neither
the expected origin nor terminal permission is read from the actor's current phase/pin flag.
Retain terminal evidence through retries so a lost Complete reply permits release only if
Complete actually committed. A stale callback for a deleted incarnation cannot release the
pin of a recreated destination. Retaining a pin after a terminal commit is safe and retryable.

Deterministic and seeded cases cover crash/retry before and after acquire/admission, failed
and lost completion (both uncommitted and committed outcomes), successful completion, deletion
during repository outage, and same-name recreation while old callbacks are delayed. Mutation
`omit_initial_pin` exposes Hydrating without acquisition; mutation `early_release_pin` removes
the pin while Hydrating, including the all-Local/pre-Complete and failed-CAS states. Each is
caught by this monitor on a paired clean-passing scenario even though repository objects and
all block reads remain available. No repository collector or runtime credential protocol is
needed to exercise these violations.
