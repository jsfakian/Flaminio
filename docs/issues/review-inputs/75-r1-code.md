# Issue #75 — plan review, round 1, code focus (design-code + plan code checklist)

Scope: `docs/issues/75/{intent,spec,design,adr-proposal}.md`, checked against the code of the
worktree `/tmp/flaminio-plans-2026-11-12/75` and the dependency drafts it relies on.
Facts that hold: `EngineConfig` (`flaminio-node/src/engine.rs:167-178`), the three unbounded
channels (`net.rs:98,139,212`), the unbounded node input (`flaminio-io/src/node.rs:288`), four
bulk slots (`flaminio-repl/src/target.rs:432`), the gather controls, `lab.yml` manual and
exclusive, #81's 64-change bound, #72's tag 72 and #13's tag 71.

### R1-C1 — Critical
**The two lanes break the single ordered session that the resync and fencing rules rest on.**
`design.md:80-87` puts "heartbeat/ack/stale and request headers" on a control connection and
"payload data/copy" on a data connection. The protocol defines a session as "one ordered
connection" (`docs/replication-protocol.md:44`) and argues safety from that order:
- `BeginResync` (no payload, control lane) starts the resync filter; a live `Write` sent after
  it (data lane) that arrives first is applied outside the filter, and a later `Copy` of an
  older value overwrites it (`replication-protocol.md:235-241`). The replica then joins the ISR
  with a stale block: an acknowledged write is lost on one member.
- "No copy can miss them: a copy reads its source over the same ordered session that carried
  the write" (`:252-253`): the copy's `Read` (control) can overtake the in-flight `Write` (data).
- `Hello` goes on the data lane ("large Hello … bodies"), heartbeats on control: a request that
  arrives before its session's `Hello` is dropped (`:59-63`).
The design states no cross-lane ordering rule, does not amend the protocol document, and the
simulator still models a session as one ordered stream (`flaminio-repl/src/sim/mod.rs:235-237`).
**Fix:** either keep one ordered connection per session and bound head-of-line delay inside it
(urgent frames ahead of queued bulk frames, sender-side credits so the receiver never stops
reading), or define exactly which frames may leave the ordered stream (heartbeat and its reply
only), prove each ordered-session argument of §3, §6 and §7 again, change the simulator's
`Conn` to two independently delayed streams, and add an injected bug (for example
"BeginResync on the control lane") that it must catch. Add the spec rule that scheduling never
reorders a session's requests.

