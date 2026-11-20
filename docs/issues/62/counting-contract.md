# Issue #62: receipt, lifetime and baseline contract

Proposed, unapproved R1 appendix to [design](design.md) and [spec](spec.md).
The [observer contract](observer-contract.md) owns telemetry schema3, resource bounds and
isolated topology scheduling. This appendix preserves the approved measurement boundaries;
it creates no replication, attach, fencing, placement or clock authority.

## One target instance before any observation (S1, S2, S4, S6)

**Identity.** Construct one fresh `Arc<TargetInstance>` marker with each `TargetRt`, even if
volume and metadata epoch equal a detached runtime's. Immutable marker identity is compared
with `Arc::ptr_eq`, never reconstructed from a name, epoch, node index, address or session key.
Old events keep the old Arc alive, so its allocation cannot be reused while a callback exists.
The marker is observer metadata, absent from wire/token/metadata and all admission decisions.
A separate checked, nonzero u64 ordinal supplies schema3's diagnostic `instance`; it is not
the equality test. Do not wrap/reuse that ordinal in one daemon process. Exhaustion leaves
Arc isolation and counting intact, reports instance=null, valid=false and the bounded reason
instance_id_exhausted (which takes precedence over counter_overflow), and changes no I/O result.
Without telemetry neither marker/counters nor observation clocks are required.

**Captured route.** `TrafficAttribution` carries the marker, volume, physical peer ID, and
immutable outgoing route/session binding. `NetCommand::Connect` captures it only after the
existing route/session/length checks; `Send` carries the same binding and the classified flow.
The outgoing session retains it and supplies it on decoded receipts. No completed-session map
or lookup through a current address/node-name cache is added. For current source the binding
includes node index, epoch and session. With #90 it includes the already required exact
receiver ref, store generation, volume incarnation (positive first_epoch or signed legacy LID),
epoch/session and active profile. No authority tuple is shortened to the physical node.

**Order.** Before TX addition, or RX flow lookup/addition, find the current volume runtime and
compare its marker with the captured marker. Absent/different marker drops observation only.
Then validate/use the captured route under the owning adapter's existing rules; only then
select the physical-node counter row. Read-only flow matching additionally requires the exact
current session/request binding; unmatched/duplicate/superseded-session replies of the same
live instance are Other. An ended instance contributes nothing, including initial refusals
queued before a same-volume/same-epoch detach/reattach. Existing protocol dispatch still uses
its existing key/session validation and receives exactly the same frames/loss events.

**No history.** Each retained stable node row has TX/RX × four flows × three checked u64
counters. Keep departed peers as today’s stable target node vector does; free rows with that
runtime. Flow is one enum on each existing inflight request and on Output::Send, retained until
that inflight entry ends; do not recover it from an already completed WriteOp or add a retired
request/session history. A retained obsolete session’s attribution is only its existing
session/event ownership, not a new process-wide map. Local replica dispatch is zero network
traffic, including #90 local exact-ref and fast-read routes.

## Decoded receipt before Refused becomes Lost (S1, S2)

**Actual hook.** Current `flaminio-net/src/session.rs::Conn::recv` reads the four-byte prefix,
bounded body and decodes Frame. Add a checked result carrying the actual prefix+body byte
length and stored payload length alongside that decoded frame; keep any nonobserving callers'
behavior. No wire byte changes and no second encode/copy of a payload. Only successfully decoded
frames yield a receipt. TLS failure, short/invalid frame and undecodable bytes remain outside
the settled decoded-receipt boundary.

**Outgoing only.** In `flaminio-node/src/net.rs::outgoing_session`, run the single receipt hook
immediately after that decoded result, before matching Refused, validating a reply body or
emitting a protocol loss. In current source `session::connect` sends Hello without reading its
answer: therefore its first Refused passes this same hook. If an active profile moves the first
reply read into a handshake helper, that helper must use this same hook exactly once.

