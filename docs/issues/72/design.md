# Issue #72: Capacity safety, balancing and locality — design

[Spec](spec.md), [contracts](capacity-contracts.md), [decisions](decisions.md). One step: a
per-node capacity ledger, and admission of creates and moves. [ADR](adr-proposal.md), accepted
with the plan, 2026-11-13.

The facts it starts from:
- **Creation** lists every chunk record for replica counts (`meta/service/service.go:384–396`),
  places by preferred node, count and id (`:404`, `meta/domain/domain.go:359–422`), then claims
  comparing only the volume and tombstone keys (`:415–417`), never retried. Chunk batches and the
  last header write compare the claim (`:438–468`); a deletion's mark stops them (`:435–444`).
- **Moves:** `MoveReplica` checks only that the destination is registered (`:870`), then
  compares chunk and header (`:907–909`).
- **Deletion:** `DeleteVolume` marks the header, lists the chunks written, writes the tombstone
  with `replicaNodes` (`service.go:92–116, 539–570`), then deletes the chunk records (`:577`).
  `ConfirmDeletion` rewrites or deletes the tombstone by CAS (`:649–661`).
- **#13 (approved):** `ApplyIsrChange` adds a completed move's source to `Chunk.Released`;
  `CheckMove` refuses a move to a released node; `ConfirmRelease` removes the node from 1 to 119
  chunks (as approved; S11 states the change to 118) in one transaction of header and chunk
  compares, adding it to `Header.Fenced`; a deletion names released nodes and fence holders
  (`flaminio-13/docs/issues/13/design.md:58–93`). Its sweep confirms released copies deleted
  or absent, only with release deletion on (`spec.md` S6, S14 there).
- **Nodes:** `NodeHeartbeat` writes `alive/<node>` = `1` by `PutTTL` (`:228`); `nodes()` reads
  only the keys (`:244–251`). `RegisterNode` compares the record's revision, 0 when absent
  (`:209–210`). `resolveDomain` counts the domains of the candidates it gets (`domain.go:343–352`).
- **Data plane:** the engine heartbeats every 3 s (`flaminio-io/src/node.rs:331`,
  `engine.rs:720–733`) and owns the `ReplicaService` (`engine.rs:465`). Its store has `stats()`
  (`flaminio-store/src/store.rs:2159–2171`, O(1) counters, `bitmap.rs:56,74`) and
  `superblock().layout.chunk_slots` (`store.rs:698`, `flaminio-format/src/layout.rs:63`).
- **Limits:** 120 compares and 120 writes per transaction, enforced by the etcd store only
  (`meta/kv/kv.go:31–33`, `etcd.go:91`).
- **Drain** picks the least loaded node by count and ends the step at the first `MoveReplica`
  error (`control/drain/drain.go:120–137`). CSI passes `RESOURCE_EXHAUSTED` through and maps
  `FAILED_PRECONDITION` to it (`control/csi/controller.go:248–251`).

## Reports and ledgers (S1, S2, S7, S10)

- **Data plane:** with each heartbeat, `Engine::turn` sets `capacity` from
  `self.service.store()`: data bytes `data_clusters × cluster_size`, used `data_used ×
  cluster_size`, quarantined `data_quarantined × cluster_size`, slots
  `superblock().layout.chunk_slots`, slots used `stats().chunks` plus #13's fence-only count
  once its `stats` has one. Checked multiplication; an overflow sends no report. Same thread and
  timer as today; nothing on the I/O path.
- **`NodeHeartbeat`:** `auth.go` allows `capacity` only to that node's `io` identity.
  `domain.Report.Valid()` applies S1: valid, `PutTTL` writes the report JSON, otherwise `1`. A
  per-instance map of each node's last validity limits the warning to changes.
- **Reads:** `s.reports(ctx)` parses the `alive/` values; `s.ledgers(ctx)` lists `capacity/`
  into node → state, value and revision. `nodes()` fills `Node.capacity` for `ListNodes`.
- **`RegisterNode`:** with no record (`e.Rev == 0`) it gets `capacity/<id>`. Absent: one
  transaction writes the record and a zero ledger, comparing both absent. Present: today's
  transaction. An existing record, unparseable included, takes today's path.

