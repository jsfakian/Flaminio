# Issue #47 — plan review, round 1, focus: design-security

Scope: docs/issues/47/{intent,spec,design,decisions}.md at HEAD, checked against the code in
/home/jsfakian/Documents/src/flaminio-47 and the review-clear plans #71 and #83 on origin/main.

The plan removes nothing from the ISR and sends no report between nodes or to the metadata
service, so the brief's ejection, epoch, fencing and auth.go questions have no surface here.
The findings are about what the judgement can be made to say, what the simulator can reach,
and the report's boundary.

### R1-S1 — Medium — The simulator cannot make the local replica slow, so S9 and S10 have no simulator coverage

The design splits the fast-read completion check from `source` (design.md "Source selection",
lines 71–74) and makes fast admission depend on the judgement. That is the one change to a
safety check in this plan. Intent (Out of scope, lines 100–101) and spec Q2 (line 148) say a
slow local replica "is checked in the simulator". It cannot be: the simulator delivers every
message with `from == to` in 5–50 us, before it looks at `slow_links`
(dataplane/crates/flaminio-repl/src/sim/mod.rs:658–662), and a slow link is always between two
different nodes (sim/mod.rs:1168–1177, `others` excludes `a`). The local node can never be
behind there, so the campaign never refuses a fast read for a slow local node, never completes
one across a judgement change, and the injected bug is only ever exercised on remote redirects.
The design's Invariant row (line 103) adds no local fault.

Fix: add a local-delay fault to the simulator (a per-node delay applied when `from == to`),
report the counts of fast reads refused for a slow local node and of fast reads completed
across a judgement change, both required above zero, and state that the injected bug is caught
on that path too. Otherwise correct intent and spec to say the simulator does not cover S9/S10.

### R1-S2 — Medium — A member that stalls is under-counted: the share is of samples, not of time

"Over" is "behind on more than the upper share of its samples" (spec Terms), and an outstanding
write counts once per window (spec S2; design lines 41–44). A member that stalls blocks the
queue, so it produces few samples exactly while it is slowest. Example with the defaults: a
member frozen 2.5 s of every 5 s window, queue depth 32, 2,000 writes/s when it answers: about
32 behind samples against about 5,000 good ones, under 1%. That is below the lower share (10%),
so the node is not judged slow, and a slow judgement is cleared. Reads keep going to it.

S1 adds to it: "a write sent again on a new session counts from its last send"
(`retransmit` → `send_write`, target.rs:1332–1376, stamps the new send). A write that waited
3 s for a timeout and was then acknowledged in 1 ms is a fast sample. A node, sick or
deliberate, clears its record by dropping its session.

Fix: weigh by time as well as by count (for instance: over when the time writes waited on this
member alone exceeds a share of the window), and count a write resent after a failed session
from its first send, or count the failed leg as a behind sample at its age. If the count rule
is kept, record the limit in decisions.md and add a unit test that shows what a periodic stall
yields.

### R1-S3 — Medium — No bound on flapping when the node's latency depends on the reads moved

S3's hysteresis covers noise and a fault of fixed length; the tests are those two cases
(spec Tests, lines 106–110; design row S3–S5). Moving reads off a node lowers its load, so its
write times drop, it falls under the lower share, the reads return, and it is over again. On a
saturated disk that is a change every window or two, for as long as the load lasts: every read
of the node's chunks changes source, and fast reads of local chunks switch off and on, every
5–10 s. The only bound is one change per window. Intent 3 and the issue's "it does not flap"
are not met in that case, and no test or simulator run has a latency that depends on where
reads go.

Fix: a rule that bounds repeated changes (a minimum time slow before it may clear, growing
when a node becomes slow again within some windows of clearing, with a cap), with a unit test
on a load-dependent latency. Or record it in decisions.md as accepted, with the bound stated.

### R1-S4 — Medium — `stats` on the root control socket contradicts #71's telemetry boundary and leaves its bounds unstated

#71 is review-clear. Its S3: "Observations leave the daemon only through a telemetry socket
that answers `observe` and refuses every other line", in a directory that is not the control
socket's; its contracts say "`control.rs` is unchanged" and its intent lists gray-failure
metrics "as #45–#47 land". Design.md lines 85–91 instead add `stats [VOLUME]` to the root
socket with a second envelope (`version:1`, against #71's `schema:1`), and spec S13 defers
the shape to "whichever of #62 and this lands first", without naming #71. Reading a latency
then needs the socket that also attaches, detaches and deletes
(dataplane/crates/flaminio-io/src/control.rs:83–103), which #71 built a non-root reader to
avoid. #83's `clock-status` is a precedent for a root-socket state line, not for histograms.

The command's own bounds are also missing where #71 states them:
- no limit on the wait for the engine's snapshot (attach, delete and detach each have one,
  control.rs:33–37, 205; #71: 2 s) and no limit on how often a snapshot is built (#71: one a
  second, one pending);
