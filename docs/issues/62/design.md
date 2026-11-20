# Issue #62: Per-volume target traffic, grouped by node and zone — design

[Spec](spec.md), existing [intent](intent.md), [decisions](decisions.md). Approved 2026-11-13;
not implemented.
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
- #47 proposes root stats v1 traffic/latency siblings. #61/#72 propose ReplicaRef(node,UUID)
  and paged disk/capacity state, not a cost model. These are drafts, not implemented facilities.

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

Receive path exposes validated prefix/body byte length beside decoded Frame, without a wire
change. NetEvent for outgoing sessions carries immutable target-volume/epoch/peer attribution,
so a late old-session frame can count Other for the still-current target without a growing
retired-session map. Drop attribution to ended target generations, never transfer totals to a
new epoch. Do not count accepted incoming replica-role traffic: its own remote target counts
that exchange. Undecodable/TLS bytes are outside this declared measurement boundary.

#61/#72 multi-disk routes preserve UUID/ref validation and current read-ready/fence gates.
Group observer rows by physical node only, even when two refs on that host exist; count each
actual frame handoff/receipt, not an inferred replica vote. In-process local disk dispatch is
still zero network bytes. No node/disk proto field or allocation/reservation ledger is added.

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

## Topology cache and shared stats v1 (S5, S7, S8)

One engine-wide ListNodes refresh every5s, independent of volume count and foreground I/O.
Coalesce with an already useful ListNodes result; additional observer reply updates zone cache
only, never endpoint choice, read source, liveness admission or placement. Existing address
update semantics remain unchanged. Failed/oversized list retains last labels with visible age.
Future #72 paging can feed a complete consistent node-label snapshot; device class/UUID is
never substituted for a zone. Mark absent/empty target or peer zone as Unknown.

Use report-time cached topology to classify all historical counters; labels added/corrected
later can reclassify earlier bytes. Report classification_basis=report_time_labels and cache
source/age. This is intentionally not historical location/billing evidence. Unknown is not
placement's shared empty-zone domain and never equals Same merely because both labels are empty.

Root `stats [VOLUME]` keeps #47 syntax/latency fields and adds exactly:
```
{version:1,counter_domain:"target_engine_v1",capture:{start_ns,end_ns},
 topology:{observed_ns,age_ns,classification_basis:"report_time_labels"},
 volumes:[{id,epoch,target_node,target_zone,client:{read_bytes,write_bytes,completion_basis:"frontend_success_offered"},
 valid,reason,nodes:[{id,zone,zone_relation,latency:null_or_47_object,
 traffic:{tx:{client_write:C,client_read:C,copy:C,other:C},rx:same}}],
 zone_totals:{same:TRAFFIC,other:TRAFFIC,unknown:TRAFFIC},ratios:R}]}
```
C={frames,framed_bytes,payload_bytes}; counters are decimal u64 strings, not lossy JSON numbers.
TRAFFIC has the same tx/rx shape. R contains per-flow known_other/unknown payload and framed
numerator/appropriate successful-client denominator strings; zero denominator→null. Copy has
no false client ratio: report raw hop bytes and workload-supplied copied-data denominator later.
Zones are string or null; absent latency stays null. Keep #47's existing latency scalar encoding.

Deterministic volume/node order, immutable captured values. Bound raw snapshot to1024 peer
rows and encoded reply4MiB; reject too-large before lengthy formatting, no truncation. Unknown
ID/extra arguments are errors. Existing status/list device identities remain unchanged; stats
never exposes payload, token, wrapped key or a new authority. No metrics exporter/new API.

## Floors, lab and restoration (S9–S12)

Write down payload floors with locality assumptions: zone RF=R has≥R−1 crossings when a replica
is in consumer zone; node/rack can reach0. Reads need0 with a usable same-zone copy else1;
direct source→joiner copy needs0/1; current target path can add a second crossing. Framed bytes
include response/control overhead, retransmission and attempted sends; never compare them as
if they were the payload floor. Unknown bytes make the cost bound incomplete, never cheap.

