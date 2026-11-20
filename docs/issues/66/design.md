# Issue #66: Frontend boundary — design

[spec](spec.md) · [decisions](decisions.md)

Approved 2026-11-13; not implemented.
Step 1, built after #15's registry/router. No new external protocol or frontend is shipped.
Module/type contracts below are proposed extraction boundaries, not claims about existing code.
Settled at approval, 2026-11-13: these contracts as proposed (see decisions.md, Settled at approval).

## The facts it starts from

- `flaminio-io/src/ublk.rs:506` Shared owns engine, inputs, routes, ublk id and queue wakeups.
  `:582` EngineCtx pumps/flushes and routes output. `:619` deposition calls kill_dev.
- `:1230` handles copy-mode requests; `:1293` local_read plans/verifies and retries checksum
  mismatch once; `:1629` fast_read_copy has separate admission/planning/finalization.
- `flaminio-node/src/engine.rs:181` already takes Read/ReadDirect/Write and returns Done,
  LocalRead and Piece. Its `FastLocal`/read plans carry engine authority and pinned state.
- `flaminio-io/Cargo.toml:30` gates libublk/io-uring/smol/libc to Linux; testability must not
  accidentally make libublk a portable requirement. Current binary has no reusable library.
- #15 proposes Node, registry and route generation tagging inside ublk.rs, with a single
  engine input owner. This moves that future code without changing its lock/thread protocol.
  #17's device-less target and #48's kept ublk devices must compose with the endpoint enum.

## Node and binding (S1, S2, S6)

Extract `flaminio-io/src/runtime.rs`: Node, Registry, Routes and engine input-owner thread from
#15. Add `frontend/mod.rs` for binding/request/policy types and `frontend/ublk.rs` for device
adapter. Keep node.rs as configuration/build entry; control.rs imports runtime, not ublk.
Registry entry holds `Binding { volume, first_epoch, instance, generation }`, state and
`Endpoint::Ublk { id, dev_t, held_fd }`. A cfg(test) `Endpoint::Memory` exists only in tests.
Device-only record/recovery metadata remains in the ublk adapter, surfaced through typed
start/stop/status/recover methods, not opaque fabricated paths.

Runtime controls a `FrontendControl` lifecycle interface: `start(binding, config)`,
`stop(binding, cause)`, `describe(binding)`; completion is asynchronous and tagged by binding.
Describe returns typed endpoint data so control.rs preserves existing ublk status text exactly.
No trait method can request a new metadata epoch or override target readiness. The control
socket's attach still chooses ublk; production never registers Memory. Non-Linux returns its
existing explicit attachment error. No new configuration or CLI mode.

Router key is global op id plus binding generation, registered before engine submission.
It sends bounded events to the owning queue/request. A terminal event removes the route once;
a later event is discarded before touching frontend memory. Stop closes admission first,
requests engine detach, fails/removes pending routes and signals adapter stop. Adapter drains
all outstanding local I/O before recycling slots or destroying buffers. Detach response and
registry slot release retain #15's existing ordering. #48 may keep a kernel device, but its
successor starts a fresh binding and never receives predecessor routes.

## Shared request and read policy (S3–S5, S7)

Extract non-ublk `frontend/policy.rs` from EngineCtx/submit/read loops. `QueuePolicy` owns
binding, qid, due/staging bookkeeping and per-tag state; Node owns engine/router. It uses
static dispatch over `LocalReadExecutor` to preserve current queue-thread execution. The
executor receives an engine ReadPlan plus a bounded buffer range, returns disk completion
status, and cannot manufacture Done or a lease permit. ublk implements the executor on its
existing ring; Memory uses a deterministic in-process executor over the test store.

Request contract: `Request { tag, sequence, op, offset, len, flags }`, checked against
`Geometry { size_bytes, logical_block=4096, max_io_bytes=1MiB }` and `QueueLimits` before
route allocation. Use checked arithmetic/conversions; bounds include offset+len, per-piece
range and tag/depth. Copy-mode and registered-buffer mode share validation but retain their
own storage. `RequestKey { binding, qid, tag, sequence }` distinguishes every reuse; counters
fail closed on overflow. Internal errors are Invalid, Io, Stopped; ublk maps Invalid to
-EINVAL, all others -EIO as today. Unsupported operation/flag mappings are frozen by tests.

Read states: Admitted → Planned → Submitted → Reaped → Verified → Finalized. Fast admission
failure falls back; final engine rejection fails/falls back according to existing policy,
never returns copied bytes as successful. Local mismatch releases its first plan, replans
once, and only then reports failure so the engine can select another ISR member. Remote
Piece is bounds-checked and correlated before copy. Success requires current request Done and
full requested-range completion; direct-read success cannot be inferred from a Piece alone.

Each executor batch owns every submitted operation until all completions are reaped. Error
collects the rest; it cannot return early after one failed completion. Cancellation marks the
request canceled but retains buffers/plans and drains. Drop may release an unsubmitted plan;
a submitted batch cannot be dropped into buffer reuse. If draining is impossible, invoke the
existing daemon fail-stop path. #85's preflight/publish/drain principle applies to queue SQEs
too; extraction must not move borrowed pointers to another thread or extend their lifetime
by an unchecked cast. No new unsafe block outside the existing permitted low-level boundary.

