# Issue #62: bounded observation contract

Normative draft appendix to [design](design.md), [spec](spec.md) and
[counting contract](counting-contract.md). These are R1 corrections after the historical
2026-11-13 approval; they are not implementation or a new approval.

## Ownership and request (S5–S8)

**One observation boundary.** #71 owns the separate telemetry socket and its filesystem,
credential and direct-mode rules. #47 adds latency through `observe`, schema 2; it defines no
root `stats` command. #62 changes neither `control.rs` nor any administration command. The
request stays exactly `observe\n`, at most 64 bytes including newline; no volume argument.
The node-local reader selects a volume from the complete reply, after strict decoding; an
absent requested volume is `unknown volume`, never a fabricated zero row.

**Inherited admission.** Keep #71's one serving thread, 1 s socket read/write timeouts, at
most four targets, one pending `Input::Observe`, at most one engine capture per second,
2 s engine wait, and cached engine snapshot younger than 1 s. A timed-out engine receiver
remains the sole pending receiver; later connections await it, never enqueue another input.
Retain every #71 usage/invalid-id/shutdown/timeout/four-target error verbatim. Reply size,
including `ok ` and newline, is at most 65,536 bytes. An error is one bounded line and closes
the connection. Cache an oversize refusal for the same one-second interval too.

**Off and privilege separation.** Without `--telemetry`, #62 records no counters, reads no
observation clock, starts no topology worker and creates no observation socket. Under #74
there is no telemetry socket in this step. The existing `--telemetry`/`--privsep` pair is
refused with #71's exact message; no root-socket fallback or new helper operation. The
observer reports not fresh/stale through #71's existing behavior. The node's I/O identity,
not the non-root observer certificate, authenticates the optional topology RPC.

## Schema 3 and composition

**Producer.** Whichever implementation adds the second of #62 and #71 extends the Rust
`flaminio-io/src/observe.rs` encoder, `flaminio-node` observation snapshot and the strict Go
`control/observe` decoder together. Schema 3 keeps #71's `node`, `disk`, target recovery
fields and `io` shapes, order and integer encoding exactly. Each target gains, after those
fields, `latency` then `traffic`. `latency` is null without #47, otherwise exactly #47's
schema-2 latency object, including its 32-node diagnostic selection, totals and ordering.
A null means unavailable, never zero latency. No latency field is copied into traffic rows.

```text
"traffic":{"instance":D|null,"valid":B,
 "reason":null|"counter_overflow"|"instance_id_exhausted",
 "capture":{"start_ns":D,"end_ns":D},
 "tx_basis":"engine_handoff_attempt","rx_basis":"decoded_receipt",
 "client":{"read_bytes":D,"write_bytes":D,
           "completion_basis":"frontend_success_offered"},
 "topology":{"source":"ListNodes"|"none","observed_ns":D|null,"age_ns":D|null,
             "classification_basis":"report_time_labels","target_zone":Z},
 "nodes":[{"node":ID,"zone":Z,"zone_relation":"same"|"other"|"unknown",
           "tx":{"client_write":C,"client_read":C,"copy":C,"other":C},
           "rx":{"client_write":C,"client_read":C,"copy":C,"other":C}}]}
C = {"frames":D,"framed_bytes":D,"payload_bytes":D}
```

**Types and invariants.** `ID` and `B` are #71's; `D` is a JSON string containing canonical
unsigned decimal u64 (`0` or a nonzero digit followed by digits, at most 20; checked <= u64
max). `Z` is null or a nonempty UTF-8 string of at most 128 bytes without U+0000–U+001F;
quotes and backslashes use ordinary JSON escaping. `instance` is the nonzero opaque
diagnostic ordinal of the fresh `Arc<TargetInstance>` marker from the counting contract.
Allocate ordinals with checked process-local monotonic u64 increment, never reuse/wrap;
marker pointer equality, not this ordinal, isolates receipts. On ordinal exhaustion the marker
is still fresh, instance=null, valid=false and reason=instance_id_exhausted (takes precedence
over counter_overflow); storage and counting continue. Otherwise instance is non-null. Epoch
and volume stay #71's target fields; instance is not an epoch, fence or readiness input.
Capture bounds include copying client counters and network rows, start <= end. Ages use
one engine monotonic clock, never Unix time. Before a good topology snapshot, source=none,
observed_ns=age_ns=null and every zone is null. Otherwise both times are non-null, source
is ListNodes, age is measured at capture end, and target/peer zones come from one installed
snapshot. Target node is the top-level `node`. Peer rows are sorted lexically by node ID,
with no duplicates; retain moved-away peers. Missing/empty zones map to null. Relation is
unknown if either zone is null, else same iff equal. Each direction has exactly four flows;
payload_bytes <= framed_bytes when valid. valid iff reason is null. Invalid counter values are not
usable totals; the consumer emits no ratios/deltas from them.