Later authorized `scripts/checks/zone-traffic.sh`, exclusive known fl-a/fl-b/fl-c, no other
workload/volume or operator moves. Private16GiB≤64-chunk RF2 node/zone volumes, deterministic
aligned write/read phases and pre/post stats after all operations drain. Exercise target-local
and nonlocal placements; temporary draining can exclude target from initial placement, then
restore before attach. Verify actual replica sets/domain before load, never infer selection.
Do not relabel fl-c: unknown rows are a required test, so measured cross-zone claims are partial.
One ordinary drain of a private replica measures source/joiner hops, digests/probes separately
from payload. Lab's three-node topology cannot demonstrate every possible two-remote-hop case;
unit tests use four nodes. No fault, formatting, price query or claimed optimal policy.

Require no I/O errors and offered totals equal the job
completed bytes. Wire framed-minus-payload matches codec and observed messages. Copy payload matches differences/mapped content, not whole capacity.
Retain raw stats/workload/placement/cached zones and binaries, report counts/ratios beside
floors and limits in docs/benchmarks. Trap stops only owned loads, detaches/deletes private
volumes and restores own drain marks; do not overwrite a changed operator state. fl-c cleanup
uses #14's explicitly authorized operator procedure if needed, never auto-confirm tombstones.
Incomplete collection/healing is a failed check with named pending actions.

## Files and every-rule tests

| Rules | Concrete proof |
|---|---|
| S1, S2 | repl traffic-class/inflight tests cover every purpose, reply after WriteOp gone, duplicate/unmatched/retry; engine/net codec tests compare encoded bytes to counted lengths for Hello/Fenced/data/control and future v2 frames, failed enqueue vs attempted connect. |
| S1, S3 | Node/ublk completion tests local fast/slow, zero-copy/staged, remote/cross-chunk read, successful/failed/reissued write; exact once per full-success offer (even when next fetch is held/idle), zero local-network bytes, no incoming replica-role double count. |
| S4 | Engine tests detach/failover/new epoch with late net/device callbacks, move source retained row and multiple UUID refs on one physical node; no counter-based voting/selection. |
| S5 | Fake ListNodes clock/caches: labels added after attach, target missing, empty/changed peer labels, failure/staleness/recovery, snapshot-basis reclassification; no endpoint or read/placement change. |
| S6 | Enabled/disabled observer twin has identical messages/meta requests/ordering and I/O outcomes apart from scheduled topology observations; fixed counters at different completed I/O rates, no completed-request history or new hot-path lock. |
| S7, S8 | Control/engine report tests with/without #47, exact strings/nulls/order/capture age, unknown ID,1024-row/4MiB refusal, overflow validity, zero/mixed denominators and no secret content. |
| S9–S11 | scripts/tools/zone-traffic-test.sh: floors/unknown cases, actual-placement validation, known bytes, one-/two-hop fixture, setup/load/report errors, every cleanup branch, hostile IDs/paths and no format/automatic confirm. Add to make lint. |
| S12 | Docs/help contract and retained artifact fixtures; no placement/source-policy API added. Existing sim/fencing tests unchanged. |

Touched: repl target/inflight classification; net frame length/recv metadata; node engine/net
traffic module and tests; ublk final commit hook/control stats; scripts/checks/tools, Makefile
and docs. No proto/on-disk/token change or new dependency. Implementation `make test lint race
contracts sim`; performance-plan no-fault baseline before claiming overhead within spread.
No test, measurement, fault or lab operation ran while drafting. No new ADR needed: observation
never alters authority. Direct destination copiers introduced later must extend this measurement
boundary explicitly; today counters cover target-mediated transport only. #47/#72 integration
and actual minimal-cost policy remain reviewed future work, not accepted by this draft.
Settled 2026-11-13 (user's instruction): values for a later cost or latency policy are set by the
later steps from this step's lab baseline, none in this step (see decisions.md).
