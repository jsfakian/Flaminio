# Issue #78 proposal: Fence placement admission and validate drain generations

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Scope:** issue-local draft; no global ADR number is allocated.

## Context

[Issue #78](https://github.com/jsfakian/Flaminio/issues/78) and its [intent](intent.md) show that
node eligibility reads and volume claims are not serialized with drain marking. Chunk publication
is batched after a hidden creating header. A visible-volume scan misses these earlier admissions.
Accepted ADR 0002 provides linearizable revision compares, but does not define this protocol.

## Decision

Propose one durable placement-gate revision, atomically advanced with changed topology and
mark/unmark operations. Creation and direct move admission compare its pre-planning revision.
Creation records that immutable generation in its durable header and retains the incarnation CAS
through every publication batch. A drain generation is its marker's modification revision.

Completion scans older creating reservations before raw chunk placement/incoming moves and then
validates the same drain marker. No timeout retires publication rights. Older admissions may
finish; admission after marking cannot target that node. Completion is metadata accounting only.
Require exclusion of old writers/controllers during rollout and initialize the gate to a
positive revision newer than every retained legacy marker before new admissions. Do not reset
a legacy marker's D or record generation0 on a new claim. Exact contracts: [design](design.md).

## Options

- **Global fence and conservative reservation scan (proposed):** constant-sized admission CAS
  regardless of selected-node count; current creating header is the durable reservation. An
  abandoned earlier creation on another node can block drain; topology writes cause retries.
- **Per-node reservation index:** avoids unrelated blocked drains; admitting many destinations
  exceeds one transaction and requires a restartable multi-stage reservation protocol. Better
  precision, greater complexity; not silently included in this fix.
- **Drain-key compare only:** sufficient for a bounded direct move, but does not solve creating
  reservations hidden from drain completion or unbounded selected-node compare lists.

## Consequences

Q1 (conservative coupling), Q2 (#76 recovery integration) and Q3 (maintenance rollout barrier)
remain open; no availability trade-off is accepted. #18's draft permission for direct draining
moves must be narrowed when combined. #72 capacity reservations must compose with this fence.
No on-disk format, replication durability or lease rule changes. Current full-prefix read costs
remain a scaling limitation; transaction bounds alone are not operating-limit measurements.
Settled at approval, 2026-11-13: Q1–Q3 take the plan's recommendations, accepting Q1's availability cost (see decisions.md, Settled at approval).

## Confirmation

Both-store interleaving tests demonstrate each admission/mark order and hidden-reservation handoff.
Generation validation catches undrain/re-drain. Simulator admission schedules and Go fixture replay
must detect injected omitted-fence, omitted-reservation and omitted-final-marker-compare bugs,
including an empty scan for D invalidated by unmark/re-mark D2 before final validation. Authorization and contract tests
cover the new administrator-only completion RPC. No such future checks have run for this draft.
