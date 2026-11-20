# Plan review: issue #45, round 2 (incremental), design-code and design-security

Scope: `git diff 9acc145..f333d49 -- docs/issues/45/ ':!docs/issues/45/report.md'`, checked
against the code in /home/jsfakian/Documents/src/flaminio-45 and the plans of #13, #17, #18, #72,
#78 and #82.

## Earlier findings

- R1-C1 (High): fixed. #18's `choose` with #13's order, both reasons, `Applied=false` sorted by
  `reason`, FORGET, S9 whatever the drain mark, the invariant's carve-out. Two gaps in FORGET's
  gate are new: R2-B1 and R2-S1.
- R1-C2: fixed. Dropping "also active" opens R2-C1.
- R1-C3: fixed.
- R1-C4: fixed.
- R1-C5: fixed.
- R1-C6: fixed. The enums pass buf STANDARD, and UNSPECIFIED gives `InvalidArgument`.
- R1-C7: fixed. The gate is bumped into and out of Failed only, as #78 says.
- R1-C8: fixed.
- R1-C9: fixed.
- R1-C10: fixed.
- R1-C11: fixed: the probe is now defined. Whether it is sound is R2-B2.
- R1-C12: fixed.
- R1-C13: fixed. One check in it is wrong: R2-C2.
- Code Nit (stale "blocked"/"unapproved" text): fixed.
- R1-S1: fixed.
- R1-S2: fixed.
- R1-S3: fixed. The owner compare in `AttachMaintenance` does fence a late owner.
- R1-S4: fixed for the case it named. One case is left: R2-S2.
- R1-S5: fixed. The heartbeat writes #72's report or `1`, and #72 already limits reports to `io`.
- R1-S6: fixed.
- R1-S7: fixed.
- R1-S8: fixed (agent refused; administrator or the node's `io` only).
- R1-S9: fixed.
- R1-S10: fixed.
- R1-S11: fixed.
- R1-S12: fixed.
- Security Nit 1 (#14's confirmation): fixed.
- Security Nit 2 (fixed prefix): fixed (`node failed: `).

### R2-B1 — Medium
**#13's own `ForgetNode` RPC and `metactl forget` still forget a node that is not Failed, so S4's
new refusal can be bypassed.** FORGET is a `SetNodeState` action that checks Failed and compares the
state in #13's mark transaction (design.md:69-74, :258; spec S4, spec.md:50-51: "a node not failed
is not declared failed for good"). #13 still ships `rpc ForgetNode` and `metactl forget NODE`
(#13 design.md:892-895, :265, :1237). Neither checks state or compares `node-state/`. The plan
neither removes them nor gates them. decisions.md:86-87 even rejects "a separate RPC" because
it would duplicate checks, yet #13's separate RPC stays.

What goes wrong: an administrator can forget an Active or Missing node through #13's path. The node
is then drain-marked and forgotten, but S9 asks no move for an active or missing node. If it is
alive, it never becomes Failed, so the rebuild never re-protects it. And an Admit that commits
first is not caught by any compare.

**Fix:** state that FORGET is the only way to forget a node once #45 lands. Either remove #13's RPC
and its `forget` CLI, or give `ForgetNode` the same Failed check and `node-state/` compare.
Add a test: `ForgetNode` of an Active node is refused.

### R2-S1 — Medium
**FORGET accepts a Failed node that is alive right now, or one that is only partitioned.
Forgetting it cannot be undone.** FORGET checks only Failed at `expected_state_revision`
(design.md:69-72). A Failed node may be heartbeating: Failed is sticky (S6, spec.md:57-58). Timeout
Failed is also exactly what a partition between the node and the metadata service produces. #82's
invariant says a live partition alone authorizes no node-wide action (#82 spec.md:22-26). #13
retires the id for good and never forgets a live node: "A live node is not forgotten" (#13
design.md:1270-1272; spec.md:102, :147).

Today the only guard is the runbook's "a failed node gone for good is declared so once
re-protected" (design.md:319-320). A partitioned node meets "re-protected" too. If such a node is
forgotten, it can no longer register, its released copies are never confirmed or deleted, and its
id is lost.

**Fix:**
- FORGET's mark transaction also compares `alive/<id>` absent. Refuse otherwise with
  `FailedPrecondition`, `node N is alive`.
- The runbook and `docs/lab.md` say that Failed, timeout or declared, is no evidence that the
  node is gone. A partition gives the same Failed (#82). Forget only on evidence outside the
  cluster: powered off, disk destroyed.
- Add a test: FORGET of a Failed node that heartbeats is refused.

### R2-C1 — Medium
**A new maintenance attach may pick the failed node itself, and #17 S17 then pins the copy
there.** Round 1 dropped "also active" for every maintenance node, not only for standing records
(decisions.md:107-110; spec S12, spec.md:93-94; design.md:192-194). #17 S16 allows any alive node
that holds a replica. So a Failed node that is alive is a candidate: one declared failed while it
runs (S4), or one back but not admitted (S6). It holds as many of the volume's chunks as any other
holder, and ties are broken by name. A target reads from its local replica first
(`flaminio-repl/src/target.rs:1649-1651`). So the copy reads the disk of the node the
administrator declared failed.

That is the plan's own path for a dead disk (intent.md:125: "A node that heartbeats on a dead disk
is failed by an administrator"). Once that node takes an epoch, #17 S17 never asks again
(design.md:202-203).

**Fix:** keep #17 S16 for a standing record, so a started target is not deposed. For a new ask,
choose among S16's nodes that are also Active, and with none, report `no maintenance node`. Add a
test: a Failed, alive node holding the most chunks is not chosen for a new ask, while a standing
record on it stands.

### R2-B2 — Medium
**The probe shows neither that a heartbeat could commit nor that a node could connect, against
S5.** The probe is a linearizable etcd `Get`, "in a process whose gRPC server is serving"
(design.md:100-103). Two common outages pass it while no heartbeat can arrive:
- **etcd's NOSPACE alarm.** etcd then accepts reads and deletes only. The heartbeat is a
  transaction with a put, so it is refused, while the `Get` succeeds. The design does not say that
  a failed clock checkpoint drops its interval.
- **An expired server certificate.** It is loaded once at start (`meta/cmd/flaminio-meta/main.go:116`
  `ServerConfig`, `pkg/pki/pki.go:285`), and ARCHITECTURE plans 24 h certificates. Once it expires,
  every node's handshake fails, but `Serve` has not returned.

In both cases, nodes whose leases lapse go Missing, then Failed. That is S5's "an outage of the
metadata service fails no node" broken. The cost is copies, plus admitting every node afterwards.
decisions.md:165-168 justifies dropping the TLS probe with "gives the same evidence", which
is not true for the certificate case.

**Fix:** credit an interval only when that tick's checkpoint, a conditional write, committed
within the bound. Count no interval while the serving certificate is outside its validity. Add
tests for a refused checkpoint and an expired serving certificate, each adding zero.

### R2-S2 — Low
**S12 still attaches maintenance targets that can never end, which R1-S4 meant to stop.** S12
and the design exclude only a pending move "at the minimum by S15's second case" (spec.md:91-94;
design.md:177-182, :188-189). Under #17, a target has work while any move of the volume is
pending (#17 spec "Work"), and it never ends with work left (#17 S11). Two cases remain:
- A volume with one carryable move and another chunk's move below the minimum on alive nodes. For
  example, RF 3/min 2: the rebuild asked all of X's moves, then Y failed before the target
  started.
- A pending move whose destination is lost (S15 "destination lost"; no move is withdrawn).

Each keeps one of the node's 2 slots for good (#17 S14) after its useful work. The Risks line
(design.md:333-335) names only a target "already started".

**Fix:** either ask no attach for a volume with any pending move that cannot be carried out
(below the minimum on alive nodes, or destination lost), or extend the Risks line and S15's report
to these cases. Add a test either way.

### R2-C2 — Low
**Under #13, the lab's "holds nothing" check fails, and enabling at 5 s fails any node already
missing.**
- Design step 3, "fl-b holds nothing of either volume" (design.md:300-301), and the spec, "it holds
  nothing (#13)" (spec.md:176-177), are false under #13. A node that confirms a release becomes a
  fence holder and keeps a fence-only slot once it has no chunk (#13 spec.md:10-11, S3). #13's own
  lab expects "leaves fl-a only its fence-only slot".
- Step 1 runs `rebuild-policy 5 on` (design.md:293) without first requiring every data node to be
  Active and alive. The observer records Missing even with the policy off (design.md:56-58, :97-98).
  So a stopped fl-c, or a stale registration, fails at once and starts copies or removes the
  destination.

**Fix:** check "no release, and only its fence-only slot of each volume" after the sweep. Before
step 1, assert that every data node is Active and alive in `metactl nodes`, and fail the run
otherwise.

## Nits
- design.md:164: "#13's forgotten node" is listed as a `FailedPrecondition` refusal of
  `MoveReplica`. #13 answers `Applied=false` with `reason` `node T is forgotten` (#13
  design.md:1004), and with #78, the `FailedPrecondition` is #78's `is draining`.
- design.md:173 against spec S11 (spec.md:86, "blocks the chunk for the pass") and design.md:167-168
  ("asked again at a later pass"): say which answers start the 30 s back-off (a collision? a
  `reason`-carrying `Applied=false`?), so that S11's "for the pass" and the 30 s agree.
- design.md:118-120: "the commit compares the gate and the revisions read" can be read as one
  `node-state/` compare per candidate. In the creation claim, with #72's 116 placement nodes, that
  goes past 120 (#13 counts 119). Say that no transaction compares `node-state/` and the gate
  covers it.
- design.md:206-207: "the ownership compare failing, this instance asks nothing more": for the
  rest of the step, or until it takes ownership again? As written, an owner that loses and
  regains the key would stop for good.

Reviewed commit: ba9d1d5d9e81a68f9971b7407627bad433291e07
