# Issue #62: Measure per-volume node and zone traffic — specification

Step3, measurement first ([intent](intent.md)); [design](design.md), [decisions](decisions.md)
if any. Historical approval: 2026-11-13; not implemented. The R1 revisions and appendices are
**proposed and unapproved**. No placement/read-policy change. **Issue #62 stays open.**

## Terms

- **Attachment:** one constructed target instance; counters start at zero there, including a
  detach/reattach at the same volume and metadata epoch. An opaque instance identity isolates
  observation callbacks; it creates no attach or fencing authority.
- **Peer:** a physical node, even when future replica references distinguish several disks.
- **Framed bytes:** replication frame length prefix/body handed by the engine to transport
  (TX), or decoded from that target's remote session (RX). TLS/TCP/IP/overlay and metadata
  traffic are excluded. TX is attempted handoff, not proven delivery/provider billing.
- **Payload bytes:** stored data bytes within Write/Copy/Data frames and permitted versioned
  snapshot/root counterparts, a subset of framed bytes.
- **Flow:** client_write, client_read, copy (both source/joiner hops), or other (reconciliation,
  probes, fencing, heartbeat and all remaining replication traffic).
- **Client bytes:** full-success frontend read/write dispositions offered to ublk, counted once
  per whole device operation; this is not proof of kernel/application acknowledgement. Attempts/
  failures/reissues can create transport bytes without a successful frontend disposition.
- **Zone relation:** same, other or unknown, using the last observed nonempty node zone labels
  at report time. Unknown includes either endpoint missing/unlabelled. Labels do not prove past
  physical location, prices, device health or storage authority.

## The invariant

Counting and reporting change no placement, source selection, valid replica set, session,
lease, timeout, metadata mutation or I/O result. Local dispatch contributes no network bytes.
Every counted frame/direction belongs to exactly one peer/flow, and counters never silently
wrap, reset on a session reconnect or pretend unknown topology is a known zone.

## Behaviour

**S1. Frame accounting.** Count every remote target frame once at handoff/decoded receipt,
with its actual framed length and stored payload length. Retransmissions count again; decoded
late/duplicate/unmatched replies count as other. Decoded Refused counts exactly once as other
before its existing Refused-to-Lost conversion. Exclude local replica dispatch, TLS handshake,
undecodable traffic and incoming requests handled solely in this node's replica role.

**S2. Flow attribution.** Preserve a request's purpose until matched reply accounting: client
writes and their replies, client reads and their replies, source-read/joiner-copy/discard/digest
work for resync/moves as copy; everything else as other. Reconciliation writes are not client
writes. One copied payload can cross two target-mediated hops, counted separately.

**S3. Client accounting.** Count full-success frontend read/write results once when offered to
ublk, including local fast, slow and remote/mixed paths. Pieces and internal retries do not
inflate it; failed/short frontend results add zero. Kernel delivery can fail after that offer,
so the report labels the boundary and does not claim actual application acknowledgements.

**S4. Identity/lifetime.** Per-volume/per-peer TX/RX flow counters live for this attachment,
retain peers after a move and reset only on a new target instance. Check immutable instance
identity before flow lookup or addition; old sessions of a live instance may count other, but
ended instances add nothing even at the same epoch. #90 owns exact-ref/store-lifetime/session
and codec admission, all preserved before physical-node aggregation. Two disks on one node do
not become independent redundancy. See [counting contract](counting-contract.md).

**S5. Topology.** Keep target/peer zone labels and refresh them off the I/O path, including
labels added after attach. Report source/age and classify all historical counters using that
reported label snapshot; do not imply historical zone accuracy. Missing/empty labels mean
unknown even though placement treats them as an empty failure domain. A failed refresh retains
last observed labels with visible age; it changes no address, read or placement policy.

**S6. Cost.** Steady-state accounting adds bounded counter/classification work only: no new
lock, metadata round trip, network request or completed-operation history per I/O. Storage is
bounded by existing peers/inflight operations, not request rate. Refresh and report serialization
stay outside foreground critical work; a report cannot stall the engine doing large formatting.

