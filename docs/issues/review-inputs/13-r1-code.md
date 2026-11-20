# #13 plan review, round 1, focus: design-code

Scope: `docs/issues/13/` at HEAD (intent, spec, design, decisions; report.md for context only),
checked against the code in `/home/jsfakian/Documents/src/flaminio-13`. The code claims in the
design's "facts" (domain.go, service.go, auth.go, main.go, node.rs, metaclient, control.rs,
engine.rs, replica/mod.rs, replica.rs, target.rs, frame.rs, net.rs, store.rs, state.rs,
superblock.rs, sim/mod.rs, sim/tests.rs, lab.sh, drain.go, driver.yaml) hold at the cited
lines. Tag 71, journal record 9, chunk state 4 and incompat bit 2 are free in the code and in
the other plans. The findings are one simulator gap and five contracts shared with approved
plans.

### R1-C1 — High
**Summary.** S2 (no move back while released) is a data-loss safety rule. It has no simulator
check and no injected bug: in the simulator the drop-to-deletion window that S2 guards never
exists.
**Evidence.** spec.md:28-29 (S2); spec.md:86-87 (S14: without S2 "at RF 1, the copy deleted may
be the only one"). The simulator deletes the source's copy and confirms its release in the same
step as the drop's commit: design.md:457 and 486-488, today `sim/mod.rs:769-794` (the `MetaReq`
arm). So `released` is always empty whenever a simulated move runs, and the model's refusal
(design.md:108) is dead code in every run. The bug table (design.md:499-507) has nothing for S2
or for S7's read-then-delete. `TestNoMoveBackWhileReleased` and `TestReleaseAndMoveRace`
(design.md:669-672) are Go unit and race tests, not a simulator check. AGENTS.md Principles: "A
new safety rule gets a simulator check and an injected bug that the simulator must catch".
decisions.md:251-253 says what the simulator does at the drop. It does not exempt S2.
**Fix.** In the simulator, split the source's release deletion into S7's two steps: a read of the
model, then a deletion 0–5 s later from `relay_rng`. The deletion runs only if the node's fence is
at or below the epoch read; it then calls `confirm_release`. Add an injected bug in the model,
`MoveReplica` ignoring `released`, and a mutation test or a scripted RF 1 scenario that the
simulator must catch: move A→B; move back B→A, resynced and joined, between A's read and A's
deletion; drop B.

### R1-C2 — Medium
**Summary.** #13 does not yet carry the change to `ConfirmRelease` that #72 needs: 118 chunks
and the capacity ledger. #72 is approved and lands only with or after #13.
**Evidence.** #72 spec.md:137-150 (S11) and capacity-contracts.md:37: 1–118 chunks, the sweep
batching 118, and the transaction also comparing and writing `/flaminio/v1/capacity/<node>`
by #72's release rule. A failed compare with every chunk still at its listed revision is
retried. #13 still says 119 and has no ledger: design.md:72 ("1 to 119 chunks"), 76-81 (the
transaction compares the header and the listed chunks only; it retries only for a moved
header), 142 ("<= 119 chunks"), 628 ("0 or over 119"), 674 ("120 chunks …
`InvalidArgument`"), decisions.md:248 ("at most 119"). No engine test covers a confirmation in
more than one batch (design.md:688-697).
With #72's ledger compare added to a 119-chunk batch, the transaction has 121 compares.
`kv.MaxTxnOps` is 120 (`meta/kv/kv.go:33`), and `etcd.go:91` refuses larger transactions
("transaction too large"). Every full batch then fails, so its releases are never confirmed:
drains wait and moves back stay refused.
**Fix.** Apply #72's S11 in these places:
- 1–118 chunks, and the sweep confirms in batches of 118;
- the transaction compares the node's ledger (revision 0 if absent) and writes it by #72's
  release rule: n × chunk size and n slots, less one slot when it adds the node to `Fenced`;
- step 5 retries when only the header or the ledger moved;
- in the tests, 119 refused and 118 accepted on etcd, and an engine test confirming 119
  released chunks in two batches.

### R1-C3 — Medium
**Summary.** `Volume.fenced = 9` collides with two other approved plans, and the plans around
it have taken the neighbouring tags.
**Evidence.** #13 design.md:615 (`repeated string fenced = 9`). #18 design.md:115
(`string failure_domain = 9; string failure_domain_policy = 10`). #17 design.md:186
(`bool held = 9; bool maintenance = 10`). Also claimed on `Volume`: 11–14 by #70 (#75
design.md:175), 20–21 by #50 (contracts.md:38), 30–31 by #51 (contracts.md:10), 40 by #75.
#13's other numbers are free: `ChunkMeta.released = 6` (#61 takes `ChunkMeta` 20–21), the new
messages, and frame tag 71. #72's tag 72 is on `Node` and `NodeHeartbeatRequest`, which #13
does not touch. #75's 40 is not used by #13.
**Fix.** Give `fenced` a `Volume` tag nobody claims (for example 15), in design.md:615 and in
decisions. Tell #17 and #18, which also share 9 and 10 with each other.

