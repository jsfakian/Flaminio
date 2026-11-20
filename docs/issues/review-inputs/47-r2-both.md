# Plan review: issue #47 (Gray-failure detection), round 2, focuses design-code and design-security

Scope: `git diff c60ba6a..5afe3b3 -- docs/issues/47/` (report.md excluded) in
`/home/jsfakian/Documents/src/flaminio-47`, checked against that checkout's code (its
`dataplane/` and `scripts/` equal `origin/main` but for two lock files) and against #71, #74
and #83 on `origin/main`.

Counts: Critical 0, High 0, Medium 0, Low 2, Nit 5.

## Earlier findings

- R1-C1 (Medium): fixed. No `stats` line, `control.rs` untouched; the figures ride #71's `observe` as a schema-2 `latency` key, under #71's bounds.
- R1-C2 (Medium): fixed. S1/S2 carry the departed member, "complete" is defined, the test has its counts, the stale sentences are gone.
- R1-C3 (Medium): fixed. The hold bounds the changes; the arithmetic holds (see below). One consequence is unnamed (Nit 3).
- R1-C4 (Medium): fixed. A slow link `(n, n)` and the `gray` campaign reach a slow local replica.
- R1-S1 (Medium): fixed, but for the queued-read catch (R2-B2).
- R1-S2 (Medium): fixed. Times run from the write's start; late time is a second arm of "over". The reviewer's own example (frozen 2.5 s of 5 s) is still "neither", by the strict upper share: named in decisions.md.
- R1-S3 (Medium): fixed (the hold, with its unit test).
- R1-S4 (Medium): fixed. #71's wait, rate, 64 KiB, `ID` grammar and `error: invalid id` apply.
- Lows (R1-C5 to C12, R1-S5 to S8): all fixed. R1-S5 is recorded as a risk with its bounds and its unit test, left to the user to accept.

## What the brief asked to check, and holds

- **#71's boundary.** The key fits #71's contract: same socket and group, one line, 4 targets, strict decoder, `/v1/node` and the 73 series unchanged because the observer drops the key. A schema-1 observer does refuse a schema-2 line (#71 contracts, "Golden fixtures": "schema 2" is a refused line) and goes not fresh; the upgrade order is stated and the risk named. The hand-off is in the batch ledger and names the encoder, `EngineObservation`, the decoder, the fixtures and the refused lines: enough to build. The line stays under 64 KiB (Nit 1 for the figure).
- **The simulator.** Buildable as written. With `local_slow` off and no `(n, n)` key, `Sim::send` and `fault` draw what they draw today (`sim/mod.rs:659-664`, `:1170-1173`); with the judgement off `next_wakeup` and `source` are today's, and the completion check split from the preference is the same predicate, so histories and both hinted seeds stand. The three counts are reachable: 5% of the simulator's fast reads last up to twice the lease (`sim/mod.rs:979-983`), far longer than the 1 s window. The injected bug is caught on a fast read as soon as the target's own replica is out of the ISR with an open session, which a failover produces.
- **The hold.** 18 changes in 64 windows at a cap of 8, 11 at a cap of 64, two in any span of the cap's length at the cap, 64 windows = 5 min 20 s: all correct (recomputed). The spec's two-member case gives 17 changes per node; without the hold, one per window. No state leaves a read without a source: with every usable member slow S7 falls back to today's order.
- **Late time.** Spec, design and tests agree on the arithmetic (2.8 s > 2.5 s is over; 0.4 s < 0.5 s is under). See Nit 4 for the one wording gap.
- **`next_wakeup` and on/off.** No lock, no metadata request, nothing per request beyond the sample; off is one branch. `Engine::next_wakeup` (`engine.rs:621-633`) and the 10 ms ticker (`ublk.rs:1178-1186`) are as the design says.
- **#83.** Consistent: the target's `Time` becomes #83's clock; `Target::invalidate_clock` uses the depose cleanup (#83 design) and a restart is required, so a new target starts clean.
- **The lab check.** Scripts are described as they are (`chaos.sh:7-9`, `lab.sh:1426-1428`, `:1633`, `lab.yml:53`). Names pass `valid_id`; `run_on_vm` quotes every argument with `printf %q` (`scripts/lib.sh:152-158`). See Nit 5.
- **The three narrowings.** Each rests on a finding that shows the rule could not hold (S13: #71's S3; S2: `target.rs:2602-2605`; S3: the feedback through `source`), and intent, spec and design say the same thing.

## Findings

### R2-B1 — Low
**The source check covers two of the four places a source is used, and does not say whether it runs with the judgement off.**
Design, "The simulator", "The source check": "Where a source is chosen ... in `start_read` (`:1674`) ... and in `try_fast_read_inner` (`:662`)". Spec Tests: "at each choice".
- `source` has two more callers that send to the chosen member without `start_read`: the resync's digest (`target.rs:1852`, then `:1874`) and the resync's `Mapped` probe (`:2345`, then `:2373-2377`). The design's facts list "client, repair and copy reads" only. With the preference inside `source` both follow it, and under `prefer_open_replicas` both can pick a replica out of the ISR, unchecked. S7 and Q5 do not say whether they follow the preference.
- The check asks the predicate "ignoring `bugs`". If it runs in every test build, a run with `read_from_removing` fails with the new line before the history is checked, and `scenario_removal_in_flight_catches_reads_from_removing` asserts `not linearizable` (`sim/tests.rs:204-209`). That contradicts "the existing ... mutation checks keep their histories".
Fix: take the verdict on what the preference returns (every caller), or name the two sites; say in S7 that the resync's probe and digest use the same choice. State that the check counts only while the judgement is on (or that it keeps `read_from_removing` inside the predicate).

### R2-B2 — Low
**No mutation check has to catch the bug on a queued read.**
Spec Tests: "The simulator must catch it on a queued read and on a fast read." Design: `catches_prefer_open_replicas` passes "on any catch line"; only the fast-read check names its line. Under `gray` fast reads are always on and a client read tries the fast path first (`sim/mod.rs:922`), so the first catch is likely the fast-read line, and the first check then passes without `read sent to an unusable member` ever being produced. A queued-read check that is missing or misplaced fails no test.
Fix: make `catches_prefer_open_replicas` pass only on `read sent to an unusable member` (or run it with fast reads off), as the fast-read check passes only on its line.

## Nits

1. `design.md`, Bounds: the line "at every bound" is not 46,203 bytes. From the stated grammar a valid line is at most 46,075 (a node with a 20-digit `slow_for_us` has `"slow":true`; 46,203 counts `false`, which the decoder refuses). Say 46,075, or "at most".
2. `design.md`: `Target::latency` orders "by node id", the line "sorted by `node`". The target's `NodeId` is the engine's interning index (`engine.rs:1623-1633`), not the name's order. Say that the encoder sorts by name, and which order cuts at 32.
3. `decisions.md`, "Changes are bounded, not excluded": add that while a recovered node is held and another turns slow, every usable member is slow, so reads return to today's order, which may be the node now slow, for the rest of the hold (up to 5 min 20 s; never worse than today).
4. `spec.md`, Terms, "late time": defined as the exact union; the design counts a lower bound (spans ending out of order). Say "counted as a lower bound" in the Terms, as decisions.md does.
5. `design.md`, "Removing the fault": name the timer's unit, have `slow on` stop an earlier timer before arming (a killed run's timer would otherwise remove the next run's fault), and have `chaos.sh` refuse a SECONDS that is not decimal digits.

Reviewed commit: c4a3290a6cae67a1cc439ed95df8b5360fdc50ae
