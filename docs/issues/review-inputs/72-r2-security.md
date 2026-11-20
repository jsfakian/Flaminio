# Issue #72 plan review, round 2 (incremental), design-security

Scope: `git diff 813d9a5..629bc32 -- docs/issues/72/ ':!docs/issues/72/report.md'` in
`/tmp/flaminio-plans-2026-11-12/72`, checked against that worktree's code. Q1–Q5 are open
questions and not findings in themselves. The findings below are claims about them that the code
contradicts, mechanisms that are wrong as written, and abuse paths that have no test.

## Earlier findings

- R1-S1 (one node holds a class closed): fixed. The cut removes the class operation and its lock.
- R1-S2 (distinct-node min ISR, no simulator check): fixed. The cut removes refs, and ISRs stay node lists.
- R1-S3 (replica-side route enforcement): fixed. The cut removes routes and Hello v2.
- R1-S4 (`NoSpace` conditions): fixed. The cut removes `NoSpace`, and out-of-space handling stays as it is today.
- R1-S5 (old metadata instance): not fixed. It is now open as Q3. The ack-durability claim holds (a failed
  replica, never an acknowledgement), but "old binaries ignore the new values" is wrong (R2-S4).
- R1-S6 (tag collisions): fixed. Only `NodeHeartbeatRequest.capacity = 72` and `Node.capacity = 72` are added. #45 (`Node` 6) and #61
  (`Node` 20–21) are clear, and no other draft touches either message.