Retain plan pins until disk completion and engine `finish_plan`/read finalization exactly once,
including partial setup, failed verification and stop. Use owned state tokens and explicit
terminal transition, not a second independent policy in each adapter. The concrete GAT/future
signature must preserve !Send queue-local borrows; no Box<dyn Future> allocation per request.

Writes hand Bytes to the engine as today; registered writes retain staging until consumed.
Move Arrival guard semantics intact. Round-end pump runs once after ready tag tasks; pending
staging/gather deadlines preserve #10. Flush/discard dispatch goes through shared policy but
retains #84's documented current behavior and qualification gates, not an invented ordering
barrier. Keep engine/input-owner and lock order exactly #15; no extra lock/round trip/allocation
in steady-state request handling. Compile both modes to detect accidental Linux-only policy.

## Contracts

No proto, wire, disk format or socket grammar change. Binding, Request/RequestKey, Geometry,
QueueLimits, Endpoint, FrontendControl and LocalReadExecutor are internal Rust contracts above.
Lifecycle events and read results always include binding and request key; mismatches drop.
Bounded queues derive capacity from configured queues×depth; each request owns one route and
its bounded read-plan pieces. Existing metadata/transport input queues are not silently
claimed bounded by this refactor; their separate capacity work remains separate.

Expose common runtime/policy as crate-internal modules. Prefer unit tests beside policy;
if engine integration needs cross-crate access, add `flaminio-io/src/lib.rs` with a documented
narrow test-support feature disabled by default, not a public production memory frontend.
The final library/test placement must not create two independent runtime implementations.

## Files touched

- `flaminio-io/src/{main.rs,node.rs,control.rs,runtime.rs}`: node entry and lifecycle, S1/S2/S6.
- `flaminio-io/src/frontend/{mod.rs,policy.rs,ublk.rs}`: extracted common policy and adapter,
  S1–S7; move Linux-only queue implementation without rewriting algorithms.
- `flaminio-io/src/frontend/{memory.rs,tests.rs}` cfg(test): S3–S8 actual shared-policy harness.
- `flaminio-node/tests/engine.rs`: retain core tests; remove duplicated frontend orchestration
  only when equivalent production-policy coverage exists in flaminio-io.
- Cargo manifests only if required for test layout; no new runtime dependency.

## Tests

| Rules | Planned checks |
|---|---|
| S1/S2 | cargo checks Linux/non-Linux; exact control status/attach/detach transcripts; no production memory selector; ublk option snapshots |
| S3 | validation table: overflow, zero/misaligned/out-of-range/oversize request, flags, bad tag, exhausted depth; counters/route/memory bound |
| S4 | withhold Done; duplicate/late Piece; invalid piece range; two volumes same tags; generation and sequence reuse; compatibility errors |
| S5 | shared executor injected short/error reads, first/second checksum mismatch, fast permit rejection, final lease rejection, drain after partial SQ publication |
| S6 | detach/depose at every read state, delayed CQE before tag reuse, exactly-once plan release, canceled write completion, fail-stop on unprovable drain |
| S7 | fake rounds prove group commit/staging/arrival timing; bounded wake saturation; no new hot-path alloc/lock in trace |
| S8 | Memory drives actual engine/store harness through production policy; no duplicated retry/finalization code |
| S9 | authorized A/B and existing fault/CSI/zero-copy checks described below |

Use `make lint test race hygiene contracts`, `make miri sanitize` for changed low-level lifetime
paths where supported. No new replication safety rule: existing `make sim` remains regression
evidence, while frontend adapter races need the shared executor tests. No tests were run to
validate future implementation during drafting.

## Lab

Only after authorization: `make csi-test`, `make node-fault`, `make bench-reads` with zero copy,
plus #15 S12 A/B against extraction's parent: 4K QD1/QD32 and1M QD8 reads ≤3% loss; 4K writes
RF1/RF2 and1M QD8 RF1 ≤5% loss; ≥80% raw large-write criterion; zero integrity failures.
Use interleaved repeated baseline/candidate runs normalized to same-session raw. Repeat noisy
job once, report inconclusive if unresolved; no invented acceptance. Existing campaigns can
format dedicated disks and delete benchmark data, so preserve their preflight/restore and
record exact settings. No lab execution authorized by this document-generation task.

## Docs, risks and ADRs

Update architecture frontend boundary and issue followups to show step1 only; later NVMe/TCP,
vhost-user guest memory isolation, fallback distribution survey and export failover remain open.
Settled at approval, 2026-11-13: the distribution survey comes before any plan of steps 2–4 (see
decisions.md, Settled at approval). Settled 2026-11-13 (user's instruction): the
kernel's NVMe target or our own is chosen by step 2's ADR after the survey (see decisions.md).
ADR0001/0003 consistent: Rust extraction, same kernel/ublk baseline. ADR0005/current stricter
takeover contract unchanged; #83 profile support still required. No new architectural ADR for
this internal extraction; later protocols/dependencies/security exceptions require their own.
The extraction crosses #15/#17/#48. Land after #15 and reconcile kept/device-less endpoint
states before coding; do not duplicate registry ownership or weaken #48's fresh-epoch rule.
Unknown adapter contract details are resolved by a compileable thin extraction prototype
before broad moves; performance claims need the actual A/B, not line-count reasoning.

## Parallelizable?

No. Move runtime and policy first with one owner; split adapter/tests only once the internal
signatures compile. There is no independent Go/API unit in this scoped increment.