**Exact traffic, bounded refusal.** A complete schema-3 reply has at most eight traffic peer
rows summed across all targets. This cap never deletes a counter row or changes counting.
If the retained row count is greater, answer `error: traffic has more than 8 peer rows` before
copying rows. Do not choose the busiest/first peers or label a subset complete. This deliberate
report limit may refuse a volume with a long move history; counters and storage keep running.
There is no pagination/cursor or per-volume command in this step. Unknown/invalid target or
peer ID refuses the whole reply as #71 requires. A bounded sink serializes into one 65,536-byte
buffer outside the engine; exceeding it answers `error: observation exceeds 65536 bytes`.
No partial JSON/line is written. Existing schema-1/2 latency diagnostic selection is unchanged;
that documented selection never truncates #62's exact traffic totals.

**Totals and ratios.** The local reader adds every peer's C component with checked arithmetic
into same/other/unknown, separately per direction and flow. An aggregate overflow yields an
explicit invalid result (`aggregate_overflow`), not a wrapped ratio. For client_write use
client.write_bytes, for client_read client.read_bytes; expose known-other and unknown payload
and framed numerators separately as exact decimal strings and the corresponding denominator.
A zero denominator yields null ratios. Copy/other retain raw counters with no client ratio.
Derive these values outside the engine; they are not repeated on the bounded socket line.
Use integer numerator/denominator pairs for exactness; display rounding is labeled. An absent
peer is never inferred zero from a refused/invalid report. This adds no exporter, HTTPS field,
Prometheus family, metric label or cost-policy setting; #71's observer validates then discards
traffic/latency for its unchanged `/v1/node` and 73-series contract. The campaign reads the
telemetry socket locally under its existing permissions and retains the original line.

**Compatibility.** The new Go decoder accepts schema 1 with neither added field, schema 2
with required non-null #47 latency and no traffic, and schema 3 with both keys and required
traffic. Schema-3 latency may be null. It rejects unknown/duplicate/missing keys, trailing
bytes, schema >=4, malformed decimals/IDs/zones, semantic violations and all #71/#47 invalid
cases; key order is a producer golden property, not a trust assumption. A schema-1/2 line is
valid old telemetry but explicitly lacks traffic, so a #62 campaign refuses `traffic unavailable`
rather than zero. Old observers refuse schema 3 and become not fresh. Upgrade the decoder/
observer before the daemon; rollback the daemon before its decoder. If #62 lands before #71,
ship gated counters/snapshot tests without a socket; the later #71 implementation supplies
this composition. If #47 lands after schema 3, it fills latency without another schema change
and updates the same encoder/decoder fixtures. If #47 lands first, the later #62/#71 composition
retains schema-1/2 acceptance and the exact latency shape. The implementation landing second
records the composition in #71's contracts; clear dependency plans are read-only in this review.

## Bounds and capture work

**Engine work.** Preflight the existing target count and retained-node vector lengths before
walking traffic rows: <=4 target lengths and <=8 copied rows. Copy fixed counters (24 u64s per
peer), two client totals, fixed metadata and the at-most-32-per-target latency selection already
owned by #47. No full peer scan to compute traffic totals, JSON formatting, ratio calculation,
RPC wait or new lock is introduced by #62 on the engine thread. Topology lookups perform at
most nine string comparisons for each of <=8 peers plus <=4 target labels in the sorted,
<=256-entry cache. The existing #71 recovery walk and #47 latency work retain their own bounds.

