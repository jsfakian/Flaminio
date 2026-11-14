# Proposal: Admit only complete, representable volume geometries

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#80](https://github.com/jsfakian/Flaminio/issues/80)

## Context

[Intent](intent.md), [spec](spec.md), [design](design.md). Metadata accepts partial final chunks
and counts above Hello's 65,536 limit. Replica sessions reject the former; encoding panics on
the latter. Store chunks have a 1 GiB limit and layout-specific table bounds. Full metadata
views can exceed clients' 4 MiB receive cap even below the Hello limit. Geometry must cover
normal replication moves too; changing the initial placement cannot repair these limits.

## Decision

Propose whole chunks only; power-of-two chunk sizes 1 MiB through 1 GiB, positive capacity,
4 KiB logical blocks and at most 65,536 chunks. CSI rounds capacity to whole chunks inside the
caller's signed range; direct metadata creation never rounds. Keep current wire, token and
on-disk representations.

Keep a 4,194,304-byte metadata response budget. Require the conservative bound
`2048 + N*(288 + 264*(R+1))` to fit before provisioning, irrespective of actual placement or ID
lengths. It budgets a future move's temporary extra replica/ISR, endpoint names, maximum-width
versions, header, wrappers and token. Effective N caps for RF=1…5 are 5137/3881/3119/2607/2239;
this intentionally sacrifices maximum capacity to avoid large-message transport changes.
Use exact protobuf fixtures to verify the bound as the schema evolves. A separate read-only,
administrator-authorized CheckCreateVolume evaluates full non-capacity identity plus CSI signed
range; no scalar-only conflict override. Its exact [contract](contracts.md) preserves direct
Create semantics and makes legacy exceptions explicit. #86's conservative uncertainty/report
bounds are required for reconnect representability; legal geometry alone does not supply them.

Unsupported legacy geometries fail before target/store mutation; no silent resize or deletion.
Validate session geometry before old-incarnation cleanup. Store checks actual table and free
slot bounds locally; distributed resource reservations are #72, not a geometry guarantee.

## Options

- **Support partial tails:** preserves requested byte capacity but requires new target/store
  boundary and recovery semantics; deferred rather than added to the audit fix.
- **Paginate Hello and metadata views:** allows larger volumes but needs session/membership
  snapshot contracts and protocol work; separately design and measure before lifting bounds.
- **Raise RPC receive limits:** simpler mechanically but permits much larger allocations and
  failure-refresh work without evidence from #72/#75; not proposed for this fix.
- **Validate only initial protobuf size:** admits volumes that later moves/longer node IDs can
  make unrepresentable, so it does not satisfy the invariant.

## Consequences

Some formerly accepted creates and existing volumes will be refused. At 1 MiB chunks the
conservative metadata ceiling is only a few GiB; larger chunks increase maximum capacity.
The limits provide representation safety, not scale certification or guaranteed physical
capacity. Chunk-slot exhaustion can occur before disk data is full, especially below the
format's 64 MiB slot-sizing baseline. Operator migration and any higher limits remain open.
Settled at approval, 2026-11-13: higher limits wait for a separate design; operator migration: Settled 2026-11-13 (user's instruction): none in this plan; such volumes stay refused, and a separate design defines one if it is needed (see decisions.md).

## Confirmation

Shared Go/Rust geometry vectors, CSI range/idempotency cases, real Hello count boundaries,
maximum protobuf future-move fixtures and memory-store table/slot tests in the design.
Keep proposed audit ADR 0010 separate; accepted fencing/ISR ADRs are unchanged. This proposal
has no global ADR number to avoid batch branch collisions and has not been accepted.
Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).
