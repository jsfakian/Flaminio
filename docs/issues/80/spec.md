# Issue #80: One supported volume geometry — specification

Approved 2026-11-13; not implemented. [Intent](intent.md); settled choices, if any: [decisions](decisions.md).
How: [design](design.md). Proposed geometry policy: [ADR proposal](adr-proposal.md).
Settled at approval, 2026-11-13: the geometry policy is accepted with the plan (see decisions.md, Settled at approval).

## Terms

- **Geometry:** capacity V, chunk size C, count N=V/C and requested replication factor R.
- **Envelope:** C is a power of two, 1 MiB ≤ C ≤ 1 GiB; V is positive and divisible by C;
  N ≤ 65,536; 1 ≤ R ≤ 5 and 1 ≤ minimum ISR ≤ R. No partial final chunk is supported.
- **Metadata budget:** 4,194,304 encoded protobuf bytes for any single create/get/attach reply,
  including its wrapper and attach token. Admission reserves for the worst legal future view:
  maximum-length 128-byte node/volume IDs, maximum integer widths, moving endpoints and
  temporary membership growth during a move. This can impose a lower N ceiling than Hello.
- **Resource exhaustion:** insufficient local free chunk slots/pages/data or distributed
  capacity; distinct from an unsupported geometry.

## The invariant

Every successful create describes geometry that all normal metadata and replica transport
paths can represent, including after placement/moves. With adequate local resources, its
engine/session/store geometry is valid. Request-controlled arithmetic or framing never panics,
wraps or narrows into a different volume. Durability and fencing rules remain unchanged.

## Behaviour

**S1. Direct creation.** Metadata accepts only the envelope and metadata budget. It rejects
invalid inputs before allocating placement or writing a creating header/chunks. It never rounds
a direct caller's capacity. Validation covers both bounds, not just the initial distribution;
RF, preferred placement and moves cannot bypass them.

**S2. CSI capacity.** Negative range fields, or nonzero limit smaller than required, are invalid.
For positive required bytes, choose the least whole-chunk capacity at least that large. With no
required capacity, prefer 1 GiB; when a positive limit is smaller, choose the greatest whole-chunk
capacity no larger than that limit. Every result contains at least one chunk. Reject a range
with no supported result, an overflow, or an exceeded geometry/metadata ceiling before metadata
mutation. A successful response reports the selected capacity within the caller's limit.

**S3. Idempotency.** A repeated create with the same effective geometry succeeds with the same
capacity. Existing-volume retries authoritatively check all non-capacity creation parameters
and current capacity against the requested range, and never accept a full-identity conflict
using a scalar-only fallback. Direct metadata Create retains #76's strict identity contract;
the separate read-only compatibility check described in the design handles CSI range semantics. Incompatible geometry/name conflicts
retain AlreadyExists semantics; arithmetic/range errors do not create or modify anything.

**S4. Checked arithmetic.** Parse suffix quantities, round capacity, compute byte/count bounds,
convert signed/unsigned sizes and narrow chunk/block/index counts only after checking bounds.
Malformed persisted metadata returns a controlled error rather than division-by-zero, overflow,
large unchecked allocation or process panic, including later metadata refresh/ISR replies and
administrative suffix parsing. Reject zero-sized engine volumes explicitly.

**S5. Defense at attach/session boundaries.** The engine validates full geometry, chunk-record
count, unique in-range chunk IDs/placement membership and per-node Hello limits before target
construction. Authenticated remote and local replica Hello validate scalar geometry and a set
of unique in-range chunks before deleting old-incarnation chunks, fencing or creating replicas.
Capability signature/identity/epoch checks remain required; token fields do not authenticate
geometry, so valid token holders still pass bounded geometry validation.

**S6. Wire refusal.** Outbound Hello count, short strings/token and encoded-body size are checked
before allocation/transmission. Invalid Hello returns a normal transport error with no bytes
sent. Keep the decoder bounded and reject oversized/duplicate/out-of-range session input;
request admission must not depend on encoder assertions. #86 is a required dependency for
bounded accumulated uncertainty and checked Fenced replies; successful scalar validation alone
cannot prove reply representability. Do not close/release this transport guarantee without it.

**S7. Store constraints.** Preserve cluster divisibility, 1 GiB chunk maximum and actual layout
page-table bounds for every chunk. Check free slots for all missing session chunks before
creating any of them; duplicates/existing chunks consume no new slots. Insufficient free slots
returns a resource error without starting partial chunk creation. A later I/O failure may leave
retryable partial work; no rollback of durable fencing or earlier chunks is promised. Distributed
reservations remain #72, and no on-disk sizing/version change is made here.

**S8. Compatibility and documentation.** Valid stored volumes remain byte-for-byte unchanged.
Unsupported existing volumes are refused with an actionable geometry/budget error before
starting a target or modifying replicas; no automatic rounding, resizing, deletion or rewrite.
Document the new effective per-RF maximum, 4 KiB logical block, CSI rounding/range behavior,
resource distinction and coordinated service/node upgrade requirement. Do not claim scale or
performance certification from boundary tests.

## Tests

- **S1, S4:** Go domain/service tables and Rust scalar vectors: 4 KiB, 1 MiB, C−4096/C/C+4096,
  1 GiB/2 GiB chunk sizes; zero; 65,536/65,537; old 2^20 floor/ceiling boundary; MaxInt64/MaxUint64;
  R=1…5 and effective metadata-budget boundary. Rejection leaves metadata unchanged.
- **S1, S5, S6:** RF=1 concentration, RF=5, balanced/preferred placements and in-progress moves;
  exact worst-case protobuf sizes versus budget; real Hello round trips at 65,536, normal refusal
  at 65,537, and malformed metadata/duplicate chunks without panic or side effects.
- **S2–S4:** CSI fake metadata service checks every signed range, unaligned request, limit below
  rounding, limit-only/default request, suffix overflow, same-name retry and zero metadata calls
  on invalid input. Reported capacity satisfies required/limit on every success.
- **S5, S7:** engine/replica/store tests on memory devices for each legal cluster size and
  table boundary; insufficient/exact free slots, existing/duplicate chunks, invalid geometry
  before old-incarnation deletion, and retry after injected chunk-creation I/O failure.
- **S6:** frame fuzz/regression corpus covers hostile lengths/counts and both directions.
- **S8:** persisted legacy-valid/unsupported fixtures and documented upgrade refusal behavior.

## Open questions

- The full-chunk policy and conservative 4 MiB future-view admission are **proposed**, not
  approved. Raising limits or supporting partial chunks/pagination needs a separate design.
  Settled at approval, 2026-11-13: the full-chunk policy and 4 MiB admission bound are accepted; larger limits wait for a separate design (see decisions.md, Settled at approval).
- An operator migration path for pre-existing unsupported volumes is not defined; refusal
  prevents further mutation but does not recover already inaccessible data.
  Settled 2026-11-13 (user's instruction): no migration path in this plan; such volumes stay
  refused, and a separate design defines one if it is needed (see decisions.md).
