# Issue #47 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`86607bffb6f227d53a0c8e61101deb51bc379c85a74b93d6fa4604d7bf122277`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

Plan review round 1 (2026-11-17) fixes, made after approval: see below.

Plan review round 2 (2026-11-17) fixes, made after approval: see below.

Plan review round 3 (2026-11-17) fixes, made after approval: see below.

Plan review round 4 (2026-11-17) fixes, made after approval: see below.

Plan review round 5 (2026-11-17) fixes, made after approval: see below.

## Settled at approval

- **Q1. What a replica is compared against:** (a), the other ISR members of the same chunk, on
  the same write, with an absolute floor; (c), disks and device classes, becomes the cluster-wide
  check of a later step. `spec.md`, Open questions Q1 and S1–S3.
- **Q2. May the local replica lose its reads:** yes. `spec.md`, Open questions Q2 and S9.
- **Q3. A load without writes:** not judged in this step; a slow judgement lapses. `spec.md`,
  Open questions Q3, S1 and S4.
- **Q4. The thresholds:** the design's proposed defaults: a window of 5 s, an absolute floor of
  5 ms, `k` = 4, upper share 50%, lower share 10%, at least 32 samples, a lapse of 3 unjudged
  windows; none per StorageClass. The floor and `k` stay provisional until the lab's healthy RF 2
  write times are measured, before rollout, as the plan states. `spec.md`, Open questions Q4 and
  S14; `design.md`, Fixed-memory paired measurements.
- **Q5. Bulk reads:** repairs and resync copies follow the same preference as client reads.
  `spec.md`, Open questions Q5 and S7.
- **Q6. The lab fault:** accepted: a slow link (`chaos.sh slow`) stands for a slow disk in this
  step; a `dm-delay` fault is its own change. `spec.md`, Open questions Q6.
- **The open sample boundary** (a write completing after an ISR shrink that its sampled member
  never acknowledged): the proposed exclusion of the departed member after the committed shrink.
  S1 and S2 are to be clarified to say so, and the case tested before implementation, as the
  design asks. `design.md`, Fixed-memory paired measurements (Open sample boundary), and Lab,
  docs, risks and ADRs.

## Plan review round 1 (2026-11-17): changes made after approval

Each change names the finding that raised it: R1-C*n* from the design-code review, R1-S*n* from
the design-security review, both of commit c60ba6a. Nothing here is approved or implemented,
and no risk is accepted.