### R1-C2 — High
**The DRR scheduler and the "owned return" backpressure sit after the state machine has
emitted a request, so scheduling delay becomes session failure and ISR removal, and emitted
requests of different classes are reordered.** `design.md:66` ("in each owning device/network
dispatch path") and `:56-58` (a failed `try_send` is retained and retried by the producer).
- `Target::tick` fails a peer when any in-flight request is older than `request_timeout`
  (3 s, `flaminio-repl/src/target.rs:811-828`, `types.rs:60`), counted from emission. A copy or
  write held by DRR, a paused lane or a full channel times out, the session closes, and an ISR
  member is removed. This is the livelock `replication-protocol.md:269-273` records from the
  lab. It contradicts the spec's invariant (scheduling never causes an ISR change).
- DRR across foreground and rebuild reorders a `Write` against a copy's `Read` on one session:
  the same hazard as R1-C1. "Sequencing barriers" (`design.md:71`) names no mechanism.
- The target already has a class-aware work queue and slot grant (`target.rs:262-283`, `Class`
  `Client`/`Reconcile`/`Resync`; order rule of #11, `replication-protocol.md:194-199`). The
  design does not mention it, and reconcile work (digests, repairs a client waits on) has no
  place among the four classes at 8/4/1/1.
**Fix:** schedule at admission inside `flaminio-repl` (which work the target starts, extending
the existing queue and bulk slots), so that what is emitted is sent in order and at once; give
a bound on post-emission queueing well under `request_timeout`; map reconcile, repair, digest,
resync copy and discard to classes and state that #11's order holds; add a simulator check
that a saturated scheduler causes no `SessionFailed` and no ISR change.

### R1-C3 — High
**The lane contract does not fit the transport and is incomplete.** `design.md:80-81,155-168`.
- Lanes are "per node pair" with "128 sessions/peer", but the transport is "one connection,
  one session": the first frame must be a `Hello` with a capability token, a later `Hello` is
  refused, and requests carry only a request id, no session or volume
  (`flaminio-net/src/session.rs:1-11`, `frame.rs:1-4`). Sharing two connections among sessions
  needs a session id in every frame, a new rule for "a valid node certificate alone grants
  nothing", and a meaning for "close that session"; the contract adds only `LaneOpen`. If lanes
  are per session instead, the design must say so, and say which lane carries `Hello`.
- A shared data lane lets one volume's bulk frames delay another volume's writes, which the
  per-volume rules are meant to prevent.
- `flaminio-net` has no ALPN today (no match for "alpn" under `dataplane/crates`). #72
  (`capacity-contracts.md:126`) and #61 (`design.md:140`) each propose `flaminio-repl/2` with
  different content; #75 adds lanes under the same name. A `/2` peer without lanes refuses tag
  73, so mixed versions cannot replicate; no single-lane fallback or order of landing is given.
**Fix:** state the lane's unit (session or node pair) and write the full frame and handshake
contract for it, including token checks and session teardown; give lanes their own version or
a negotiated capability with a single-lane fallback, and reconcile the `/2` content with #72
and #61 in one place.

### R1-C4 — High
**The simulator checks are nominal: the new logic that can lose an accepted write or
acknowledgement is outside what the simulator runs.** Budgets, owned returns, completion
credits and lanes go to `flaminio-node/src/{engine,net,replica/mod}.rs` and
`flaminio-io/src/{node,ublk}.rs` (`design.md:151-152`). Injected bugs are fields of
`target::Bugs` and `replica::Bugs` in `flaminio-repl` (`sim/mod.rs:192-198`), and the
simulated network has no bounded queue, refusal or lane. The design names no model change, no
check and no bug field (`design.md:150,180`: "sim modules", "catches omitted credit and
early-ack-on-overload bugs"). AGENTS.md requires a simulator check and an injected bug per new
safety rule, with the logic in `flaminio-repl`.
**Fix:** say which decisions move into the sans-I/O core (refused send, retained work, reserved
completion), how the simulated network bounds and refuses, the exact checks in `sim/check.rs`,
and the `Bugs` fields (for example `ack_on_refused_send`, `drop_on_full_queue`) with the seeds
that must catch them.

### R1-C5 — High
**S1/S3's policy change has no path to a running target, and the contract lacks the fields it
binds to.** `SetVolumeQoS` is a CAS in the metadata service (`design.md:26,162`); the target
reads the volume only at attach and on its own refreshes
(`flaminio-node/src/engine.rs:668,1550,1782`), and the steady-state path may not call the
metadata service. Nothing says how a new revision reaches the target, when the old credit is
revoked, or what bounds the delay. `QoSPolicy` has no revision, `Volume` gets only `qos` at tag
40, and `Volume` has no `first_epoch` on the wire (`api/proto/flaminio/meta/v1/meta.proto:117-127`;
#70 proposes it at tag 12), though permits and `SetVolumeQoS` bind both.
**Fix:** choose and specify the delivery (for example a control-socket line from the node
plugin or agent, validated and quoted, carrying revision and policy; or a bounded periodic
refresh off the I/O path), its ordering against attach and failover, and the test that a
lowered limit takes effect within the stated bound. Add `revision` (and the source of
`first_epoch`) to the `Volume` contract.

### R1-C6 — Medium
**Throttled or over-budget client I/O "may wait in bounded ublk tags or return -EAGAIN"
(`design.md:59-60`), with no rule for which.** A negative result from the ublk server ends the
request with that error; for an ordinary bio (not `REQ_NOWAIT`) `-EAGAIN` is an I/O error, so a
filesystem on the volume sees failed writes and can abort its journal under plain overload.
Today the frontend returns only `EIO`, `EINVAL` or `EOPNOTSUPP` (`flaminio-io/src/ublk.rs:1248-1281`).
The wake-up of a waiting request is also unspecified (the engine has only the gather timer,
`engine.rs:912`), and no minimum rate bounds the wait: 1 unit/s is valid and holds a 1 MiB
request for 256 s.
**Fix:** state that QoS and budget waits always hold the tag (the tag count is the bound) and
never complete with an error; define the timer that releases them; set a minimum rate or a
maximum wait; confirm the errno behaviour in #84's kernel check and test it.

### R1-C7 — Medium
**The #73 ceilings that S1 depends on do not exist in #73's contract.** `spec.md:27`,
`design.md:23-25,164`. #73's `TenantPolicy` has tags 1-7 (`allow_unencrypted`, `allow_rf1`,
`required_protection`, `max_logical_bytes`, `max_volumes`, `max_snapshots`, `max_backups`;
`73/docs/issues/73/design.md:147-148`) and its plan never mentions QoS. #75 adds no ceiling
field either. The comparison is also undefined where zero means unlimited: an override of 0
under a finite ceiling must be refused, and an unlimited ceiling must admit any value.
**Fix:** define the ceiling fields and their tags (in #75's contract or agreed in #73), the
comparison including zero, and what happens to an existing volume when a ceiling is lowered.

### R1-C8 — Medium
**Per-queue permits are not the simplest change, and the fast-read fallback can charge
twice.** The design says fast reads need permits because they bypass `Input::Io`
(`design.md:10-11,36-41`). Both fast paths call `Engine::try_fast_read` under the engine mutex
and through `Target::try_fast_read` (`flaminio-io/src/ublk.rs:1598-1600,1636-1638`;
`flaminio-node/src/engine.rs:2127-2157`), so the bucket's owner can debit there, with no
permit, refund, queue id or stale-permit rule. A fast read that `finish_fast_read` refuses is
submitted again through `Input::Io` (`engine.rs:2222-2226`); nothing says that second
submission is free, and no test covers it.
**Fix:** charge once per client request at one point reached by both paths (under the lock
already taken), drop the permit machinery unless a measured lock-free path needs it, and add
the fallback case to the S2 vectors.

### R1-C9 — Medium
**The design adds work to every request and gives no bound for it.** Each request takes a
bucket debit, a node-wide `CreditPool` reservation shared by all queue threads and volumes,
and a DRR pass (`design.md:29,45-47,66`). The synchronization of the pool is not stated, there
is no rule that an unlimited policy skips the bucket, and no gate measures the default path
against the pre-change binary. #9 is still short of its QD1 goal, and #15 bounds its own cost
(`flaminio-15/docs/issues/15/spec.md:150-152`).
**Fix:** state the cost on the default path (no limit configured, no background work): which
steps are skipped, what the pool uses, and a pass bound (for example 4K QD1 reads and writes
at most N% lower) in the measurement section.

### R1-C10 — Medium
**Admission caps can refuse the `Hello` that fences.** "64 peers, 128 sessions/peer" and
"over-budget new peers/refresh/attaches are refused" (`design.md:50-51,63`). A new target's
`Hello` at a higher epoch must always be able to fence a replica
(`replication-protocol.md:47-50`); if sessions of a deposed or slow target fill the cap,
failover waits on them.
**Fix:** rule that a `Hello` for a volume replaces that volume's older session within the
reserved safety budget and is never refused for count or byte limits; test it at the cap.

### R1-C11 — Medium
**S11 and S1's "rejected rather than silently disabled" have no approach.** No section builds
S11 (the headings stop at S10). Nothing names the switch that keeps a partial stage disabled,
its default, or how the metadata service knows a node can enforce a policy. A data plane
without QoS ignores the new `qos` field and serves the volume unlimited. #15 solved the same
problem with a reported capability (`flaminio-15/docs/issues/15/spec.md:109`).
**Fix:** add an S11 section: a data-plane capability checked at attach (refuse a limited
volume on a node that cannot enforce it), the feature switch per stage, and their tests.

### R1-C12 — Medium
**The tests are topics, not tests, and the lab plan cannot be run.** `design.md:176-182` lists
subjects per rule group with no file, case or level, so none can be judged able to fail
without the change; S11 has no row of its own. `meta/testdata/qos-v1.json` has no generator:
`make contracts` regenerates `meta/testdata` from the Rust model and diffs it
(`Makefile:105-106,124-126`). `make perf-check` is not wired into `ci.yml` or `make lint`
(script tests live in `scripts/tools`). `design.md:189-191` gives no make target, setting,
pass criterion, or list of what is formatted and how it is restored.
**Fix:** one line per rule with file, test name and level; generate the QoS vectors in
`meta-vectors`; say where `perf-check` runs in CI and update `docs/ci.md`; write the lab
section with targets, variables, pass criteria and restore steps, marked as waiting on Q3.

### R1-C13 — Medium
**The release gate and the lab lock rest on things that do not exist and are not specified.**
"Release workflow consumes a passing report" (`design.md:140-141`): the repository has
`ci.yml`, `fuzz.yml`, `lab.yml` and `nightly.yml` only; `release.yml` is #74's proposal and is
absent from the file list. "The same exclusive host lock in perf, lab.yml and manual lab.sh
entrypoints" (`:137-138`): `lab.yml` has only a GitHub concurrency group and `scripts/lab.sh`
takes no lock. The host, the path, the holder record, stale-holder handling and how a
workstation run takes it are not given.
**Fix:** name #74's `release.yml` as a dependency and define the report's name, the digests
checked and the failure mode when it is missing; specify the lock (where, how taken and
released, which `lab.sh` commands take it) with its script tests.

### R1-C14 — Medium
**No first step can be built.** The issue depends on #15, #45, #46, #50 and #52; the design
adds #72 (protocol v2), #73 (ceilings, broker), #74 (release, confinement), #76 (create
identity), #80, #81, #83 and #84. None has landed; #45, #46 and #52 have no design. The plan
defines no stages, though `spec.md:86-89` speaks of them and `docs/issues/README.md` asks the
intent to name the step.
**Fix:** name the stages and their order in the intent and design, with a first stage that
stands on today's code (for example the bucket and bounded queues for one volume, plus the
synthetic gate harness), and what each later stage needs.

### R1-C15 — Low
**FLUSH and DISCARD have no cost rule.** The spec charges "read plus write bytes"
(`spec.md:7-9`). The node frontend answers both with 0 today (`flaminio-io/src/ublk.rs:1279,1454`).
**Fix:** state their cost (zero, or a rule for
when #84 gives discard an effect) and add them to the vectors.

### R1-C16 — Low
**The create identity and `SetVolumeQoS` conflict.** The policy is persisted in #76's create
identity (`design.md:25`), which a retry must match exactly
(`76/docs/issues/76/spec.md:29-31`); after `SetVolumeQoS` a retry of the original create no
longer matches the current policy. **Fix:** say that the identity keeps the policy as created
and the current policy is stored apart, and test a retry after a change.

### R1-C17 — Low
**The gate's decision function is undefined.** `design.md:127-132` gives thresholds (5%, 10%)
and "spread > 10% is inconclusive" but not the statistic compared across repetitions or how
spread is computed. A 5% threshold inside an accepted 10% spread makes "the unchanged baseline
passes three campaigns" and the synthetic tests of `make perf-check` undecidable.
**Fix:** define the aggregate (for example the median of the repetitions), the spread formula,
and the relation required between threshold and measured noise.

### R1-C18 — Low
**The foreground latency bound is missing or misnamed.** `design.md:113-114` bounds
"background p99", while S7 and the issue's Done-when need the foreground p99 under background
work. **Fix:** name the metric (foreground p99 with each producer running) and its reference.

## Nits
- `design.md:9`: `Net::send` is at `flaminio-node/src/net.rs:91-93`, not `:87-92`.
- `design.md:166-167`: frames are `[u32 body length][u8 type][body]`, little-endian
  (`flaminio-net/src/frame.rs:1`); "network-order" and "body 34 bytes including tag" do not
  match that convention (the body is 33 bytes; the type is outside it).
- `design.md:22-23`: annotation keys such as `flaminio.io/qosIops4k` need the exact list
  written out, since the CSI controller refuses unknown parameters (`control/csi/controller.go:157-162`).

Reviewed commit: 6597c44579ef672a037b04f500bb7831ccc918d9