**Events.** Non-Refused frames carry their receipt metadata on the existing Reply event; the
engine accounts it once before from_frame/read-size/core handling. A Refused emits one
`NetEvent::RefusedReceipt { attribution, framed_bytes }` at the receive hook and then returns
the existing `SessionError::Refused(reason)`, producing the same Lost and reason as before.
For that event add one RX Other frame, actual framed_bytes and zero payload after the marker/
route checks. Never forward Refused as a second Reply. Lost adds no frame and does not repeat
receipt accounting; a locally generated failure without a decoded frame adds no RX bytes.
The same producer emits RefusedReceipt before Lost. No incoming replica-role request is counted.

**Semantic rejection.** The immutable admitted outgoing route identifies whose decoded bytes
were received; fields in an untrusted reply cannot select another counter row. #90 still
rejects wrong UUID/G/incarnation/profile/session and malformed Fenced evidence before storage,
lease/source credit or normal flow matching. A successfully decoded but rejected response may
count Other using that captured route, never as a validated ack. Refused is always Other,
including #90's initial_complete_reopen; it neither opens a session nor changes readiness.
This preserves both exact decoded-byte accounting and full authority validation before grouping.

## #90 is the single identity/profile owner (S1, S2, S4)

**Composition.** #90/contracts.md, core-design.md and wire-contracts.md own ReplicaRef,
store-lifetime G, incarnation, bootstrap/readiness and immutable session authority. #61 is a
tracker wrapper; #72 supplies capacity through existing ListNodes and no label paging or
ReplicaRef. Whichever implementation of #62 and #90 lands second carries the observer marker/
length metadata through the exact existing remote/local adapters, without changing those
contracts. Observe each actual frame once, then aggregate by the captured physical ref.node;
two UUIDs on one node never become two redundancy/domain votes.

**Real profiles.** Length tests use production encode/decode, not assumed future-v2 formulas.
Cover today's ordinary codec and whichever draft profiles are implemented: #51 snapshot2,
#58 roots3 before identity; once #90 activates, exact ALPN flaminio-repl/4 and its profiles0
ordinary,1 snapshot,2 roots,3 legacy ordinary and4 legacy snapshot. Use #90's actual 0x32
HelloIdentity/0x51 FencedIdentity layouts and corresponding allowed payloads, including their
four-byte prefix. Never insert identity bytes into an older profile or enable fallback after
activation. Local synthetic/fast paths use #90 admission but still add no network bytes.

## Payload floors use each chunk’s actual placement (S9)

For a quiescent successful workload without concurrent join/copy, inspect the actual ISR refs
and the same known target/peer labels as the reported snapshot. Per logical write byte the
steady write payload crossing count is the number of destination replica nodes in a different
known zone from the target; the target's in-process replica contributes zero. With R replicas
in R distinct known zones that is R−1 if one is in the target's known zone, otherwise R.
Strict zone placement establishes those assumptions only while labels/domains remain valid.
A requested zone/bestEffort header establishes none of them: #18 may fall back per chunk to
rack/node. Node/rack or fallback can reach0; a different actual placement can cost more.

A read needs0 with a usable same-zone source and1 otherwise, assuming all relevant zones known.
Direct source→joiner copy needs0/1; today's source→target→joiner path sums each different-zone
hop and may cost2. A source/joiner on the target node contributes no transport hop. Unknown
endpoints leave an incomplete comparison, never a zero floor. Framed/control/retry/failed
attempt bytes are reported separately; they do not redefine these payload assumptions.
Weight chunk floors by the known bytes actually written/read there, not total capacity or the
header's requested level. Mixed copy/foreground counters retain separate raw denominators.

Required fallback fixture: one requested zone/bestEffort RF2 volume has chunk0 on a(z1),b(z1)
and chunk1 on a(z1),c(z2), target a. Equal successful writes yield per-chunk payload floors0
and1, and their byte-weighted crossing floor0.5. Merely reading the requested level must not
produce R−1 for chunk0. A strict/distinct-known-zone counterpart yields1; missing/changed
labels make the relevant comparison explicitly incomplete. These are measurement fixtures,
not new placement preferences or policy values.

## Honest environment preflight and collection (S10, S11)

