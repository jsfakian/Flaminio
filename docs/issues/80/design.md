# Issue #80: One supported volume geometry — design

Implementation of [spec](spec.md); [decisions](decisions.md), if any.
Approved 2026-11-13; not implemented.

The facts it starts from:
- Metadata validates 4 KiB capacity, unbounded power-of-two chunks and floor N≤2^20;
  ceiling count can overflow (`meta/domain/domain.go:57,176`). Creation validates before
  placement and the creating-header transaction (`meta/service/service.go:362,403`).
- CSI rounds to 1 MiB and multiplies suffixes unchecked (`control/csi/controller.go:193–229`).
- Engine and replica Hello require full chunks (`flaminio-node/src/engine.rs:1603`,
  `replica/mod.rs:179`); replica deletion of earlier incarnations follows that scalar check.
- Hello encoding asserts N≤65,536 (`flaminio-net/src/frame.rs:196`); `Conn::send` directly
  calls it (`session.rs:59`). Receiver body budget is 1 MiB+128 bytes, including Hello.
- Store validates cluster divisibility, ≤1 GiB and actual table capacity; free slots fail
  inside each create (`flaminio-store/src/state.rs:210`, `store.rs:1192`). Format sizing uses
  64 MiB chunks but does not forbid smaller chunks (`flaminio-format/src/layout.rs:112`).
- Replica lists can temporarily contain R+1 members during a move (`meta/domain/domain.go:264`).
  Full metadata responses repeat those lists; Rust/CSI metadata clients use normal receive
  limits (`flaminio-metaclient/src/lib.rs:174`, `control/csi/meta.go:40`).

## Admission and metadata bounds (S1, S4, S8)

Add pure Go `pkg/geometry/geometry.go` and Rust `flaminio-repl/src/geometry.rs` helpers.
Constants: BlockBytes=4096, MinChunkBytes=1<<20, MaxChunkBytes=1<<30,
MaxChunks=1<<16, MetadataBytes=4<<20. Scalar validation orders C/range checks before division,
requires V>0 and V%C=0, then computes N=V/C. Checked conversions follow validation.

The conservative metadata admission contract is exactly:
`B(N,R) = 2048 + N * (288 + 264 * (R+1))`; require B≤4,194,304.
It covers R+1 replicas and ISR names (each at most 128 bytes), two moving endpoints,
maximum uint64 versions, all nested tags/lengths, worst header and a maximum-size token.
The effective N maxima for R=1…5 are **5137, 3881, 3119, 2607, 2239**.
Use checked operations; reject malformed R before multiplication. This intentionally leaves
headroom versus exact protobuf size and makes admission independent of placement/name lengths.

`Spec.Validate` applies both scalar and B checks. `Header.ChunkCount` becomes a checked
`(int,error)` operation; update readVolume, create and index-check callers. Raw deletion and
listing still permit operator cleanup of an unsupported header; do not rewrite its geometry.
`CreateVolume` validates before placement/creating writes; also check exact `proto.Size` of the
resulting Create/Get/Attach worst-view fixtures against B to catch schema drift. Metadata Attach
validates legacy geometry before changing its epoch/target. No automatic migration.

Keep 4 MiB metadata receive limits; make them explicit in Go CSI/admin clients and Rust unary
client so defaults cannot silently drift. No larger-message override or pagination is added.
For schema additions, contract fixtures must prove B still bounds every relevant response, or
admission must change with a new documented compatibility decision. The cap is representation
safety, not measured operating-scale certification.

## CSI selection and retries (S2, S3, S4)

`controller.go`: validate chunk/R/minISR parameters before capacity arithmetic. `parseQuantity`
and admin CLI `parseSize` use checked multiplication; overflow is InvalidArgument before RPC.
Preserve each caller's supported suffix grammar. Check both admin capacity/chunk arguments.
Read required/limit as int64; reject
negative fields or required>positive limit as InvalidArgument before casting.
For required>0 use quotient/remainder to round up to C, checking multiplication. For required=0,
start at 1 GiB; with a smaller positive limit, round that limit down to C. Require ≥C and run
shared admission. No representable supported capacity or rounding beyond limit returns
OutOfRange; no metadata call. Invalid chunk/R/minISR returns InvalidArgument.

