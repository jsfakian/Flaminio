# Proposal: Bound uncertainty with whole-chunk maximum-Version covers

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#86](https://github.com/jsfakian/Flaminio/issues/86)

## Context

[Intent](intent.md), [spec](spec.md), [design](design.md). Audit F09 proves legal 1- and 2-chunk
volumes accumulate Fenced replies above existing count/body bounds. Geometry admission alone
does not bound unpruned writes or retained allocations. Truncation would omit recovery evidence.
The budget is reached by ordinary workloads: a write prunes only its own chunk, so idle chunks
keep their last in-flight window, and one stalled write holds the volume's prune bound.

## Decision proposed

Use 65,536 per-volume uncertainty slots: one reserved per held chunk plus bounded precise
records in one arena. On exhaustion, coarsen only the chunk being written (for a new chunk, one
chunk that holds records) to a whole-chunk cover with its maximum represented Version. Later
writes/prune update that fixed marker, never rebuild an unbounded list. Clear a coarse cover
only when prune exceeds its full maximum Version; the chunk then holds precise records again,
since a cleared cover represents no write. Drop a Write or Settle whose epochs differ from its
session's: a forged epoch then cannot clear restart cover set in the fenced epoch, and a maximum
never exceeds the fenced epoch. A bound of the session's own epoch stays its holder's claim and
is not checked, as today. Keep restart's separate
strictly-newer-epoch condition unchanged.

Emit every held chunk, with full/precise/empty uncertainty, inside the existing Fenced frame:
N ≤ 65,536 and T ≤ 65,536 give a body of at most 1,048,589, below the current 1,048,704 limit.
Validate before encoding,
decoding and installing target fencing/lease facts; one encode_checked entry point is shared
with #80, not forked, and whichever issue lands first adds it. The Hello message carries its
required chunk IDs; include newly-created empty rows, accept valid
extra held/released copies and reject missing required or malformed rows without authority.
An empty row carries exactly the trust a missing row carries today: create-on-Hello is
unchanged, and telling a fresh allocation from a lost one stays #13/#61's work. A volume found
at startup beyond the budget is quarantined, never erased. No metadata/wire/disk schema change,
limit increase, earlier ack or reconciliation shortcut.

## Options

- **Drop/truncate records or report extents:** smaller but loses potentially unfinished data;
  prohibited, and checked by a coverage mutation.
- **Cap only report union size:** duplicate writes and retained Vec capacities still grow;
  insufficient memory protection.
- **Promote the whole volume once, and stay coarse:** simplest state, but a sequential fill of
  a 128 GiB volume at the defaults reaches it, and every chunk written afterwards is then
  reconciled whole at each takeover until the process restarts. Dropped after plan review round 1.
- **Clear coarse cover on any same-epoch prune:** forgets newer versions; preserve max tuple
  and independently apply original restart epoch rule.
- **Pagination/new version:** could retain precision, but needs a bounded complete-session
  report protocol; unnecessary for this conservative fixed-envelope proposal.

## Consequences

A chunk written while the budget is full is digested whole at the next takeover, unless a
later prune or Settle above its maximum clears it first; other chunks stay precise. With as
many held chunks as slots, every written chunk is whole, as after a restart. No
promise of existing recovery latency is made. Bounds are per volume, not total scheduling
certification (#75). Old producers missing new empty rows require coordinated rollout before
strict consumers. Unsupported legacy held sets fail closed without automatic data deletion, and
block their name on that node until deleted.
No risk/policy is accepted merely by drafting this issue-local proposal.
Settled at approval, 2026-11-13: the policy and these consequences are accepted with the plan (see decisions.md, Settled at approval).

## Confirmation

Actual retained F09 probes, max-frame roundtrip, bounded arena reuse/work tests and simulator
exact-version shadow coverage plus drop/prune/budget/foreign-epoch/invalid-reply mutations in
the design.
Existing restart, early-Settle and reconciliation mutations remain. No tests/lab were run
for this draft; accepted fencing/ISR ADRs and proposed clock contract are unchanged.
