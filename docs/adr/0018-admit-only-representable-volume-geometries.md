# 0018. Admit only complete and representable volume geometries

- **Status:** Accepted
- **Date:** 2026-11-27
- **Issue:** [#80](https://github.com/jsfakian/Flaminio/issues/80)

## Context

Metadata could accept a partial final chunk that replica sessions could not open, or more
chunks than Hello could encode. CSI rounded independently of chunk geometry and parsed some
quantities without checked multiplication. Even below Hello's count limit, a full metadata
reply with long node names and temporary move members can exceed clients' 4 MiB receive cap.
Admission must cover future legal views as well as initial placement. See the
[spec](../issues/80/spec.md), [design](../issues/80/design.md),
[compatibility contract](../issues/80/contracts.md) and
[approved decisions](../issues/80/decisions.md).

The policy was accepted with the plan on 2026-11-13 and the published clear snapshot on
2026-11-20. Implementation code/security review is clear after two rounds and conformance
passes S1–S8 on 2026-11-27; this numbered record is accepted under that approval. [ADR 0017](0017-bound-uncertainty-with-per-chunk-covers.md) independently bounds retained
uncertainty so normal reconnect reports remain representable.

## Decision

Use 4 KiB logical blocks and positive volumes made of whole chunks. Chunk size C is a power
of two from 1 MiB through 1 GiB; N = V/C is at most 65,536. Replication factor R is 1–5,
and minimum ISR is 1–R. Check arithmetic and conversions before division, allocation or
narrowing. Direct metadata creation never rounds capacity.

Keep the external metadata response budget at 4,194,304 encoded protobuf bytes. Before
provisioning, require the conservative future-view bound
`2048 + N * (288 + 264 * (R + 1)) <= 4,194,304`. It reserves maximum-width IDs and versions,
temporary R+1 replica/ISR membership, moving endpoints, wrappers and an attach token.
The effective N caps for RF 1–5 are 5137, 3881, 3119, 2607 and 2239, independent of placement.
Exact protobuf fixtures must keep proving the bound as the schema evolves.

CSI validates signed ranges and chooses whole chunks within them. For positive required
capacity, round up to the least whole-chunk result. With no requirement, prefer 1 GiB; if a
smaller positive limit applies, round that limit down. Return the actual selected capacity.
Reject a range with no supported result before provisioning.

Keep strict direct-create identity. A separate administrator-only, read-only CheckCreateVolume
RPC uses coherent metadata to compare every non-capacity identity field and current capacity
against the signed CSI range. It can accept a compatible larger or grown current volume.
Malformed new-format identity is DataLoss; only a genuinely legacy visible header uses the
documented scalar exception, explicitly reported by the response. CSI fails closed when the
RPC is missing rather than overriding an identity conflict with scalar GetVolume fields.

Validate initial and later engine metadata before changing peer maps, cached geometry or core
views. Generic refresh preserves immutable attached geometry; a higher authoritative epoch
still deposes through a safe current-geometry view even when its payload is malformed.
Replica Hello validates scalar geometry and unique in-range indices before incarnation
cleanup or mutation. A read-only store preflight checks actual layout and all unique missing
chunk slots before creation. Checked Hello encoding extends the existing checked Fenced
entry point and refuses before sending any bytes.

Unsupported existing geometry is refused before target or replica mutation. Keep valid stored
bytes, token fields, data-plane wire fields and the on-disk format unchanged. No automatic
rounding, resizing, migration or deletion is introduced. A migration path or larger envelope
requires its own design.

## Options

- **Support partial tails:** preserves exact byte requests, but needs new boundary and
  recovery semantics across target and store.
- **Paginate Hello and metadata:** permits larger volumes, but needs a bounded complete-view
  session/membership protocol.
- **Raise RPC limits:** mechanically simple, but permits larger allocation and refresh work
  without capacity/scheduling evidence.
- **Validate only the initial response:** can admit a volume that later legal moves make
  unrepresentable, so it does not satisfy the invariant.

## Consequences

Some previously accepted creates and stored volumes are refused. At 1 MiB chunks the metadata
ceiling is only a few GiB; larger chunks increase maximum capacity. These are representation
bounds, not measured operating-scale certification or physical-capacity reservations.

Local chunk slots can run out before data space, especially below the format's 64 MiB
slot-sizing baseline. Counting occupied old-incarnation slots before cleanup can refuse a
request that would fit after reclamation. Later I/O faults can still leave retryable durable
partial creation; preflight promises slot feasibility at its call, not I/O atomicity.

Upgrade metadata admission before CSI/nodes and pause creates during mixed-admission upgrades.
Unsupported existing volumes remain refused without an automatic migration. Growth (#70),
distributed reservations (#72), and measured aggregate limits (#75) must preserve this
envelope or introduce a separately approved replacement. Future catalog, disk-reference or
release-list schema profiles need complete bounded shapes and revised admission fixtures.

## Confirmation

Shared independent Go/Rust vectors cover scalar, integer and per-RF admission boundaries.
Service tests prove rejection before KV mutation and full read-only compatibility semantics;
CSI/admin cases prove checked quantities, signed ranges, actual capacities and fail-closed
old-server behavior. Maximum legal future-view protobufs and a production-service/Rust-client
loopback fixture use the normal 4 MiB limit.

Memory-store, replica and engine tests check layout/slots, legacy refusal before cleanup,
startup classification, retry after partial creation, later malformed views and higher-epoch
deposition. Frame/session/fuzz cases exercise exact Hello limits and zero-byte refused sends,
while ADR 0017's Fenced coverage remains required. The
[implementation report](../issues/80/report.md) records final results and review clearance.