- R1-S7 (#80 bound): fixed. No `Volume` or `ChunkMeta` field is added.
- R1-S8 (administrator-typed totals): partly fixed. The store now reports its own totals, but an administrator can still type them
  into a heartbeat (R2-S6).
- R1-S9 (report rate, clock): fixed. The report is the liveness value and expires with it, and the rate exposure is the same as today.
- R1-S10, R1-S11, R1-S13, R1-S14: fixed. The cut removes all of them.
- R1-S12 (string validation): fixed. There are no new string inputs, and ledger keys use registered, `ValidID` node ids.
- R1-S-N1, R1-S-N3: fixed by the cut. R1-S-N2: fixed. There is one rule, and equality is admitted (spec S5).

### R2-S1 — High

**As defined, `ChangeIsr` releases a move's source on every ISR-only change of the moving chunk.
That includes the destination's join, so every move releases its source at least twice.**
design.md:64–66 defines the completed moves as "a named chunk whose `Moving.From` is not in its
`NewReplicas`". An ISR-only change carries no replicas: the engine sends `change_replicas:
c.new_replicas.is_some()` and an empty list (`flaminio-node/src/engine.rs:1818–1824`), and the
service copies it as-is (`meta/service/service.go:761–762`). So `From ∉ NewReplicas` holds for every
ISR-only change. The target's join of the destination is such a change: `send_meta` sets
`drop_replica` only after the destination is in the ISR (`flaminio-repl/src/target.rs:2409–2419,
2437–2453`). Member removals during the move are ISR-only changes too. `ApplyIsrChange` completes a
move only when `c.ChangeReplicas` is set (`meta/domain/domain.go:276–279`).
Effects:
- Each move under-counts its source by at least one charge.
- Repeated releases underflow the source's ledger. The source's ledger is then deleted, and the node
  takes no create or recovery move until `InitCapacity`.
- A target node's `io` identity can drive this against other nodes' ledgers by changing the ISR
  repeatedly.

`TestMoveCompletionReleasesSource` (design.md:142) covers only the add step, the remove step and a
refused retry, so it would pass with the bug.

Fix: define a completed move as `ChangeReplicas && Moving != nil && Moving.From ∉ NewReplicas`,
the condition `ApplyIsrChange` uses, or take it from `ApplyIsrChange`'s result. Add test cases for
the join step, for a member removal while the move is pending, and for both changes in one
request, none of which may release.

### R2-S2 — Medium

**An interrupted creation that is then deleted leaks its unwritten charges permanently. "Until #76
recovers it" does not hold.**
The claim charges the whole placement (design.md:54–60, contracts table). A `DeleteVolume` can
mark a header that is still `Creating`. That stops the creation (`service.go:435–444, 455–456`),
and the deletion lists only the chunk records already written (`service.go:539–570`). Its tombstone
`Charges` are `VolumeCharges` over that listing (design.md:71–72). After that, no record names the
unwritten chunks' charges. #76 has no header left to recover, `InitCapacity` never rewrites an
existing ledger, and no RPC can.
A creation that times out (the CSI provisioner's deadline on a large volume) followed by the
cleanup delete, or a delete that races a create, is a realistic path, and each one loses up to
the whole volume × RF of committed room. The plan states the gap only as "until #76 recovers it"
(intent.md:51–52, spec.md S4 56–57 and Q2 127, adr-proposal.md:47–48). No test deletes a volume
whose header is still `Creating` (design.md:143, 146).
Fix: record the claim's per-node charges in the `Creating` header's JSON (an etcd-only field, at most
116 entries), and have `DeleteVolume` of a `Creating` header write those as the tombstone charges.
Alternatively, state the leak and its bound, and correct the "until #76" wording. Add
`TestDeleteInterruptedCreationReleasesAll`.

### R2-S3 — Medium

**Q2's over-commit is reachable by a routine drain and undrain, and placement concentrates new
creates on exactly that node.**
After a drain, every completed move has released the drained node's charges (spec S3), while
its copies stay on the disk (`docs/followups.md:244–249`). So L is close to 0 and U is
the node's old contents. Admission takes `max(L, U)` (spec S5). While L ≤ U, each
create is checked against U alone, so L can grow to the soft budget on top of the leftovers. With
leftovers D, physical use reaches about soft + D. For example, 80 % + 70 % = 150 % of T.
The undrained node also has the lowest replica count, because the count comes from chunk records that no
longer name it (`service.go:384–396`). `placeChunk` puts the lowest count first
(`meta/domain/domain.go:395–404`), so it is every create's first choice until its room runs out.
The same leftovers also refuse a move back to that node that needs no new space: the copy is
already counted in U, and the move adds c on top of it.
Q2 states the bound ("the smaller of the two"). Risks presents the result as "a failed member"
(design.md:189–190), but a full node fails every replica on it that needs to allocate. The lab check only
detects the gap (design.md:163). No test covers drain, then undrain, then creates.
Fix: keep the released charge on the chunk record as a `left` entry until the copy is gone. A move
back reuses it at no charge. The tombstone names the node, so the sweep deletes the copy and the
confirmation releases the charge. Alternatively, release a source only once #13 deletes the copy.
At the least, state this path in Q2 and in Risks, and add a service test: drain, undrain, create
to the room, and assert the recorded over-commit.

### R2-S4 — Low

**Old binaries do not ignore the new tombstone field. They erase it.**
Old `ConfirmDeletion` decodes into `tombstoneRecord{Nodes, Confirmed}` and writes the record back
(`service.go:72–75, 635–661`), so it drops `charges`. An old `DeleteVolume` writes none. The new
instances' later confirmations then release nothing for any remaining node, a permanent over-count
that nothing can correct (no reset, design.md:202–203). design.md:202 says "Old binaries ignore
the new values and keys". decisions.md:34–37 and Q3 cover only uncharged placements. No test
writes an old-style confirmation, or an uncharged create or move, after initialization. The
latter is also the only evidence for Q3's detection claim ("rows name a recorded charge above the
ledger").
Fix: keep the charges in a key that old binaries never rewrite. For example,
`tombstone-charges/<id>`, written in `DeleteVolume`'s transaction and deleted with the last
confirmation. Correct design.md:202. Add both old-style writes to `TestInitCapacity` and
`TestDeleteReleasesOnConfirm`.

### R2-S5 — Low

**An unreadable ledger blocks its node permanently, and `ConfirmDeletion` does not say what it does with one.**
A ledger that does not parse with schema 1 is "no readable ledger" (contracts:8–10), and the node
takes no new replica (spec S2). `InitCapacity` creates ledgers only by comparing absence
(design.md:106–107), so it cannot replace an unreadable one. No RPC can either (spec S10).
Such a ledger can come from a later schema followed by a rollback, or from corruption. The node
then gets no creates and no recovery moves until someone edits etcd by hand. The code's own idiom
for an unparseable node record is to overwrite it (`service.go:178–186, 205–208`).
`ConfirmDeletion` specifies only "Readable" and "Absent" (design.md:73–76), while `ChangeIsr`
skips an unreadable ledger (design.md:68).
Fix: `InitCapacity` replaces an unreadable ledger with the recorded charge, comparing the
revision it read. `ConfirmDeletion` treats an unreadable ledger as absent and logs it. Add both
cases to the tests.

### R2-S6 — Low

**Administrators still type totals, which the R1-S8 resolution says no longer happens.**
decisions.md:25–26 says "no administrator-typed totals". Yet `heartbeat <node> <data-bytes> <slots>` sends
typed figures with zero use (contracts:108–109), and spec S10 accepts an administrator's report for
any node. `make meta-smoke` sends `100G 3200` for each data VM as admin (design.md:175,
`scripts/lab.sh:529–534, 612`). That is more than the lab disk's usable data. Until the data plane's next
heartbeat, such a report replaces the real one, and its U = 0 admits past leftovers that the real U
counted. Nothing checks it against the disk.
Fix: accept a report only from the node's `io` identity, so an administrator's heartbeat stores `1`.
Alternatively, keep typed reports explicitly as test input (a flag the CLI requires), and correct
decisions.md. Either way, add the case to the S10 matrix.

### R2-S7 — Low

**Room exclusion silently weakens best-effort failure-domain spreading, and a tenant can trigger it.**
Nodes with no room are removed from the candidates (design.md:81–83). The same list feeds
`resolveDomain` (`domain.go:343–352, 363`). When one zone has no room left, a `bestEffort`
volume therefore falls back to rack or node level and may put two replicas in one zone. Committing
a zone's room takes only thin creates, with no writes. The plan does not state this effect, and
`TestPlaceRoom` (design.md:144) does not cover it. A zone with a little room left is handled
differently: its nodes still count in `resolveDomain`, and `placeChunk` then fails. So the outcome
depends on how full the zone is.
Fix: state the rule. One option is to count a full zone in `resolveDomain` and refuse with
`ErrCapacity` rather than fall back. Add `TestPlaceRoom` cases for a full zone and a nearly full
zone under both policies.

## Nits

- contracts:95–96: every invalid report logs a warning, and the heartbeat has no rate limit. Log once per
  node when its report state changes.

Reviewed commit: 8ad446a17f420736478593a9dd81afe2af160770 (plan documents as of 629bc32)
