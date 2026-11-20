# Plan review #72, round 3, design-code (incremental: 629bc32..bbdb51d)

## Round-2 findings
- R2-C1 (completion condition): fixed. `domain.CompletedMove` is `ApplyIsrChange`'s own test
  (domain.go:277); `TestCompletedMoveOnly` covers the ISR-only change on a moving chunk.
- R2-C2 (absence not compared): fixed for ConfirmDeletion (revision 0 compared, unreadable stated);
  `ChangeIsr` no longer touches a ledger, so its half is gone. Test gap: see R3-C4.
- R2-C3 (fixtures): fixed. The four fixture lines cited exist (service_test.go:58, drain_test.go:45,
  csi_test.go:52, main_test.go:103); `meta-interop.sh` and `meta-smoke` are listed. Direct service
  calls in tests skip the auth interceptor, so the `io`-only rule does not break them.
- R2-C4 (rescan per attempt): fixed (scan once before the loop).
- R2-C5 (unnamed tests): fixed; all four tests named. R2-C6 (lab disk size): fixed by failing with
  the figures in step 1.
- Nits (116 as a margin, "56 distinct source nodes"): fixed or moot.

## Findings

### R3-C1 — Medium: `InitCapacity` restores a blocked node's old ledger against a mark that a confirmation may already have rewritten, so a release is lost for good
design.md "Initialization" step 4: a blocked node "gets its readable ledger from before the call",
"comparing its mark's revision in the snapshot". A `ConfirmDeletion` that lands between step 1 (mark)
and step 2 (snapshot) finds the ledger marked, rewrites the mark and releases nothing. The snapshot
holds the new mark revision, so the restore commits with the pre-call value, which still includes the
charge that confirmation was meant to release. The charge is never released: the tombstone is
confirmed, so no later run counts it, and without `all` the readable ledger is not a target. A node is
blocked while a legacy tombstone or an unparseable-header volume names it; it can still be pending on a
normal tombstone with charges. `TestInitRacingConfirm` ("each counted once") does not say it covers a
blocked node.
Fix: for a blocked node, compare the revision this call's own mark wrote (a failed compare leaves it
marked, run again), not the snapshot's. Or restore the old value less any release that happened in
between. Add the case to `TestInitRacingConfirm`: a blocked node, a confirmation between mark and
snapshot.

### R3-C2 — Low: a charge with no release path short of deleting a volume the operator cannot see
- An interrupted creation (caller gone after the claim) leaves a `Creating` header, which is hidden from
  listings (service.go:410 comment), and now also a `capacity-claims` key and charged ledgers. Today
  that only blocked the id; now it removes capacity. `capacity-init` rows do not name it.
- A copy left on a node whose disk was replaced under the same id is charged until its volume is
  deleted, and `capacity-init --all` recomputes the same charge from the same records.
Fix: have the rows (or `nodes`) name creations with a claim key that is not progressing, and say in the
docs and `followups.md` how to clear them (`delete`). State that a replaced disk keeps its left-copy
charge until the volumes go. Not a safety issue.

### R3-C3 — Low: a left copy now makes a drained or retired node part of every later deletion
`replicaNodes` (service.go:105–113) names `Moving.From` but not a completed move's source, so today a
volume whose chunks were all drained off a node deletes without it. With `Left` in the tombstone, a
node that is gone (decommissioned after its drain) never confirms: the tombstone and the id stay held
and its charge never leaves. The plan names only fl-c and `vol-confirm`. Fix: say in `docs/lab.md` and
`followups.md` that an administrator confirms for a retired node (admin may call `ConfirmDeletion`,
auth.go:77), and test that the admin confirmation releases the charge.

### R3-C4 — Low: tests that cannot fail where the limits matter
- The in-memory store enforces no `kv.MaxTxnOps` (only `etcd.go:91` does). `TestCreateTooManyNodes`
  (117) tests the service's own bound and `InitCapacity`'s batches of 120 cannot be tested with 121
  nodes, so a claim or batch of 119+ operations would pass on the Mem store and fail on etcd. Fix: a
  Mem option or wrapper that rejects over 120, a create at exactly 116 nodes, and a 121-node
  `InitCapacity`.
- `TestInitRacingConfirm` lists three timings but not an absent ledger (a confirmation reading it absent
  while Init creates it: the revision-0 compare must fail). Also untested: two concurrent
  `InitCapacity` calls (the second's marks must make the first leave its nodes marked), and
  `InitCapacity` giving `ABORTED` after 32 failed mark batches.
- The claim key's overwrite "without a compare" has no test.

### R3-C5 — Low: `meta up` runs `capacity-init` where it should not
scripts/lab.sh `cmd_meta up` is also called by the chaos step (lab.sh:1407, `cmd_meta up w` with output
discarded) and loops over VMs, so `capacity-init` would run once per VM, in the middle of a chaos run,
listing all metadata each time, with its failure hidden. Fix: run it once, after the loop and only for a
full `meta up` (not from `chaos`), and fail loudly on its error. Also `MoveReplica` demands a valid
report and readable ledger even for a move back to a left copy, which charges nothing; say so or skip
the check there.

### R3-C6 — Low: approved answers and the documents still disagree
- design.md:4 and :222 call the ADR "Proposed"; adr-proposal.md says "Accepted with the plan"; the
  decisions say accepted.
- capacity-contracts.md:1,4 "draft ... All of it remains Proposed"; decisions.md:27 says accepted as
  written.
- design.md:215 "Accepted: none. Open: Q1, Q3–Q5"; decisions.md settles Q1, Q3, Q4, Q5; spec.md:142
  keeps an "Open questions" section with "Recommended".
Fix: align the status lines and move the questions to "Settled" in spec and design.

## Nits
1. design.md "Charging and release" calls `ApplyIsrChange` the caller of `CompletedMove` while the
   service also calls it before applying; say the service collects first, as the text below it does.
2. Lab step 3's "R" depends on a 100 GiB disk (320 chunk records, fine) and on U <= L; say that step 1's
   check is what guarantees the "exactly R" claim.