## Charging and release (S3, S4, S11)

`meta/domain/capacity.go`: `Charge{Bytes, Slots}`, `Ledger`, `Report`, `SoftPPM = 800_000`,
`HardPPM = 950_000`, `MaxPlacementNodes = 116`; `Budget(total, ppm)` with `bits.Mul64` and
`bits.Div64`; `Room(ledger, report, ppm, chunkBytes)`, S5's rule and nothing else;
`VolumeCharges(h, chunks)`: replicas, `Moving.To` when not among them, and #13's `Released`,
each a chunk size and a slot; each of #13's `h.Fenced`, one slot.
- **`createVolume`:** the chunk scan runs once, before the loop: its counts only order
  placement. Then up to 32 attempts: the tombstone check; `nodes()` with the reports, then the
  ledgers; candidates and `Place` (S5); over 116 distinct nodes, `ErrPlacement`; the claim, with
  `capacity-claims/<volume>`, written without a compare. A false claim with volume and tombstone
  keys both absent is a ledger conflict: back off and try again; otherwise today's handling
  (`:421–434`). Then `ABORTED`. The last header write also deletes the claim key.
- **`MoveReplica`**, in its loop after `CheckMove` (which refuses a released `to`, #13): needs
  `alive/<to>` with a valid report and a readable `capacity/<to>`, else `FAILED_PRECONDITION`.
  `Room(…, HardPPM, ChunkBytes) < 1` is `ErrCapacity`; otherwise the transaction charges it.
- **`ChangeIsr`:** unchanged by this plan; #13's drop makes the source released, still charged.
- **`DeleteVolume`:** after its mark, when the header parsed (`:539`): a `Creating` header with a
  claim key takes the claim's charges and nodes; otherwise `VolumeCharges(h, chunks)`.
  `replicaNodes` (with #13's released nodes) adds the claim's nodes. The tombstone transaction
  also deletes the claim key. The mark stops the creation's last write, so the claim key cannot
  change after it is read.
- **The release rule,** `s.releaseCharge(node, charge) (kv.Cmp, *kv.Op)`, the one place it is
  stated: it reads the node's ledger and compares its revision (0 if absent). Readable: written
  less the charge. Being initialized: the mark rewritten by the contracts' one rule
  (Initialization and releases), `prev` less the charge or the same mark, its revision moved.
  Absent or unreadable: the revision compared, no write. Underflow deletes the ledger (or drops
  `prev`), logged at error level as `capacity ledger below a released charge: removed`.
- **`ConfirmDeletion`**, when it newly confirms (`!already`; by whichever identities `auth.go`
  allows: the node's agent today, its data plane after #13's S12, or an administrator) and
  `t.Charges[node]` exists, adds the release rule to the tombstone's transaction.
- **`ConfirmRelease`:** #13's, changed as S11 lists; this plan writes no other part of it.
- **Growth (S3):** once #70 has landed, each step of its `ExpandVolume`
  (`meta/service/expand.go`) charges the ledger by #70's "Charges": `Room` at `SoftPPM` and the
  room map as `createVolume`, each used node's ledger compared and written in the step's
  transaction, the step's chunk count shrinking to the longest prefix that fits 120 operations and
  120 compares (#70's terms), at least one chunk; deletion releases it by the release rule. #70 lands with or after #72
  (#70's ordering prerequisite since its round 6), so #70 adds this charge; it has no step-1
  refusal to remove.

## Admission (S5, S8)

- **Room** is S5's rule, in `domain.Room` only; every caller uses it.
- **Candidates:** today's filter (`service.go:397–402`), less the nodes without a valid report
  or a readable ledger, named in errors. Nodes without room stay: `Place` takes a room map, and
  a node absent from it, or any node with a nil map, has none. The domain tests build a full map
  with `domain.Unlimited(nodes)`.
- **`Place`** keeps its order. It counts the volume's copies per node, and `placeChunk` skips a
  node whose count reached its room. `resolveDomain` still counts nodes without room, so a full
  zone refuses instead of falling back. The preferred node is kept only if its room is at least
  the chunk count. A failure while some node was skipped for room is `domain.ErrCapacity` (new,
  `RESOURCE_EXHAUSTED` in `errStatus`); otherwise `ErrPlacement`, as today.
- **Drain:** in `stepVolume`, a `MoveReplica` error with `RESOURCE_EXHAUSTED` or
  `FAILED_PRECONDITION` drops the destination from the step's candidates (shared by pointer with
  later volumes) and picks again. None left: today's message plus `with room); last refusal: %s`.
  #13's drain already skips released destinations.

## Initialization (S6)

`InitCapacity` (`meta/service/capacity.go`):
1. **Mark:** lists `nodes/` and `capacity/`; each target (S6) gets the contracts' mark,
   `{"schema":1,"init":true,"prev":P}`. It compares the revision read, in batches of at most
   120 by `TxnRev`, whose revision is this call's own mark. A failed batch rereads its ledgers
   and tries again, up to 32 attempts, then `ABORTED` (nodes already marked stay marked, their
   `prev` kept).
2. **Snapshot:** one `List` of `/flaminio/v1/`, a single revision: headers, chunk records, claim
   keys, tombstones. A deletion is in it as a header or as a tombstone, never both, never neither.
3. **Recorded charge:** a published header's `VolumeCharges`; a `Creating` header's claim key,
   or its written chunks if it has none (an older creation); the charges of tombstones for nodes
   not yet confirmed. Chunk records without a header are skipped: their tombstone counts them.
4. **Write,** by the contracts' one rule (Initialization and releases): a target not blocked
   gets its recorded charge; a blocked one the `prev` of this call's mark, or it stays marked if
   that mark keeps none.
5. **Answer:** one `NodeCharge` per registered node, by id, and the volumes holding a claim key.

While marked, a node takes no new copy, so only releases change `prev`. A run costs one listing
of all metadata.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): the contracts (S1, S6, S7).
- `meta/domain/capacity.go` (new), `meta/domain/domain.go`: arithmetic, `Room`, `Unlimited`,
  `Place` (S3–S5).
- `meta/service/service.go`: heartbeat, `nodes()`, registration, create, move, deletion,
  confirmation, `releaseCharge`, `errStatus`, the key list (S1–S5, S7).
- `meta/service/capacity.go` (new): `InitCapacity`, report and ledger reads (S2, S6).
- `meta/service/auth.go`: `InitCapacity`; reports from the node's data plane only (S10).
- `control/drain/drain.go`: S8.
- `meta/cmd/flaminio-meta/main.go`: `heartbeat` figures, `capacity-init [--all]` and its exit,
  usage (S1, S6).
- `dataplane/crates/flaminio-metaclient/src/pb.rs`: the messages (S1, S7).
- `dataplane/crates/flaminio-node/src/engine.rs`: the report (S1).
- `meta/service/expand.go`, `expand_test.go` (#70's): the growth charge, added by #70, which lands
  with or after #72 (S3).
- **Fixtures** that heartbeat with no report send one (`meta/service/service_test.go:58–68`,
  `control/drain/drain_test.go:39–47`, `control/csi/csi_test.go:52`,
  `meta/cmd/flaminio-meta/main_test.go:103`); `domain_test.go` uses `Unlimited`;
  `scripts/dev/meta-interop.sh:31–35` issues fl-b an `io` certificate and heartbeats each node as
  itself.
- `scripts/lab.sh`: `meta up` for the whole cluster (not a single VM, as `chaos` calls it,
  `:1407`) runs `capacity-init` once after starting every instance, its output shown: an RPC
  failure (exit 1) is fatal, exit 2 (a target still marked or blocked) a warning with the CLI's
  lines; `meta-smoke`
  (`:604–615`) runs `ensure_nodes`, heartbeats only the witness as administrator, and waits for
  the data nodes' reports before its create; `capacity-check`.
  `Makefile`: `capacity-check`. `scripts/checks/resync.sh` already needs the nodes up; the stubbed
  `scripts/tools/lab-deletion-test.sh` is unaffected.

## Contracts

[Capacity contracts](capacity-contracts.md): records, transactions, protobuf, RPC, errors,
authorization, CLI, upgrade order. No replication frame, token, control-socket line or on-disk
record changes. `make proto`; `pb.rs` by hand; `make contracts` checks both and the vectors.

## Tests

| Rule | Test (file: case), level |
|---|---|
| S1 | `meta/service/capacity_test.go`: `TestHeartbeatReport`, both stores: a valid report in `ListNodes`; zero T, zero S, used + quarantined above T, slots used above S, an overflowing sum: stored as none, alive, one warning; expiry with liveness (Mem clock); no report, no candidate. `flaminio-node/tests/engine.rs`: `heartbeat_carries_capacity` (MemDevice figures). `interop.rs`: fl-a's report read back (`make meta-interop`). |
| S2 | `TestRegisterNodeCreatesLedger`: new node zero; re-registration keeps its revision; legacy record gets none; two concurrent first registrations make one ledger. |
| S3 | `TestCreateCharges` (ledgers and claim key, which the last write deletes); `TestClaimKeyOverwritten` (a stale claim key is replaced by the claim); `TestMoveChargesDestination`; `TestCompletedMoveKeepsSourceCharged`; `TestMoveToReleasedNodeChargesNothing` (#13's refusal: no ledger write). Once #70 has landed: #70's `TestExpandChargesLedger` (shared; #70 adds it, landing with or after #72) and `TestExpandRefusals` (#70's). |
| S4, S11 | `TestDeleteReleasesOnConfirm`: tombstone charges and nodes include released nodes and fence holders (one slot each); one release per confirmation, by the node's identity `auth.go` allows and by an administrator; repeat releases nothing; unparseable header writes none; underflow deletes the ledger. `TestDeleteInterruptedCreationReleasesAll`. `TestReleaseRuleByLedgerState`: readable; marked with `prev` (less the charge; underflow drops it) and without (rewritten, its revision moved); absent and unreadable (compared, not written). `TestConfirmReleaseReleasesCharge` (with #13's change, both stores, etcd enforcing 120): 118 chunks released at once, 119 refused; the first confirmation on a node keeps one slot for its fence hold, a later one none; stale (a chunk moved: `ABORTED`), replayed (node no longer released) and non-releasing (`INVALID_ARGUMENT`) calls leave the ledger unchanged; a ledger-only conflict retries; a racing create rereads. |
| S5 | `meta/domain/capacity_test.go`: `TestRoom`: both spec examples, equality admitted, one byte or slot short, either condition binding, T = 2^64−1 exact. `domain_test.go`: `TestPlaceRoom`: fill to room, preferred kept and ignored, a full and a nearly full zone under `strict` and `bestEffort` give `ErrCapacity`, a nil map refuses. `capacity_test.go`, both stores (etcd enforcing 120): `TestDrainUndrainCreates` (L + U never above the budget), `TestCreateRefusedForRoom` (full listing equal before and after), `TestCreateAt116Nodes` (succeeds) and `TestCreateTooManyNodes` (117), `TestMoveBetweenSoftAndHard`, `TestUnreadableLedger` (create, move). |
| S5 contention | On `interleaveStores` (`service_test.go:619,841`): `TestInterleavedCreatesDoNotOverAdmit` (after A lists `capacity/`, B creates; room for one: A rereads and gets `RESOURCE_EXHAUSTED`; without the ledger compares both commit), `TestInterleavedMoveAndCreate`, `TestInterleavedConfirmAndCreate`, `TestCreateRetriesOnLedgerConflict`, `TestCreateContention` (a ledger rewritten before every claim: `ABORTED` after 32). |
| S6 | `TestInitCapacity`, both stores: a fixture with a pending and a completed move, a creating volume with and without a claim key, a fence holder with no other copy of its volume and a tombstone pending on another (one slot, no bytes each), a partly confirmed tombstone, a legacy tombstone and an unparseable header: exact rows, blocked nodes, claims listed; a second call changes nothing; a call naming one node (not `all`) while another, readable node is pending on a legacy tombstone: the untouched node's row has an empty `blocked_by`, `marked` false and its ledger unchanged. `TestInitRacingConfirm`, both stores, with `ConfirmDeletion` and with `ConfirmRelease`, for a node with a readable ledger, a node with none, and a blocked node: a release before the mark is counted; a release between mark and snapshot, or between snapshot and write, leaves the node marked, with `prev` less the release where it had one (for the node with none, the mark's revision moved and no `prev`), and the rerun is exact (the blocked node restored to that `prev`); a release on an absent ledger as the mark is written fails its revision-0 compare and reruns. `TestInitBlockedWithoutPrev`: a node with no ledger pending on a legacy tombstone stays marked, blocked by it; once the node confirms it, a rerun is exact. `TestConcurrentInit`: a second call's mark makes every write of the first fail, its nodes left marked. `TestInitAbortedAfter32`. `TestInit121Nodes` (two mark batches, both stores, etcd enforcing 120). `TestInitAfterOldBinary`: an older binary's rewrite of a pending tombstone keeps its nodes blocked, restored to their earlier ledgers; once the tombstone clears, `all` makes them exact. Unreadable and marked ledgers are replaced; `previous_*` and `had_ledger` report a mark's `prev`. |
| S7 | `TestHeartbeatReport`; `meta/cmd/flaminio-meta/main_test.go`: `nodes` JSON has `capacity`. |
| S8 | `control/drain/drain_test.go`: `TestDrainSkipsAFullDestination`, `TestDrainSkipsAnUnreportedDestination`, `TestDrainFailsWhenEveryDestinationIsFull`. |
| S9 | `meta/domain/domain_test.go` over `meta/testdata/isr-vectors.jsonl`, as #13 leaves it; this plan changes no vector. |
| S10 | `meta/service/auth_test.go`: `InitCapacity` for an administrator only; a report from the node's `io` identity allowed, from its agent, another node's `io` or an administrator `PERMISSION_DENIED`. `main_test.go`: `heartbeat n 100G 3200` sends a report, `heartbeat n 100G` exits 1 without a call, `capacity-init --all` prints rows; exits 2 with the contracts' line for a raced node, a blocked node whose restore raced a release, a blocked node without `prev`, and a blocked node restored from `prev`; exits 0 when only a node the call did not target is pending on a tombstone without charges; exits 1 on an RPC error. |

Proof: `make lint test race hygiene contracts`; `make sim` as CI runs it, unchanged. No
simulator check or injected bug: no replication, membership or recovery rule changes (S9).
`crash-test`, `miri` and `sanitize` cover nothing this changes. Nothing has run yet.

## Lab

`make capacity-check` (`scripts/lab.sh capacity-check`). Needs: metadata service and the nodes
of fl-a and fl-b up, their sweeps running, no other create, move or delete meanwhile; one disk
per node (100 GiB in the lab, `docs/lab.md:11`). Volumes `cap-<t>-*`, RF 1, 256 MiB chunks:
1. `capacity-init` exits 0; fl-a and fl-b have a ledger and a report. Their ledgers are noted.
   It fails, printing fl-a's figures, if fl-a's U is above its ledger (copies no record names),
   or if fl-a's budgets leave less than one chunk at soft or between soft and hard.
2. `cap-<t>-x`, sized at the sum of every data node's T: exit 1, `RESOURCE_EXHAUSTED`, no
   volume, no ledger change.
3. fl-a's room at soft, R, from `nodes` by S5's rule. `cap-<t>-a`, R × 256 MiB, preferring fl-a:
   every chunk on fl-a, its ledger up by exactly R × 256 MiB and R slots, its room at soft 0.
   Step 1's U check is what makes L, not U, bind, so the room ends at exactly 0.
4. `cap-<t>-b`, 256 MiB, preferring fl-b. `move cap-<t>-b 0 fl-b fl-a`: applied.
5. Both deleted through `delete_volume`, whose tombstones the nodes' tombstone sweeps confirm
   (the CSI node plugins' today, #13's data-plane sweep after it); fl-a's and fl-b's ledgers
   equal step 1's. The move of step 4 is never carried out, so it releases nothing.

Pass: every step as stated. It writes no data, formats nothing, deletes only its own volumes;
on failure it prints the ones left, for `delete_volume`.

## Docs

- **`docs/lab.md`:** `make capacity-check`; `meta up` initializes older nodes' ledgers; creates
  and moves need nodes whose data plane reports; `capacity-init` exits 2 when a target is still
  marked or was restored blocked, with the contracts' line saying what to do.
  **`README.md`, Makefile help:** the new target and `capacity-init`. **`meta.proto`:** the
  changed comments.
- **`docs/followups.md`:** item 13: a released copy stays charged until its node confirms its
  deletion through #13's sweep; while release deletion is off, one deleted by hand or lost with
  a disk stays charged until its volume is deleted, so hand deletion frees no capacity; a node
  gone for good keeps its charges, which do not matter, and its ids taken (#13's S11); an
  interrupted creation holds its claim until deleted (`flaminio-meta delete`),
  and `capacity-init` names it; a row for #72's open steps (the intent's list).
- **None:** ARCHITECTURE (§8.2, §11.6 describe the target), `docs/performance-plan.md`, benchmarks.

## Risks

- **Safety:** admission only. Fencing, epochs, ISR decisions and durability do not change. A
  wrong charge at worst lets a store run out: each replica that then fails to allocate is a
  failed member, removed above the minimum ISR, else its writes wait
  (`flaminio-repl/src/target.rs:1296–1298`); never an acknowledgement.
- **Crash points:** each ledger change commits with its placement or not at all. A retried
  create finds its volume, a retried move fails `CheckMove`, a repeated confirmation is
  `already`. An interrupted, `ABORTED` or raced initialization leaves its nodes marked: they take
  no copy, rebuild and drain moves included, until a rerun; with `all`, that pauses every create
  and move in the cluster until its writes land.
- **Copies no record names** (from before this change, or a release an older binary dropped)
  count only through U. A node holding them and unwritten commitments can be over-committed by
  the smaller of the two. On a replaced disk, every copy stays charged: a replica until it is
  moved off, when it becomes released, and a released copy until #13's sweep, with release
  deletion on, confirms it absent; with it off, until its volume is deleted. A fence holder is
  charged one slot even while its store still holds chunks of the volume: conservative.
- **A misreporting node** (one compromised `io` identity; availability only, never an
  acknowledgement): an inflated T makes it the candidate with room once honest nodes are full, so
  it takes copies that then fail to allocate; reporting no room as its zone's only member makes
  every create spread over zones fail, `bestEffort` included.
- **Security, who must not:** no RPC sets a ledger; only administrators initialize; only a
  node's data plane reports, for itself. A confirmation sent early releases that node's charge
  (#14's risk): `ConfirmDeletion` by the identity `auth.go` allows for the node (its agent today,
  its data plane after #13's S12) or an administrator; `ConfirmRelease` by its data plane.
- **Compatibility:** without a reporting data plane a node takes no new copy (fail closed). Older
  binaries ignore the new keys but drop `charges` when they rewrite a tombstone (Q3's procedure).
- Settled at approval, 2026-11-13: the spec's Q1, Q3–Q5, and the handling of copies no record
  names: delete them before `capacity-init` (see decisions.md, Settled at approval).

## ADRs

- **Relied on, consistent:** 0001 (Go service, Rust report); 0002 (etcd conditional
  transactions); 0004; 0005 (no lease or I/O-path change); 0006 (a 2-node edge site places RF 2
  while both nodes have room; no reserve rule here). 0003 does not apply.
- **Accepted with the plan, 2026-11-13:** [per-node capacity ledger charged in the placement
  transaction](adr-proposal.md).

## Parallelizable?

Partly. The Rust units (`pb.rs`, the engine's report and its test) and the Go proto, CLI, drain
and lab units build against the contracts alone; `make meta-interop` joins them. The Go domain
and service units follow #13's metadata rules and S11's change to them: `VolumeCharges`,
`DeleteVolume` and `InitCapacity` read `Chunk.Released` and `Header.Fenced`; `MoveReplica`
relies on `CheckMove`'s refusal; `ConfirmRelease` carries the release rule; and so do their tests
(`TestCompletedMoveKeepsSourceCharged`, `TestMoveToReleasedNodeChargesNothing`,
`TestConfirmReleaseReleasesCharge`, `TestInitCapacity`'s fixture).
