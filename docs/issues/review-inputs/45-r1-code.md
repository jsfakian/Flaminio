# Plan review: issue #45, round 1, design-code

Scope: docs/issues/45/ (intent, spec, design, decisions, adr-proposal) in /home/jsfakian/Documents/src/flaminio-45.
The code facts the plan cites check out: service.go:154/159/221/239/827/866, domain.go:259/287,
drain.go:44-167, target.rs `member_failed`:1300, `source`:1644, `advance_chunk`:2195,
`drive_move`:2285, `drive_resync`:2308, node.rs:330, store.rs:743, replica/mod.rs:236, sim/mod.rs:1261,
kv.go `MaxTxnOps`=120, `MaxChangesPerRequest`=64. The findings below are about the cross-plan
items, mechanisms the plan leaves unspecified, and the contracts.

### R1-C1 — High
**The #13 hand-offs are missing, and the rebuild can stall on a released node.** #45 does not
exclude a chunk's released nodes from destinations. Its own choice is "#18's least-loaded legal
destination excluding the source/staying replicas" (design.md:99), and #18's `GetReleased()`
exclusion is not named. It treats every `Applied=false` as "a concurrent winner … reread next
pass" (design.md:101). S15 lacks #13's two reasons, and nothing declares a node failed for good
or calls `ForgetNode`.
Realistic case once #13 lands (release deletion is off by default): a chunk drained B→C keeps
B's release, and B is undrained. When A then fails, B is the least loaded candidate. #13's S2
refuses the move with `Applied=false` and a reason, and the rebuild picks B again on every pass.
The chunk is never re-protected and is never reported as blocked.
Adding `ForgetNode` also collides with two of #45's own rules:
- #13's `ForgetNode` writes `drain/<node>` (#13 design.md:162-171), but S9 and the design skip
  draining sources (spec.md:60-62; design.md:97). A forgotten node's replicas would then never
  be rebuilt.
- `ForgetNode` rewrites chunk `released` lists and header `fenced` lists. That breaks the
  invariant's "no … fence changes" (spec.md:23-27) and the new simulator check, "a declaration
  leaves every chunk record and epoch as it was" (spec.md:122-124).
Evidence: #13 decisions.md:230-234 and design.md:1281-1287; #13 spec S2 (:30-34), S16 (:110-112)
and S19 (:140-147); batch notes ("#45 closes it by forgetting…"). The #45 plan has no mention of
releases beyond spec.md:18 and :76.
Fix:
- **Destinations and refusals.** Use #18's `choose` with #13's order: a chunk at `MaxReleased`
  waits; eligible nodes exclude `GetReleased()`; with only releasing nodes left, the chunk waits.
  Classify `Applied=false` by #13's `reason` instead of treating it as a concurrent winner.
- **S15 reasons.** Add `holding 2 releases of chunk C` and `only nodes releasing it are left`.
- **Failed for good.** Add the declaration, for example a `SetNodeState` action and a
  `forget-node` CLI. It requires Failed and calls `ForgetNode`. Admit of a forgotten node is refused.
- **Fit S9 and the invariant.** S9 and the design treat a forgotten node's replicas as lost
  although it is marked draining. The invariant and its simulator check carve out the forget
  step, whose safety is #13's check.
- **Tests:** a rebuild beside a releasing node; each new reason; forget, then rebuild.

### R1-C2 — Medium
**#17's maintenance rules are deferred, not taken, and one is misstated.** The design says only
"Reask only under #17's conditional hold/epoch rules" and "use its eligible-node ranking
restricted to Active nodes" (design.md:103-107). #17's rewritten S17 counts no epochs. It is
missing:
- any maintenance record stands, whoever asked;
- ask only while a move is pending on the volume, not "after recording work" in this pass;
- never ask again a node that is alive and took an epoch;
- a record ended with a move pending is a failed start: ask elsewhere at once;
- the rebuild's own applied `AttachMaintenance` counts as a read and resets the sighting's time
  and epoch;
- `EndMaintenance` leaves the record unheld and naming the node, so "held" must be read from
  `held`, never from `target`;
- the handling of each `AttachMaintenance` answer (`applied=false`, `NotFound`,
  `FailedPrecondition`, `Unimplemented`).
The "restricted to Active" eligibility differs from #17's S16, under which a record stands.
For example, a drain's standing record on a Failed-but-alive node would be asked elsewhere by
the rebuild, deposing a started target. The intent and spec also call #17's unhold a
"release" (intent.md:125-126; spec.md:163). "Release" is #13's term, and the RPC is
`UnholdVolume`.
Evidence: #17 spec S15-S17 (:116-142); #17 design.md:414-462, :66-68, :92-108; batch notes (#17
round-1 fix and round 3).
Fix: state each rule above in the design's maintenance paragraph and give each a test in
`rebuild_test.go`. Say whether a standing record names a node by #17's S16 or adds "Active", and
make the two agree. Rename "release a hold" to "unhold" (`UnholdVolume`).