Coordinate #76: propagate true full-identity conflicts; never convert AlreadyExists to success
by scalar GetVolume comparison. After that response, call the new administrator-only read-only
`CheckCreateVolume` with the complete desired create parameters and signed range. It checks all
non-capacity fields against stored canonical identity and current V against range, returning a
coherent current volume only on compatibility. This supports the [CSI v1.13 requirement](https://raw.githubusercontent.com/container-storage-interface/spec/v1.13.0/spec.md)
for a compatible existing volume even if V exceeds the new minimum candidate. No resize or
identity mutation. Direct Create identity semantics stay strict. Exact API/legacy/error contracts
are in [contracts](contracts.md). Missing RPC fails closed; no old-server scalar fallback.
Response CapacityBytes is the returned V checked int64 and within the current signed range.

## Target, session and store checks (S4, S5, S7, S8)

Engine `start_target` validates scalar geometry plus B using volume RF, exact record count N,
replica/ISR sets and in-progress R+1 bound, valid IDs, and the per-node union of placed/moving
chunks. Enumerated metadata records supply indices; reject malformed memberships/lengths before
creating Target or narrowing block/index types. Every generated Hello chunk list is a unique
in-range set; full geometry still allows at most 65,536 even for direct session tests.
Reuse validation before ANY later GetVolume/ChangeIsr conversion, node-id insertion or cached
view mutation. Generic refresh must match immutable attached size/C/R/minISR and exact record
count; ISR replies must match the saved requested-index count/order and bounded valid sets.
Reject malformed same/lower-epoch replies without installing them or calling apply_view; retain
known authoritative state and existing retry/deadline behavior. A higher authoritative epoch
still deposes immediately using a safe current-geometry view, even when its chunk payload is
malformed. Never let validation suppress that fence signal. #70 expansion requires its explicit
validated growth transition; it cannot silently loosen this generic immutable-geometry guard.

`ReplicaService::hello` validates scalar geometry and unique in-range requested chunks for local
and authenticated remote paths before old-incarnation deletion or core/store mutation. Hello
has no RF field: enforce the scalar/wire envelope here, with B enforced by metadata/engine.
Do not invent geometry claims in the token or remove existing signature/peer/epoch checks.

Add Store read-only `preflight_chunks(keys, chunk_size)` checking actual layout divisibility,
max chunk/table limits and available slots for unique missing keys. Call before Hello's
old-incarnation cleanup; counting currently occupied older-incarnation slots conservatively can
reject a request that would fit after cleanup, which is safer than destructive cleanup before
validation. Report `OutOfSpace("chunk slots")` as a session refusal; operator/retry may reclaim
space separately. Existing keys cost zero new slots. Single-owner store serialization keeps
preflight/create contiguous; no new lock, distributed reservation or success guarantee after
I/O faults. Earlier partial creates remain retryable and retain epoch/incarnation evidence.

## Bounded Hello sending (S6)

Add `Frame::encode_checked() -> Result<Vec<u8>, EncodeError>` and use it in `Conn::send`.
Preflight Hello chunks≤65,536, unique entries, both short fields≤u16::MAX, token≤token::MAX_LEN,
scalar geometry and checked body length≤MAX_BODY before allocation; Hello fixed body is
49+volume.len()+token.len()+4*chunks.len(). Reject out-of-range indices too. Existing encode
may remain for proven internal/test frames; network-originated Hello never takes an assertion
path. `SessionError::Encode` is a controlled local error and send writes zero bytes on it.
Decoder limits remain; session validation rejects duplicates/range errors before store work.
#86 additionally bounds accumulated uncertainty conservatively and validates checked Fenced
count/body/chunk identities. It extends this same encode_checked entry point; normal legal
reports must round-trip, not merely disconnect instead of panicking. #86 must land before this
transport guarantee is complete. Other unrelated frame contracts are not redesigned here.

## Files touched

- `pkg/geometry/{geometry.go,geometry_test.go}`, `meta/testdata/geometry-v1.json`: S1–S4, shared vectors.
- `meta/domain/{domain.go,domain_test.go}`, `meta/service/{service.go,service_test.go}`: S1/S4/S8.
- `control/csi/{controller.go,csi_test.go,meta.go}`, `meta/cmd/flaminio-meta/main.go`: S2–S4 retry/range,
  checked admin quantities and RPC budget. Proto/generated Go/auth/service add CheckCreateVolume.
- `dataplane/crates/flaminio-repl/src/{lib.rs,geometry.rs}`: S4/S5, pure checked geometry.
- `dataplane/crates/flaminio-node/src/{engine.rs,replica/mod.rs}` and `tests/engine.rs`: S5/S7/S8.
- `dataplane/crates/flaminio-net/src/{frame.rs,session.rs}`, `tests/session.rs`: S6.
- `dataplane/crates/flaminio-store/src/{store.rs,state.rs}` and existing store tests: S7.
- `dataplane/crates/flaminio-metaclient/src/lib.rs`, `tests/interop.rs`: S1/S8, explicit RPC budget.
- Existing frame fuzz target/regressions and `Makefile` contracts prerequisites: S4/S6.

## Contracts

- Add read-only CheckCreateVolume RPC/messages with explicit administrator-only auth, per
  [contracts](contracts.md); run make proto/contracts and update Go CSI client/interface. No new
  Rust call is required for this CSI-only method. Token bytes/wire tags/on-disk records unchanged.
- Public geometry constants, B formula and effective per-RF N caps above are the shared
  admission contract. JSON vectors contain V/C/R/minISR, scalar/admission acceptance and N;
  represent uint64 values as decimal strings, including overflow cases.
- Metadata Create: InvalidArgument for unsupported geometry/budget before any provisioning.
  CSI: InvalidArgument for malformed parameters/ranges, OutOfRange for no supported capacity,
  AlreadyExists for incompatible existing volume; CheckCreateVolume success is authoritative,
  never the old scalar Get fallback. Attach rejects legacy unsupported geometry
  with FailedPrecondition. Control socket retains `error <reason>` refusal syntax.
- Hello remains one bounded frame per node/session; 65,536 passes frame representation and
  65,537 is rejected. Metadata rejects both counts under the stricter B bound for all legal R.
- `preflight_chunks` performs no writes, fencing or deletion; exhaustion remains distinct from
  geometry validation. It promises slot feasibility at the call, not crash/I/O atomicity.

## Tests

| Rules | Concrete test and level |
|---|---|
| S1, S4 | `pkg/geometry/geometry_test.go`: shared 4 KiB/1 MiB/C±4096/1 GiB/2 GiB, zero, u64/i64 overflow, old 2^20 and new per-RF boundary vectors; Rust geometry consumes same file. |
| S1, S4 | `meta/service/service_test.go`: no KV writes on rejection; concentrated/balanced/preferred R=1…5 placements. Worst synthetic Create/Get/Attach protobufs (128-byte IDs, R+1 sets, max versions, moves, maximal token) have proto.Size≤B≤4 MiB at each admitted maximum; next N rejected. |
| S2–S4 | CSI/service tests: every signed range, rounding and overflow; same geometry/different preference/domain/policy conflicts; authoritative compatible larger/current-grown capacity, legacy-identity exception, incomplete/tombstoned/racing volume and missing compatibility RPC. No writes in CheckCreateVolume. |
| S4 | Admin CLI parser/command tests: `17592186044417M`, overflowing K/G/T where supported and chunk argument; reject before Create RPC, exact boundary preserves value. |
| S5, S8 | Engine tests: invalid initial and post-attach GetVolume/ChangeIsr counts/memberships/geometry refuse before map/core mutation; higher epoch plus malformed payload still deposes. No panic, new peer ID or malformed Hello; validated growth is separate. |
| S6 | Frame/session tests: Hello count/body boundaries with zero bytes on invalid send; #86 regression uses65,537 disjoint extents in one legal1GiB chunk and131,086 in two, conservative coverage plus reconnect/encode/decode. No normal report refusal/panic. |
| S5, S7 | Replica/store tests on MemDevice: all five legal cluster sizes, exact/over table bounds and slot availability, existing/duplicate chunks, no deletion/fence on invalid preflight; injected I/O fault after first create retries safely. |
| S1, S8 | `flaminio-metaclient/tests/interop.rs`: production Go gRPC and Rust decoder carry an admitted maximum future-move view under explicit 4 MiB limits; no test-only limit increase. |

`make test race contracts lint` proves unit/interop/vector consistency; add Go/Rust vector
checks to `contracts`. No replication safety semantics change, so no new simulator rule is
claimed. `make sim` remains regression coverage. No tests were run while drafting this plan.

## Lab

None required for this representation/validation fix. Use memory devices and loopback TLS/RPC
fixtures; no shared disks are formatted and no lab restoration is needed.

## Docs

Update architecture geometry and limits, format text to distinguish slot-sizing baseline from
accepted chunk minimum, CSI parameter help/README and lab capacity examples. Record #80's
implementation results in followups only when implemented. Link #70's growth and #72's
reservation accounting to this envelope; #75 must measure capacity/latency at real limits.

## Risks

- **Open policy:** full chunks and conservative metadata limits can reject previously accepted
  creates and stored volumes. No automatic migration/recovery procedure is accepted here.
  Settled at approval, 2026-11-13: the policy is accepted; a migration path: Settled 2026-11-13 (user's instruction): none in this plan; such volumes stay refused, and a separate design defines one if it is needed (see decisions.md).
- **Upgrade:** old metadata can still admit bad geometry; upgraded replicas fail safely.
  Upgrade service admission before new provisioning, then CSI/nodes; pause create during mixed
  admission deployments. Existing unsupported volumes require an operator plan.
- **Availability:** counting old-incarnation slots before cleanup can refuse a reclaimable
  attach. Distributed slot/data reservations and pressure handling belong to #72.
- **Security:** valid-token peers can supply hostile geometry, so checks precede deletion and
  allocation. Preserve mTLS, token signatures, authorization and fencing on every path.
- **Compatibility:** the budget includes one temporary move member; new membership shapes or
  protobuf fields must update fixtures/admission, not silently exceed the bound.

## ADRs

[Issue-local proposal](adr-proposal.md) records the new cross-layer admission policy, awaiting
approval. Accepted ADRs 0001/0002 (Go/Rust and transactional etcd) and 0005/0006 (fencing and
ISR rules) remain consistent. Audit ADR 0010 is Proposed in the audit checkout; this design
aligns with its 4 KiB geometry wording without claiming that proposal is accepted.
Settled at approval, 2026-11-13: the issue-local proposal is accepted with the plan (see decisions.md, Settled at approval).

## Parallelizable?

Yes, after agreeing the proposed constants/B formula, error contracts and shared vectors:
Go admission/CSI and Rust geometry/session/store can be implemented independently; interop
and exact protobuf bound checks join them before merge. Neither unit may raise limits alone.
Settled at approval, 2026-11-13: the constants and B formula are agreed (see decisions.md, Settled at approval).
