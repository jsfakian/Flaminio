# Issue #59: WAN-tolerant backup transfers — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Historical spec approved 2026-11-13; not implemented. Review-round-1 revisions below and in
[policy contracts](policy-contracts.md)/[transport profile](transport-profile.md) are proposed,
unapproved changes; historical approvals remain in decisions.md.
Scoped increment: per-run transfer caps, daily windows, progress-aware retry and resume.
Long-outage snapshot retention/capacity and fleet/shared-site budgets remain later steps;
#72/#51/#52 now track dependencies absent when the existing intent was written. #59 stays open.

## Terms

- **Run:** one fixed snapshot/incarnation and destination, retaining a durable operation ID.
- **Complete object:** immutable object/version whose validated durable receipt is recorded.
- **Cap:** configured outbound bytes/second R with declared burst B and measurement boundary.
- **Window:** half-open daily local-time interval in an explicit IANA time zone, possibly
  crossing midnight. Multiple intervals form their union; no window means always open.
- **Eligible time:** phase-specific elapsed time excluding only observed local pacing or gate
  suspension; remote socket backpressure counts as stall time (transport profile).
- **Recovery-point age:** now minus the latest fully committed restorable snapshot time,
  not the configured backup/snapshot interval. Unknown clock quality makes age uncertain.

## The invariant

Pacing controls transfer admission only. It cannot weaken snapshot/key pins, immutable object
verification, Object Lock, manifest publication or application I/O durability. A paused or
interrupted transfer is never advertised as a completed recovery point.

## Behaviour

**S1. Settings.** Without #59 flags, preserve #52 behavior, including exit 1 on store failure.
Explicit `--transfer-policy wire-v1|payload-v1` enables this increment; other #59 flags require it.
A policy accepts its bounded positive cap and optional daily windows/time zone. Invalid or
ambiguous settings fail before transfer. Persist canonical settings and pinned zone-rule identity;
a rules/config mismatch refuses restart until an explicit revision. Changes affect future
admission, never immutable data. Budget is per-run, not a site-wide guarantee.

**S2. Rate envelope.** For any measured outbound interval dt, bytes ≤R×dt+B at the declared
measurement boundary. Count control requests, TLS/transport overhead and retries within that
boundary, not only object payload. Bound queued/in-flight bytes and publish B. The authoritative
wire-v1 boundary is the owned local egress after its shaper; independently capture there. Remote
arrival captures are diagnostics because downstream queues can batch traffic. Payload-v1 makes
only its named payload claim; neither profile claims physical NIC/tunnel overhead.

**S3. Windows.** When the window is closed, admit no new request or byte-send work; pause or
abort in-flight transfers under an explicit bounded-tail policy. Publish the observation
boundary and tail bound. Bytes already in a remote/network queue cannot be recalled: literal
zero packets after a wall-clock boundary is not promised without a separately qualified gate.
Clock jumps/DST change scheduling only; monotonic rate credit and safety pins never depend on
wall time. No catch-up burst from accumulating credit while the window is closed.

**S4. Automatic retry.** A policy-enabled living run waits through closed windows/transient
outages and resumes automatically while its grant and retention/commit policy remain compatible.
Use bounded jitter/backoff, one cap over retries/control requests and bounded outstanding uploads.
After every wait re-admit against #53 store-time deadlines and exact-version retention. Never
dispatch a known-stale root; preserve one root/operation and terminal late-root behavior.
Diagnose incompatible commit budgets, late roots, auth/config/corruption separately from outages;
a terminal operation requires the prescribed administrator abort/new grant, not endless retry.

**S5. Slow progress.** Do not terminate a progressing upload merely for total request age.
Stall detection measures remote progress only while work could advance, with explicit idle
bounds for connection, headers and response. Only measured local pacing/gate wait is excluded;
remote-caused nonwritability and absent server responses advance eligible idle. Cancellation
is prompt and leaves resumable pinned state; no unbounded accumulation of tasks or memory.

**S6. Resume.** Reuse verified immutable complete objects without uploading their bytes again.
Ambiguous lost acknowledgements are resolved by authenticated object/version verification.
A link cut retransmits at most uncommitted in-flight objects, with an explicit concurrency
bound: K*(4194304+80) encoded block PUT bytes per cut, K<=4. Required exact-version verification
GETs, page/root traffic and requests are graph-sized, separately bounded/accounted in both
directions for the defined resume episode (policy contracts). A receipt alone never proves
restorability. Wrong/missing objects are not silently accepted by matching names.

**S7. Status.** Before transfer, report remaining encoded bytes/requests if known, cap/window,
a lower-bound/estimated finish time with assumptions, and uncertainty. While running report
sending/window-wait/link-wait/failed, completed and pending objects/bytes, resume probes,
backlog, last completed recovery point and age. Unknown remains unknown. An outage or incoming
change rate can make RPO/finish time unbounded; configured interval is never achieved RPO.

**S8. Shared rules.** Define limiter/window/backoff rules once over injected clock/counters,
with deterministic vectors for Rust/Go consumers. #58 may adopt them independently; this step
does not implement a site-shared budget or pace shipping until that integration exists.

**S9. Durable lifetime.** Persist snapshot/layer/key pins across process death and pauses.
Publish manifest and recovery point only after complete validated durable objects and required
retention. Cancellation/outage cannot silently GC the only unshipped point. Capacity pressure
reports blocked progress; no implicit pin eviction or weaker durability.

**S10. Evidence.** Lab tooling affects only the dedicated object-store traffic path, not etcd,
replication, CA or KMS. Independently measure rate/window tail and resume traffic while
application writes continue; restored backup equals the pinned snapshot. Retain actual results
and amplification/finish-time data. No performance or lab claim exists in this draft.

## Tests

- S1–S5/S8: pure clock/vector tests for envelopes, burst/refill overflow, actual UTC DST/midnight
  vectors, pinned-zone mismatch/revision, no-flags compatibility, policy-enabled retry, closure,
  backoff and local-wait versus remote-backpressure accounting (both appendices).
- S4/S6/S9: close during cover/before root, low-rate large root, late root and lost root ACK;
  large manifests/receipt revalidation/lost PUT ACK; crash points at upload/receipt/manifest/lock/
  pin publication, corrupt/missing object, failed GC/restart; exact completed-object byte reuse.
- S7: status fixtures for unknown sizes/clock, closed windows, stale recovery point, unbounded
  backlog, insufficient window capacity and inconclusive estimates.
- S10: authorized dedicated-path cut/cap/window check with independent capture and restore.

## Open questions

No unspecified mechanism is delegated to implementation: the two appendices define the proposed
profile and policy contracts. Backend/platform measured qualification remains unperformed. The
local-egress boundary, supported wire rate floor, explicit activation and deterministic zone-rule
identity are review revisions requiring approval; they do not reopen clear dependency plans.

Historical settlements, 2026-11-13: S3's bounded-tail window contract; IP-byte accounting with
65536-byte default burst and qualified tail; separate idle controls and owned-namespace helper.
The user's separate instruction settled #52's whole 4 MiB block unit, amplification and
`ObjectStore`/`before_attempt` common choke point. See decisions.md for the preserved records.