### R1-C4 — Medium
**Summary.** S9 (a Hello at the fence creates nothing) conflicts with #86's required-row rule,
which #86 (approved) leaves for #13 to define. #13 does not define it.
**Evidence.**
- #86's rule: every chunk named in a Hello must have a row in `Fenced`; a missing row is "an
  invalid Fenced reply", and the session does not open (#86 spec.md:80-82, design.md:153-161).
  #86's simulator `Replica` creates every listed chunk it lacks (design.md:148-152).
- #86 defers to #13 "how `NotHeld` … meets the required-row rule" (design.md:138-141,
  decisions.md:55-57).
- #13 spec.md:64-66 and design.md:231-235: at the fence nothing is created, so the reply has no
  row for a named chunk that was deleted. design.md:446-448 (the simulator) and 509-519
  (`scenario_reconnect_after_deletion` expects `NotHeld` on the reconnected session).
- Both plans redefine `Request::Hello`: #86 as `{ chunks }`, #13 as `{ chunks, shape }`.

With both plans as written, a stale target that reconnects at its epoch to a source that has
deleted a named chunk fails the session at the Hello. It never sends the request that S17/S18
answer with `NotHeld`, and it loses every other chunk on that node until it reaches the
metadata service. That is the cut-off proposer case S16 is meant to close.
**Fix.** In #13, define four things:
- what the reply to an at-fence Hello carries for a named chunk the node lacks (for example an
  explicit not-held row, or a required set limited to the chunks held, with `NotHeld` expected
  on first use);
- how #86's validator treats it;
- one `Request::Hello` shape for both plans;
- a target or engine test of a reconnect that names a deleted chunk, under #86's validator.

### R1-C5 — Medium
**Summary.** `released` and `fenced` have no bound. #80 (approved) requires bounded shapes for
both, and fixtures, before #13 activates them.
**Evidence.** #80 contracts.md:46-49 ("#13 release/fence lists likewise need bounded complete
shapes and revised fixtures/admission before activation"). #80's bound (decisions.md:16-19) is
`2048 + N×(288 + 264×(R+1)) ≤ 4 MiB`: 264 B per member for replicas and ISR, one extra member
for a move. At RF 1 and N = 5137 it comes to 4,193,840 B, so 464 B of slack.
#13 design.md:57-59 makes both fields unbounded sets, and design.md:614-615 puts them in every
`ChunkMeta` and `Volume` of `GetVolume` and `Attach`. `released` grows with each unconfirmed
move off a node, and release deletion is off by default (S14). `fenced` grows with every node
that ever confirmed a release, and is kept until the volume is deleted.
At #80's largest RF 1 volume, four chunks in a move that still carry an earlier unconfirmed
release (about 131 B each) exceed 4 MiB. Then `GetVolume` and `Attach` fail at the 4 MiB
receive limit: the target's refresh, the sweep, the lookups and the CSI.
**Fix.** State and enforce a bound for both lists, and add them to #80's worst-case fixtures and
admission. Otherwise, record in #13 the bound #80 must budget.

### R1-C6 — Low
**Summary.** #13 adds two `GetVolume` callers as the node's `io` identity, but #75 sets its
`enforces_qos` flag only on the two existing calls.
**Evidence.** #75 design.md:66-69: a limited volume read by an `io` caller without
`enforces_qos` gets `FailedPrecondition`. #75 design.md:133 sets the flag only at
`engine.rs:668, 1782`. #13's new callers: the sweep's read (design.md:140, 172) and the lookups'
read (design.md:339).
Whichever plan lands second without the flag breaks QoS-limited volumes:
- the sweep skips the volume every time (design.md:150), so its released copies are never
  deleted;
- S17 drops the request on a failed read, so `NotHeld` is never sent.

**Fix.** State that every data-plane `GetVolume` (refresh, attach, sweep, lookup) is built by one
constructor, so #75's flag covers all four.

### R1-C7 — Nit
design.md:219: `node.sh list` takes `VOLUME` as optional, "checked by `volume_id`". But
`volume_id` exits when `VOLUME` is unset (`: "${VOLUME:?}"`, `scripts/guest/node.sh:103`). Check
it only when `VOLUME` is set.

### R1-C8 — Nit
design.md:72: step 1 checks that each chunk is "below the chunk count", but that count comes
from the header, which step 2 reads. Move the range check after `readHeader`, as `ChangeIsr`
does (`service.go:775-778`).

### R1-C9 — Nit
spec.md:10 defines a fence holder as keeping its fence "with no chunk". But S3 and
design.md:76-78 add the node to `Fenced` at its first confirmation, even while it still holds
other chunks of the volume, and #72 charges it a slot on that basis. Reword the term: "a node
that confirmed a release of the volume, so keeps its fence".

### R1-C10 — Nit
design.md:490-492 adds `one_way` beside `down_links`, but does not say that `end_chaos` clears
it, as it clears `down_links` (`sim/mod.rs:1332-1341`). A script that leaves a one-way cut
would never settle.

Reviewed commit: 7defc5913e7534ed29a3e4ef4e645a813b848803
