# Issue #66: Frontend boundary — design

[spec](spec.md) · [decisions](decisions.md)

Approved 2026-11-13; not implemented.
Review corrections of 2026-11-20 are proposed and unapproved (decisions.md).
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
A client entry holds a fresh routing `Binding { volume, first_epoch, instance, generation }`
and a typed endpoint. Separately, the ublk adapter preserves immutable `DeviceLifetime`
`{ volume, first_epoch, boot_uuid, instance, generation, rdev, diskseq }` from #15, plus its id.
With #48, recovered devices keep the original lifetime tuple; they get this process's fresh
routing binding, never substituted into device-info or conditional detach. Endpoint states are
`Kept { lifetime }` (no fd), `Resuming { lifetime, phase, fd: Option }` (fd only after #48's
verified ready-open), and `Running/Stopping { lifetime, verified_handle }`. Handle acquisition
keeps #15/#74's direct versus privsep rules; no fabricated fd or reopening for describe.
Describe/status/device-info and detach-device use the preserved lifetime under existing locks;
I/O routes use only the fresh binding. A cfg(test) `Endpoint::Memory` exists only in tests.
#17 maintenance entries have neither client binding nor frontend endpoint and accept no client
I/O; they retain their own lifecycle and limit. No device slot is invented for maintenance.

Runtime controls a `FrontendControl` lifecycle interface: `start(binding, config)`,
`stop(binding, cause)`, `describe(binding)`; completion is asynchronous and tagged by binding.
Describe returns typed endpoint data so control.rs preserves existing ublk status text exactly.
No trait method can request a new metadata epoch or override target readiness. The control
socket's attach still chooses ublk; production never registers Memory. Non-Linux returns its
existing explicit attachment error. No new configuration or CLI mode.

Router key is global op id plus binding generation, registered before engine submission.
It writes a pre-reserved request mailbox without blocking while retaining #15's engine-lock
output order. The mailbox/terminal ownership and stop latch are specified below; removal
follows terminal ownership, not an ignored try_send failure. Stop closes admission first,
requests engine detach, installs explicit failed terminal states and signals adapter stop. Adapter drains
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
`Geometry { size_bytes, logical_block=4096, max_io_bytes=configured_max_io }` (at most 1MiB)
and `QueueLimits` before route allocation. ublk first uses #84's sole descriptor classifier:
registration failure → EIO, unsupported flags/op → EOPNOTSUPP, then invalid READ/WRITE range →
EINVAL; Flush/DiscardHint keep its immediate no-staging/no-engine behavior. Whoever lands
second moves/reuses that one classifier in the adapter, with the advertised size/max_io in
both modes; no second raw-descriptor decoder is introduced. Common typed Request validation
checks arithmetic, buffer/per-piece bounds and tag/depth before staging/admission. Results
include Unsupported → EOPNOTSUPP, Invalid → EINVAL, Io/Stopped → EIO, retaining prior classifier
refusals unchanged. `RequestKey { binding, qid, tag, sequence }` distinguishes every reuse;
counters fail closed on overflow. Copy and registered modes retain their respective storage.

Read states: Admitted → Planned → Submitted → Reaped → Verified → Finalized. Fast admission
failure falls back; final engine rejection fails/falls back according to existing policy,
never returns copied bytes as successful. Local mismatch releases its first plan, replans
once, and only then reports failure so the engine can select another ISR member. Remote
Piece is bounds-checked and correlated before copy. Success requires current request Done and
full requested-range completion; direct-read success cannot be inferred from a Piece alone.