- **The report follows #71's telemetry boundary** (R1-C1, R1-S4). #71 is review-clear and a
  fixed point: its S3 lets observations leave the daemon only through the telemetry socket.
  The `stats` line on the root control socket is cut, with its `version:1` envelope and its
  4 MiB bound. The per-node figures leave in #71's `observe` snapshot, under #71's bounds: one
  snapshot a second with one request pending, a 2 s wait for the engine, 4 targets, a 64 KiB
  line, the `ID` grammar and `error: invalid id`. A target lists at most 32 nodes, the slow
  ones first, with its totals, which keeps the line under 64 KiB at every bound. The addition
  to #71's contract (schema 2, a `latency` key per target) is a cross-plan item, made by
  whichever of #71 and #47 lands second; #71's plan is not edited. Of the two fixes offered
  (carry the report on #71's path, or drop it and keep the log), the first: S13 and the
  intent ask for a report on request. `spec.md`, S13 and S15; `design.md`, Events and the
  report.
- **The departed member is in S1 and S2** (R1-C2, R1-S7). What the approval settled is now the
  rule. A member that leaves the chunk's ISR or its replica list leaves the sample,
  acknowledged or not; a sample left with one member is dropped. A sample is complete once
  every member it still has has acknowledged, and is counted once, even under a joining
  replica. S2 says where it holds: in every window that ends before the removal commits. The
  test is named with its counts. The design's "Open sample boundary" paragraph and its two
  stale sentences ("Q1–Q6 and thresholds remain unapproved", "an explicit spec/design review
  blocker") are gone. `spec.md`, S1, S2 and Tests; `design.md`, Leaving and completing.
- **A member's time runs from the write's start** (R1-S2). S1 said that a write sent again on
  a new session "counts from its last send": a member could then clear its record by losing
  its session. `spec.md`, Terms and S1.
- **Late time, beside the share of samples** (R1-S2). A member that stalls blocks the queue,
  so it has few samples while it is slowest. A node is now also over when it is late for more
  than the upper share of the window, and under only when it is late for less than the lower
  share. Of the two fixes offered (weigh by time, or keep the count and record the limit), the
  first, as recommended. `spec.md`, Terms; `design.md`, Late time.
- **The hold** (R1-C3, R1-S3). Moving reads moves the load that made a member late, so S3
  alone let a node change every window for as long as the load lasted. A slow node now stays
  slow for its hold: one window the first time, twice the last hold when it turns slow again
  within the cap's length, up to the cap. The cap's default, 64 windows, is Proposed. Of
  R1-C3's fixes (a hold, or bulk reads out of the preference), the hold: the other would undo
  Q5 as approved. `spec.md`, S3, S4 and S14; `design.md`, The hold.
- **The simulator can make a node's own replica slow** (R1-C4, R1-S1). A slow link from a node
  to itself, a campaign of its own with the judgement on and fast reads on, three counts that
  must be above zero, a check of every chosen source, and the injected bug caught on a queued
  read and on a fast read. S9 and S10 keep their simulator coverage; nothing is narrowed.
  `spec.md`, Tests; `design.md`, The simulator.
- **The existing campaigns keep the judgement off** (R1-C5). `Config::random` draws nothing
  new, so the hinted seeds stay valid. The source check is where the choice is made, reported
  through `Sim::after`; `sim/check.rs` is not touched. `design.md`, On and off, The simulator.
- **A node that leaves the volume is forgotten** (R1-C6), with its judgement, its hold and its
  histograms; added back, it starts clean. `spec.md`, S5.
- **The window's end is a wake-up** (R1-C7). `Target::next_wakeup` includes it, and S2's ages
  are taken at the window's end, whenever the tick runs. `spec.md`, S6; `design.md`, Judging.
- **Estimates and their error** (R1-C8). The histogram is fixed: four buckets per octave over
  every `u64` of microseconds. A reported figure is never below the true order statistic and
  less than 25% above it, and is named `…_estimate` in the event and in the report.
  `spec.md`, S12 and S13; `design.md`, Histograms.
- **Ages, not instants** (R1-C9). The report gives `slow_for_us` and `window_age_us`: a reader
  needs no engine clock. Of the two fixes offered (add the snapshot's `now_us`, or report
  ages), ages: one field fewer and no clock to explain. `design.md`, the cross-plan item.
- **#83's clock** (R1-C10, R1-S6). "Not accepted by this design" is gone. Windows and samples
  run on the clock the target is driven with, #83's once it lands; #83's quarantine ends the
  target and its judgements. A tick more than a window late ends the windows it passed as
  unjudged and drops the samples in flight. `spec.md`, S5; `design.md`, The clock and Gaps.
- **The lab check is spelled out** (R1-C11): its command and variables, its six phases, what
  the before run asserts, and the evidence of where the reads go (the bytes each replica node
  sends, read each second). `design.md`, Lab, docs, risks and ADRs.
- **A slow link at the target's own node** (R1-C12) is stated in the Terms, S12 and S15, with
  the RF 2 and RF 3 unit cases.
- **A member's own acknowledgement time** (R1-S5): named below, with its bound and its unit
  test. Not accepted here.
- **The fault's removal is armed on the node** (R1-S8): `chaos.sh slow on DELAY SECONDS` arms
  a transient timer before it adds netem; the check removes the netem qdisc and restores
  nothing. `design.md`, Removing the fault.
- **Nits** fixed, each a wording or a reference: N1 (`target.rs:730–732`), N2 (the intent's
  "logs only session failures"), N3 (spaces before numbers in the design), and the security
  review's Nit (a queued read has no peer yet).

### Rules narrowed, because a finding showed they could not hold as stated

- **S13** (R1-C1, R1-S4). A report on a channel of its own contradicts #71's S3. So there is no
  report until #71's socket exists, nor on a node without telemetry (off, or under #74's
  `--privsep`); and a target with more than 32 nodes lists 32, with its totals, to stay
  within #71's 64 KiB.
- **S2** (R1-C2). "Counted before its request times out" holds in the windows that end before
  the member's removal commits, not after: by the code the write is completed and gone
  (`flaminio-repl/src/target.rs:2602–2605`).
- **S3** (R1-C3, R1-S3). A slow node that is under clears only once its hold has passed. The
  first time that is one window, as approved; a node that turns slow again within the cap's
  length waits longer. The intent's "it does not flap" could not hold otherwise.

### Risks named, not accepted

- **A member sets its own acknowledgement time** (R1-S5). A member that acknowledges early (a
  compromised replica node, a device that lies about its flush) makes every honest member
  above the floor behind. With two replicas that ends the target's fast reads of the shared
  chunks and sends their reads to that member, which is never judged on reads (Q3). Bounds:
  the chunks it holds a usable replica of (ARCHITECTURE §15.1); a member under the floor
  (5 ms) cannot be made slow, and the unit test shows it; a read it fails goes elsewhere
  (docs/issues/5). The reviewer's alternative, if this is not accepted: do not demote the
  local replica on the evidence of one faster peer.
- **Time is measured in windows** (R1-S2). A node late for half a window or less is not over
  by time, and a stall across two windows is split between them. `late_us` is a lower bound
  when late writes are acknowledged out of order.
- **A member removed within the window leaves no sample** (R1-C2). It is no usable member
  then, so no read is preferred to it; its judgement is what it was.
- **Changes are bounded, not excluded** (R1-C3, R1-S3). Before its hold reaches the cap a node
  can change up to 11 times in 64 windows; at the cap, twice per 64 windows (5 min 20 s). A
  node that recovered for good stays slow for the rest of its hold: reads prefer the others
  for up to 5 min 20 s longer. While it is held, if the chunk's other usable members turn
  slow, every usable member is slow and reads return to today's order, which may be the node
  now slow, for the rest of the hold (up to 5 min 20 s; never worse than today) (R2, Nit 3).
- **No report without #71's socket** (R1-C1, R1-S4): the log is what an operator has. A
  schema-1 observer refuses a schema-2 line, so an observer upgraded after its daemon reports
  the node not fresh until it is.
- **A pause shorter than a window looks like slowness** (R1-S6). The writes in flight across
  it are late by its length. It can move reads for a hold; it changes no membership.
- **The target's own slow link is judged as its peers'** (R1-C12): with two replicas and a
  local one, the log and the report name the healthy remote node. Reads are not affected: the
  local replica is preferred anyway. The later removal step must not inherit it.
- **A window can close late** (R1-C7): by up to 10 ms on an attached node, which turns every
  10 ms. Ages are still taken at the window's end.
- **The lab fault can outlive a killed check** (R1-S8) by the margin, 30 s. `make chaos` keeps
  its own trap and the same gap; a follow-up, to be filed.

## Plan review round 2 (2026-11-17): changes made after approval

Findings of the review of commit c4a3290 (one reviewer, design-code and design-security).
No rule is narrowed and nothing is accepted.

- **The source check covers every caller of `source`** (R2-B1): the resync's digest and its
  `Mapped` probe as well as queued reads and fast admission, with a failure line of their
  own. S7 says they follow the same choice. The check counts only while the judgement is on,
  so the existing checks fail as they do today. Of the two fixes offered (the verdict on
  what the preference returns, or naming the two sites out of the rule), the first, as
  recommended. `spec.md`, S7 and Tests; `design.md`, The simulator.
- **One mutation check per path** (R2-B2): the queued-read check passes only on
  `read sent to an unusable member`, the fast-read check only on its line. Of the two fixes
  offered (its own line, or fast reads off), the line: the gray configuration stays one.
  `design.md`, The simulator.
- **Nits,** each a wording or a figure: the worst-case line is at most 46,075 bytes (Nit 1);
  one order, the target's `NodeId` with the slow nodes first, cuts at 32 and is kept by the
  encoder (Nit 2); the consequence of a held node while another turns slow is named above
  (Nit 3); the Terms say that late time is counted as a lower bound (Nit 4); `chaos.sh`
  names the timer's unit, stops an earlier timer before arming and refuses a SECONDS that is
  not decimal digits (Nit 5).

## Plan review round 3 (2026-11-17): changes made after approval

Findings of the review of commit c4e321c (one reviewer, design-code and design-security).
No rule is narrowed and nothing is accepted.

- **The resync's sites have a test** (R3-L1): the injected bug reaches the digest and the
  probe only on a chunk a campaign need not produce, so a target unit test builds that chunk
  by hand and asserts the count at both sites, with the bug on and off. Of the fixes offered
  (a third mutation check, the unit test, or dropping the line), the unit test: it is
  deterministic. `design.md`, The simulator.
- **The lab fault's timer is stated against the script** (R3-L2): SECONDS is `$4`, one to
  five decimal digits, refused with `exit 2` before the first `tc` call; an earlier timer is
  stopped and its failed state reset; the timer runs under `sudo` and its command ends in
  `|| true`. `design.md`, Removing the fault.

## Plan review round 4 (2026-11-17): changes made after approval

Findings of the review of commit fc5b072 (one reviewer, design-code and design-security).
No rule is narrowed and nothing is accepted.

- **One case per resync site** (R4-L1): the unit test has a digest case and a probe case,
  each asserting the resync's count alone, so neither site can be left unwired. The setup of
  each is named. `design.md`, The simulator.
- **No fault without its timer** (R4-L2): a failed `systemd-run` ends the script before
  `tc qdisc add`. The script test fakes `tc`, `systemctl` and `systemd-run` and covers the
  three refusals of SECONDS and the failed arming. `design.md`, Removing the fault and the
  S15 row.

## Plan review round 5 (2026-11-17): changes made after approval

Findings of the review of commit e998e2a (one reviewer, design-code and design-security).
No rule is narrowed and nothing is accepted.

- **The unit test's chunk is named** (R5-L1): the local node is not a replica; `replicas` is
  the member, a second outside replica, the ISR member, in that order, with a session to
  each. So the bug reaches the outside replica and the unmutated preference the ISR member.
  `design.md`, The simulator.
- **The steps of `chaos.sh slow` are one ordered list** (R5-L2): refusal, stop of the
  earlier timer, delete, arming, add. The delete now comes before the arming, so a failed
  arming leaves no earlier fault on. `slow on DELAY` with three arguments also stops a
  leftover timer first. `design.md`, Removing the fault.
- **The script test's fakes and cases** (R5-L3): `sudo` is faked too, and the cases gain a
  five-digit SECONDS, `slow on DELAY` with three arguments and `slow off`. `design.md`, the
  S15 row.