**Memory and bytes.** #62 owns no completed-operation/session history. Its per-peer counters
stay indexed by the existing stable target node vector, as the counting contract specifies;
the report cap is independent of that existing lifetime state. A raw report has <=8 ×24 u64
counters (=1,536 bytes), <=12 copied ID/zone pairs (<=3,072 string bytes), fixed per-target
fields, and #47's bounded latency data. Allocate at most one 16 KiB traffic snapshot arena;
refuse before exceeding it. At most one engine capture, one cached snapshot and one formatting
buffer exist; replacing a cache drops the prior snapshot. A compact UTF-8 JSON
upper envelope is 60,615 bytes including prefix/newline: four targets, eight traffic rows,
128-byte IDs, 128 quote/backslash zone bytes, max-u64 fields (even mutually incompatible
maxima), all four maximal #47 latency objects, maximal disk/io and the longest legal reason
with a non-null instance. The inherited schema-2 envelope alone is 46,075 bytes. Non-ASCII
zone characters are emitted directly as UTF-8. The 65,536-byte sink is the final checked bound. If a future base schema
would make the combination exceed that limit, the whole reply refuses, never truncates.
Producer/decoder temporary storage is bounded by these arenas and the line limit; the Go
decoder reads <=65,537 bytes to detect excess, and enforces array/string caps while decoding
before growing slices. These bounds cover failures and repeated abusive requests too.

## Topology refresh (S5, S6)

**Actual RPC.** Use existing `/flaminio.meta.v1.MetadataService/ListNodes` with empty
`ListNodesRequest`; the response is one unpaged `repeated Node nodes`. Node.id=1, address=2,
labels=3, alive=4, draining=5 (`api/proto/flaminio/meta/v1/meta.proto:63–102`). Read only label
key `zone` and id. #72 adds capacity to that existing reply; it supplies no label paging.
No new protobuf field/RPC, metadata mutation or server authorization is added. Observation
never consumes addresses, alive, draining, capacity or disk UUID as a zone/authority input.

**Isolated bounded lane.** Add a ListNodes-only `TopologyObserver` in
`flaminio-metaclient/src/topology.rs`, separate from the existing `MetaClient::call` worker.
It owns one dedicated worker/current-thread runtime, a capacity-one nonblocking request slot,
a capacity-one completion slot and at most one private tonic channel. It never uses the
unbounded authority FIFO, `Engine::meta_call`, its waiter map, or an authority HTTP/2 connection.
Registration, heartbeats, GetVolume, ChangeIsr and existing address refresh stay on their
existing lane and cannot queue behind this observation call. Same CA/node-client identity and
metadata-server identity checks apply. Failure to start this optional worker only leaves zones
unknown/stale; it cannot fail attachment or storage. Foreground accounting submits no RPC.

**Schedule.** Initially start one refresh when telemetry starts. Keep one outstanding attempt
from admission until its completion/cancellation acknowledgement is consumed. While pending,
discard all 5 s ticks; do not retain a missed-tick count. The next eligible start is >=5 s after
that attempt's completion, whether it succeeded, failed or timed out; a busy engine delays it,
never catches up. Each attempt uses one configured endpoint, rotating after failure, with a
2 s total deadline covering connect/TLS/body/decode; no per-endpoint retry within the attempt.
The bounded projection decoder checks this deadline at each node/label field and before
publishing; an expired candidate is discarded even if its async timeout delivery was delayed.
Only one endpoint channel exists; replacing it drops the old one. Thus no accumulating 9 s
multi-endpoint request and no more than one attempt per 5 s even during failure. No useful
address/attach ListNodes result is copied into this cache: a separate lane and validator avoid
an unbounded duplicate decode/copy or observation work on the authority callback path.

