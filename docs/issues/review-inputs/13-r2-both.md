# #13 plan review, round 2 (incremental), focuses: design-code and design-security

Scope: `git diff 7defc59..551dc8e -- docs/issues/13/ ':!docs/issues/13/report.md'` in
`/home/jsfakian/Documents/src/flaminio-13`, and the code and plans the changed lines depend on
(`sim/mod.rs`, `sim/tests.rs`, `replica.rs`, `target.rs`, `engine.rs`, `drain.go`, `kv.go`,
`etcd.go`, `meta.proto`; #45, #72, #80, #86 and every plan in the batch manifest for field 15).

## Earlier findings

- R1-C1 (High): fixed. `release::may_delete` is shared by the sweep and the simulator, which now
  keeps a released copy until a modelled read and a later delete. Traced
  `scenario_move_back_before_deletion`: without the bug the move back is refused, node 1 deletes
  at 5.1 s and T2 reads node 2's copy, so it passes. With `move_while_released`, node 1 deletes at
  5.1 s on its 1.1 s read (released, not named, fence 1 <= 1, no local target). T2's raising
  `Hello` then creates chunk 0 empty and the read returns 0, so it fails as stated.
- R1-S1 (High): fixed, the same change.
- R1-C2 (Medium) and R1-S2 (Low): fixed. These now match #72's S11 and capacity-contracts.md:37:
  1 to 118 chunks, the ledger compared and written by `releaseCharge`, a moved header or ledger
  retried. That is 120 compares and at most 120 writes, against `etcd.go:91`.
- R1-C3 (Medium): fixed. `Volume.fenced = 15` is free in `meta.proto` (tags 1–8), and no plan in
  the manifest claims 15.
- R1-C4 (Medium): fixed. See the note on #86 below.
- R1-C5 (Medium): fixed as a bound. But the narrowing of S2 that pays for it opens R2-B1.
- R1-C6 (Low): fixed (`get_volume_call`).
- R1-S3 (Low): fixed (the abuse cases are in `TestConfirmRelease`).
- R1-C7, R1-C8, R1-C9, R1-C10, R1-S N1, R1-S N2 (Nits): fixed.

### R2-B1 — High

**Summary.** The narrowed S2 refuses every move of a chunk that holds a release. A release ends
only when the released node deletes its copy and confirms. So a chunk whose released node never
deletes can never move again:
- with release deletion off, which is the default (S14), this is every chunk ever moved;
- with a node gone for good, it is forever.

Nothing else ends a release. Only the node's own `io` identity confirms (S12; administrators
never call `ConfirmRelease`), and "forgetting a node" is a follow-up.

Failover (an attach) still works. Drain and rebuild do not:
- When node F dies, its chunks are moved off it, by a drain of F or by #45's rebuild. Each such
  chunk then holds F's release for good.
- A later failure of another replica of that chunk can never be rebuilt. The chunk stays below
  its RF, and the next failure can lose it.
- A drain of any node holding such a chunk loops forever without saying why: `Progress.Released`
  counts only the drained node's own releases.
- A released node that is compromised, or just runs with deletion off, can block every move of
  the chunks it left. Before the revision it could block only a move back to itself.

The design files this under "Liveness only". It is also durability, because re-replication
stops.

**Evidence.**
- spec.md:29-31 (S2), 78-81 (S11: "the chunks it releases unmoved"), 83-85 (S12), 90-95 (S14:
  "off unless started on"), 101-102 (S16).
