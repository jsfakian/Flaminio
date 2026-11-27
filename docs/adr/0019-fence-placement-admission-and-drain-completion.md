# 0019. Fence placement admission and validate drain generations

- **Status:** Accepted
- **Date:** 2026-11-27
- **Issue:** [#78](https://github.com/jsfakian/Flaminio/issues/78)

## Context

Eligibility reads and volume claims were not serialized with drain marking. Creation publishes
chunks in batches after a hidden Creating header, so a visible-volume scan can miss earlier
admissions. ADR 0002 supplies linearizable revision comparisons and ADR 0016 retains the immutable
admitted plan; neither defines placement/drain ordering. See the approved
[spec](../issues/78/spec.md), [design](../issues/78/design.md) and
[decisions](../issues/78/decisions.md).

## Decision

Use one durable placement-gate revision, atomically advanced with changed topology and drain
mark/unmark operations. Creation and direct move admission compare its pre-planning revision.
Creation records that positive immutable generation in the header, retaining ADR 0016's plan and
root/claim fences through recovery and every publication batch. A drain generation is the
marker's modification revision.

CheckDrain scans older Creating reservations before raw chunk placements and incoming moves,
then validates the same drain marker. No timeout retires publication rights. Earlier admissions
may finish; admission after marking cannot target that node. Every direct move destination check
rejects a draining node before treating an existing move as a no-op. A controller succeeds only
when CheckDrain certifies zero counts for its current generation.

Require exclusion of old writers/controllers during rollout. Initialize the gate to a positive
revision newer than retained legacy markers before new admissions, without resetting their
generations. Legacy headers without an admission generation count as earlier reservations.
Completion certifies metadata accounting at the final marker comparison, rather than physical
replica erasure or node shutdown authority.

## Options

- **Global fence and conservative reservation scan (chosen):** constant-sized admission CAS,
  using the existing Creating header as the durable reservation. Unrelated earlier abandoned
  creations can block drain, and topology changes can force planning retries.
- **Per-node reservation index:** more precise accounting, but large placements require a
  restartable multi-stage reservation protocol. That added protocol is outside this issue.
- **Compare only the destination drain key:** enough for a bounded direct move, but misses
  hidden Creating reservations and does not bound comparisons for large volume placement.

## Consequences

- Operators must resolve an older stuck creation through recovery or explicitly fenced Delete
  before drain can finish, even when its intended placement is elsewhere. No automatic reaping.
- A global gate couples admission retries to topology changes. Full-prefix inventory reads remain
  a scaling limitation; bounded transactions do not certify production operating limits.
- Rollout uses a maintenance barrier; mixed old/new writer safety is not claimed.
- Future capacity reservations (#72) must compose with this admission fence. Tombstone cleanup
  and physical replica deletion remain separate (#13).
- No replication durability, lease, data-plane wire or on-disk format changes are introduced.

## Confirmation

Both-store service schedules exercise admission/mark orders, hidden reservation handoff,
crashes and uncertain responses. Phased model schedules and actual Go CAS replay catch omitted
admission compare, reservation count and final marker compare mutations, including unmark/re-mark
between an empty inventory scan and validation. Independent occupancy and fixture expectations
also catch missing placement/incoming counts and publication after deletion fencing. Authorization
and controller checks cover administrator-only completion and validation with empty visible
inventory. All required project checks, code/security reviews and S1–S8 conformance passed;
see the [report](../issues/78/report.md).