### R1-C3 — Medium
**S15's "no destination" is not #18's S5 refusal, and the choice drops #18's rules.** S15 still
says "under `strict` every one collides" (spec.md:86). #18 asks for "every one is refused by
#18's S5", with `domain.Strict`'s reason (e.g. `replica "x" has no zone label`). The design's
"least-loaded legal destination" (design.md:99) omits the `bestEffort` fall-back by level. It
also does not say how a refused proposal is handled. #18's S14 has two cases:
- **A collision** (`FailedPrecondition` with the `failure domain collision: ` prefix) blocks the
  chunk for the pass, and the destination stays a candidate.
- **Any other `FailedPrecondition` or `ResourceExhausted`** takes the destination out of the
  pass, and `choose` runs again.
#45's own S7 refusal (a destination failed after the pass read the nodes) is such a refusal.
If the rebuild fails the pass on these errors, as `drain.stepVolume` does today on any error
(drain.go:135-137), one refusal stops every other chunk, against S15.
Evidence: #18 design.md:199-203, :131-175; #18 spec S5 (:56-72), S10 (:98-104), S14 (:127-134).
Fix: S11 and the design name #18's `choose` (strict and bestEffort) as the destination function.
S15 uses #18's wording and reason. The design states S14's two refusal classes, with tests for
a collision refusal and for a destination failed mid-pass.

