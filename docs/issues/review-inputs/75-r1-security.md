# Issue #75 plan review — round 1, security focus

Scope: `docs/issues/75/` (intent, spec, design, adr-proposal) in `/tmp/flaminio-plans-2026-11-12/75`,
checked against the code of that worktree and the draft plans of #72 and #73 where this plan relies
on them. Cited facts in intent/design (`engine.rs:167-179`, `net.rs:91-92,98,139,212`,
`flaminio-io/src/node.rs:288`, `engine.rs:2127-2270`) match the code.

### R1-S1 — High

**Two lanes break the ordered-session argument the protocol's safety rests on, and the
lane-to-session binding is contradictory; no simulator check is planned.**

Evidence:
- design.md:80-89 splits traffic per node pair: control lane = heartbeat/ack/stale "and request
  headers", data lane = payloads/copy plus an "urgent subqueue". DRR (design.md:66-72) further
  reorders classes on one session.
- The protocol defines a session as "one ordered connection" (`docs/replication-protocol.md:44`)
  and proves resync on it: "a copy reads its source over the same ordered session that carried the
  write, so the read sees it" (`:252-257`). `Read`, `BeginResync`, `Settle`, `Discard`, `Digest`,
  `Mapped` carry no payload (`flaminio-net/src/frame.rs:54-83`); if they are "request headers" on
  the control lane they can overtake a `Write`/`Copy` on the data lane (copy misses a write,
  discard lands after the write it preceded, settle before its copies).
- The simulator models a session as one ordered stream (`flaminio-repl/src/sim/mod.rs:235`), so
  the planned "simulator schedules with delayed/dropped traffic" cannot see cross-lane reordering.
  The only two-lane test is a real-size fixture (design.md:179).
- Frames carry no volume/session id (`frame.rs:23-110`); today "one connection, one session"
  (`flaminio-net/src/session.rs:1`), and a heartbeat reply confirms that session's epoch for the
  lease (ADR 0005 :20-22, protocol :100). "Lanes per node pair" with "128 sessions/peer"
  (design.md:50,80) leaves undefined which (volume, epoch, session) a control-lane heartbeat
  confirms. A heartbeat shared by several sessions that still confirms a volume fenced at a higher
  epoch is a stale-read lease.
- The design's one sentence ("do not reorder overlapping logical-block work around the core's
  sequencing barriers", :70-71) names no mechanism and covers DRR only, not lanes.

Fix: state the frame-to-lane table and the binding unit (one lane pair per session, or an explicit
session id in every frame). Either keep every frame the core orders on one lane, or add an explicit
per-session sequence the receiver enforces across lanes. State that a heartbeat confirms exactly one
(volume, epoch, session) and is refused once that session is fenced. Add this as a spec rule with a
simulator model of two lanes/reordered classes, and injected bugs the simulator must catch (header
overtakes payload; heartbeat confirms a fenced session), per AGENTS.md Principles.

### R1-S2 — High

**Tenant-controlled PVC overrides are checked against ceilings that no plan defines; unlimited
rates and weight are claimable.**

Evidence:
- spec.md:25-29 (S1): "Overrides cannot exceed #73 namespace/class ceilings". design.md:22-27:
  PVC annotations `flaminio.io/qos*` including `qosWeight`; "metadata checks authority/class
  ceilings"; design.md:164 "#73 ceiling failures PermissionDenied".
- #73's draft has no QoS ceiling: `TenantPolicy` tags 1-7 are allow_unencrypted, allow_rf1,
  required_protection, max_logical_bytes, max_volumes, max_snapshots, max_backups
  (`/tmp/flaminio-plans-2026-11-12/73/docs/issues/73/design.md:147-148`); it never mentions QoS or
  #75. Metadata does not know StorageClasses, and `CreateVolume` is admin-only today
  (`meta/service/auth.go:66`), so it cannot tell a class value from a tenant override.
- Zero means unlimited (design.md:18): a numeric "override <= ceiling" check passes `0`. Weight
  (1..1024, design.md:161) is a share, not a rate; no ceiling is named, so any PVC can claim 1024x
  the foreground share of its neighbours. Burst has no upper bound either.
- No behaviour is stated when #73 is absent (#73 is not in the issue's "Depends on").

