# Issue #47: Measure write latency and prefer responsive read sources — design

Approved 2026-11-13; not implemented. Changed after approval, in rounds 1 and 2 of the plan
review (2026-11-17; see [decisions](decisions.md)).
[Spec](spec.md), [intent](intent.md).
Only measurements and read preference are built; **#47 stays open** for ISR eviction,
readmission, disk/class health, auto-drain and CSI VolumeCondition.

The facts it starts from:
- Session inflight records send time; on_reply removes a matching request and refreshes its
  lease from that send (`flaminio-repl/src/target.rs:359,899–909`). WriteOp tracks sent/acked
  members and is removed on quorum completion (`:311,1421`). There are no histograms.
- `source` filters ISR/session/removing and prefers local then lowest id (`:1644`). Client,
  repair and copy reads use it, and so do the resync's digest and its `Mapped` probe
  (`:1852`, `:2345`). Fast admission also uses it; fast completion presently checks
  `source()==local` (`:670,730–732`), which must be separated from preference to satisfy S10.
- Timer/session failure remains 3 s; reconnection and resync do not know latency. The target
  reports one event, a failed session (`Event`, `:132–135`), which the engine logs
  (`flaminio-node/src/engine.rs:1769–1773`).
- A write leaves the target's books in more ways than its quorum. A committed ISR shrink
  completes the writes that waited for the removed member (`apply_view`,
  `:2520–2545,2602–2605`). A move's drop forgets the dropped replica's sends and
  acknowledgements (`adopt_replicas`, `:2626–2635`). A deposition clears every write
  (`depose`, `:1144`). A joining replica keeps a write that its ISR has acknowledged
  (`write_quorum`, `:1605–1621`). `retransmit` sends a write again on a new session
  (`:1332–1344`). A node that holds none of the volume's chunks loses its peer (`apply_view`,
  `:2592–2601`).
- `Target::next_wakeup` says when `tick` has work (`:769–798`). The simulator and
  `Engine::next_wakeup` wake a target by it (`sim/mod.rs:851–853`,
  `flaminio-node/src/engine.rs:621–633`). An attached node also turns every 10 ms
  (`flaminio-io/src/ublk.rs:1178–1186`).
- Bulk work shares a member's session with its write acknowledgements (`:393–397`), so the
  member that serves repairs and copies acknowledges later for it.
- The simulator delays a node's messages to itself by 5 to 50 us before it looks at
  `slow_links` (`sim/mod.rs:659–664`), and its slow-link fault picks two different ends
  (`:1168–1180`). Its mutation checks count a run as caught on `not linearizable` or
  `replicas diverge` only (`sim/tests.rs:237`), and two of them run one hinted seed
  (`:459–482`).
- The target's local replica is served in process (`flaminio-node/src/engine.rs:478`).
- #71 and #83 are review-clear. #71 lets observations leave the daemon only through its
  telemetry socket (`origin/main:docs/issues/71/spec.md`, S3; its `contracts.md`, "Telemetry
  socket"). #83 puts every timing decision on one elapsed clock (its `spec.md`, S2).

## Fixed-memory paired measurements (S1–S6, S14)

Add `flaminio-repl/src/latency.rs`: `LatencyConfig`, `NodeWindow`, fixed histograms and
`Judgement`. Use existing target Time, checked subtraction and integer cross multiplication;
negative/invalid time is not a zero-latency success sample.

**The clock.** Windows and samples run on the `Time` the target is driven with: today the
engine's `Instant` (`flaminio-node/src/engine.rs:604–606`), and #83's elapsed clock once #83
lands. #83's quarantine on a clock fault ends the target (its `Target::invalidate_clock`,
through the depose cleanup) and the judgements with it; the restart's new target starts clean
(S5).

**On and off.** `Target::new` keeps its signature and starts with the judgement off.
`Target::set_latency(LatencyConfig)` turns it on, as `set_max_bulk` sets the bulk bound
(`:546`). The engine calls it with a new `latency` field of `EngineConfig`, which the daemon
fills with the defaults (`flaminio-io/src/node.rs:326–332`). Off, nothing below runs: no
sidecar is filled, and `next_wakeup` and `source` are today's. The existing tests and the
simulator's existing campaigns stay off.

