# Issue #62: Per-volume target traffic, grouped by node and zone — design

[Spec](spec.md), existing [intent](intent.md), [decisions](decisions.md). Historical approval:
2026-11-13; not implemented. R1 revisions and both normative appendices are **proposed and
unapproved**: [counting contract](counting-contract.md), [observer contract](observer-contract.md).
Measurement-first step3 only: **#62 stays open** for placement scoring, zone read/copy source
preference and cloud defaults. Assumes #18's placement constraints without changing them.

The facts it starts from:
- Target send knows ReqKind/purpose and stores inflight(send time,kind)
  (`flaminio-repl/src/target.rs:1182,359`). WriteOp.client distinguishes client from repair;
  ReadFor distinguishes Client/Repair/Copy (`:296,311`). Frame tags alone cannot do that.
- Engine routes local requests in process and remote requests through NetCommand
  (`flaminio-node/src/engine.rs:1915–2082`). Direct/fast local reads bypass transport entirely.
- `on_reply` removes inflight before handling body (`target.rs:899`). Classification must be
  captured before removal and survive WriteOp completion for late joiner acknowledgements.
- Target keeps addresses but discards node labels; current label refresh only fills missing
  addresses (`engine.rs:1558–1590`). Zones added later need a separate observation refresh.
- Outgoing `net.rs:282` converts decoded Refused directly to Lost; ordinary Reply accounting
  alone misses it. `start_target` constructs a runtime (`engine.rs:1596,1665`), and standalone
  detach/reattach can retain the same metadata epoch (`:640,1859`). Epoch is not instance identity.
- #71 owns bounded telemetry `observe`; #47 extends it to schema2 latency. #90 alone owns
  ReplicaRef/store lifetime and composed replication v4; #61 is its tracker wrapper and #72
  capacity accounting. #18 bestEffort fallback is per chunk; #83 refuses current lab VMs.
  These reviewed dependency plans are drafts, not implemented facilities.

## Classify and count at the transport boundary (S1, S2, S4, S6)

Add pure `TrafficFlow::{ClientWrite,ClientRead,Copy,Other}` beside ReqKind. Store flow in the
existing inflight item at send; carry it as observer metadata in Output::Send. Add read-only
`request_flow(node,session,req)` for RX classification before on_reply removes the request.
No extra completed-request map/history: memory is one enum per existing inflight entry.

Mapping: client Write→client_write, repair Write→other; ReadFor::Client→client_read,
ReadFor::Copy→copy, ReadFor::Repair→other; resync Copy/Discard/BeginResync/Mapped and
resync Digest→copy; reconciliation Digest/Hello/Heartbeat/Settle→other. Matched replies inherit
that flow, including malformed body reported to core. Unknown/stale/duplicate reply→other.
Retransmitted request gets counted again; classification never alters validation/lease logic.
Future snapshot catch-up requests use the same Copy purpose, not guessed payload/tag heuristics.

TargetRt owns `TrafficCounters` rows indexed by the existing stable node index; retain rows
when a node leaves placement, drop all only with this target. Each row has TX/RX for each
flow: frames, framed_bytes and payload_bytes. All checked u64; overflow latches invalid=true,
reason=counter_overflow without wrapping or changing storage results.

At remote NetCommand enqueue, after existing session/route and encoded-length validation,
count one successful handoff with frame bytes including4-byte prefix. Hello at Connect is an
attempt even if TLS/connect later fails; report tx_basis=engine_handoff_attempt. Failed queue
acceptance is not counted. Local service dispatch adds zero network bytes. Compute TX length
with checked nonallocating Frame::encoded_len sharing codec size rules; never encode payload
a second time just to count. Write/Copy/Data length alone contributes payload, not tokens.

The [counting contract](counting-contract.md) fixes the receive/lifetime adapter. Allocate a
fresh opaque shared `TargetInstance` marker at each TargetRt construction. Carry it with the
validated immutable outgoing route through NetCommand, session and receipt; compare the current
runtime's marker before flow lookup or any addition, including TX and initial Hello. Never
infer it from volume/epoch. Old sessions of that same live instance count unmatched replies as
Other; detached generations add nothing, including same-volume/same-epoch reattach.

Expose actual decoded prefix/body lengths from Conn::recv, without wire changes. The outgoing
receive hook emits exactly one receipt per decoded frame before dispatch or Refused-to-Lost.
Refused contributes one RX Other frame, its actual framed bytes and zero payload; it still
returns the same SessionError and Lost. Loss itself adds no frame. No second receipt is emitted
by the normal Reply path. Accepted incoming replica-role traffic remains excluded.