Fix: define the ceiling fields (max IOPS, bytes/s, both bursts, weight), where they are stored and
who writes them, with tags reconciled with #73. State the comparison: unlimited exceeds every finite
ceiling; an absent ceiling means no override at all. Without #73's trusted broker, PVC annotations
are refused, not ignored. Name tests: override above ceiling, `0` against a finite ceiling, weight
above ceiling, direct `CreateVolume`/`SetVolumeQoS` by each role, #73 absent.

### R1-S3 — High

**Shared node pools have no per-volume or per-peer share: one volume (unlimited by default) can
fill them, and other volumes then fail their sessions and lose ISR members.**

Evidence:
- design.md:45-63: node-wide pools by class (foreground 256 MiB, bulk 128, ...); the only
  per-party caps are counts (1024 descriptors/volume, 64 peers, 128 sessions/peer), and
  1024 x 1 MiB exceeds the foreground pool. DRR weights (:66-72) order work already admitted; they
  do not share admission. Default rates are unlimited (:20).
- When credits run out, ingress pauses (:58). The target's core arms `request_timeout` (3 s
  default, `flaminio-repl/src/types.rs:35-36,60`; `target.rs:778,811`) when it issues the request,
  before any queue. A victim volume's write that waits behind another volume's backlog fails its
  session, and the target then removes a healthy replica from the ISR. This is the livelock the
  protocol already records for unbounded bulk (`docs/replication-protocol.md:269-273`).
- spec.md:18 says scheduling never authorizes an ISR change; nothing says it cannot cause one.
  With RF=2 and ADR 0006 this lets one tenant push another's volume to a single copy.
- No named test has a second volume or peer during pool exhaustion (design.md:179).

Fix: give each volume and each peer a reserved minimum and a cap in every pool, at source and
destination. State where `request_timeout` starts relative to the new queues, and bound queue delay
below it or fail pre-dispatch without a session failure. Add a rule and a deterministic/simulator
test: a saturating volume or peer never fails another volume's session nor changes its ISR.

### R1-S4 — Medium

**`LaneOpen` is under-specified as an authenticated input, collides with #72's protocol v2, and has
no negative tests.**

Evidence:
- design.md:80-81,166-168: both lanes use ALPN `flaminio-repl/2` and `LaneOpen` tag 73. No ALPN
  exists in the code today. #72's draft defines `flaminio-repl/2` as Hello-with-`device_uuid`
  only (`/tmp/flaminio-plans-2026-11-12/72/docs/issues/72/capacity-contracts.md:126-133`). A
  #72-only peer refuses tag 73 (`frame.rs` `UnknownType`); a #75 node cannot tell a single-lane v2
  peer from a peer that skips `LaneOpen` to keep payloads and control on one unbounded connection.
- Not stated: which lane carries `Hello` and its capability token (today the first frame must be
  `Hello`, `session.rs:6-11,124-135`; the design puts large `Hello` on the data lane); what a lane
  accepts before the group's token verifies; who chooses `instance`/`group` and what the acceptor
  checks them against; a duplicate `LaneOpen` for a bound (group, lane); a group with one lane
  only; whether lanes or sessions count against 128/peer.
- Tests (design.md:178-182) name "two lanes" only.

Fix: give the lanes their own protocol id (or a negotiated capability) and say whether a
single-lane peer is refused or served in a bounded legacy mode, with the rollout order. Specify the
handshake as a state machine (order of `LaneOpen`/`Hello`, token check per lane group, fresh random
group, duplicate and half-open handling, counting). Add tests and fuzz regression inputs: wrong
principal, wrong group/instance, lane value >1, wrong length, duplicate, missing `LaneOpen`,
payload frame on the control lane, lane opened before/without a valid token.

### R1-S5 — Medium

**The destination cannot classify or weight a peer's work without trusting the peer, and "police
per-peer excess control work" has no limit, action or test.**

Evidence:
- design.md:69-70 charges classes "at source and destination" with volume weights, but frames have
  no class or weight (`frame.rs:23-110`): a `Read` may be foreground, scrub or backup. The design
  adds no field and changes no token (design.md:165), so the replica either guesses from the frame
  type or would trust a value the peer states.
- Safety work is served ahead of DRR from a reserved budget (design.md:77-78). `Hello` is safety
  work and costs a durable fence write per accepted session; heartbeats are free to send. The
  policing rule gives no rate, no bound and no consequence (close the session? that removes an ISR
  member), and no test names it.

