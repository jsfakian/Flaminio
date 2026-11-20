# Issue #47 — report

## Review rounds

### Plan review round 1 — c60ba6a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code); plan-reviewer (design-security).

- **R1-C1 — Medium:** S13's `stats` report on the root control socket contradicts #71's telemetry boundary; "#62 has intent only" is stale. Evidence: design.md, "Events/report contract"; `origin/main:docs/issues/71/spec.md` S3 and `contracts.md` lines 48–86; `flaminio-62/docs/issues/62/design.md:91`.
- **R1-C2 — Medium:** the open sample boundary is settled in decisions.md but S1 and S2 are unchanged, and S2 claims what the design does not deliver; "completed" is undefined. Evidence: `flaminio-repl/src/target.rs:811–828`, `:1421–1426`, `:1605–1620`, `:2602–2605`, `:2626–2635`, `:1144`; `types.rs:60`.
- **R1-C3 — Medium:** flapping through feedback: the preference moves the load that made the member slow, and S3 has no hold time. Evidence: `target.rs:393–397`, `:1801`, `:1819`, `:1852`.
- **R1-C4 — Medium:** the simulator cannot make a target's own replica slow, yet intent and spec rest S9 on it. Evidence: `sim/mod.rs:659–663`, `:1170–1177`; `target.rs:670`, `:730–732`.
- **R1-C5 — Low:** the simulator plan does not say which campaign judges; the hinted seeds are at risk; `sim/check.rs` sees no send. Evidence: `sim/tests.rs:462–503`, `:507–512`; `sim/check.rs:1–5`; `sim/mod.rs:856`.
- **R1-C6 — Low:** a node that leaves the volume is not covered. Evidence: `target.rs:2592–2601`.
- **R1-C7 — Low:** `Target::next_wakeup` is not extended to the window's end. Evidence: `target.rs:768–798`; `sim/mod.rs:851–853`; `engine.rs:621–633`; `node.rs:362–367`; `types.rs:59`.
- **R1-C8 — Low:** S12 and S13 promise medians and a 99th percentile; the design gives power-of-two bucket bounds, unlabelled in the event. Evidence: design.md, "Use64 logarithmic microsecond buckets".
- **R1-C9 — Low:** the report's times (`since_us`, `window_end_us`) cannot be placed in time. Evidence: `engine.rs:604–606`; `types.rs:15–16`.
- **R1-C10 — Low:** the design declares #83 "not accepted". Evidence: design.md, "Fixed-memory paired measurements"; `origin/main:docs/issues/83/spec.md` S2.
- **R1-C11 — Low:** the lab plan is not runnable as written: no command, phases not listed, the before run's mode and the evidence of the redirect unnamed. Evidence: design.md, "Lab, docs, risks and ADRs"; `.github/workflows/lab.yml:53`.
- **R1-C12 — Low:** a slow link at the target's own node is judged as the peers' slowness. Evidence: `engine.rs:478`.
- **R1-S1 — Medium:** the simulator cannot make the local replica slow, so S9 and S10 have no simulator coverage. Evidence: `sim/mod.rs:658–662`, `:1168–1177`; design.md lines 71–74, 103.
- **R1-S2 — Medium:** a member that stalls is under-counted: the share is of samples, not of time; a write sent again counts from its last send. Evidence: spec.md Terms, S1, S2; `target.rs:1332–1376`.
- **R1-S3 — Medium:** no bound on flapping when the node's latency depends on the reads moved. Evidence: spec.md Tests; design.md row S3–S5.
- **R1-S4 — Medium:** `stats` on the root control socket contradicts #71's telemetry boundary and leaves its bounds unstated (engine wait, snapshot rate, reply size, id grammar). Evidence: design.md lines 85–91; `flaminio-io/src/control.rs:33–37`, `:83–103`, `:205`; `flaminio-node/src/engine.rs:371`, `:1623–1630`.
- **R1-S5 — Low:** a member's acknowledgement time is its own to set, and it decides where reads go. Evidence: spec.md Terms, S9, Q3; ARCHITECTURE §15.1.
- **R1-S6 — Low:** the design declines #83's clock, a review-clear plan. Evidence: design.md lines 23–24; `engine.rs:604–606`.
- **R1-S7 — Low:** what decisions.md settled is not carried into the spec, and the design still calls it open. Evidence: design.md lines 124–126; decisions.md, the open sample boundary.
- **R1-S8 — Low:** the lab check's clean-up depends on the host's trap alone, and there is no original qdisc to restore. Evidence: `scripts/lab.sh:1428–1430`; `scripts/guest/chaos.sh:7–9`.
- **Nits:** N1 (`target.rs:730–732`, not `:733`), N2 (the intent's "the engine logs only session failures"), N3 (missing spaces before numbers in design.md), and the security review's Nit ("Queued/sent reads keep their selected peer").

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 1 (2026-11-17): changes made after approval"):
- **R1-C1, R1-S4:** the root-socket `stats` line is cut and `control.rs` is not touched. The per-node figures leave the daemon in #71's `observe` snapshot, under #71's bounds (one snapshot a second with one pending, a 2 s engine wait, 4 targets, 64 KiB, the `ID` grammar and `error: invalid id`), read by `Target::latency` where #71's handler reads `Target::recovery()`. A target lists at most 32 nodes, slow ones first, with totals; the line at every bound is under 64 KiB, with a test. The schema-2 addition to #71's telemetry line (fields, fixtures, refused lines, the observer dropping `latency`, the upgrade order) is stated as a cross-plan item made by whichever of #71 and #47 lands second; #71 is not edited. S13, S15 and the note on #62 are restated.
- **R1-C2, R1-S7:** S1 and S2 carry the settled rule: a member that leaves the ISR or the replica list leaves the sample; a sample left with one member is dropped; a sample is complete once every member it still has has acknowledged, and is counted once, even under a joiner. S2 says it holds in the windows that end before the removal commits. The test is named with its counts (RF 3 before and after the window's end, RF 2, a move's drop). The stale sentences and the "Open sample boundary" paragraph are removed.
- **R1-S2:** a member's time runs from the write's start, not from a re-send. A node is also over when it is late for more than the upper share of the window, and under only when late for less than the lower share ("late time", two integers per node). Unit tests: the periodic stall at 2.8 s and at 0.4 s of a 5 s window, one span under two late writes.
- **R1-C3, R1-S3:** S3 gains a hold: one window the first time, doubling up to a cap (64 windows, Proposed) when a node turns slow again within the cap's length. Unit test on a latency that follows the read placement: at most 18 changes per node over 64 windows with a cap of 8, at most two in any 8 windows at the cap. Q5 (bulk reads follow the preference) is kept.
- **R1-C4, R1-S1:** the simulator gains a slow link from a node to itself (`Sim::send`, `Action::Slow(n, n, d)`), a `gray` configuration with the judgement and fast reads on, three counts that must be above zero (redirected reads, fast reads refused for a slow local node, fast reads completed across a judgement change), a check of every chosen source, and the injected bug `prefer_open_replicas` caught on a queued read and on a fast read. S9 and S10 keep their simulator coverage.
- **R1-C5:** the judgement is off unless `Target::set_latency` turns it on; `Config::random` draws nothing new, so the existing campaigns keep their histories and the hinted seeds stay valid. The source check's verdict is taken at the choice and reported through `Sim::after`; `try_seed` counts its lines as catches; `sim/check.rs` is not touched.
- **R1-C6:** a node that holds none of the volume's chunks is forgotten with its judgement, hold and histograms, and leaves the report; added back, it starts clean (S5, a unit case).
- **R1-C7:** `Target::next_wakeup` includes the open window's end; ages are taken at the window's end; unit test with no other timer due.
- **R1-C8:** the histogram is fixed (four buckets per octave, 252 buckets, no overflow); a figure is never below the true order statistic and less than 25% above it; S12 and S13 say "estimates", and the event's and the report's fields are named `…_estimate`.
- **R1-C9:** the report gives ages (`slow_for_us`, `window_age_us`), not instants on the engine's clock.
- **R1-C10, R1-S6:** "not accepted" is removed. Windows and samples run on the clock the target is driven with, #83's once it lands; #83's quarantine ends the target and its judgements. A tick more than a window late ends the windows passed as unjudged and drops the samples in flight (S5), with the unit case of a clock jump under writes in flight.
- **R1-C11:** the lab section gives the command and its variables, the six phases, what `MODE=before` runs against and asserts, and the evidence of the redirect (each replica node's `wg0` `tx_bytes`, read each second; the judgement lines in the target's journal).
- **R1-C12:** the Terms, S12 and S15 say that the target's own slow link is judged as its peers'; unit cases for RF 2 and RF 3.
- **R1-S5:** recorded in decisions.md as a risk named, not accepted, with its bounds (the chunks the member holds, the floor, the retry rule) and the reviewer's alternative; unit test added: a peer acknowledging in 0 us does not make a member at the floor behind.
- **R1-S8:** `chaos.sh slow on DELAY SECONDS` arms a transient timer on the node that deletes the qdisc after the fault's length plus 30 s; the trap stays the fast path; the design says "removes the netem qdisc"; script-test case: the check killed during the fault leaves no qdisc after the margin.
- **Nits:** N1, N2, N3 and the security review's Nit are fixed (a reference and wording each).

Nits left: none.

Rules narrowed (each because a finding showed the rule could not hold as stated; decisions.md): S13 (no report until #71's socket exists, nor without telemetry or under #74's `--privsep`; at most 32 nodes listed per target), S2 (holds in the windows that end before the member's removal commits), S3 (a slow node that is under clears only once its hold has passed).

Risks named, not accepted (decisions.md): a member sets its own acknowledgement time (R1-S5); time is measured in windows, and `late_us` is a lower bound (R1-S2); a member removed within the window leaves no sample (R1-C2); changes are bounded, not excluded, and a recovered node stays slow for the rest of its hold (R1-C3, R1-S3); no report without #71's socket, and a schema-1 observer refuses a schema-2 line (R1-C1, R1-S4); a pause shorter than a window looks like slowness (R1-S6); the target's own slow link is judged as its peers' (R1-C12); a window can close up to 10 ms late on an attached node (R1-C7); the lab fault can outlive a killed check by the 30 s margin, and `make chaos` keeps the same gap (R1-S8).

For other plans: #71's telemetry line needs the schema-2 addition (made by whichever of #71 and #47 lands second; #71 not edited). #62's draft builds on "#47 stats v1", which no longer exists: #62's own review must move its bytes per node onto #71's path. A follow-up for `make chaos`'s trap is to be filed.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — c4a3290

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R1-C1 to R1-C12 and R1-S1 to S8 fixed (R1-S1 but for the queued-read catch, R2-B2); R1-S5 stays a risk named, not accepted.

- **R2-B1 — Low:** the simulator's source check covers two of the four places a source is used (not the resync's digest and its `Mapped` probe), and does not say whether it runs with the judgement off, where it would fail `read_from_removing`'s scenario with a new line. Evidence: design.md, "The source check"; `flaminio-repl/src/target.rs:1852`, `:1874`, `:2345`, `:2373–2377`; `sim/tests.rs:204–209`.
- **R2-B2 — Low:** no mutation check has to catch `prefer_open_replicas` on a queued read: `catches_prefer_open_replicas` passes on any catch line, and under `gray` the first catch is likely the fast-read line. Evidence: design.md, "The injected bug"; `sim/mod.rs:922`.
- **Nits:** 1 (the worst-case line is 46,075 bytes, not 46,203), 2 (`Target::latency` orders by node id, the line by `node`: which order cuts at 32), 3 (a held recovered node plus another turning slow sends reads back to today's order), 4 (the spec's Terms define late time as the exact union, the design counts a lower bound), 5 (`chaos.sh`: the timer's unit, an earlier timer, a non-numeric SECONDS).

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 2 (2026-11-17): changes made after approval"):
- **R2-B1:** the verdict is taken on what the preference returns, for every caller of `source`: client reads, repairs, copies, the resync's digest and its `Mapped` probe, and fast admission. The digest and the probe have a failure line of their own (`resync asked an unusable member`); `try_seed` counts the three lines. The check counts only while the judgement is on, so a run with an existing bug fails as it does today. S7 says the resync's digest and probe follow the same choice; the design's facts and the spec's Tests name them.
- **R2-B2:** two checks, each passing only on its own line and passing over a seed that fails with another: `catches_prefer_open_replicas_on_a_queued_read` (`read sent to an unusable member`) and `catches_prefer_open_replicas_on_a_fast_read`. The spec's Tests say one check for each.
- **Nit 1:** "a valid line is at most 46,075 bytes" (recomputed from the grammar: 4 targets of 32 slow nodes); the encoder's test still builds that line.
- **Nit 2:** one order: slow nodes first, each group by the target's `NodeId` (the order names first appear in the volume's view, not the names' order); `Target::latency` cuts at 32 in it and the encoder keeps it.
- **Nit 3:** added to decisions.md under "Changes are bounded, not excluded", as a risk named, not accepted.
- **Nit 4:** the Terms say late time is counted as a lower bound, never overcounted.
- **Nit 5:** the timer's unit is `flaminio-chaos-unslow`; `slow on` refuses a SECONDS that is not decimal digits before it touches the qdisc, and stops an earlier timer of that unit before it arms the new one.

Nits left: none.

No rule narrowed. One consequence added to a risk already named, not accepted (Nit 3): while a recovered node is held and the chunk's other usable members turn slow, reads return to today's order for up to 5 min 20 s.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — fc5b072

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-L1 and R3-L2 fixed.

- **R4-L1 — Low:** the unit test asserts the count at two sites, but the design gives a count per failure line only: one resync count passes with one of the two sites unwired. Evidence: design.md, "The simulator"; `target.rs:1852`, `:2345`.
- **R4-L2 — Low:** the script has no `set -e`, and the design does not say that a failed `systemd-run` stops the run before `tc qdisc add`: the fault would be armed with no timer. Evidence: design.md, "Removing the fault"; `scripts/guest/chaos.sh`.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 4 (2026-11-17): changes made after approval"):
- **R4-L1:** the unit test has two cases, each asserting the resync's count alone (one with the bug on, zero with it off): the digest, with a queued `Work::ResyncDigest` and a live resync round; the probe, with a resync started on a chunk whose first replica outside the ISR with a session is the member being resynced.
- **R4-L2:** a failed `systemd-run` ends the script non-zero before `tc qdisc add`. Whether SECONDS is given is decided by the argument count, and an empty one is refused. The script test fakes `tc`, `systemctl` and `systemd-run`: a SECONDS that is not digits, one of six digits and an empty one are each refused before any `tc` call, and a failed `systemd-run` adds no netem (S15 row).

Nits left: none.

No rule narrowed; no new risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — e998e2a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R4-L1 and R4-L2 fixed in substance.

- **R5-L1 — Low:** the two cases of `prefer_open_replicas_reaches_the_resync` fix neither the local node's role nor the order of `replicas`: with the ISR member first, the bug returns a usable member and "one with the bug on" fails. Evidence: design.md, "The simulator"; `target.rs:1644–1653`.
- **R5-L2 — Low:** the earlier timer is stopped before the arming and the delete comes after it, so a failed `systemd-run` exits before the delete: a fault left by a killed run stays on with no timer. Evidence: design.md, "Removing the fault".
- **R5-L3 — Low:** every command of `chaos.sh` runs under `sudo`, which the script test does not fake; no case covers `slow off`, `slow on DELAY` with three arguments, or a valid five-digit SECONDS. Evidence: design.md, the S15 row.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 5 (2026-11-17): changes made after approval"):
- **R5-L1:** the chunk is named: the local node is not a replica; `replicas` holds the member being resynced, a second replica outside the ISR and the ISR member, in that order, with a session to each. The bug reaches the outside replica; the unmutated preference returns the ISR member.
- **R5-L2:** the steps of `chaos.sh slow` are one ordered list: refusal, stop of the earlier timer, delete, arming, add. The delete comes before the arming, so a failed arming leaves no earlier fault on and adds none. `slow on DELAY` with three arguments also stops a leftover timer first.
- **R5-L3:** `sudo` is faked as a pass-through and the calls are recorded; the cases gain a five-digit SECONDS (the five steps in order), `slow on DELAY` with three arguments (no `systemd-run` call) and `slow off` (the stop and the delete, no add).

Nits left: none.

No rule narrowed; no new risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 6 — 3bb708eec85f3a1a9476b57cfa8843e2e0a3fac7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Reviewed commit d12ebf8. Nothing of Low or worse. One Nit: a SECONDS of `0` passed the
"one to five digits" rule, and a timer armed with it could fire before the add. Fixed in
3bb708e (a zero is refused; the script test gains the case), with no further round, under the
batch's rule for Nits.

Status: clear. Approval remains separate; no approval is inferred.
