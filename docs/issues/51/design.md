# Issue #51: Durable snapshot cuts and generation-ordered catch-up — design

Approved 2026-11-13; not implemented; changed after approval in plan review rounds 1 and 2
(decisions.md). [Spec](spec.md), [intent](intent.md), [contracts](contracts.md),
[proposed ADR](adr-proposal.md). Scope is steps1/2/7;
**#51 stays open** for deletion/merge, CSI clones, groups, quiesce/hooks and external retention.

The facts it starts from:
- Store snapshot pushes existing LayerCreate after settling writes, caps64 layers
  (`flaminio-store/src/store.rs:1417`); maps resolve newest-to-oldest (`state.rs:131`).
  Empty layer push uses a header entry; pool allocation occurs on subsequent mapped writes.
- Write/Copy/Read/Digest/Mapped lack generations (`repl/src/msg.rs`, `net/src/frame.rs`).
  Copy/Discard can update top only; no current journal operation writes below an existing cut.
- Target dirty sets are current-epoch memory, resync current content; metadata and simulator
  have no snapshot catalog/layers. Existing offline digest uses MVP Volume::open, unsuitable
  as a supposedly read-only snapshot reader if it creates/fences missing chunks.
- Audit proposed0011 requires operation IDs/recoverable commit and untouched-chunk semantics;
  proposed0009/#50 requires retained-key/data identity decisions. Neither ADR is accepted here.

## Catalog and atomic publication (S1–S6, S10)