**Before mutation.** The later check requires separate campaign authorization and an exact
fixture manifest identifying hosts, daemon binary hashes/revisions, running process identities,
clock feature/profile, existing configuration, owned test volumes/disks and current workload/
placement/eligibility state. Use read-only inspection and existing startup/admission evidence;
do not start/restart daemons, manufacture qualification or alter eligibility during preflight.
For #83-active binaries require the already provisioned physical-boottime-v1 contract, actual
host provenance and an already admitted daemon of that exact process/build. Preserve the
operator's real qualified-hardware/rate assertions and #83 checks; a manifest entry is not an
alternative attestation. Unknown build/profile/provenance, unsupported VM, missing evidence or
conflicting workload yields exit2 with status=blocked, baseline=uncollected and the reason,
before creating volumes, changing marks, loading or draining. Failed collection/cleanup is
exit1; only complete validated collection and restoration is exit0. These script results do
not add a daemon admission API or bypass.

**Current VMs.** With #83 admission active the existing fl-a/fl-b/fl-c VMs are unqualified and
must take that blocked branch. A separate authorization explicitly identifying a genuinely
pre-#83 binary permits only an exploratory VM campaign, labelled pre83_exploratory in retained
artifacts. It cannot disable an active clock guard, use a force flag or claim supported-profile
proof. Generic future lab authorization is insufficient to turn a VM into physical hardware.

**Physical equivalent.** Provisioning/qualification is separate authorized work. Use three
exclusive disposable physical nodes admitted under physical-boottime-v1, two with distinct
known zone labels and one deliberately unlabelled for observation; labels express fixture
layout, not actual cloud billing. Use the same private16GiB≤64-chunk RF2 node and zone volumes,
local/nonlocal target phases, deterministic aligned successful writes/reads and one ordinary
private-replica drain as the VM campaign. Check actual placements/#18 constraints before each
load; skip an impossible arrangement with blocked/uncollected for that case rather than infer
it. No formatting/raw fault/permanent relabel/automatic tombstone confirmation is authorized.
Four-node deterministic unit fixtures cover two remote copy hops absent from three hosts.
If this qualified disposable topology does not exist, the supported baseline stays uncollected.

**Restoration.** Save exact pre-state, stop only owned loads, detach/delete only private test
volumes, restore only owned eligibility changes whose state has not changed concurrently, and
wait for their ordinary cleanup/healing. Retain binary/profile evidence, raw lines, workload,
actual placement/labels, quiescent deltas, incomplete cases and pending cleanup actions. Do not
claim success after cleanup failure or act on another operator's objects. No cost/read/source/
placement choice or later policy value is selected by this measurement step.

## Concrete future checks

- `flaminio-node/src/net.rs` outgoing-session tests feed real codec bytes for initial and
  established Refused; assert one Other receipt precedes the unchanged single Lost, zero
  payload, correct prefix-inclusive size, no second count and no TLS/undecodable-byte count.
- Engine tests hold queued Reply/Refused across detach/reattach with identical volume/epoch;
  assert no old addition or request_flow lookup. A superseded session of the same live marker
  counts Other. Check old client-counter object isolation, changed epoch and ordinal exhaustion.
- #90 adapter/real-codec fixtures cover wrong UUID/G/incarnation/session/profile and rejected
  Fenced evidence without authority credit; all active profile lengths, boundary/+1 encodings,
  same-node/different-ref aggregation and local-zero bytes. Observer-enabled/disabled twins
  preserve authoritative messages, ordering and storage outcomes; no new safety rule/mutant.
- `scripts/tools/zone-traffic-test.sh` supplies per-chunk fallback/distinct-zone/unknown fixtures,
  exact bytes/hops, pre83_exploratory versus active-#83 VM/unknown/refused and admitted physical
  fixtures. Refusal asserts zero setup/load/drain/cleanup mutation; exercise every later failure,
  concurrent-owner mark change and incomplete cleanup. `make lint` owns the script fixtures;
  `make test contracts sim` remain future implementation checks, not drafting results.
