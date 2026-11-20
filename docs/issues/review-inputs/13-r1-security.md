# Plan review: issue #13, round 1, design-security

Scope: the whole plan folder `docs/issues/13/` (intent, spec, design, decisions; report.md read
for context only), checked against the code in `/home/jsfakian/Documents/src/flaminio-13`.

### R1-S1 — High

**S2 (no move back while released), the rule that makes a delayed deletion safe, has no
simulator check and no injected bug. The simulator never has a release outstanding.**

The plan treats S2 as the safety rule behind deletion. There is "no release token" because "a
released chunk cannot move back" (decisions.md:193-194). S14 says that without S2 "a move back
can commit and, at RF 1, the copy deleted may be the only one" (spec.md:84-90). The hazard
exists only because the real data plane deletes after a read that can be up to 5 s old (S7). A
move back that adds, resyncs and joins the old source inside that window is followed by the
sweep deleting a copy that is in the ISR again.

The simulator cannot see any of this. Its sources "delete at the drop, as today; the sim then
calls `meta.confirm_release`" (design.md:457; spec.md:143). Today the `MetaReq` arm removes the
source's chunk in the same step as the commit (`flaminio-repl/src/sim/mod.rs:770-795`). So
`released` is always empty when `request_move` runs. `request_move` does pick former sources
(`sim/mod.rs:1260-1285`, any node without a replica). The model's new
"`MoveReplica` is `Rejected` for a released `to`" (design.md:106-110) is therefore never
exercised in a run. A bug that drops it in `meta.rs`, or in both `meta.rs` and Go so that the
vectors still agree, passes `make sim`.

The S7 deletion conditions (the record still releases the node, not a replica or mover, read
age, local view) sit in `flaminio-node/src/sweep.rs`, which the simulator does not run. None of
the seven injected bugs (design.md:499-507) covers S2 or the sweep. Under AGENTS.md's
Principles ("Replication, membership and recovery logic lives in `flaminio-repl` … A new safety
rule gets a simulator check and an injected bug"), this is a safety rule without its simulator
check. The decisions entry "Superseded: the simulator only confirms the releases it makes"
(decisions.md:251-253) added `NotHeld` but did not address it.

**Fix:**
- Put the per-chunk deletion decision in `flaminio-repl`, as a sans-I/O function that both
  `sweep.rs` and the simulator call. The decision covers: the record releases the node, the node
  is not a replica or a mover, the read is at most 5 s old, and the local target does not place
  the chunk there.
- In the simulator, replace the delete at commit with a modelled sweep. It reads the model at a
  random time after the drop, deletes 0–6 s later if that function allows it, then calls
  `confirm_release`. Keep the random moves, which already pick former sources.
- Add an injected bug `move_to_released` (the model's `MoveReplica` ignores `released`). A
  `mutation!` test must show it breaking linearizability or identical replicas in some run;
  RF 1 and RF 2 configurations make a move back inside the window likely.
- Update spec.md:143 and the Simulator sections of design.md (443-539) and decisions.md.

### R1-S2 — Low

**#72's prerequisite change to `ConfirmRelease` (at most 118 chunks, the node's ledger
compared and written in the same transaction) is not in #13's plan. #13 uses the whole etcd
budget, so the two plans contradict each other on a shared contract.**

#13 gives `ConfirmRelease` 1 to 119 chunks, with header plus 119 compares and puts, which is
exactly `kv.MaxTxnOps` = 120 (`meta/kv/kv.go:31-33`). The places are design.md:72, 76-81, 142,
628 and 674, and decisions.md:248.

#72 builds its capacity credit on this call. It needs "`ConfirmRelease` takes 1 to 118 chunks,
and #13's sweep confirms in batches of at most 118", plus a ledger compare and write, with a
moved ledger retried like a moved header (`/tmp/flaminio-plans-2026-11-12/72/docs/issues/72/`:
spec.md:137-150, capacity-contracts.md:37). It names these very lines as a prerequisite.

If #13 ships as written, #72 must change the RPC's limit and the sweep's batch size after
release. During that rollout, a data plane that still sends 119-chunk batches to a new service
gets `InvalidArgument` and never confirms those batches until it is upgraded.

On authorization, #13's rules already give #72 what it needs. `ConfirmRelease` accepts only
`io(r.Node)`, and the service confirms only chunks whose record lists `r.Node` in `released` at
the listed revision, refusing duplicates. A node can therefore credit only its own released
copies, and each one only once. See R1-S3 for the missing test.

**Fix:** adopt 118 now: design.md:72, 142, 628 and 674 ("119 chunks" becomes "118"; the S3 test
uses 119), and decisions.md:248, which should state why (#72's ledger). Alternatively, record in
decisions.md that #72 changes these lines and that #13's sweep must batch at most 118 before #72
lands.

### R1-S3 — Low

**The abuse case that binds a confirmation to the caller's own copies has no named test.**

`auth.go` checks only that `r.Node` is the caller's node. Step 3 of `ConfirmRelease` (design.md:73-75)
is the only check that the listed chunks are released to that node: "node not in `Released`,
`InvalidArgument`", nothing written. That check is what stops node X from confirming chunks
released to Y, or released to nobody. It adds X to `Fenced`, and under #72 it would credit X's
ledger.

The S3 test list (design.md:673-677: ISR change, move back, 120 chunks, none, a duplicate, mark,
attach, log) does not name a not-released chunk, a chunk released to another node, or an
out-of-range chunk. The S12 auth tests (design.md:683-685) cover only the identity-to-node
binding. The vectors cover `domain.ConfirmRelease` returning false (design.md:112-116), but not
the service's refusal or the guarantee that nothing is written.

**Fix:** add these cases to `TestConfirmRelease`, in memory and on etcd:
- a chunk released only to another node;
- a chunk released to nobody;
- a batch mixing one released chunk with one not released, where nothing is written, `Fenced`
  is unchanged and no log line appears;
- a chunk index at or beyond the chunk count.

### Nits

- **N1.** The spec's term "Fence holder: … keeping its fence with no chunk" (spec.md:10) is
  wrong as a definition. S3 and the design add the node to `Fenced` at its first confirmation,
  whatever it still holds of the volume (design.md:76-78). So a fence holder may still hold
  chunks and have no fence-only slot, and #72's "one slot" charge leans on this term. Say "a
  node that confirmed a release of the volume, which keeps its fence once it has no chunk".
- **N2.** `node.sh list` is described as "`VOLUME` optional, checked by `volume_id`,
  `node.sh:102`" (design.md:219). But `volume_id` starts with `: "${VOLUME:?}"`
  (`scripts/guest/node.sh:102-105`), so calling it as written makes `node.sh list` without
  `VOLUME` exit with an error, not the stated exit 0. Say "checked by `volume_id` when set".

### Outside this plan (not counted)

#73's contracts (`/tmp/flaminio-plans-2026-11-12/73/docs/issues/73/contracts.md:37-38`) still
give `ListTombstones` and `ConfirmDeletion` to "the agent for itself". #13 moves both to `io`,
and #73's table does not list #13's `ListReleases` or `ConfirmRelease`. This is for #73's
reviewer.

Reviewed commit: 7defc5913e7534ed29a3e4ef4e645a813b848803
