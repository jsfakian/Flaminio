# Proposal: Explicit remote presence for opt-in instant restore

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#68](https://github.com/jsfakian/Flaminio/issues/68)

## Context

The [intent](intent.md) scopes protocol groundwork. The current map's Unmapped means zeros at
the bottom; reinterpreting it as backup data loses writes during resync and repair. A target
has no durable state. #51/#52/#54's runtime contracts and measured restore rate are not ready.

## Proposed decision

Represent Remote(base, logical block) explicitly in every durable replica and protocol copy.
Only a target coordinates hydration; a separate bounded read-only worker verifies repository
objects. Hydration installs data conditionally over the exact Remote reference, under current
fencing and durable all-ISR acknowledgement. Presence participates in recovery and membership.
Complete is an epoch/version-conditional lifecycle transition after all-ISR local-state proof.
Keep the base pinned through completion/deletion. Only opt-in hydrating volumes may require the
repository on application I/O; already-local data never does. Recommend explicit transient
I/O failure on repository outage, distinct from missing/corrupt data; this remains unaccepted.
No snapshot/backup/shipping while hydrating in the first protocol increment.
Settled at approval, 2026-11-13: this decision is accepted with the plan, explicit transient I/O
failure included (see decisions.md, Settled at approval).

## Options

- Metadata bitmap: cannot commit atomically with data and adds per-block metadata traffic.
- Implicit unmapped holes: smaller format change, but ambiguous zeros and unsafe copy behavior.
- Replica-side independent fetch: distributes credentials and cannot preserve ISR state without
  additional consensus; target coordination reuses existing ordering and copy filters.
- Wait indefinitely on outage: avoids temporary I/O errors, but applications can hang without
  bound. Explicit failure exposes filesystem error behavior requiring validation.

## Consequences

Requires a versioned presence format, typed wire state, credential protocol and worker budgets
before runtime support. It increases failure modes and can make remote reads expensive.
Local-only completion removes the dependency permanently. No choice is Accepted and no format
number or dependency is reserved here. Approval and #54 measurements precede runtime design.
Settled at approval, 2026-11-13: the decision above is accepted with the plan (see decisions.md,
Settled at approval). Settled 2026-11-13 (user's instruction): the credential distribution
protocol is #52's worker input, its delivery and revocation fixed by the runtime design; #52's,
#54's and #51's contracts as their own approved plans settle them (see decisions.md).

## Confirmation

Use the [design](design.md) rule-to-test and mutation mapping. Later add decoder fuzzing,
crash recovery, mixed-version refusal, credential revocation, filesystem and bucket-loss lab
checks. The model increment alone does not close #68.

## Proposed confirmation refinement, 2026-11-20 — unapproved

The accepted text above remains historical. The review revision confines delivery to the
isolated model facade and adds independent original-operand and destination/base pin checks
in [model checks](model-checks.md), including zero-fill, omitted-pin and early-release mutants.
It changes no accepted protocol, worker, credential or later-runtime-format decision.
