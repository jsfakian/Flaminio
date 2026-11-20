# Issue #59: WAN-tolerant backup transfers — design

[Spec](spec.md) · [decisions](decisions.md) · [historical ADR acceptance](adr-proposal.md) ·
[transport profile](transport-profile.md) · [policy contracts](policy-contracts.md)

Historical design approved 2026-11-13; not implemented. Review-round-1 revisions and appendices
are Proposed/unapproved. Preserve historical approvals; no dependency approval is reopened.
This increment builds #52's manual Rust backup worker plus #53's retention protocol, neither
implemented. It adds no schedules, long-outage retention, site budget or #58 shipper integration.

## The facts it starts from

No backup worker/object client exists in current workspaces. #52's reviewed contract chooses
`flaminio-backup`/aws-sdk-s3, whole4MiB blocks with80-byte maximum envelope overhead and
<=1MiB+80 page/root objects. The adapter seam admits all HTTP attempts; it is not an existing
SDK implementation. #53 fixes store-time D/E, exact COMPLIANCE-version cover, at most one
re-cover, one root/operation and a terminal late-root outcome. #51's reader close/drain barrier
keeps pins when holders are unreachable. #72's capacity ledger does not supply outage eviction.

`scripts/guest/chaos.sh:6` changes wg0's root qdisc globally; it cannot isolate object traffic.
#74's approved helper is for ublk/disk privilege, with SYS_ADMIN, not a networking API. #83's
physical-only clock admission excludes current lab VM data planes once active. #54's restore
path does not authorize overriding a live volume for this check; use #52 raw exclusive output.
Audit ADR0011's durable references/full publication and ADR0009's retained-key constraints apply;
this increment does not lift #51/#52's current refusal of encrypted snapshots.

## Pure policy and activation (S1–S5, S8)

Add `dataplane/crates/flaminio-backup/src/transfer_policy.rs`, sans I/O. Inputs are injected
monotonic_ns, UTC instant, pinned zone transitions, ready bytes, budget, progress and classified
result. Outputs are permit(n), wait, abort-at-safe-boundary, retry-at or permanent failure.
Rust and future Go consumers replay `contracts/transfer-policy/`; no per-byte cross-language RPC.
Checked u128 fixed-point bucket uses positive R, declared B, initial0 credit and monotonic refill
saturating at B. Debit before send; refund only provably unhanded bytes, never ambiguous failure.
No credit accrues while windows close; no backward-monotonic input is accepted. Window union is
half-open local intervals; repeated hours admit both actual UTC instants, absent minutes admit
none, crossing midnight splits at00:00. Wall rollback changes only scheduling, never safety pins.

Require explicit `--transfer-policy wire-v1|payload-v1` for all #59 options; no flags retains
#52's exit1 on store failure. Existing run activation/revision/restart semantics are exact in
policy contracts. Proposed flags: `--upload-rate-bytes-per-second U64`, `--upload-burst-bytes U64`
(default65536, range4096..1048576), up to32 `--upload-window HH:MM-HH:MM`, required
`--upload-time-zone IANA` with windows, `--upload-concurrency 1..4` (default1),
`--update-transfer-policy`. Equal endpoints/unknown zones/overflow refuse exit2 before setup.
Wire-v1 supports only the transport profile's qualified rate range, starting at1024B/s;
payload-v1 may use1B/s and explicitly claims no IP-wire envelope. The profile selector requires
an explicit rate for wire-v1; payload-v1 may omit the rate for window-only logical admission
without a byte cap. No rate flag means an absent optional field, not a numeric sentinel.
Canonical settings, IANA2025b table identity and deliberate update are persisted as
policy contracts define. No host tzdb changes a run's windows silently.

## Object connector, wire boundary and stalls (S2, S3, S5)

[Transport profile](transport-profile.md) is the normative proposed integration: exact compatible
SDK/Smithy pins, custom HTTP client, attempt/body admission, retry/redirect/credential ownership,
pooled-socket cancellation, owned root helper/broker, Linux qdisc/filter/buffer constants,
phase-clock progress signals and negative tests. Metadata/snapshot-reader sockets remain in the
host namespace; only object sockets use its broker. Worker has no networking capabilities.

The envelope is enforced/observed at owned local egress after the shaper, counting IP bytes
including TLS/TCP/control/retransmission. Publish R, conservative B and qualification identity.
Independent local packet-event capture proves it. Remote captures diagnose path batching and
cannot certify the same arbitrary-interval burst without a downstream queue bound. This is an
explicit unapproved correction to the former remote-arrival wording. Window closure stops
admission, closes/purges the owned packet gate and cancels owned sockets; publish bounded
observation tail and exclude already-emitted packets. Reopen cannot reuse old permits/credit.
The historical bounded-tail settlement is preserved; no literal universal zero-tail promise.

No whole-upload age timeout. Local limiter/gate suspension is observed explicitly; socket
nonwritability caused by a peer that stops reading counts as remote stall. Connect/header/body/
response clocks and transport progress follow the profile, including TCP ACK progress versus
server response and qualified low-rate local queueing. Wire1B/s is refused instead of pretending
that pausing userspace timers also pauses kernel TCP. Profile qualification remains unperformed;
the mechanism is selected now, not a future SDK/helper design gate.

## Resume, retention and publication (S4, S6, S9)