Each executor batch owns its buffer/tag lease, plans, prepared bookkeeping and completion
futures through reaping; the queue owner retains the backing buffers and registered mappings.
Prepare all lengths, buffers and container capacity before the first SQ publication, including
libublk helpers whose future construction eagerly publishes. `PublishedBatch` has a custom
Drop which calls `std::process::abort()` while armed, before Rust drops any of its owner fields.
Arm before the first possible publication; no reliance on async-generator/local drop ordering.
The enclosing queue owner likewise checks outstanding published batches in its custom Drop
before destroying task sets, buffers, mappings or ring. This is a new explicit frontend guard;
#85's UringDevice guard protects a different ring. Unexpected partial publication, panic in
completion/cleanup, task-future drop or unprovable drain aborts without unwinding these owners.
Ordinary error/cancellation marks the request canceled and drives every published CQE to
completion before releasing ownership; a cancel signal does not drop its task. Keep armed
through ownership-releasing cleanup and disarm only when no published operation can reference
a released owner. An unsubmitted batch may drop normally. No closing-fd/cancel-request shortcut
proves drain. Preserve queue-local !Send borrows and permitted unsafe boundaries.

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
Use direct-read engine I/O in both adapters; remote payloads arrive only as Piece, not an
additional full-read Done buffer. At queue setup, checked products allocate per-tag mailboxes:
`P = max_io_bytes / 4096`, `2*P` LocalRead/Piece descriptor slots, payload credit max_io_bytes,
one reserved Done slot and one binding stop/error latch, all retained through request retirement.
A request reserves its tag's complete mailbox before engine admission; no hot-path allocation
or capacity wait. Current engine.rs splits I/O into at most P aligned chunk pieces and emits
one final Piece per completed remote suboperation plus one Done. LocalRead attempts/retries
share descriptor credits; consuming an event retains its credit while local I/O still owns it.
Retries are not claimed finitely bounded over the request's whole life: extra simultaneous
attempts may exhaust credits, and then follow the explicit failure rule below. Frontend's one
checksum retry reuses the existing local descriptor after its preceding CQEs are reaped.

Router nonblockingly appends descriptors in engine output order and installs Done separately
after earlier descriptors are owned. It removes the engine route only after mailbox ownership
of that terminal result is established; queue wake signals coalesce and are hints, not events.
Stop/overflow/closed consumer atomically latches binding failure without consuming a data slot,
stops admission and signals the existing lifecycle owner once per binding for detach after
dropping the engine lock. Every outstanding request
then has an explicit failed terminal state; successful Done cannot override failure. Drain or
abort under the batch ownership rule before request completion/tag reuse. Retain mailboxes and
local plans until their owners are drained; no discarded LocalRead token releases borrowed I/O.
Never block under the engine lock or silently discard a live result. After terminal/failure,
stale events are discarded by binding/request identity before touching client memory. At most
queues*depth routes, queues*depth*(2P+1) event/result slots and queues*depth*max_io payload bytes
are reserved, plus bounded descriptor/owner metadata and the per-binding latch; setup refuses
overflow/allocation failure before starting the frontend. Existing metadata/transport input
queues are not claimed bounded by this extraction.

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
| S1/S2 | cargo checks Linux/non-Linux; exact control/device-info/detach transcripts; pre-restart tuple still matches after recovery; kept requires no fd; predecessor routes cannot reach successor; maintenance allocates no frontend; no production Memory selector |
| S3 | sole classifier table: registration-first EIO, unsupported op/flag EOPNOTSUPP, invalid range EINVAL; configured max_io both modes; refusals do no staging/engine work; overflow/tag/depth limits and reserved memory arithmetic |
| S4 | withhold Done; duplicate/late Piece; invalid piece range; two volumes same tags; generation and sequence reuse; compatibility errors |
| S5 | shared executor short/error reads, checksum retry/fallback, fast/final lease rejection; injected subprocess panic after first/partial SQ publication, task drop and panic during completion/cleanup abort before backing release; independent backing reference keeps negative tests memory-safe |
| S6 | detach/depose at every read state, delayed CQE before tag reuse, exactly-once plan release, canceled write completion, fail-stop on unprovable drain |
| S7 | paused depth-one multi-piece reader retains Piece/Piece/Done; full/closed mailbox and retry-credit exhaustion with stop races yield terminal or explicit failure, no lock wait/early memory release; wake coalescing; batching/arrival timing; no new hot-path alloc/lock |
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
The extraction lands after #15. Whoever lands second with #17/#48/#84 retains the semantic
composition above without changing those clear plans or duplicating their authority. Preserve
#48's fresh-epoch rule and #17's separate maintenance lifecycle.
Unknown adapter contract details are resolved by a compileable thin extraction prototype
before broad moves; performance claims need the actual A/B, not line-count reasoning.

## Parallelizable?

No. Move runtime and policy first with one owner; split adapter/tests only once the internal
signatures compile. There is no independent Go/API unit in this scoped increment.
