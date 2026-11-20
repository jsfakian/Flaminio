# Plan review: issue #47 (Gray-failure detection), round 1, focus design-code

Scope: `docs/issues/47/{intent,spec,design,decisions}.md` in `/home/jsfakian/Documents/src/flaminio-47`,
checked against the code of that checkout (identical to `origin/main` for `flaminio-repl`,
`flaminio-node`, `flaminio-io`, `scripts/lab.sh`, `scripts/guest/chaos.sh`) and against the clear
plans #71, #83, #75, #81, #82, #86, #13, #74 on `origin/main`.

Not findings: no rule's safety rests on a timing threshold (the judgement only orders usable
members); no proto, etcd, on-disk or wire change; no ADR number or claimed field is used; the
batch ledger queues no hand-off for #47. #45's Missing/Failed are metadata node states and this
step adds no node or disk state, so there is no overlap yet; the later `Degraded` step will meet
#45's `node_state`.

Counts: Critical 0, High 0, Medium 4, Low 8, Nit 3.

### R1-C1 — Medium
**S13's report on the root control socket contradicts #71's telemetry boundary.**
Design, "Events/report contract": `stats [VOLUME]` on the root socket, JSON `version:1`, 4 MiB
bound. #71 (review-clear) S3: "Observations leave the daemon only through a telemetry socket that
answers `observe` and refuses every other line", with a strict schema-1 decoder (unknown key and
schema 2 refused), a 64 KiB line and at most 4 targets (`origin/main:docs/issues/71/spec.md` S3,
`contracts.md` lines 48-86). #71's intent also keeps "gray-failure progress as #45–#47 land" and
doctor's `latency` for its own later steps. #47 adds a second observation channel, a second
engine snapshot input and different bounds. The design's "#62 has intent only" is also stale:
#62's draft now builds on "#47 stats v1" (`flaminio-62/docs/issues/62/design.md:91`).
Fix: carry the per-node latency report on #71's path (a versioned addition to `observe`, with its
golden line, Go decoder and bounds, ordered against #71's landing), or drop the socket report
from this step and keep S12's log plus a test accessor; then restate S13, the lab check and the
note to #62.

### R1-C2 — Medium
**The "open sample boundary" is settled in `decisions.md` but the spec was not changed, and S2
still claims what the design does not deliver.**
`decisions.md` (last entry): "S1 and S2 are to be clarified to say so, and the case tested before
implementation". `spec.md` S1/S2 at HEAD say nothing of a departed member. S2: "a member that
stops answering is counted before its request times out". By the code a stalled member's session
fails at 3 s (`types.rs:60`, `target.rs:811-828`), its removal commits, and `apply_view` completes
and removes every waiting write (`target.rs:2602-2605`, `check_write` `:1421-1426`); with the
5 s window, a stall that starts in a window's first 2 s leaves no sample at all. The same loss
happens on a move's drop (`adopt_replicas` forgets `sent`/`acked`, `:2626-2635`) and on
deposition (`:1144`). Unspecified: whether the whole sample or only the departed member's entry
is dropped (with RF 2 the first), and whether "completed" means every sampled ISR member
acknowledged or the `WriteOp` was removed (a slow joiner keeps it, `write_quorum` `:1605-1620`,
so it would be counted again each window). The test row only says "Include the open ISR-shrink
cancellation case". `design.md` still says "Q1–Q6 and thresholds remain unapproved" and calls
this "an explicit spec/design review blocker".
Fix: write the rule into S1/S2 (what a sample is once a member leaves the ISR or the replica
list; S2 holds only when a window ends before the removal commits), define "completed", name
the test with its expected counts, and remove the stale sentences.

