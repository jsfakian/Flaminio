# 0017. Bound uncertainty with per-chunk maximum-Version covers

- **Status:** Accepted
- **Date:** 2026-11-27
- **Issue:** [#86](https://github.com/jsfakian/Flaminio/issues/86)

## Context

Legal one- and two-chunk workloads can accumulate Fenced uncertainty above the existing
frame count and body limits. Geometry admission alone does not bound unpruned writes or
retained allocations; truncating the report would omit recovery evidence. Ordinary writes
can reach the bound: idle chunks retain their last in-flight window, and one stalled write
holds the volume's prune bound. See the [spec](../issues/86/spec.md),
[design](../issues/86/design.md), [approved decisions](../issues/86/decisions.md) and
[replication protocol](../replication-protocol.md).

The policy was accepted with the issue plan on 2026-11-13 and the clear post-review snapshot
on 2026-11-20. Implementation code/security review is clear after two rounds and final
conformance passes S1–S9 on 2026-11-27; the numbered record is accepted under that approval.

## Decision

Use 65,536 uncertainty slots per replica volume: one per held chunk plus precise records in
one bounded arena. When a write needs a record at the bound, coarsen only its chunk to a
whole-chunk cover with the maximum full Version represented. Adding a chunk at the bound
coarsens the lowest-numbered chunk holding records before inserting the new empty chunk.
Later writes and prunes update the marker without rebuilding an unbounded list. A prune
clears it only when its bound is strictly greater than the maximum; subsequent writes can
be precise again. Restart coverage independently retains its strictly-newer-epoch rule.

Drop Writes whose version or prune epoch, and Settles whose bound epoch, differ from their
session's epoch. This prevents a foreign epoch from clearing restart evidence or creating
an unprunable maximum. A bound in the session's own epoch remains its holder's unchecked
claim, as before.

Emit every held chunk in sorted order, including explicit empty rows. Inside the existing
Fenced frame, N and T are each at most 65,536, giving a maximum body of 1,048,589 bytes,
below the existing 1,048,704-byte limit. Validate before encoding, decoding or installing
target session, lease or fencing authority. The checked encoder is shared with #80. Hello
carries its send-time required chunk set; valid extra held rows are allowed, while missing
required or malformed rows grant no authority. Existing create-on-Hello trust is unchanged;
distinguishing a fresh allocation from a lost copy remains #13/#61's work.

Quarantine a startup volume whose held set exceeds the supported bound or chunk-index
range. Preserve its data and fences; the existing replica deletion clears quarantine.
No metadata, wire or disk schema change, limit increase, earlier acknowledgement or
reconciliation shortcut accompanies this representation.

## Options

- **Truncate records or report extents:** small, but can lose unfinished-data evidence.
- **Cap only the report union:** duplicate writes and retained vector capacities still grow.
- **Coarsen the whole volume permanently:** simple, but ordinary fills make every subsequent
  takeover reconcile all written chunks until restart.
- **Clear a cover on any same-epoch prune:** forgets writes newer than the bound.
- **Pagination or a new protocol version:** could preserve precision, but needs a bounded,
  complete-session report protocol; unnecessary for the conservative existing envelope.

## Consequences

A chunk written at the bound is digested whole on the next takeover unless a later prune
or Settle exceeds its maximum. Other chunks stay precise. At the default geometry, a
512-chunk fill with a 128-write window coarsens four chunks. At N equal to the budget,
every written chunk is whole, as after restart. Existing recovery latency is not promised.

The arena and report bounds are per replica volume. The target's existing reconciliation
union is bounded by spatial geometry, and its B-tree insertion costs remain; aggregate
memory and scheduling policy belongs to #75. No new aggregate certification is claimed.

Old producers that omit empty rows require coordinated rollout before strict consumers.
An unsupported earlier incarnation blocks its name on that node until deleted. Empty rows
do not prove disk survival, and a faulty holder can still claim an early same-epoch prune.
These costs and residual risks were accepted with the plan.

## Confirmation

Core reference tests compare exact-version coverage, arena allocation/reuse and work at
small and default budgets. Retained F09 workloads and maximum-frame tests cover the checked
codec, zero-byte send refusal and fuzz regressions. Node tests preserve quarantine data,
pre-create refusal and uncertainty after store failure. Target tests verify complete
validation before authority and captured Hello sets.

An independent simulator shadow checks exact versions, restart/session coverage, resource
bounds and invalid-report authority. Injected omissions must be caught, alongside existing
restart, early-Settle and reconciliation controls. Resync integration tests also require
fresh replay receipts, pre-snapshot pending coverage and stable retirement of unresolved
move sources; clean same-seed reruns distinguish injected failures from normal behavior.
The [implementation report](../issues/86/report.md) records the final checks and reviews.