Fix: state how the destination derives class and weight (frame type only, or a field bounded by
metadata-signed data), and that a peer-stated value can only lower its own priority. Give the
per-peer control limits and the action on excess. Test a peer that floods `Hello`/heartbeats,
mislabels bulk as foreground, and stops reading replies.

### R1-S6 — Medium

**Completing a throttled frontend request with `-EAGAIN` is an I/O error to the tenant's
filesystem.**

Evidence: design.md:59-60 ("may wait in bounded ublk tags or return -EAGAIN"); spec.md:45 allows
"reject pre-dispatch". The frontend completes failures with `-EIO` only today
(`flaminio-io/src/ublk.rs:341,400,1249`); a negative ublk result ends the kernel request with that
status, which a filesystem treats as a failed write, not as a retry (kernel behaviour, not
re-verified here). Combined with R1-S3, a neighbour's load turns into write errors on a victim.
Nothing says which of the two outcomes applies when.

Fix: QoS and pool pressure only delay frontend requests held in ublk tags; preflight that
depth x queues fits the descriptor/byte reservation so no admitted tag needs rejecting. List the
exact cases that may fail a request, align the errno with #84, and test that throttling alone
never completes a request with an error.

### R1-S7 — Medium

**The destructive steps of the performance gate have no stated guard, and the "same exclusive host
lock" has no location or recovery rule.**

Evidence:
- design.md:189 lets campaigns "format named disposable benchmark devices"; the profile "binds
  hardware IDs" (:98-101), but a mismatch is only an "inconclusive" result after the run (:129).
  No preflight refuses to format a device whose identity is not in the authorized inventory, and
  no test covers it. `PERF_PROFILE=name` and profile fields reach shell commands with no
  validation rule (AGENTS.md: ids reaching shell commands are validated and quoted).
- design.md:136-139: one lock shared by perf, `lab.yml` and manual `lab.sh`. Today the only
  exclusion is GitHub `concurrency: group: lab` (`.github/workflows/lab.yml:28-30`);
  `scripts/lab.sh` takes no lock, and a manual run can start from a machine other than the runner
  host (`docs/ci.md:92-96`). A lock local to one machine does not exclude the other. "No timeout lock
  stealing" leaves a crashed holder's lock with no release procedure.

Fix: preflight before any destructive step: device serial/WWN, host and role match the authorized
inventory, the device is unmounted and not a claimed Flaminio disk; refuse otherwise, with a
script test. Validate the profile name and every profile field that reaches a shell. Put the lock
on the shared resource (the lab host), record owner/run id, and define how a dead holder's lock is
verified and cleared by an administrator.

### R1-S8 — Low

**How a live target learns a new policy revision is not stated.**

Evidence: spec.md:39-40 (S3) revokes credits on policy change; design.md:26 makes `SetVolumeQoS` a
CAS in metadata. The target reads its volume only at attach and on the periodic refresh
(`flaminio-repl/src/types.rs:41-43`, 2 s default); the design names neither. A lowered limit has no
stated bound to take effect, and a resent metadata read can deliver an older view after a newer one.

Fix: name the path (the existing refresh), the staleness bound, that a revision lower than the one
applied is ignored, and that a failed refresh keeps the current policy. Add both to the S3 vectors.

### R1-S9 — Low

**The release gate trusts `result.json` without stating its provenance.**

Evidence: design.md:140-141 "Release workflow consumes a passing report for the exact
source/profile/baseline". No release workflow exists (`.github/workflows/`: ci, fuzz, lab, nightly),
and nothing says the report must come from `perf.yml` on the protected branch at that commit, or
how its digest is checked. Any workflow run can upload an artifact of that name.

Fix: bind the report to the producing workflow, ref, commit and run conclusion (or a #74
attestation), verify the artifact checksums the design already records, and test that a report
from another workflow, commit or profile is refused. Say which issue adds the release workflow.

## Nits

- design.md:166 "network-order fixed bytes": `LaneOpen` has no multi-byte integer, and the codec is
  little-endian (`flaminio-net/src/frame.rs:157-163,343`). Drop the phrase so no one encodes a
  later field big-endian.

Reviewed commit: 6597c44579ef672a037b04f500bb7831ccc918d9