Proposed defaults: window 5,000,000 us; absolute floor 5,000 us; k = 4; upper 50%; lower 10%;
minimum 32 samples; lapse 3 unjudged windows; hold cap 64 windows (5 min 20 s). Validate
tests' configurations (positive window, k ≥ 1, 0 ≤ lower < upper ≤ 100, nonzero
minimum/lapse, a cap that is a power of two). Floor/k are provisional policy constants,
not measured healthy-lab results; healthy RF2 benchmark evidence must settle Q4 before rollout.
Settled at approval, 2026-11-13: these defaults (Q4); the floor and `k` stay provisional until
healthy RF 2 evidence, before rollout (see decisions.md, Settled at approval). The hold's cap
is new in round 1 of the plan review and is Proposed.

**Samples (S1).** At a write's start on a chunk with two or more ISR members (`start_write`,
`:1378–1419`), the existing WriteOp's measurement sidecar stores the start Time and those
members, bounded by replica membership; a pending joiner is not stored. On a member's matching
Ok it stores the time elapsed since the start; duplicates/stale sessions add nothing. A re-send
on a new session leaves the start as it is: a member cannot shorten its time by losing its
session. Include client writes and repair writes, exclude Copy, Discard/Read/Digest/Heartbeat
and one-ISR-member writes. No extra RPC, probe or lock occurs.

**Leaving and completing (S1, S2).** When `apply_view` shrinks a chunk's ISR or
`adopt_replicas` drops a replica, the sidecars of that chunk's writes forget the member, its
time included. A sidecar left with one member is cleared and never counted. `depose` drops
the sidecars with the writes. A sidecar is complete when every member it still has has a time:
it is folded into the open window at that moment and marked counted, so a write kept for a
joiner is not counted again. This is the departed member's exclusion that the approval
settled; S1 and S2 now state it, and the rule-to-test table names its case.