**S7. Shared observation.** Use #71's separate telemetry socket and `observe`, extended by
this issue's [observer contract](observer-contract.md) to schema3 with exact traffic totals and
optional #47 latency. That appendix fixes fields, strict Rust/Go fixtures, upgrades, bounds,
refusals and trust/scheduling behavior. No root `stats` command or administrative syntax is
added. Unavailable latency stays null; oversized observations refuse explicitly without
truncating traffic rows or totals. Telemetry-off and privsep behavior remain #71's.

**S8. Validity.** Counts are exact until representational exhaustion; overflow makes the report
invalid with a bounded reason, never wraps into a believable result or affects storage success.
Ratios use successful client bytes and payload/framed counts separately; zero denominators yield
null. Unknown-zone bytes remain separate from known-other, not silently excluded from claims.

**S9. The floor.** Document payload floors per actual chunk placement: R replicas in R
distinct known zones need at least R−1 crossings per steady write when a replica is in the
target's known zone (R when none is). Strict zone placement establishes that only while its
label/domain assumptions hold. Requested zone/bestEffort alone does not: #18 can fall back per
chunk to rack/node, including zero crossings. Node/rack may need zero. Reads need zero
with a usable same-zone copy, otherwise one. A direct copy needs zero if source/joiner same
zone, otherwise one; today's target-mediated path may cross twice. Framing, retries, placement
without locality and unknown labels can put measurements above/incomplete against these floors.

**S10. Baseline environment.** Before any mutation, check the exact binaries and #83 admission
profile/provenance. Its active physical-boottime-v1 refuses the current VMs: record
blocked/uncollected before setup. A separately authorized pre-#83 VM campaign is exploratory
only. The supported alternative is a separately authorized qualified disposable physical
fixture, with equivalent known/missing-zone topology and workload; no invented attestations or
bypass. See [counting contract](counting-contract.md). An admitted check uses known reads/writes
on zone/node volumes,
with target local and nonlocal, plus one drain. Require payload/framing counts consistent with
actual flows, and successful client totals match the load. Record counts/deltas/binaries and
zone assumptions beside floors. Existing unknown fl-c labels remain unknown; no permanent
lab relabeling, cost-policy improvement or provider bill is claimed.

**S11. Restoration.** Check refuses conflicting attached/workload state; only its private
volumes/temporary eligibility marks are changed. Cleanup stops owned workloads, restores state
and reports unfinished actions rather than claiming success. No disk formatting, raw corruption,
manual tombstone confirmation or changed replica-safety rule is introduced.

**S12. Docs/scope.** Document observation/refresh/attempt accounting and lab's limits in lab/docs;
retain architecture's desired policy as future. Steps1/2/4 (cost scoring, zone read/source
preference and provider defaults), metadata traffic and metrics export remain out of scope.

## Tests

- **S1–S4:** core/engine tables for every purpose, local/remote/two-hop copy, retries and
  duplicates; real outgoing codec/session tests for initial/established Refused before Lost,
  no double count, and same-volume/same-epoch detach/reattach with queued old receipts. Both
  frontend paths and #90 exact-ref/session/store-generation/profile rejection remain covered.
  Framed length matches actual current and composed v4 production codec bytes.
- **S5:** delayed/missing/empty/changed labels, target label absent, refresh failure/recovery,
  age/report snapshot and no address/selection changes.
- **S6–S8:** accounting-disabled twin has identical protocol/storage outputs; no per-rate state,
  counter overflow and zero denominator; bounded immutable schema3 observations with/without
  #47, isolated topology single-flight and strict decoder/upgrade/refusal fixtures per appendix.
- **S9–S12:** script fixtures cover actual per-chunk floors, #18 bestEffort fallback, missing
  zones, #83 VM/unknown-profile refusal before mutation and admitted physical equivalence; each
  failed load/report/cleanup branch, shell validation and command help. Actual collection is later.

## Open questions

- TX handoff and report-time topology are proposed measurement boundaries, not billing or
  delivery evidence; composition with #47/#71 and #90 follows the two appendices.
- Mixed foreground/copy traffic has no causal amortized bill; report flows and raw denominators,
  not a single claimed minimum. Values for later cost/latency policy remain unapproved.

Settled at approval, 2026-11-13: TX at engine handoff and report-time labels as the measurement
boundaries; flows reported with raw denominators, no amortized bill (see decisions.md, Settled at
approval).

Settled 2026-11-13 (user's instruction): values for a later cost or latency policy are set by the
later steps from this step's lab baseline, none in this step (see decisions.md).