- design.md:79-82 (`CheckMove` is false whenever `Released` is not empty), 658-664 (the drain
  waits; `Progress.Released` counts the drained node's releases only), 1007-1010.
- decisions.md:79-84: the cost is named, not accepted. decisions.md:413: the accepted risk "A
  node gone for good" still names only the id that stays taken.
- `control/drain/drain.go:124`: the drain is the only Go caller of `MoveReplica`, besides
  `metactl` by hand (`main.go:430`). No other path re-replicates.
- #45, approved, "Assumes #13 … as planned". Its rebuild is moves off the failed node
  (/home/jsfakian/Documents/src/flaminio-45/docs/issues/45/spec.md:60-62 S9, 75-77 S13: "The
  drop releases the source (#13)"). Its blocked reasons (S15, 85-97) have no "holding a release".
  So its refused requests repeat every pass with no reason given, and its lab result ("every
  chunk ends with two in-sync replicas") fails for a chunk moved earlier.

**Fix.** Give a release an end that does not need its node, and keep the bound. Either:
- **Bring "forgetting a node" into this plan.** Add an administrator RPC, with an `auth.go` rule
  for administrators only. It acts only on a node first marked forgotten:
  - `CheckMove`, placement and `RegisterNode` then refuse that node, and its `io` identity's
    `ListReleases` and `ConfirmRelease` are refused;
  - the RPC then removes the node from every `released`, `fenced` and tombstone, in conditional
    transactions.

  This is safe because the hazard S2 guards is only a move back to the released node, which can
  then never commit. Tests: service tests (each refusal, the clearing, a racing move), and a
  model op with an injected bug, clearing without the mark, that the move-back scenario catches.
- **Or have the user accept this cost explicitly,** including its default-off consequence. Then
  add "holding a release of N" to the drain's progress and to #45's S15. Also say in
  `docs/lab.md` that a drain needs release deletion on wherever a release is held. With the
  default `DELETE_RELEASED=0` (`node_env`), `make drain-test`'s move back waits forever.

### R2-C1 — Low

**Summary.** The cap's effect on #72's ledger has no test. The plan's rule: a confirmation that
would add a 17th fence holder confirms, does not add it, and keeps no slot. An implementation
could key the kept slot on "the node was not in `Fenced`" rather than on "this call adds it".
That keeps one slot at every confirmation by a 17th or later node. No tombstone or release ever
returns those slots, so the ledger creeps up until `InitCapacity` reruns.

**Evidence.** decisions.md:86-90; design.md:96-101 (step 4, worded correctly); design.md:811-812
(`TestFenceHoldersAtMost16` checks only `fenced` and the header). #72 design.md:174 has "the first
confirmation on a node keeps one slot …, a later one none", but no case with `fenced` full.

**Fix.** Extend `TestFenceHoldersAtMost16`, with #72, in whichever plan lands second: two
confirmations by the 17th node each credit n chunk sizes and n slots, with no slot kept. A rerun
of #72's `InitCapacity` then leaves that node's ledger unchanged.

### Nits

- **R2-N1.** design.md:804: "after a move back and a new drop, the new release stays". Under the
  narrowed S2, that move back is refused while the release is pending, so the test cannot be built
  as described. Reword it: the release confirmed, the chunk moved back and dropped again, then a
  replay of the first confirmation at its old revision gets `Aborted`, and the new release stays.

## The brief's questions (no finding unless listed above)

- **The simulator:** the check fails with the bug and passes without it (R1-C1 above). Delays come
  from `relay_rng`, and `end_chaos` restarts every node, so each sweep chain ends.
- **The 17th fence holder:** fencing is not weakened. The store's fence-only slot is the fence
  (S10), and `fenced` drives only the tombstone. A holder the tombstone does not name keeps its
  fence longer. A volume created again starts above every earlier epoch (`kv.go:35-37`), and #30's
  drop takes the slot. What remains is a slot leak that `list` shows and #72 counts through U.
- **#86's rows:** a session still opens only at an epoch at or above the fence
  (`replica.rs:169-171`). A missing row grants fewer facts than today's `on_fenced`, which marks
  every chunk naming the sender (`target.rs:1311-1316`). So no stale target gains a session.
- **#72 and field 15:** see R1-C2 and R1-C3.

## Outside this plan (not counted)

- #72's text still describes the old S2. Its design.md:18 and 66 say `CheckMove` "refuses a move
  to a released node", and its S11 says the same. This is harmless, but stale.
- #75's design.md:175 still lists "9-10 for #13".

Reviewed commit: 93ed53b96bb5e7118c908ee3839a34516d8388f9