**Folding (S1, S2).** For a complete sample, calculate every member's median peer time
exactly (even median uses doubled integer quantities) and compare strictly above floor and
k×median. Fold counts and each member/peer latency into fixed histograms; do not retain
completed sample lists. At a window's end, inspect the unfinished sidecars once: one with at
least one time counts with its known times and the missing members' ages at the window's end
(not at the tick's `now`). A write started at or after the window's end waits for the next
window. A sidecar contributes at most once per window (it records the last window that saw
it), and a fresh censored observation in each later window until it is complete.

**Late time (the spec's Terms).** Each node keeps this window's `late_us` and `late_until`,
the end of the last span counted. When a folded member is behind, its late span runs from the
write's start plus the bar to its acknowledgement, or to the window's end when censored. The
part of the span after `late_until` and after the window's start is added to `late_us`, and
`late_until` moves to the span's end. So writes late over the same span count it once. Spans
that end out of the order they began in can be undercounted, never overcounted: `late_us` is a
lower bound of the late time, exact when a node acknowledges its late writes in the order
they became late. Two integers per node, nothing per sample.

**Judging (S3, S4, S6).** Windows are the multiples of the window's length on the target's
clock; a target's first window is the rest of the one it starts in. A window is closed by the
first tick at or after its end. While the judgement is on, `Target::next_wakeup` includes the
open window's end, so a driven target closes it on time: at once in the simulator and in
`Engine::run`, within 10 ms on an attached node. A sample completed between a window's end
and the tick that closes it counts in the window being closed. Closing judges every node by
the spec's Terms, stores the window's figures for the report (samples, behind, late time, the
estimates), applies the hold and the lapse, then zeroes the buffers. Empty/intermediate-share
windows retain state; under evidence clears Slow once the hold has passed, lapse consecutive
unjudged windows clear it whatever the hold, and a new target resets everything. Session
failure/reopen does not reset evidence.

**The hold (S3).** `Judgement` keeps, per node: slow or not, the window it became slow in, its
hold in windows, and the window it last stopped being slow in. Becoming slow sets the hold to
1, or to twice the last hold, up to the cap, when the node stopped being slow no more than the
cap's length ago. Moving reads moves the load that made a member late (bulk work shares the
session); the hold bounds the changes that follow. A node whose peer `apply_view` removes
(`:2592–2601`) loses its entry and its histograms with the peer, and leaves the report; added
back, it has none (S5).

**Gaps (S5).** A tick more than one window after the previous tick (the target was not
driven, or the clock jumped) closes the open window and every window passed since as unjudged,
discards the open figures and clears every sidecar: no sample is invented and no old window is
stretched across the outage. Those windows count toward the lapse. A pause shorter than a
window is not detected: the writes in flight across it are late by its length for the members
whose acknowledgements had not been handled (decisions.md, named risks).

**Histograms (S12, S13).** A time's bucket is the time itself below 4 us; from 4 us, its
highest set bit and the two bits under it: four buckets per octave, 252 buckets for every
`u64` of microseconds, and no overflow bucket. A reported median or 99th percentile is the
upper bound of the bucket where the cumulative count reaches half, or 99%, of the samples:
never below the true order statistic and less than 25% above it. Every such figure is named
`…_estimate`. Behind comparisons use the exact paired times. Aggregate each node across the
volume's sampled chunks. O(nodes×buckets + existing-inflight×RF) storage, no array sized by
requests/second or completed writes in a window.

## Source selection without authority changes (S7–S11)

Extract `usable_read_member(chunk,node,exclude)` from today's ISR/session/removing predicate.
`source` first iterates usable non-Slow local/IDs, then falls back to all usable local/IDs.
Keep existing avoid/retry exclusions. `has_other_source`, lease/required quorum, fencing,
write recipients/timeouts and resync eligibility do not read Judgement. Preference may select
repair/copy sources but never change their allowed membership or pending-removal rules.

Fast admission uses preferred source, so a Slow local with another healthy usable member
falls through to queued remote I/O. Fast completion instead tests the original local member's
continued usability, member generation, session, fence and lease; it must not call the new
preference function. A judgement change alone cannot reject or restart an existing fast read.
A read already sent keeps its peer. A queued read has no peer yet: its source is chosen when it
starts (`try_start`, `:1737`), by the preference of that moment.
All-Slow/one-member/no-new-write cases always fall back; no measurement creates a wait/error.

## Events and the report (S12, S13, S15)

Add `Event::LatencyJudgement {node,slow,hold_windows,window_end,samples,behind,late_us,
median_us_estimate,peer_median_us_estimate}`;
the engine logs one structured line with volume/name/epoch per actual change, where it logs
`SessionFailed`. Lapse clearing logs reason=unjudged_lapse with its actual zero/insufficient
evidence; no synthetic peer latency. No token or payload appears. State/counters survive a
session reopen, not target replacement.

**The report is #71's.** There is no `stats` line, and `control.rs` is not touched: #71's S3
lets observations leave the daemon only through the telemetry socket's `observe`.
`Target::latency(&self, now) -> LatencyReport` copies the last whole window's stored figures
and each node's judgement: at most 32 nodes, the slow ones first, each group in the target's
node order (`NodeId`), with the totals. It reads only and evaluates no window. It is called
where #71's `Input::Observe` handler calls `Target::recovery()`, so #71's bounds are its
bounds: one snapshot a second with one request pending, a 2 s wait for the engine, 4 targets,
a 64 KiB line, and the `ID` grammar checked in the daemon.

**Cross-plan item, made by whichever of #71 and #47 lands second.** #71's plan is not edited
here. The telemetry line becomes schema 2: each entry of `targets` gains a last key.

```text
"latency":{"window_age_us":U|null,"nodes_total":U,"slow_total":U,
           "nodes":[{"node":ID,"slow":B,"slow_for_us":U|null,"samples":U|null,
                     "behind":U|null,"median_us_estimate":U|null,"p99_us_estimate":U|null}]}
```

- **Fields.** `B` is `true` or `false`; `ID` and `U` are #71's. Ages are on the target's
  clock, at the snapshot: `window_age_us` since the last whole window's end, null before the
  first; `slow_for_us` since the node became slow, null when it is not. `samples` and `behind`
  are null before the first window's end. The estimates are null when the node had no sample
  in that window. Nothing unavailable is reported as zero.
- **Bounds.** `nodes` holds at most 32 entries, in the one order `Target::latency` gives: the
  slow nodes first, each group by the target's `NodeId`. That is the order in which names
  first appear in the volume's view (`flaminio-node/src/engine.rs:1623–1633`), not the names'
  order. The cut at 32 is made in that order, and the encoder keeps it. `nodes_total` and
  `slow_total` count all of the target's nodes. A node name outside `ID` answers
  `error: invalid id`, as #71's table says for an id. A valid line is at most 46,075 bytes
  with its `ok ` and its newline (4 targets of 32 slow nodes, 128-byte ids, 20-digit
  numbers), under #71's 64 KiB; the encoder's test builds that line.
- **Fixtures.** `control/observe/testdata/observe-v2.line`: what the Rust encoder produces,
  byte for byte, and what the Go decoder accepts. The decoder accepts schema 1 without
  `latency` and schema 2 with it. It refuses: schema 3 (in place of #71's schema-2 fixture),
  `latency` in a schema-1 line, a schema-2 target without it, 33 nodes, `behind` above
  `samples`, `slow_total` above `nodes_total`, `slow_for_us` on a node that is not slow, and
  a bad node id.
- **The observer** decodes and checks `latency`, then drops it: `/v1/node`, the metric
  families and the 73-series bound stay #71's. Exporting it, and doctor's `latency` check,
  stay #71's later steps (its intent, "What stays open in #71").
- **Order.** If #47 lands first there is no socket yet: #47 ships `Target::latency` with its
  unit tests, and S13's report exists once #71 lands with the schema-2 line. If #71 lands
  first, #47 makes this change and records it in #71's `contracts.md`. A schema-1 observer
  refuses a schema-2 line and reports the node not fresh, so a node's observer is upgraded
  before its daemon (`docs/lab.md`).
- **Reading it.** On the node, a member of the telemetry socket's group, or root, sends
  `observe` to the socket (`/run/flaminio-telemetry/telemetry.sock` in the lab). There is no
  report with telemetry off or under #74's `--privsep`, as #71's contracts say.

#62's draft plan names a root `stats` report of this plan
(`flaminio-62/docs/issues/62/design.md:91`). Its bytes per node belong on #71's path too.
That is for #62's own review; this plan does not edit #62.

## The simulator (the invariant, S9, S10)

- **A slow local replica.** `Sim::send` looks up `slow_links` for `(n, n)` before the 5 to
  50 us default (`sim/mod.rs:659–664`), so `Action::Slow(n, n, d)` delays a node's messages to
  its own replica: its local writes are acknowledged late, and the target judges its own node.
  No existing run holds such a key (the fault picks two different ends), so none draws
  differently.
- **The gray configuration.** `gray(seed)` in `sim/tests.rs`, built as `more_chunks` is
  (`:281–285`): `Config::random(seed)` with fast reads on, the judgement on with low
  thresholds, and a new `local_slow` set, which adds the node itself to the ends the slow-link
  fault picks from. The thresholds start at a 1 s window, a 1 ms floor, `k` = 2, 4 samples
  and a cap of 4, and are fixed with the campaign. `Config::random` draws nothing new and
  leaves `local_slow` and the judgement off: the default campaigns and every existing mutation
  check keep their histories, and the hinted seeds stay valid.
- **Counts.** The target's test-only `Stats` (`target.rs:109`) gain `redirected_reads` (a read
  sent to another member than today's order gives), `fast_refused_slow_local`, and
  `fast_done_across_judgement` (a fast read whose local node's judgement differs between its
  admission and its completion). `Report` sums them as it sums the others
  (`sim/mod.rs:475–480`). `campaign_gray_in_release`, run by `make sim`, fails unless each
  total is above zero. If one is zero the configuration changes, never the assertion.
- **The source check.** The target's test build asks the unmutated predicate
  (`usable_read_member`, ignoring `bugs`) about what the preference returns, for every
  caller of `source`: client reads, repairs and copies (`:1737`, `:1801`, `:1819`), the
  resync's digest and its `Mapped` probe (`:1852`, `:2345`), and fast admission (`:670`). The
  verdict is taken at the choice, because a later step of the same event may change the
  membership. A failed verdict is counted. `Sim::after` (`sim/mod.rs:843`) reads the counts,
  and the run fails with one of three lines: `read sent to an unusable member` (a client
  read, a repair, a copy), `resync asked an unusable member` (the digest, the probe) or
  `fast read admitted on an unusable member`. `try_seed` (`sim/tests.rs:237`) counts the
  three as catches. The check counts only while the judgement is on: with it off, a run with
  an existing bug such as `read_from_removing` fails as it does today (`not linearizable`,
  `sim/tests.rs:204–209`). `sim/check.rs` checks block histories only and is not touched.
- **The injected bug.** `Bugs::prefer_open_replicas`: the preference iterates the chunk's
  replicas with an open session, not its usable members. Fast admission uses the same
  preference, so the bug also admits a fast read on a local replica that is out of the ISR or
  being removed. Two checks under `gray`, each passing on its own line only and passing over
  a seed that fails with another: `catches_prefer_open_replicas_on_a_queued_read`
  (`read sent to an unusable member`) and `catches_prefer_open_replicas_on_a_fast_read`
  (`fast read admitted on an unusable member`).
  The resync's line has no mutation check of its own: the bug reaches the digest and the
  probe only when a chunk has a replica outside the ISR with an open session other than the
  member being resynced, which a campaign need not produce. A target unit test,
  `prefer_open_replicas_reaches_the_resync`, covers the two sites in two cases, so that
  neither site can be left unwired. Both use one chunk built by hand:
  - **The chunk.** The local node is not one of its replicas. `replicas` holds, in this
    order, the member being resynced, a second replica outside the ISR, and the one ISR
    member; a session is open to all three. With the member excluded, the bug's iteration
    reaches the second outside replica first, and the unmutated preference returns the ISR
    member.
  - **The digest** (`:1852`): a queued `Work::ResyncDigest` with a live resync round.
  - **The probe** (`:2345`): a resync started on the chunk. `drive_resync` takes the first
    replica outside the ISR with a session (`:2338–2341`), which is the member.

  Each case asserts the resync's failed-verdict count alone: one with the bug on, zero with
  it off.
  The existing `fast_read_ignores_membership`, `fast_read_no_lease` and
  `fast_read_no_fence_check` checks stay as they are, against the completion check that this
  change splits from the preference.

## Files and rule-to-test mapping

| Rules | Files and concrete validation |
|---|---|
| S1, S2 | `latency.rs/target.rs` unit tests: paired 2/3/5 ISR, repair vs copy, local sends, a re-send timed from the write's start, malformed/duplicate ack, age at the window's end, cross-window attribution, one-member exclusion, a complete sample counted once under a joiner, a peer at 0 us against a member at the floor. The departed member, with the spec's counts: RF 3 with the removal before and after the window's end, RF 2, a move's drop, a deposition. Late time: the periodic stall (2.8 s and 0.4 s of a 5 s window), one span under two late writes, spans ending out of order never overcounted. |
| S3–S5 | Scripted Time tests for strict floor/k/share equality, min 32, four-window fault exactly two changes, alternating/middle-share/no evidence/lapse (which ends a hold), target reset versus session reopen. The hold: the spec's read-following latency (64 windows, cap 8: at most 18 changes per node, at most two in any 8 windows at the cap), and a hold back at 1 later than the cap. A node dropped by `apply_view` and added back starts clean. A tick two windows late with writes in flight: no judgement changes, the sidecars are cleared. The target's own link slow, RF 2 and RF 3. |
| S6 | Bounded allocation/state-size test at different completed I/O counts, exact protocol outputs/no added messages or meta calls on window evaluation; no per-request judgement. With no other timer due `next_wakeup` is the window's end; a late tick takes ages at the window's end. With the judgement off, outputs and `next_wakeup` equal today's over a recorded run. |
| S7, S8 | Target tests select local/lowest usable healthy, all Slow fallback, retry avoids failed peer, removing/non-ISR/closed-session never selected; `prefer_open_replicas_reaches_the_resync` (the digest and the probe, bug on and off); work starts/errors exactly as existing rules permit. |
| S9, S10 | Target + `flaminio-node/tests/engine.rs`, on `flaminio_store::DeferredDevice`, whose commits complete on command (as `Holding` uses it, `:2055–2065`): delayed local device moves new reads to remote path; local-only remains fast; change judgement during admitted fast/queued read does not invalidate its completion. Simulator: the gray campaign. |
| S11 | 2.9 s write-ack test still requires that member's ack, produces no latency-driven ISR change/failure, resync still runs; existing fencing/membership/lease tests stay intact. |
| Invariant, S9, S10 | `sim/mod.rs`, `sim/tests.rs` and the target's test-only counts: the gray campaign, its three counts, the source check and the two mutation checks (The simulator). The existing campaigns and mutation checks run with the judgement off: same histories, hinted seeds valid. |
| S12–S14 | Engine tests capture one bounded log line per transition, with names/epoch/estimates. `Target::latency`: fields, ages, nulls before the first window, the 32-node bound and its totals, an estimate at most 25% above the exact order statistic, unchanged judgement while I/O runs. Configuration validation. With the cross-plan item: the golden line, the refused lines, the line's length at every bound, a node name outside `ID`. |
| S15 | `scripts/checks/gray-failure.sh`, `scripts/tools/gray-failure-test.sh` plus Makefile lint: topology/identity/refusal, the six phases, the netem removal armed on the node and the check killed during the fault, `chaos.sh slow` with `sudo` (a pass-through), `tc`, `systemctl` and `systemd-run` faked and their calls recorded (a SECONDS that is not digits, one of six digits, a zero and an empty one each refused with no call made; one of five digits: stop, reset, delete, arm, add, in that order; a failed `systemd-run`: a delete seen, no add, a non-zero exit; `slow on DELAY` with three arguments: no `systemd-run` call, a delete then an add; `slow off`: the stop and the delete, no add), ISR/version/session checks, logs/latency evidence and no false disk-health claim. |

Touched: repl latency/target/lib/sim, node engine/tests, daemon node defaults, new
check/script-test, `scripts/guest/chaos.sh`, Makefile and docs. `control.rs` is not touched.
With the cross-plan item: #71's line encoder and `EngineObservation`, and the `control/observe`
decoder and fixtures. No metadata API, wire frame, on-disk record, device class, health
Condition or MVP-path change. Implementation `make lint test race contracts sim`; no tests or
benchmarks ran while drafting.

## Lab, docs, risks and ADRs

Later, and only when explicitly authorized: the check takes the lab over, as the checks of
`.github/workflows/lab.yml:53` do.

```bash
TARGET=fl-c REPLICAS="fl-a fl-b" MODE=after scripts/checks/gray-failure.sh
```

`TARGET` attaches the volume and holds none of its replicas; `REPLICAS` are the two nodes that
hold every chunk; `MODE` is `before` or `after`. Names pass `valid_id`
(`scripts/lab.sh:1633`) before they reach a command, and the interface is `wg0`. The check
refuses to start when `TARGET` is one of `REPLICAS`, or has a volume attached. Its six phases:

1. **Set up.** A private RF 2 volume; every chunk's replicas moved onto `REPLICAS`
   (`metactl move`, as `scripts/checks/resync.sh` does); full ISRs; attached on `TARGET`;
   filled once.
2. **Healthy.** A mixed read and write fio load at a stable queue depth, for four windows.
   Each second the check reads each replica node's `/sys/class/net/wg0/statistics/tx_bytes`:
   the node that serves the reads sends their data, the other sends acknowledgements only.
   That node is the one to slow.
3. **Fault.** `chaos.sh slow on 100ms SECONDS` on that node, for four windows.
4. **Recovery.** `chaos.sh slow off`, then four windows.
5. **Verdict.** In both modes: the load's read and write latency in each phase, from fio's
   per-second latency log; the volume's ISRs and versions as they were before the fault; no
   I/O error. With `MODE=after` also: the other node's `tx_bytes` rate passes the slowed
   node's within two windows of the fault's start, and falls back under it within two windows
   of its end (each plus the 1 s between readings); the target's journal has exactly two
   judgement lines for the slowed node, slow then not, none for another node, and no failed
   session; writes keep the slowed node's latency during the fault.
6. **Clean up,** also from the trap: `slow off`, detach, delete the private volume and its
   replicas, and check that `wg0` has no netem qdisc left. Anything left is reported.

**The before run.** `MODE=before` runs with the lab deployed from the parent of this change's
commit (`make deploy` in a checkout of it), using this script from the change's checkout. It
asserts only what that build has: the volume's state and the load. Both modes record the
latency with performance-plan repetitions. The volume's target is new in every run, so every
run starts with no hold.

**Removing the fault.** `chaos.sh slow on` deletes the root qdisc of `wg0` before it adds
netem, and `slow off` deletes it again (`scripts/guest/chaos.sh:7–9`): there is no original
qdisc to restore, and the check removes the netem qdisc. `chaos.sh slow on DELAY SECONDS`
takes a new argument, `$4` (`on` is `$2`, DELAY `$3`): a transient timer on the node
(`systemd-run --on-active`, unit `flaminio-chaos-unslow`) deletes the qdisc after SECONDS,
the fault's length plus a 30 s margin. Every command runs under `sudo`, as the script's do
today. The steps of `slow`, in this order:

1. **Refusal** (only with a fourth argument; given or not is decided by the argument count).
   A SECONDS that is not one to five decimal digits, or is zero (an empty one among them),
   gets a message on stderr and `exit 2`, with no other command run. The script defines no `die` (its usage
   line calls one, `:17`), so the refusal does not use it.
2. **The earlier timer is stopped:**
   `systemctl stop flaminio-chaos-unslow.timer flaminio-chaos-unslow.service`, then
   `systemctl reset-failed` of the same two units, each `2>/dev/null || true`. A killed
   run's timer cannot remove the next run's fault, and a unit left as failed cannot make the
   next `systemd-run --unit` fail.
3. **The delete,** as today (`tc qdisc del dev wg0 root`, `|| true`). `slow off` ends here.
4. **The timer is armed** (only with a fourth argument): `systemd-run` with the unit's name
   and `--on-active=SECONDS`; its command is the delete of step 3, ending in `|| true`. The
   script has no `set -e`: if `systemd-run` fails, the script says so and exits non-zero.
   Step 3 has already removed any earlier fault, and step 5 is not reached, so no fault is
   on without its timer.
5. **The add,** as today (`tc qdisc add dev wg0 root netem delay DELAY`).

`slow on DELAY` with three arguments runs steps 2, 3 and 5: today's command, with a leftover
timer stopped first, so `make chaos` does what it does today. So a check killed with
SIGKILL, or a workstation that loses the VM, leaves the delay for the margin at most. The
trap stays the fast path. `make chaos`'s own trap (`scripts/lab.sh:1426–1428`) keeps that
gap (a follow-up, to be filed).

**The report in the lab** belongs to the cross-plan item: the change that makes the schema-2
line adds to this check that the slowed node is `slow` during the fault and not after it.

Existing `make chaos` remains a separate requested check, never ordinary CI. No disk
format/dm-delay is performed.

Docs/lab.md records the check and its command, how to read the report, the link-fault limits
(a slow link stands for a slow disk; a slow link at the target's own node is judged as its
peers', S12) and the observer-before-daemon upgrade order. Architecture §7.2/§11.3 states
per-target paired writes/read preference only. Same-class p99, write recovery and auto-drain
are future work. Histogram estimates are not exact order statistics; their error is stated
above. The risks this round named are in decisions.md, named and not accepted. No new
ADR: preference never changes data authority; accepted fencing/etcd rules remain. Pair
measurement and reporting can proceed against the fixed contracts; source/completion changes
and simulator authority mutation form one coordinated Rust unit.
Settled at approval, 2026-11-13: Q1 to Q6 and the thresholds, each with its recommendation,
and the departed member's exclusion for cancelled samples (see decisions.md, Settled at
approval).