**Decoder and cache caps.** On this lane only, refuse compressed replies and set tonic's
uncompressed receive-message cap to 262,144 bytes. Use an observer-specific bounded protobuf
projection decoder of the existing response: walk length-delimited node and label entries,
check lengths/counts before copying, validate UTF-8 without allocating, and skip unrelated or
unknown fields with checked wire lengths (including #72 capacity). Do not first materialize
an unbounded `pb::ListNodesResponse`/label HashMap. Refuse malformed wire/types/groups,
>16,384 visited field tags, >256 nodes, >2,048 label entries total, duplicate node IDs or duplicate
zone keys. IDs obey #71's 128-byte grammar. Label keys/values inspected by this decoder are
<=128 UTF-8 bytes; zone has Z's additional no-control rule; absent/empty zone stores null.
An oversized unrelated label also refuses the entire refresh, rather than allocating it.
The raw byte cap bounds skipped-field validation work too. These diagnostic bounds neither
reject RegisterNode nor narrow placement's label grammar.

Store only a sorted fixed-capacity vector of (id, nullable zone), with <=65,536 total ID/zone
bytes, plus fixed lengths/index/clock fields; each image uses <=96 KiB, enforced by a storage
budget, including its index. The worker builds/validates the entire candidate off-engine.
The engine polls the capacity-one completion slot once per existing tick, never via its
authority waiter map or a blocking receive. Install one immutable whole image at that engine
input boundary, with observed time
set at successful decode completion. Omitted/deleted nodes disappear only on a complete good
image. Empty/absent zone becomes unknown even if an older image had a label. Keep at most
last-good plus one candidate (=192 KiB), one raw body (=256 KiB) and the two fixed channel
slots; candidate installation moves ownership, without a second cache clone. Failure, cap,
duplicate or invalid label discards the candidate and retains last-good with its original time.
No successful refresh exists initially: all zones unknown. There is no TTL that erases a good
image or silently makes old labels current; report age exposes staleness.

**Cancellation and stale results.** Requests/completions carry an opaque observer-lifetime
generation and monotonically increasing attempt ID. Accept completion only for the outstanding
pair while telemetry is enabled; duplicates/old pairs drop without install. Counter exhaustion
disables this optional observer until restart, never wraps. Stop/reconfigure cancels the worker's
active future and closes its private channel; do not start a replacement until cancellation is
acknowledged/the old worker has joined. A late RPC cannot repopulate a stopped/replaced cache.
Retain the one pending slot until acknowledgement, even after deadline; an indefinitely stalled
worker therefore makes observation stale, not an unbounded thread/call backlog. Detach/reattach
never copies peer counters; every report classifies its captured target instance using one
current immutable topology image. Zones affect observation only, never safety or I/O outcomes.

## Implementation checks (future, not results)

- **Cross-language line contract:** Rust `observe.rs::encodes_traffic_v3` and Go
  `control/observe::TestDecodeTrafficV3` share `testdata/observe-v3-no-latency.line` and
  `observe-v3-latency.line`; retain v1/v2 goldens. Assert byte order/types and exact #47 shape.
  Invalid fixtures cover each missing/extra/duplicate key, v1 traffic, v2 traffic, v3 missing
  nullable latency, schema4, ninth peer, fifth target, bad decimals/zones/relations/ages,
  malformed counters, 65,537-byte input, overflow and no secret fields. Maximum producer
  fixture covers worst legal combined line; mutated over-limit input refuses without output.
- **Snapshot abuse:** telemetry/engine tests hold one pending input through repeated 2 s
  timeouts; repeated valid/oversize requests admit <=1 capture/s, bounded memory and no
  partial reply. More than eight retained peers refuses without scanning/copying a prefix.
  Valid eight-peer deltas and locally derived zone totals/ratios equal exact known counters;
  schema1/2, null denominator and invalid totals never appear as zero-cost evidence.
- **Topology isolation:** metaclient fake-clock/endpoint tests stall TLS/body/decode while
  repeatedly ticking; one admitted call/completion, no queued ticks, 2 s whole deadline,
  >=5 s completion-based retry and endpoint rotation. Authority worker continues servicing
  register/heartbeat/GetVolume/ChangeIsr in the same order as the observer-disabled twin.
  Check success/failure/outcomes and bounded pending state, not only elapsed wall time.
- **Topology input/lifetime:** projection fixtures cover each cap at/over bound, huge labels,
  repeated tiny nodes/labels, malformed lengths, unknown #72 capacity, duplicate IDs/zone,
  late completion/cancel/restart, attempt-generation and diagnostic-instance exhaustion. Partial/failed candidates retain
  last-good atomically and age increases; an empty complete image removes old labels.
  Shutdown with stalled observer admits no replacement until acknowledgement; attachment,
  read/write/metadata authority outputs remain unchanged. Run implementation `make test lint
  race contracts`; docs-only correction uses link/whitespace/schema-bound checks, no lab.