Header gains SnapshotGeneration, SnapshotNextOrdinal and SnapshotCount, default0/omitted, and
SnapshotChargeNext, a `*uint32` with `omitempty`: nil is absent, and the mark's cursor of 0 is
written (R3-C1). Separate v1 `snapshots/<volume>/<first_epoch>/<name>` records contain operation_id
(random128 bits), ordinal, state=Charging|Pending|Committed, requested timestamp, generation and
commit_epoch. Pending is durable prepare, not a readable completed snapshot. Name/idempotency is
scoped to immutable volume incarnation. Request checks room, then compares expected
FirstEpoch/header and absent name and atomically writes a Charging record and increments
count/ordinal (its mark); the node charge follows in batches, the last making the record Pending
(Roles, audit and charges; R2-C1). Same name returns existing operation. Counts include pending and
never exceed63; no generation/name is reused.
The request's header write moves the header's revision (contracts, One catalog per header
revision): racing ChangeIsr and MoveReplica retry on their compare, and #77's pinned Attach
answers `Aborted` committed-superseded, its caller attaching again. `readVolumeView` (#77) lists
the current incarnation's catalog prefix between its two header reads; every catalog write also
writes the header, so the catalog it returns is that header's (R1-C9).

CommitSnapshot compares header revision/current target/current epoch/expected generation, exact
oldest pending operation/revision (a Charging record is never cut) and volume lifecycle. One bounded
transaction bumps generation and marks that operation Committed. No per-chunk metadata writes are
needed. Retry of an applied operation returns its same committed generation; stale epoch never cuts
another pending operation. Lost reply is resolved by reading the operation and volume, not by
aborting an uncertain committed cut. Header/catalog writes serialize with attach/delete.

DeleteVolume marks its existing lifecycle first; stale requests/cut writers then fail compares.
The catalog goes after the header, as chunk records do (R1-C4, R1-S6): once the transaction that
deletes the header commits (today's, `service.go:565-573`, or #73's `finishDeletion`),
`deleteChunks` (`service.go:481`) also lists `snapshots/<volume>/` and deletes it in batches of
at most `kv.MaxTxnOps` deletions, each conditional on the header being absent. Its two other
callers do the same: DeleteVolume's header-absent branch (`:525-531`), which finishes a
deletion that died after the header's transaction, and a creation's claim (`:441`), under the
claim's compare, which clears an earlier incarnation's leftovers before the new one writes any.
The header's transaction gains no operation, so #73's and #50's counts there stand. Reads list
`snapshots/<volume>/<first_epoch>/` only, so a leftover is never shown before it goes. Retain
#76's creation ownership and #73's deletion ownership. Snapshots in this slice die with the live
volume; no external snapshot/clone/backup reference is exposed. New incarnation starts0 and
empty; old callbacks carry FirstEpoch/opID and cannot publish to a recreated name.

Volume replies add generation/catalog, including Attach. RequestSnapshot refuses an encrypted
volume before any write (S2; R1-C11, R1-S3): #50's `Volume.encryption` (field 20) with a profile
other than `none`, or #50's mark in the header's `id`, as #50 requires ("no encrypted snapshots
... until their catalog pins this descriptor", #50 contracts), until a later step pins its keys;
whichever of #50 and #51 lands second adds this refusal and its two test cases (R2-S3). No
pretend linked-clone key sharing. No other profile is checked at request: the service does not
know a target's version; the `snapshot-v2` gate (contracts) is at GetVolume. Source/metadata
fields remain visible only under their existing RPC caller rules.

## Roles, audit and charges (S2, S4, S6; #72, #73)

- **Roles** (R1-C3, R1-S1): contracts' table. `auth.go`: `RequestSnapshot` `allowed = admin`,
  and once #73 is active its role check, `cluster-admin`; `ListSnapshots` `allowed = true`;
  `CommitSnapshot` `allowed = id.Node != "" && id.Component == "io"`, the target check the
  method's, answering `applied=false` as ChangeIsr does (R1-C8). Whichever of #51 and #73 lands
  second adds the rows to #73's table and the role tests.
- **Audit** (R1-C3, R1-S4): no event in this step. `docs/followups.md` gains "Audit events for
  snapshot operations": `snapshot.requested` and `snapshot.committed` through #73's event
  append, by #73's S7 and its decision that each later issue owns its hook; issue to be filed.
- **The charge** (R1-S2, R1-C3; round 2: R2-C1, R2-C2, R2-C3, R2-S1, R2-S2; contracts,
  Retention charge): one multiplier, m(*j*), read from the header the transaction compares,
  wherever #72 or #73 charges or releases a copy of chunk *j*: 1 + `snapshot_count`, less one for
  a chunk the request's batches have not reached (`snapshot_charge_next` present, *j* at or
  after it). It multiplies a copy's bytes and its slot alike: a layer takes one slot. The
  request checks room, marks (record Charging, count, `snapshot_charge_next` = 0, the quota),
  then charges one chunk size and one slot per named copy in batches by chunk index, each
  comparing the header and its chunk records. So a move that commits before a batch reads its
  chunk charges m(*j*) itself and the batch adds one; one between the batch's read and commit
  makes the batch read again (its chunk compare); one after the batch fails its header compare
  and charges the new count (R2-C1). Once #73 is active the mark compares `owners/<id>`, absent
  included, and `tenant/authorization`, as #70's step does, so a racing `AssignOwner` makes it
  read again (R3-B2). A batch that finds a named copy's node without a valid report or its ledger
  marked by InitCapacity answers `FailedPrecondition` naming it, earlier batches kept (R3-C3).
  No node limit remains: any volume #72 admits (116 nodes at creation, more after growth and
  moves) can be snapshotted (R2-C2). Each site and the plans whose last to land applies it, with
  its test:
  - the request's node charge: #51, #72; its quota charge: #51, #73;
  - #72's `VolumeCharges` (so the tombstone releases it all) and `MoveReplica` (its destination,
    `Room` at `HardPPM` checking m(*j*) chunk sizes and slots): #51, #72;
  - #13's `ConfirmRelease` (m(*j*) for a released copy): #13, #51, #72;
  - #70's growth (each new copy at m(*j*), the quota (1 + `snapshot_count`) times the step's
    bytes): #51, #70, #72 for the ledger, #51, #70, #73 for the quota;
  - #73's `AssignOwner` of a tenant: (1 + `snapshot_count`) times the size, so snapshots taken
    before activation or on an unassigned volume are charged once it is assigned (R2-C3, R2-S2):
    #51, #73.