#90 exact-ref/store-generation/incarnation/profile/session validation stays ahead of flow
matching and physical-node aggregation. The adapter uses the captured admitted route, never a
current node-name lookup, to group actual bytes. Invalid decoded replies can count Other on that
route without gaining storage authority. Each actual send/receipt counts once even for two refs
on one physical node; local disk dispatch still contributes zero network bytes. #62 owns this
hook whichever implementation lands second, including the active ordinary/snapshot/root v4
profiles and legacy profiles permitted by #90. No authority field or allocation ledger is added.

## Successful client denominator and immutable snapshots (S3, S4, S6)

A generation-owned shared `ClientByteCounters` is handed to that device's existing queue
context. Add read/write len once for a full-success frontend disposition immediately before
COMMIT_AND_FETCH submission. Fast, slow, staged/zero-copy, remote/mixed paths use the same final hook. Failed/short frontend results
add zero; reissues count only their full-success disposition. Pieces/internal repair/copy do not
count client bytes. Queue-local ownership/op completion prevents duplicate hook invocation.
Atomic additions introduce no new mutex or engine message; checked CAS overflow marks invalid.
Old device contexts retain only their old counter object and cannot affect replacement target.
libublk submit_io_commit_cmd awaits the next fetched request, so never count after its await: an
idle queue could otherwise hide its last operation indefinitely. Expose completion_basis as
frontend_success_offered; kernel delivery/abort after offering is not application-ack evidence.

Report copies client atomics then bounded owner-thread network rows; formatting/ratios happen
outside the engine lock. Expose snapshot capture interval, not a fictitious simultaneous global
instant across queue threads. Quiescent before/after deltas give exact benchmark totals; live
mixed snapshots are observations over that interval, not an amortized provider bill.

## Isolated topology and shared observation (S5, S7, S8)

The [observer contract](observer-contract.md) is normative for topology schedule/cache limits,
`observe` schema3, nullable exact #47 latency, strict Rust/Go decoders and upgrade order. It
extends #71's separate telemetry socket with its 64KiB line, four-target limit, one pending
capture, one capture/second and two-second engine wait. It supplies exact peer/byte refusals;
traffic rows or totals are never truncated. There is no root `stats` command.

Topology uses the appendix's bounded isolated observer worker, never the serial authoritative
metadata queue or Engine::meta_call wait map. Only one refresh can be outstanding; ticks cannot
queue. Last-good labels install atomically after full bounds/generation checks. There is no
promised #72 paging facility. Failure/oversize/timeout retains the last good labels with visible
age and cannot change addresses, liveness admission, reads, placement or foreground scheduling.

All history is classified using the report-time label snapshot. Later labels can reclassify
earlier bytes; missing/empty target or peer labels are Unknown, including two empty endpoints.
This remains observation rather than historical location, a bill, or storage authority. The
appendix fixes capture intervals, raw string counters, nullable ratios/latency, secret exclusion,
telemetry-off/privsep behavior and bounded cache/rate/refusal handling for concurrent readers.

## Floors, lab and restoration (S9–S12)

Use actual per-chunk placement and known labels for payload floors, as the
[counting contract](counting-contract.md) details. R replicas in R distinct known zones need
at least R−1 crossings with one in the target's known zone; node/rack or zone bestEffort fallback
can reach0. Requested zone/bestEffort does not establish a zone-separated floor. Reads need0
with a usable same-zone copy else1; direct source→joiner copy needs0/1 and today's target path
can add a crossing. Framing/control/retries/attempts are not that payload floor. Unknown-zone
bytes leave the comparison incomplete, never cheap.

Later separately authorized `scripts/checks/zone-traffic.sh` first inspects exact binaries and
#83 admission/profile/provenance before creating volumes, changing eligibility, starting loads
or draining. With physical-boottime-v1 active, current fl-a/fl-b/fl-c VMs yield
blocked/uncollected and no setup mutation. Only a separately authorized explicitly pre-#83
exploratory campaign may use those VMs; it is not supported-profile evidence. The supported
alternative is an equivalently labelled, qualified disposable physical fixture. No synthetic
physical attestation, forced admission or clock-policy relaxation is allowed. If absent, the
baseline remains uncollected. Exact fixture, preflight outcomes and cleanup are in the appendix.

On an admitted exclusive fixture, use private16GiB≤64-chunk RF2 node/zone volumes,
deterministic aligned write/read phases and quiescent pre/post observations. Exercise target-local
and nonlocal placements; restore any owned temporary exclusion before attach. Validate each
actual replica set/domain; include an unknown-zone node without permanent relabeling. One
ordinary private-replica drain measures both target-mediated hops and control flow. Three hosts
cannot demonstrate every two-remote-hop case; deterministic unit tests use four nodes.