### R1-C4 — Medium
**#72's refusals are not skipped, and the #72 text is stale.** design.md:22 says "No #72 draft
exists at this writing", but #72's plan exists. It has `MoveReplica` charge the destination and
refuse with `RESOURCE_EXHAUSTED` or `FAILED_PRECONDITION` (#72 spec.md:58, :96-99), and a drain
skips such a destination (#72 S8, spec.md:123; test :179). The design instead says the
controller "must call its one conditional reservation ledger" (design.md:119-121). That is the
wrong mechanism: `MoveReplica` charges, and the controller only has to skip a refused destination.
Without the skip, the rebuild's least-loaded-by-count choice can propose the same full node on
every pass, and the chunk is never re-protected.
Fix: replace design.md:22 and :119-121 with: "`MoveReplica` charges through #72's ledger. A
destination refused with `RESOURCE_EXHAUSTED` or `FAILED_PRECONDITION` leaves the pass's
candidates, and `choose` runs again; with none left, the chunk is blocked with the last refusal
as its reason (#72 S8, #18 S14)." Add a test: a full destination is skipped, and the rebuild
fails over to another.

### R1-C5 — Medium
**Who records Missing and timeout Failed, and what `enabled` turns on, is unspecified.** The
design gives the compares of each transition (design.md:44-49) and says "Observer transitions
are private service operations" (:143-144). It never says which component scans the nodes and
commits them: the `rebuild-observer` owner, or every instance's 5 s rebuild loop. It also leaves
open:
- whether Missing is recorded while the policy is off;
- whether the observer and the credit clock run while the policy is off;
- who creates `rebuild-clock` first.
The one `enabled` flag is described as switching on the loop (:91) and as "timeout failure"
(:140). S1-S4 say every data node has a state and the first observer records Missing, which is
not tied to the policy. decisions.md:51-55 enables timeout failure "once the credit clock passes
its implementation review", with no code switch named.
Fix: name the actor and cadence for Missing and for timeout Failed. Say what `enabled` gates
(the moves, the timeout, or both), whether Missing is recorded regardless, and when the
observer runs and initializes the clock. Add tests for "policy off: Missing recorded, never
Failed by timeout" (or the chosen rule).

### R1-C6 — Medium
**The new protobuf enums fail `make lint` (buf STANDARD), and an unset action means FAIL.**
`NodeState {UNSPECIFIED=0; ACTIVE=1; MISSING=2; FAILED=3;}` and `enum action {FAIL=0; ADMIT=1;}`
(design.md:129, :133) break three buf STANDARD rules (buf.yaml: `lint: use: STANDARD`):
ENUM_VALUE_PREFIX, ENUM_ZERO_VALUE_SUFFIX and ENUM_PASCAL_CASE. With `FAIL=0`, a request that
omits `action` (proto3's zero value) is a failure declaration, and "reject unknown actions"
cannot catch it.
Fix: `enum NodeState { NODE_STATE_UNSPECIFIED = 0; NODE_STATE_ACTIVE = 1; NODE_STATE_MISSING = 2;
NODE_STATE_FAILED = 3; }` and `enum NodeStateAction { NODE_STATE_ACTION_UNSPECIFIED = 0;
NODE_STATE_ACTION_FAIL = 1; NODE_STATE_ACTION_ADMIT = 2; }`. Refuse UNSPECIFIED with
`InvalidArgument` and test it.

### R1-C7 — Low
**The design depends on #78's placement gate without declaring it, and contradicts #78's rule.**
The design fences a fail against an in-flight allocation through #78's gate (design.md:50-51,
82-85). But the intent and spec assume only #13, #17, #15 and #18 (intent.md:7; spec.md:4), and
no compare is given for the case where #78 has not landed. Without one, a `Fail` committing
between `MoveReplica`'s state read and its commit records a move to a failed node, against S7's
"refused, for every caller". The design also bumps the gate on every Missing↔Active change,
heartbeat recoveries included. #78 says "Heartbeats do not bump it" (#78 design.md:26), and
Missing gates no admission: Create uses `alive`, and Move refuses only Failed. Every outage then
costs 2N gate bumps, each forcing in-flight creations and moves to retry.
Fix: list #78 as a dependency, or compare the destination's `node-state/<to>` revision until #78
lands. Bump the gate only on changes into or out of Failed.

### R1-C8 — Low
**Timeout Failed does not compare the policy.** It "compares the same evidence, Missing state
revision and sufficient credited wait" (design.md:46). It does not compare the policy key, whose
`wait_seconds` and `enabled` decide "sufficient". A failure computed under a 5 s wait, or with
the policy on, can commit after an administrator has raised the wait or turned it off.
Fix: compare the policy key's revision in the timeout transaction, and test a policy change
racing it.

### R1-C9 — Low
**The mixed-version test cannot prove S19.** "Old-server UNSPECIFIED response prevents enabling
mixed-version controller" (design.md:155). A CLI that reaches a new instance enables the policy
while older instances still place on failed nodes. The in-process loop sees only its own
instance. The test would pass with the rule unenforced.
Fix: state S19 as the operating rule it is (decisions, `docs/lab.md`), or add a per-instance
version record that enabling compares. Drop the claim otherwise.

### R1-C10 — Low
**No Go test proves the invariant for the service's writes.** The simulator check and the
vectors cover the Rust model and the pure Go transitions (design.md:157). Nothing tests that
`SetNodeState`, the heartbeat, Missing and timeout Failed write only `node-state/`, `alive/` and
the gate. A service transition that rewrote a chunk or header key would pass every listed test.
Fix: add a service test, on Mem and on etcd, asserting that every `volumes/` and `chunks/` key
keeps its revision across each transition.

### R1-C11 — Low
**The probe that earns credit is undefined.** "Consecutive successful readiness/quorum probes"
(design.md:62) does not say what is called or over which path: an etcd quorum read, or the TLS
gRPC listener that nodes use. S5's settled authority is that credit (decisions.md:51-58), so its
tests cannot be written to fail until the probe is defined.
Fix: name the probe, for example a linearizable etcd `Get` plus a loopback TLS `ListNodes` on the
serving listener, and its success condition.

### R1-C12 — Low
**Small clusters are not considered (ADR 0006, accepted).** On a 2-node edge site with RF 2 and
a witness, or on any cluster where RF equals the number of data nodes, a node down past the
wait is Failed. Its rebuild has no destination, and once back it stays Failed (S6/S7), so every
RF-N creation is refused until an administrator admits it. ADR 0006's premise is unattended
sites. The plan names partitions (spec Q2) but not this case.
Fix: name it in the ADR proposal's Consequences and in `docs/lab.md` and the runbook, and add a
service test (two data nodes, one Failed and back: creation refused, Admit restores it).

### R1-C13 — Low
**The lab plan is not runnable as written.** It names files and "Makefile" (design.md:162) but
gives:
- no `make` target or `lab.sh` command;
- no command that shortens the wait (`rebuild-policy 5 on`) or restores it;
- no command for the live declaration (`fail-node`) or for Admit.
Fix: name the target (e.g. `make rebuild-test`) and its `lab.sh` command, with each step's
`metactl` call, and its entry in the `make help` text and `docs/lab.md`.

## Nits

- design.md:75, :140, :170 and :186-188, and adr-proposal.md:32-36 and :71-73, still say that
  timeout failure is "blocked" or "disabled" pending S5, and that Q1-Q8 "remain unapproved".
  decisions.md settled both. Rewrite those lines to the settled rule rather than leaving
  "Settled" notes appended beside contradicting text.

Reviewed commit: 9acc14576c7414a20a55df1b5b1247dcc465340c