- **What the slot charge bounds:** the page pool (R2-S1; round 1's named risk, withdrawn). Each
  (slot, layer) has one page table (format §5.4) and at most ⌈C/E⌉ map pages, C the chunk's
  clusters and E the map entries a page. With the charge, a node's layers L and their clusters
  stay within its slots S and data clusters N, so its pages are at most
  L × (1 + ⌈C/E⌉) ≤ 2L + N/E ≤ 2S + N/E, within `pool_pairs` = 2⌈N/E⌉ + 2S (format §2.1). The
  slot charge binds only for chunks below 32 MiB (`chunk_slots` is a slot per 32 MiB of data);
  at 1 MiB (`domain.go:180`) it is what keeps sparse layers from filling the pool and refusing
  other volumes' map pages. S13's full pool stays for a store without a charge.

## Target barrier and lazy untouched chunks (S7–S10, S14, S23)

Add Target snapshot phase `Idle|Draining(op)|AwaitCommit(op,from_generation)`. Learn Pending
records from ordinary polls, oldest first; a Charging one is not drained for (S2). Wait all
chunks Ready before holding client writes; then hold new client writes in original queue order,
drain all current ISR client/repair writes and queued pre-barrier writes. Do not hold reads or
cancel/reissue them because of a cut.
Catch-up copies to out-of-ISR replicas are separate staged work and do not block a cut.

Commit uses the same single metadata mutation slot as ISR changes, so a pending join/replica
transition cannot race around a cut locally. Draining does not discard old sends or their
original generation. Applied/current operation readback updates local generation before held
writes resume. An unresolved reply keeps them held. Refusal at a newer epoch deposes; same
operation pending at the old generation remains held/retried, rather than unholding while an
older commit RPC could still apply. No snapshot timeout substitutes for authoritative outcome.

All client/repair writes capture generation at creation; retransmission retains that generation.
A new target starts at the returned committed generation, reconciles current content normally,
then considers pending operations. Every accepted cut already drained ISR writes, so no uncertain
historical write may need repair below it. A restarted replica's broad uncertain set is repaired
only in the current generation; existing completed snapshots remain immutable.

Physical layer presence does not define whether a cut exists. A snapshot at g resolves through
only layers with generation<g, even if no layer g was ever created. A chunk untouched over many
cuts has identical views for them, determined by the committed catalog after failover. Metadata
O(1) commit does not imply constant drain, reconciliation or read latency.

## Replica write ordering and store views (S11–S14, S19–S23)

Write/Copy/Discard carry generation h. Replica checks session/epoch first, then h versus durable
top generation. Higher: settle prior touched commits and durably LayerCreate(h) before any h
payload acknowledgement; same: existing write path; lower: no effect on the store, and the
replica ends the session with `Refused`, which has no `req` and closes the connection
(`frame.rs:110-113`); the target takes the replica as failed, as on a store failure
(`replica/mod.rs:11-12`), and its ISR removal follows as today (R1-C7). Async
pending-write records retain generation through staging/pipelining. No store::snapshot call
blocks the engine while an unrelated commit is outstanding: queue this metadata operation
until store is ready, keeping current control/heartbeat work responsive.

Add Store View::{Current,Before(g)} versions of read/plan_read/digests/mapped and
changed_ranges(key,a,b_or_current,extent). Resolve selected layer prefix, preserving Zero masks
and read pins; Changed unions map/zero references in generations[a,b), clipping/coalescing to
request. Cluster-level overapproximation is allowed; missing a changed4KiB block is not.
A max64-layer bounded iterator replaces full-volume materialization. Read-only methods never
push layers or fence. Layer cap/full pool returns existing OutOfSpace; an empty push does not
invent an allocation cost. Retain existing disk record encoding/version, add decoder/replay
monotonic-generation validation and fuzz cases without changing feature bits.

Offline tool `flaminio-io snapshot-read` opens only a stopped store, takes volume/firstEpoch/
cut/range and outputs data or per-chunk digest. It never routes through MVP Volume::open or
creates/fences chunks. Store recovery may replay already committed records; inspection itself
mutates none. Refuse if node/device lock is held. This is not a snapshot block device/CSI API.
It finds chunks by the node's key, `flaminio_node::volume_uuid` (SHA-256 of the name), not the
MVP padding `flaminio-io/src/cli.rs:41-48` uses, and refuses a chunk whose `fenced_epoch` is below
`--first-epoch`: a stopped node that missed a delete and re-create still holds the earlier
volume's chunks under the same key until #30's drop at the next `Hello` (R1-C10, R1-S7).

## Catch-up, sparse history and joins (S15–S18)

Replace one current round with per-chunk `Catchup {member,epoch,session,stage,known_generation}`.
Read destination layer generations, then inspect stages0…current in increasing order. Source
for a sealed stage h is an ISR member's Before(h+1); current stage uses Current. Compute
source/destination changed/mapped ranges and digests at that stage; include explicit zero masks
so stale extra data on the destination is cleared, not omitted as a hole. Full/unknown catch-up
checks every stage; target-owned dirty ranges are tracked per generation, including inflight
and aborted-live work. They are an optimization, never permission to skip unproved history.

All stage h copies/discards must be durably acknowledged before advancing to a later stage.
A joiner receives live writes at current generation only after every older stage is complete;
copy filtering protects live writes within that current stage. A cut can add another stage
without waiting; immediately suppress new-generation live forwarding to a lagging joiner.
Stage/session identities fence old replies (mutation `stale_stage_reply`), and restart
conservatively repeats/verifies stages.

Existing top>h cannot be overwritten under today's format. Verify its historical view against
authority and skip only if equal. A historical mismatch below that top fails closed and blocks
join; do not silently write it into current, delete/recreate the chunk or invent under-layer
records. The model must establish that correctly ordered catch-up never creates this state
under supported crashes; if it can, S15's no-format-change choice is a blocker needing revision.
Durable top alone is not assumed to prove snapshot completeness after arbitrary disk replacement;
#61's disk-incarnation gap must be closed before claiming that failure model is supported.
Settled at approval, 2026-11-13: a below-top mismatch fails closed and blocks the join (see decisions.md, Settled at approval).

Join requires every committed snapshot verified and all existing live-write/filter conditions.
Share the ISR metadata slot with cuts; if a new cut committed, recheck catch-up frontier before
proposing join (mutation `join_before_frontier`). Move source drops only after this join, #13
reclaim unchanged. Source reads remain ISR/session/lease-authorized; no non-target reader
authorization is introduced by snapshot views.
Bound request extents/frames and use existing bulk limit; never allocate N×generations payloads.

## Budget, rollout, files and tests

Snapshot catalog adds bytes to #80's shared full-response budget. The snapshot-capable bound
starts from #13's (clear; `docs/issues/13/design.md`, Releases), which changes #80's (R1-C1):

```text
B_s(N,R) = 4160 + 12549 + 415 + N × (288 + 264 × (R+1) + 132)  ≤ 4,194,304
caps N: 4406, 3446, 2830, 2400, 2084 for RF 1 to 5  (#13's: 4419, 3457, 2838, 2408, 2090)
```

4160 is #13's constant (#80's 2048 and #13's 16 `fenced` names); 12,549 the full catalog
(contracts, Size: 63 entries of 199 bytes and field 30's 12); 415 #50's maximal
`Volume.encryption` and storage mode (#50's contracts, Size), kept although a snapshot-capable
volume is plain (9 bytes). The per-chunk term is #13's worst chunk, a move beside one release.
RequestSnapshot checks `B_s(N, RF)` before any write (`FailedPrecondition` `volume <id> of N
chunks at RF R cannot carry 63 snapshots in a 4 MiB reply`), and so does every growth step of a
volume with any snapshot, at the new N (#70's ExpandVolume; R1-C2): whichever of #70 and #51
lands second adds it. Snapshot-free volumes keep #13's geometry. #80's worst-view fixtures
(Create, Get, Attach) gain a volume at each `B_s` cap with 63 maximal entries, #13's worst chunk
and #50's maximal descriptor, and prove `proto.Size` within 4 MiB; CommitSnapshot's response
(the volume and one `Snapshot`, at most 198 bytes, within the 406 a plain volume leaves of the
415) joins them. The last of #13, #80 and #51 to land applies the constant, the term and the
fixtures. Never truncate entries, raise RPC limits or silently make earlier max volumes unusable.
Settled at approval, 2026-11-13: the budget eligibility, as proposed; its bound changed in round 1
(decisions.md).

A chunk added by growth while the volume is at generation g (R1-C2) gets its generation-0 layer
at creation (`ChunkCreate`) and a layer of g at its first write, so `Before(h)` for h ≤ g resolves
its empty layer 0: zeros in every snapshot at or below g (S19). Catch-up finds nothing to copy
below g for it; a cut sees it as any other chunk, reconciled with the rest.

Coordinated wire-v2 rollout only: old/new sessions refuse before incompatible interpretation,
and io GetVolume must advertise snapshot-v2 once a volume has pending/committed snapshots;
old io cannot start a local-only target by skipping unknown fields.
all nodes/services/clients upgraded before first request. No N−1 support/finalize claim (#48).
Touched: proto/gen; meta domain/service/auth/CLI; Rust meta/msg/target/replica/sim and vectors;
net frame/session/fuzz; metaclient pb/lib/interop; store state/store/view tests; node adapter;
daemon offline tool; scripts/checks/tools, Makefile and docs (`docs/followups.md`: the audit
follow-up, issue to be filed). Exact schemas/bytes in contracts.

Each rule's tests, by file (R1-C12). Go tests run against both stores (in-memory and etcd
enforcing 120 operations) where they write.

| Rules | Tests |
|---|---|
| S1–S6 | `meta/service/snapshot_test.go`: `TestRequestSnapshot` (each refusal of contracts in order, an encrypted volume by field 20 and by the header mark, the `B_s` refusal at each cap and one chunk above, 63 counted with pending, idempotent name and operation id, stale `expected_first_epoch`), `TestRequestSnapshotCharges` (ledgers, by bytes and slots, quota and owner record raised once; no room at soft, for slots alone with 1 MiB chunks too, no quota, no report on a down node or a released copy's, nothing written; 300 nodes charged in several batches; a batch without room keeps earlier batches, the snapshot Charging and not cut, and the next request completes it, as does a retry after a failure between batches; a stop right after the mark (cursor 0, written and read back as present) followed by a MoveReplica, and a deletion with the snapshot Charging at cursor 0 and at a middle cursor: the ledgers equal Σ m(*j*) and the tombstone releases exactly what was charged (R3-C1); a report expiring after the check, and a ledger marked by InitCapacity between batches: that batch answers `FailedPrecondition` naming the node, earlier batches kept, the snapshot Charging, and once the node reports again or the run ends the next request completes it (R3-C3); with #73 active, an `interleaveKV` hook after the mark's read of `owners/<id>` that assigns the volume: the mark reads again and charges the quota once (R3-B2); a request refused by its own checks leaves a Charging snapshot as it was, and one naming it completes it (the contracts' order); with #73, an assignment after snapshots taken before activation and while unassigned charges (1 + k) times the size; the tombstone and #13's release give it back), `TestCommitSnapshot` (each predicate, a Charging record refused; another node's `io` and a deposed target `applied=false` with the current epoch and generation; lost reply; retry of an applied operation), `TestSnapshotRaces` (a request between Attach's commit and its pinned read: `Aborted`, the retry attaches; request against ChangeIsr and MoveReplica: both commit; an `interleaveKV` hook after a charge batch's chunk List commits a MoveReplica to a node with no copy of the volume: the ledgers equal `VolumeCharges` at the new count, and so does the tombstone's release, R2-C1), `TestDeleteClearsCatalog` (a failure after the header's transaction, between catalog batches, in the header-absent retry; a re-creation that clears leftovers under its claim; no read lists an earlier incarnation). `meta/service/auth_test.go`: `TestSnapshotRoles` (before and once #73 is active: an `audit-reader` or `deletion-override` caller refused a request; a non-`io` caller `PermissionDenied` on CommitSnapshot; insecure instances as contracts). `meta/service/expand_test.go` (#70's): `TestExpandWithSnapshotsKeepsBound`. #80's worst-view fixtures with the catalog. `meta/cmd/flaminio-meta/main_test.go`: `TestSnapshotCommands`. `flaminio-repl/src/meta.rs` tests and `make meta-vectors` (request, charge with a Charging record not cut, commit, attach, delete transitions in `meta/testdata/isr-vectors.jsonl`), replayed by `meta/domain/domain_test.go`. |
| S7–S10, S14 | `flaminio-repl/src/target.rs` tests: `cut_drains_in_order`, `reads_served_during_cut`, `cut_refused_resumes_at_answer`, `cut_without_answer_stays_held`, `pending_cut_in_request_order`, `new_target_starts_at_committed_generation`, `untouched_chunk_over_many_cuts`, `crash_before_and_after_commit`. |
| S11–S13 | `flaminio-store/tests/store_behavior.rs`: `push_layer_before_newer_write`, `write_below_top_is_refused`, `layer_cap_and_full_pool`, `layers_within_slots_fit_pool` (every slot taken by 1 MiB chunks' layers, each mapping one cluster: no OutOfSpace from the pool, R2-S1); `flaminio-store/tests/crash_consistency.rs`: layer push at every crash point, async staging; `flaminio-node/tests/engine.rs`: `write_below_top_ends_session` (`Refused`, store unchanged, the target removes the replica); `flaminio-net/src/frame.rs` tests: `v2_round_trip`, `v2_bounds`, `reply_tags_not_confused` (72 and 73 against #13's 71); `dataplane/fuzz/regressions/repl_frame/` inputs. |
| S15, S16 | `flaminio-repl/src/target.rs` tests: `catchup_across_one_and_many_cuts`, `catchup_into_empty_chunk`, `catchup_clears_extra_data`, `catchup_full_history_when_unknown`, `higher_top_match_and_mismatch`, `session_lost_at_each_stage`; `engine.rs`: `move_destination_gets_every_snapshot`. |
| S17, S18 | `target.rs` tests: `cut_at_each_catchup_stage`, `no_live_write_ahead_of_frontier`, `join_rechecks_after_cut`, `stale_stage_reply_ignored`, `drop_only_after_join`. |
| S19–S22 | `flaminio-store/tests/store_behavior.rs`: `views_at_each_generation`, `changed_ranges_cover_model` (property, cluster sizes), `chunk_created_later_is_zero_below`; `scripts/tools/snapshot-read-test.sh` (in `make lint`'s list): lock held, bad arguments, no chunk created, an earlier volume's chunk (fenced below `--first-epoch`) refused, the node's key derivation. |
| S23 | `engine.rs`: `plain_reads_unchanged`, `no_metadata_call_outside_cut`; `store_behavior.rs`: format version and feature bits unchanged. |
| S24 | `scripts/tools/snapshot-check-test.sh` (in `make lint`'s list): a `VOL` from the environment is not used, and an existing volume of the generated name refuses before any write or cleanup; the docs commands and limits; `TestSnapshotCommands` prints no Pending or Charging snapshot as ready. |
| Invariant | `flaminio-repl/src/sim/check.rs`: the four checks of the spec; `sim/tests.rs` mutations (`target::Bugs`, `replica::Bugs`, and a new `meta` field of `sim::Bugs` for the metadata model), each caught on a seed that passes clean: `catches_cut_with_writes`, `catches_cut_unreconciled`, `catches_stale_epoch_cut`, `catches_skip_layer_push`, `catches_current_only_catchup`, `catches_skip_old_generations`, `catches_live_ahead_of_frontier`, `catches_join_before_frontier`, `catches_stale_stage_reply`, `catches_changes_top_only`, `catches_early_publish` (R1-C6). |

Implementation: `make proto meta-vectors` after the contract changes, then the pre-PR set
`make lint test race hygiene contracts`, with `make sim crash-test` and `make sanitize QUICK=1`
for the new pinned async lifetime handling. No tests, simulations or lab commands ran in
drafting.

## Lab, docs, risks and ADRs

The check (R1-C12, R2-C4): `scripts/checks/snapshot.sh`, run from the host as
`FROM=fl-a TO=fl-b scripts/checks/snapshot.sh`, as `scripts/checks/resync.sh` is (`resync.sh:6`).
New with it: a `scripts/lab.sh snapshot-check` subcommand and a `make snapshot-check FROM=…
TO=…` target (neither exists for `resync.sh`). It names its volume itself,
`snapshot-$(date +%Y%m%d%H%M%S)`, as `resync.sh` does, takes no `VOL`, and refuses before any
write if that volume exists, so its cleanup never deletes a volume it did not create.
`scripts/tools/snapshot-check-test.sh` checks its arguments, quoting and refusals offline; it
and `snapshot-read-test.sh` join `make lint`'s explicit list (`Makefile:92-94`). It is a lab
target: run only when asked.
Later authorized exclusive lab: private≤64-chunk volume, history-recorded per-block workload;
cut under load, failover, member stopped across two cuts, and move after cuts. Stop nodes only
after collecting committed catalog/epochs, then compare each replica's offline snapshot against
cut-history bounds and peers; restore services/attachments without formatting. Record held-write
interval/no-snapshot baseline and first-small-write COW cost under performance-plan repetitions.
Cleanup deletes private volume and its catalog/replicas; tombstone confirmations follow authorized
#14 operator procedure, never forged by this script. No group freeze/thaw or retained clone.

Update protocol §9 mutation catches, architecture §13 as implemented, README/lab commands and
limits, the charge. The ADR proposal records durable prepare/commit and lazy semantics
consistent with audit0011; it is accepted with the plan. Q1–Q8, budget eligibility and the
below-top historical mismatch are settled (decisions.md, Settled at approval).
A failure to prove catch-up frontier means revise format/scope before implementation, not accept
an unverified snapshot. Retention/group publication/GC pins and encrypted data identities are
future #51/#50/#53 work. Go catalog and Rust layer/view work can parallelize against contracts;
barrier/catch-up/model mutation integration is one safety unit and needs separate fresh reviews.
Risks named, not accepted: a request's header writes (its mark and, since round 2, each charge
batch) can make a racing Attach answer `Aborted` after its commit (Catalog; round 1); a request
whose charge batches stop (no room after the check, or a crash) leaves a Charging snapshot, counted
and not cut, until a later request of the volume that is not refused completes it (Roles, audit
and charges; round 2); a
batch that finds a node gone down or its ledger marked by InitCapacity leaves it Charging until
the node reports again or its copy is moved off and `ForgetNode` removes the released copy (#13;
or InitCapacity's run ends): meanwhile
every request of the volume answers `FailedPrecondition`, so it takes no snapshot, and its count
and quota stay held (R3-C3). Round 1's uncharged page pool is withdrawn: the slot charge bounds it
(R2-S1).
Settled at approval, 2026-11-13: Q1–Q8, budget eligibility and the mismatch rule as proposed;
the ADR is accepted with the plan (see decisions.md, Settled at approval).