Require no I/O errors, offered totals equal completed workload bytes, actual codec framing
matches observed frames, and copy payload matches differences/mapped content, not whole capacity.
Retain exact binary/profile/admission evidence, raw observations, workload, placements and labels
in docs/benchmarks beside per-chunk floors and limits. No fault, format, price query or claimed
optimal policy. Trap stops only owned loads, detaches/deletes private volumes and restores only
owned unchanged eligibility marks. Report incomplete healing/cleanup with named pending actions;
no automatic tombstone confirmation or overwrite of concurrent operator changes.

## Files and every-rule tests

| Rules | Concrete proof |
|---|---|
| S1, S2 | repl traffic-class/inflight tests cover every purpose, late joiner, duplicate/unmatched/retry. `net.rs` codec/session tests count initial/established Refused once before unchanged Lost, and failed enqueue versus attempted Connect; actual current and #90 v4 profile lengths (counting appendix). |
| S1, S3 | Node/ublk completion tests local fast/slow, zero-copy/staged, remote/cross-chunk read, successful/failed/reissued write; exact once per full-success offer (even when next fetch is held/idle), zero local-network bytes, no incoming replica-role double count. |
| S4 | Engine tests queue old Reply/Refused, detach and reattach with the same volume/epoch, then prove no additions/flow lookup. Same live instance/replaced session counts Other. Test move-retained rows, #90 wrong UUID/G/session/incarnation/profile admission and node grouping; no votes/selection change. |
| S5 | Observer appendix: exact cache/node/zone bounds, atomic last-good install, delayed/missing/empty/changed labels, failure/stale completion/recovery and reclassification; no endpoint/read/placement change. |
| S6 | Enabled/disabled observer twins have identical authoritative messages/order and I/O outcomes; stalled endpoints/repeated ticks/concurrent readers prove isolated bounded observation work. Fixed per-peer/inflight state, no per-completed-request history or hot-path lock. |
| S7, S8 | Observer appendix golden/invalid Rust/Go schema3 fixtures with/without #47; strict fields, line/row/pending/rate/deadline refusals, capture/staleness, overflow, null denominators and no secret content. |
| S9–S11 | `scripts/tools/zone-traffic-test.sh`: actual chunk floors including #18 zone/bestEffort RF2 same-zone fallback, unknowns, known bytes/hops, #83 VM/unknown-profile refusal before mutation, qualified physical setup, all load/report/cleanup errors and hostile inputs. Add to make lint. |
| S12 | Docs/help contract and retained artifact fixtures; no placement/source-policy API added. Existing sim/fencing tests unchanged. |

Touched: `dataplane/crates/flaminio-repl/src/target.rs` for flow/inflight;
`flaminio-net/src/{frame,session}.rs` for checked length/decoded receipt;
`flaminio-node/src/{engine,net}.rs` and traffic tests for instance/session attribution;
`flaminio-io/src/ublk.rs` for final client hook and #71 telemetry encoder for schema3;
`control/observe/` strict decoder/fixtures; isolated metaclient observer worker;
`scripts/{checks/zone-traffic.sh,tools/zone-traffic-test.sh}`, Makefile and docs. No proto,
on-disk, token or wire-authority change; no dependency-plan edits. `make test lint race contracts
sim` are future implementation checks. Run the separately authorized no-fault baseline before
claiming overhead within spread.

## Contracts, risks and ADRs

Both appendices define the observer-only in-process attribution, topology lane, telemetry ABI
and planned tests. Existing administrative socket, replication bytes and authority are unchanged;
#90 remains sole exact identity/profile owner. Accepted ADRs0001–0006 remain consistent,
especially0005 replica-confirmed leases: observations cannot renew, admit or depose. No new
safety policy or architectural authority choice needs an ADR or simulator mutant; observer
isolation/equivalence tests prove this slice. No tests, measurements or lab operations ran
while drafting. Direct destination copiers must extend measurement explicitly in their own
plan. Later cost/read/placement policy and its values remain outside this unapproved revision.

## Parallelizable?

Rust/Go observation units can build against the appendix schema/fixtures; whichever of #62,
#47/#71 and #90 lands second installs its named adapters and shared golden tests. Physical
baseline collection remains separately authorized and unavailable until preflight succeeds.
Settled 2026-11-13 (user's instruction): values for a later cost or latency policy are set by the
later steps from this step's lab baseline, none in this step (see decisions.md).