Persist #52's fixed grant/snapshot/source/destination/repository/operation identity, exact-version
receipt cursor and #53 P/D/E plus policy revision before sending. Durable source/layer/key pins
survive pauses/death; cancel follows the existing close/drain barrier and unavailable holders
block cleanup. A policy adds no TTL eviction or alternate reference authority.

[Policy contracts](policy-contracts.md) defines dispatch re-admission after every wait, qualified
store-time refresh and exact cover-proof checks, profile-specific root finishing reservation
(wire shaper versus optional payload-body rate; no-rate known pacing cost0), one re-cover and
terminal late-root handling. A cap/window may make the repository's fixed commit budget
incompatible: exit1 with pins/checkpoint kept and a concrete diagnosis. An accepted late root
requires #53's abort/new grant; no policy promises unconditional eventual completion.

At most K<=4 total object requests in flight. A cut's ambiguous block PUT payload is bounded by
K*4194384 bytes; page/root PUTs have their own1048656-byte maximum under that same K. Resume
must authenticate exact-version GETs, including completed receipts; graph-sized inbound GETs,
control/pages/roots and requests are separately bounded in policy contracts and counted in both
directions per defined episode. Repeated failures have unbounded aggregate traffic/time.
Transient errors use full-jitter exponential backoff1..300s, bounded Retry-After and one timer;
closed windows do not advance failure count. Reopen resets bucket and probes at most one object.
Auth/signature/corruption/conflicting immutable data and incompatible policies fail distinctly.
Never list an incomplete point or weaken #52/#53's durable commit/reader-close protocol.

## Status and contracts (S7, S8)

Extend #52 schema_version1 output with policy profile/revision/hash/zone-table identity, state,
remaining encoded bytes/requests with known flags, queued bytes, verified complete/inflight
objects, current resume episode/aggregate outbound/inbound IP/payload/control/GET/PUT counters,
latest committed point/age/uncertainty, estimate and last_error_class. States are sending,
window_wait, link_wait, capacity_wait, failed, complete. No secrets/signed URLs in output.
ETA is a range/lower bound from known encoded bytes/cap/windows; unknown latency/retries/change
rate are stated. Unknown remains unknown. Backlog and achieved recovery age can grow unbounded.
Two independent runs may each consume R; status never calls it a site-shared cap.

Cross-unit interfaces, exact SDK/Linux helper protocol and bound formulas are in the transport
profile; policy activation/canonical identity, #53 admission and accounting/lab grammar are in
policy contracts. No new metadata RPC, repository format or data-I/O lock/round trip is introduced
merely for pacing. #58/future operator resources may later adopt vectors, not configured here.

## Files touched

- `dataplane/crates/flaminio-backup/src/{transfer_policy,transport,run,status}.rs`: S1–S9.
- `contracts/transfer-policy/{schema,vectors,tzdb/2025b}.json` and generator: S1/S3/S8.
- `dataplane/crates/flaminio-backup/tests/{policy,retention_policy,resume_budget,transport}.rs`.
- New `flaminio-backup-net` helper/broker and Linux transport support: exact profile paths.
- `scripts/checks/wan-backup.sh`, `scripts/guest/wan-backup-net.sh`,
  `scripts/tools/wan-backup-test.sh`, lab.sh/Make help: S2–S10 fixture/owned lifecycle.
- Docs/lab/performance/followups/README: actual commands/evidence/limitations on implementation.
No second independent backup worker; paths extend #52's proposed crate and existing hooks.

## Tests and lab

Appendices name runnable planned vectors/fakes/integration fixtures for every S1–S10 rule:
legacy/no-policy compatibility, pinned UTC rules/revisions, wire packet envelope/path batching,
window/idle/cancel/ownership failures, #53 close/cover/late-root cases, large graph/ACK-loss resume
accounting, bounded status and exact pinned-snapshot restore. Retain #52 simulator/crash pin/close
coverage; pacing adds no new replication authority. Future checks: make lint test race hygiene
contracts; relevant existing sim/crash checks remain. Drafting ran no runtime tests or lab.

Policy contracts specifies exact proposed `make wan-backup-test` actions, role fixture, worker w,
#53 store8334, netns/broker/capture,64MiB RF2 snapshot under bounded fio load,1MiB/s/minute-scale
windows, cut/reopen/full verification and retention-aware cleanup. #52 port8333 is only setup
prerequisite, never the WAN endpoint. Current VMs are Blocked once #83 is active. An authorized
legacy pre-enforcement run is exploratory; full qualification requires authorized disposable
physical nodes. Pure/fake tests remain available without a data plane or invented clock profile.

## Docs, risks, ADRs and parallel work

Update docs only to real implemented commands/results. [Issue ADR](adr-proposal.md) retains
historical acceptance and labels these boundary/activation/profile revisions Proposed. Accepted
ADR0001/0004 remain consistent; audit0009/0011 apply. Local root/kernel/helper are trusted for
wire shaping, store/backend qualification remains separate, and a slow budget can make an
operation terminal. Long-outage local capacity/retention, fleets, schedules and encrypted backup
remain outside this increment. No unselected mechanism is hidden as a qualification prerequisite.

Pure policy/vectors, helper and fake lab harness can be built independently against appendices.
Integration consumes #52/#53's exact reviewed contract; whoever lands second updates those
consumer seams without reopening their clear plans or claiming them implemented.
