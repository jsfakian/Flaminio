# #51 plan review, round 2 (design-code focus, incremental: c8908eb..2997983)

## Earlier findings
- R1-C1: fixed. `B_s = 4160 + 12549 + 415 + N*(420+264(R+1))` is right. The catalog adds up to 12,549 (195-byte `Snapshot`, 199 an entry, 63 entries, field 30 12 bytes). The caps 4406/3446/2830/2400/2084 check out against #13's 4419/3457/2838/2408/2090. The fixtures and the "lands second" rule are stated.
- R1-C2: fixed (`B_s` at every #70 step; a chunk added at g reads zeros at or below g, S19, store test).
- R1-C3: fixed (roles table, open read, audit follow-up, retention charge), but see R2-C1 to R2-C3.
- R1-C4: fixed. `deleteChunks` is reused with header-absent compares and for the creation claim (`service.go:441,481,530,577`), and nothing is added to the header's transaction.
- R1-C5: fixed (pending byte review recorded; completeness evidence defined).
- R1-C6: fixed. `join_before_frontier`, `stale_stage_reply` and `early_publish` are defined, with publication as check 4. `sim::Bugs` (`sim/mod.rs:195`) can take a `meta` field.
- R1-C7: fixed (`Refused` then close, `frame.rs:110-113`, `replica/mod.rs:57`, target side `net.rs:282`).
- R1-C8: fixed (`auth.go` rule as ChangeIsr, `auth.go:80`; `applied=false` as `service.go:794-800`).
- R1-C9: fixed (effect stated; catalog listed inside #77's header-to-header window).
- R1-C10: fixed (`flaminio_node::volume_uuid`, `lib.rs:21`; below-E refusal matches `Store::delete_volume`, `store.rs:1247`).
- R1-C11: fixed (field 20 or #50's header mark; profile refusal dropped).
- R1-C12: fixed for tests and commands; the lab invocation is new drift, R2-C4.
- R1-C13: fixed.
- Nits: fixed.

### R2-C1 — Medium
**The request's retention charge races MoveReplica, and the copy that lands in the window is under-charged.**
The design argues: "Each of these transactions already compares the header, so k is the one the charge read" (design.md:82). That covers only one order: a move that reads after the request commits. The request reads the header, lists the chunk records and charges D ledgers. It compares only the header, the record and those D ledgers (contracts.md, Transactions). MoveReplica compares the header but never writes it (`service.go:908`: compares chunk and header, writes the chunk). #72's charge sits in that same transaction and writes only `capacity/<to>` (#72 design, MoveReplica). So a move to a node that holds no copy of the volume can commit between the request's List and its Txn. The destination is then charged 1+k while the header says k+1. At deletion, `VolumeCharges` times (1 + `snapshot_count`) releases one chunk size more than was charged. That takes another volume's charge on that node, or underflows, and #72's rule then deletes the ledger. Drains issue moves continuously, so this is reachable. `TestSnapshotRaces` ("request against ChangeIsr and MoveReplica: both commit") passes with the bug.
**Fix:** order the charge after a header write that every later move sees. One way: the request first writes the header (count + 1, record Pending, charge not yet complete). Then it lists the chunks and charges the ledgers, conditional on that header revision. A move after the mark already charges k+1. CommitSnapshot refuses until the charge is complete, which is safe because a Pending snapshot retains nothing. This also removes R2-C2's limit. Test: an `interleaveKV` hook after the request's chunk List commits a MoveReplica to a fresh node; the ledgers must equal `VolumeCharges` at k+1, and so must the tombstone release.

### R2-C2 — Medium
**The 114-node narrowing of S2 is not shown necessary, and it conflicts with #72's admission.**
- **The claim:** decisions.md:73-77 says the refusal is "shown necessary by R1-S2 and the 120-operation bound", because #72's invariant charges a copy "in the transaction that names it". But a request names no copy, and nothing is retained before the cut (S12: layers are pushed only by writes at h > t). A Pending request can therefore be charged in batches, each conditional on the header, with CommitSnapshot gated on completion. #72's InitCapacity batches and #70's steps already work this way. R1-S2 shows that a charge is needed; it does not show that the charge must fit one transaction.
- **The conflict with #72:** #72 (clear) admits creation on up to 116 distinct nodes (`MaxPlacementNodes = 116`, `TestCreateAt116Nodes`, #72 design:56,62). A volume created on 115 or 116 nodes can never be snapshotted, and growth (#70) and moves add more nodes.
- **The arithmetic:** D ≤ 114 holds only with #73's four compares (D + 6). Before activation and for a system owner, the transaction is D + 2, so 118 nodes fit, yet the plan refuses at 114 uniformly.
- **The same narrowing elsewhere:** "a node without a valid report" refuses any volume with a copy on a down node, a #13 `Released` copy awaiting ForgetNode included. The spec does not state that consequence.

**Fix:** take the batched Pending-phase charge (see R2-C1) and drop the node limit. If the limit is kept, state the per-case bounds (118 without #73's compares) and reconcile them with #72's 116. State and test the down-node refusal in S2 and the docs.

### R2-C3 — Medium
**#73's AssignOwner, a place that computes a quota charge, is missing from the (1 + k) multiplier list.**
The design's list covers `VolumeCharges`, `MoveReplica`, `ConfirmRelease` and #70's growth (design.md, The charge). #73's AssignOwner charges "a tenant's size at its claim or assignment" (origin/main `docs/issues/73/contracts.md:154`; design:173-178). Before activation, a request charges no quota (contracts.md, Retention charge: "for a tenant owner once #73 is active"). A volume snapshotted k times before activation is therefore assigned at size, not (1 + k) × size, and its tenant's quota undercounts k × size for good. The plan also does not say what RequestSnapshot does for a volume that has no owner record once tenancy is active. #70's growth answers `FailedPrecondition` `unassigned` there (#70 design:230); RequestSnapshot charges nothing, so the gap persists.
**Fix:** AssignOwner charges (1 + `snapshot_count`) × size, and #73's `bytes` definition says so. Alternatively, RequestSnapshot refuses an unassigned volume once active, as growth does. Whichever lands second applies it, with a test in `TestRequestSnapshotCharges`.

### R2-C4 — Low
**The lab check's invocation is misdescribed and its `VOL` is ambiguous.**
design.md:242 says `make snapshot-check VOL=… FROM=… TO=…` runs `scripts/lab.sh snapshot-check` "as `scripts/checks/resync.sh` is run". But `resync.sh` has no lab.sh subcommand and no make target. It runs from the host, `FROM=fl-a TO=fl-b scripts/checks/resync.sh` (resync.sh:6), and it generates its own volume name (`VOL=resync-$(date …)`). The design's check uses a "private" volume that cleanup deletes (design, Lab), yet takes `VOL=` from the caller, which could name an existing volume.
**Fix:** say that the lab.sh subcommand and the make target are new. Generate the volume name, or require that `VOL` not exist (refuse otherwise), and test that in `snapshot-check-test.sh`. Add both new script tests to `make lint`'s explicit list (Makefile:92-94).

## Nits
- decisions.md (snapshot-read): "R1-S7's 'above E' would refuse the volume's own chunks fenced at its first epoch". No chunk is fenced at E: `Start` sets Epoch = FirstEpoch and every Attach adds one (`domain.go:211-219`). The two rules are equivalent, so the stated reason is wrong.
- design.md, Budget: #80's fixtures (Create, Get, Attach) omit CommitSnapshot's response (Volume plus a Snapshot of up to 198 B). The 406 B left from the 415 descriptor covers it, but the fixtures do not show it.

Reviewed commit: da886c541f0072fd2494775510b46374d492ce21