### R1-C3 — Medium
**Flapping through feedback: the preference moves the load that made the member slow, and S3 has
no hold time.**
Reads, repairs and resync copies follow the preference (S7, Q5; `try_start`,
`target.rs:1801,1819,1852`). Bulk work shares the member's session with write acknowledgements
(`target.rs:393-397`), so the copy source's acknowledgements queue behind up to `max_bulk` copy
replies; a saturated disk does the same with client reads. With ISR {X, Y}: X is over in window
n, its reads and copies move to Y; in window n+1 Y is over and X under, so at that window's end
X clears and Y becomes slow, and the load returns to X. S3 allows one change per node per window,
so both nodes change every window, with two log lines each, for as long as the load lasts (an RF 3
resync, or a move's third replica). Intent 3 and the issue's "it does not flap" are not met, and
no test has latency depend on where reads go.
Fix: add a hold (a slow node stays slow for N windows, or clears only after M under windows in
a row) or keep bulk reads out of the preference; add a unit test where the behind share follows
the read placement. If accepted instead, record it in `decisions.md` as a risk with the bound.

### R1-C4 — Medium
**The simulator cannot make a target's own replica slow, yet intent and spec rest S9 on it.**
Intent, Out of scope: "a slow local replica is checked in the simulator and the engine's tests
only"; spec Q2: "simulator and engine only". In the simulator a node's messages to itself always
take 5–50 us (`sim/mod.rs:659-660`); `slow_links` applies only between distinct endpoints
(`:663`, `fault` `:1170-1177`), and `Action::Slow(n, n, _)` is ignored for the same reason. So no
run has a slow local node, and S9/S10 (no fast read admitted, completion split from the
preference at `target.rs:670` and `:730-732`) never meet fast reads there. The design's Invariant
row adds no local delay; its S9/S10 row is unit and engine tests only.
Fix: add a local-delay fault to the simulator (drawn last in `Config::random`, or a scripted
action) and a scenario with fast reads on while the local node is slow; or correct intent and
spec to "unit and engine tests only".

### R1-C5 — Low
**The simulator plan does not say which campaign judges, and the hinted seeds are at risk.**
Design, Invariant row: "thresholds exercise actual redirects ... retain existing mutations". If
targets in `Config::random` runs judge (even with the defaults, a seed with enough paired writes
and a slow link does), read and copy routing changes and so do those seeds' histories. Two
checks run one hinted seed only and fail with "the hint no longer catches: search again"
(`sim/tests.rs:462-481`, `hinted` `:487-503`; one budget is 4,000,000 seeds). The file's idiom
for a new shape is a separate `Configure` (`more_chunks`, `:507-512`). The row also names
`sim/check.rs`, which only checks block histories (`check.rs:1-5`) and sees no send.
Fix: say that the default campaign keeps judgement off (same histories, hints valid) and a
dedicated configuration runs the low thresholds, the redirect count and the mutation; or plan the
hint search. Put the source check where sends are seen (`Sim::after`, `sim/mod.rs:856`, or
`check_accounting`), against the target's unmutated usable predicate.

### R1-C6 — Low
**A node that leaves the volume is not covered.**
`apply_view` drops the peer of a node that holds none of the volume's chunks any more
(`target.rs:2592-2601`). S5 covers a session's failure only. Unspecified: whether the node's
judgement and histograms go with the peer, whether the report still lists it, and whether a node
moved back starts clean or still slow (until S4's lapse).
Fix: one line in S5 and the design (state dropped with the peer; a node added back starts not
slow), and a unit case in the S3–S5 row.

### R1-C7 — Low
**`Target::next_wakeup` is not extended to the window's end.**
`next_wakeup` is the contract for when `tick` has work (`target.rs:768-798`); the simulator and
`Engine::run`/`drive_until` wake targets only by it (`sim/mod.rs:851-853`, `engine.rs:621-633`,
`node.rs:362-367`). The design says "Timer evaluates current histograms" and nothing more, so a
window is judged at the next heartbeat or timeout (up to 1 s late by default, `types.rs:59`), and
the age S2 counts is undefined between the window's end and that tick.
Fix: add the window's end to `next_wakeup`, state that ages are taken at the window's end, and
test it (a scripted clock with no other timer due).

### R1-C8 — Low
**S12/S13 promise medians and a 99th percentile; the design gives power-of-two bucket bounds.**
Design: "64 logarithmic microsecond buckets ... (upper bucket bound, explicitly labeled
estimated)", base unstated; 64 buckets over a `u64` of microseconds is one per bit, so a figure
can be up to twice the true value. Spec S13 says "the median and 99th percentile of its times"
and S12 "the node's and the others' median times"; `Event::LatencyJudgement` carries
`median_us`/`peer_median_us` without the label that the report's fields have. Q4 wants `k` and
the floor set from measured healthy times.
Fix: fix the bucket bounds in the design (for example 2 or 4 per octave over 1 us to the request
timeout), say "estimate" and its error in S12/S13, and name the event's fields alike.

### R1-C9 — Low
**The report's times cannot be placed in time.**
`since_us` and `window_end_us` are the target's `Time`: microseconds since the engine started
(`engine.rs:604-606`, `types.rs:15-16`). The shape has no current time, so neither an operator
("since when", S13) nor the lab check ("within two windows") can turn them into an age.
Fix: add the snapshot's `now_us` on the same clock (or report ages), and test it.

### R1-C10 — Low
**The design declares #83 "not accepted".**
Design, "Fixed-memory paired measurements": "#83's elapsed-clock proposal is separate and not
accepted by this design". #83 is review-clear and approved; its S2 puts every timing decision on
one elapsed clock (`origin/main:docs/issues/83/spec.md` S2). The measurement only needs the `now`
the target is given, so nothing conflicts but the sentence.
Fix: say that windows and samples use the target's `Time`, which becomes #83's clock when #83
lands, and that a clock fault's quarantine ends the target and its judgements with it.

### R1-C11 — Low
**The lab plan is not runnable as written.**
Design, "Lab, docs, risks and ADRs" and the S15 row: no invocation line (the existing checks are
run as `VAR=… scripts/checks/<name>.sh` and listed in `.github/workflows/lab.yml:53`); "six
phases" are not listed; "old/new binaries" does not say how the old build is deployed or how a
script that asserts on the new log lines and report runs against it; and "source redirects ≤ two
windows" names no evidence: the report has no per-node read count (only the simulator's test
report does), so it must come from the load's latency log or the judgement log's timestamps.
Fix: give the command and its variables, list the phases, state the before-run's mode, and name
the measurement that shows the reads moved and when.