- a 4 MiB reply for one attached volume ("one volume at a time", control.rs:95), against
  #71's 64 KiB;
- "names use validated IDs": the data plane has no id check; volume and node names are taken
  from the control line and the metadata service's reply as they come
  (flaminio-node/src/engine.rs:371, 1623–1630; the grammar lives only in
  meta/domain/domain.go:87). #71 checks its `ID` grammar in the daemon and answers
  `error: invalid id`.

Fix: carry the per-node latency in #71's `observe` snapshot as a schema change agreed with
#71's contract, or state in decisions.md why this report belongs on the root socket and who
may read it. Either way state the engine-wait limit, the snapshot rate, a reply bound that
follows from the volume's node count, the id grammar and the answer when an id fails it, each
with a test.

### R1-S5 — Low — A member's acknowledgement time is its own to set, and it decides where reads go

The comparison is relative (spec Terms, "behind"), so a member that acknowledges early (a
compromised replica node, or a device that lies about its flush) makes every honest member
above the 5 ms floor behind. With two replicas that switches off the target's fast reads for
the shared chunks (S9) and sends every read to that member. It is never judged on reads (Q3),
so it can then answer each read just under the 3 s timeout. Today the source is fixed by
membership and node id; ARCHITECTURE §15.1 limits a compromised node to the volumes it hosts
replicas of, and this widens what it serves within them. The design does not say who controls
this input or what bounds it.

Fix: state it in decisions.md as an accepted risk with its bound (the floor: a member under it
cannot be made slow), and add the unit test: a peer acknowledging in 0 us does not make a
member at the floor slow. If not accepted, do not demote the local replica on the evidence of
one faster peer.

### R1-S6 — Low — The design declines #83's clock, which is a review-clear plan

Design.md lines 23–24: "#83's elapsed-clock proposal is separate and not accepted by this
design." #83 is approved and clear; its S2 puts send timestamps, tick and completion on one
suspend-aware clock and its design replaces the engine's `Instant`
(flaminio-node/src/engine.rs:604–606). Samples and windows will run on that clock, and a
suspend then adds its length to the writes in flight for the members not yet acknowledged.

Fix: say the measurement uses the clock the target is driven with, #83's included; that the
judgement ends with the target when #83 quarantines the daemon; and add the unit case: a jump
of the clock with writes in flight changes no judgement (it is within "large timer gaps",
lines 52–53, but not among the tests).

### R1-S7 — Low — What decisions.md settled is not carried into the spec, and the design still calls it open

- decisions.md (the open sample boundary) says S1 and S2 "are to be clarified" to exclude a
  member removed by a committed shrink, and the case tested. Spec S1 and S2 are unchanged, and
  spec Tests does not name the case. S2 as written counts such a member at its age.
- Design.md lines 124–126: "Q1–Q6 and thresholds remain unapproved" and "censored cancelled
  samples are an explicit spec/design review blocker", four lines above "Settled at approval"
  and against decisions.md.

Fix: add the exclusion to S1/S2 and its test to spec Tests; remove the two stale sentences.

### R1-S8 — Low — The lab check's clean-up depends on the host's trap alone

Design.md lines 119–121: "trap restores original qdisc". A trap does not run when the host
script is killed with SIGKILL or the workstation loses the VM; the netem then stays on a
shared lab node for good, as it does for `make chaos` (scripts/lab.sh:1428–1430). And there
is no original to restore: `chaos.sh slow on` deletes the root qdisc of wg0 before adding
netem, and `slow off` deletes it again (scripts/guest/chaos.sh:7–9).

Fix: arm the removal on the node when the fault starts (a transient timer there that deletes
the qdisc after the fault's length plus a margin), keep the trap as the fast path, and say
"removes the netem qdisc" rather than "restores the original". Add the case to
`gray-failure-test.sh`: the check killed during the fault leaves no qdisc after the margin.

## Nits

- Design.md line 75: "Queued/sent reads keep their selected peer". A queued read has no peer:
  the source is chosen when the read starts (`try_start`, target.rs:1737). Read literally it
  asks for a peer chosen at enqueue, which could be unusable by the start. Say "reads already
  sent keep their peer; a queued read is chosen at its start", as spec S10 does.

Reviewed commit: c60ba6abede9b06b73712a741a82a4ae688774ff