### R1-C12 — Low
**A slow link at the target's own node is judged as the peers' slowness.**
Spec Terms: "a node's judgement is its disk's, or its link's". The local replica is served in
process (`engine.rs:478`, `LocalFrom::Target`), so when the target's own link is slow every
remote member is late against the local one: with RF 2 and a local replica the healthy remote
node is logged and reported slow (S12, S13); with RF 3 nobody is (the median of the other two is
half the delay, under `k` = 4). Reads are unaffected, since the local replica is preferred
anyway, but the log names the wrong node, and the later removal step would inherit it. No test
has the delay on the target's side.
Fix: say it in Terms, S12 and `docs/lab.md` (S15), and add the RF 2 and RF 3 unit cases to the
S1–S5 list.

## Nits

- **N1.** `design.md`, facts: fast completion's check is at `target.rs:730-732`, not `:733`.
- **N2.** `intent.md` Why 4: "the engine logs only session failures" is not so: it also logs lost
  sessions, ISR proposals and failed heartbeats (`engine.rs:830`, `:1804-1807`, `:1440`). Meant:
  the target's `Event` has one variant (`target.rs:132-135`).
- **N3.** `design.md` drops the space before numbers throughout ("remains3 s",
  "window5,000,000 us", "minimum32 samples", "Use64", "to4 MiB", "|2.9 s").

Reviewed commit: c60ba6abede9b06b73712a741a82a4ae688774ff
