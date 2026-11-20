# #51 plan review, round 3 (incremental, da886c5..08e9028), design-code and design-security

Scope: `git diff da886c5..08e9028 -- docs/issues/51/ ':!docs/issues/51/report.md'`, in
/home/jsfakian/Documents/src/flaminio-51.

## Earlier findings
- R2-C1: fixed, except for the cursor's encoding at 0 (R3-C1). A header mark comes before the batched charge, and m(j) covers a move before the mark, between the mark and the batch, inside the batch's read window (the chunk compare) and after the batch (the header compare).
- R2-C2: fixed. The node limit is withdrawn. The down-node and released-copy refusal is stated in S2 and tested.
- R2-C3: fixed. AssignOwner charges (1 + `snapshot_count`) × size. A race is left open by the mark's compares (R3-B2).
- R2-C4: fixed. The check runs from the host as `resync.sh` does (`resync.sh:6,15`). The subcommand and the target are named as new. The volume is generated and refused if it exists. Both script tests are in `make lint`'s list (`Makefile:92-94`).
- R2-S1: fixed. The arithmetic holds against the code. `pool_pairs = ceil(N/E)*2 + chunk_slots*2` (`flaminio-format/src/layout.rs:116`, `consts.rs:55,57`), with one slot per 32 MiB of data (`MIN_CHUNK_SIZE` 64 MiB × `CHUNK_SLOT_OVERCOMMIT` 2, `consts.rs:47,51`). Each layer gets one page table and at most ⌈C/E⌉ map pages (`flaminio-store/src/state.rs:498-517`). There is one store per node (#72 spec, Terms), so a per-node ledger bounds the per-disk pool.
- R2-S2: fixed (the same fix as R2-C3).
- R2-S3: fixed. The refusal is owned by whichever of #50 and #51 lands second.
- Nits: all four fixed. The snapshot-read reason is corrected. CommitSnapshot's response (195 + 1 + 2 = 198 bytes) joins the fixtures. Each part's owner is "the last to land". The Roles section names the Authorize classes.

Checked and sound, no finding:
- **The batch's operation count.** A batch makes 1 + b + d compares (2 + b + d on the last) and d + 1 writes (d + 2 on the last). It charges no quota, so #73's four compares do not apply. #72's own ledger-only writers (`MoveReplica`, `ConfirmRelease`) compare no tenancy keys either (#72 capacity-contracts, Transactions).
- **Concurrent writers.** `ConfirmRelease` and growth write the header, and `ChangeIsr` and ForgetNode write the chunk records, so a batch racing any of them retries. #70's step writes the grown chunks with the header size (#70 design:67-73), so the last batch sees them.
- **Crash points and charge totals.** A crash after the mark or after any batch leaves the ledgers equal to Σ m(j). InitCapacity stays exact through `VolumeCharges`.
- **Proto.** `Volume` fields 30 and 31 and the new `Snapshot` and `State` (CHARGING = 2) are claimed by no clear plan. A state is still at most 2 bytes, so `B_s` is unchanged.

### R3-C1 — Medium
**`snapshot_charge_next` = 0 cannot be told from absent as the design specifies the field. At cursor 0, R2-C1's mis-charge comes back.**
- **The contract:** the cursor is "present only while a request's node charge is in progress", and the mark writes it as 0 (contracts.md:15-16, 77-78).
- **The design:** design.md:22-23 lists `SnapshotChargeNext` with the other new fields as "default0/omitted". The header's optional fields are `omitempty` scalars (`meta/domain/domain.go:47-53`). So the mark stores no cursor, and every site reads m(j) = 1 + k_new.
- **The effect:** until the first batch commits, and for good after a crash or a refusal right after the mark:
  - a move or growth charges 1 + k_new, and the batch then adds one more, so the ledger is over-charged;
  - a deletion's tombstone releases 1 + k_new for copies charged k_new, which takes another volume's charge or underflows. #72's rule then deletes the ledger.
- **No test catches it.** The tests cover "a retry after a failure between batches" and a move hooked after a batch's chunk List. Both have a cursor above 0, or none.

**Fix:** encode the cursor so that 0 is present. Either use a pointer field (`*uint32` with `omitempty`) or store index + 1. Say so in contracts.md and design.md:22. Add tests to `TestRequestSnapshotCharges` for:
- a stop right after the mark (cursor 0) followed by a MoveReplica;
- a deletion with the snapshot `CHARGING`, at cursor 0 and at a middle cursor.

In each case the ledgers must equal Σ m(j), and the tombstone must release exactly what was charged.

### R3-B2 — Medium
**On an active store, the mark does not compare an unassigned volume's absent owner record. A racing `AssignOwner` then leaves one snapshot outside the quota for good.**
- **The compares:** contracts.md:102-105 lists them "for a tenant once #73 is active" (6) and "under `inactiveRules`" (3). An active store with an unassigned volume, which the mark admits and charges nothing (contracts.md:80), has no stated compare of `owners/<id>`.
- **AssignOwner:** it compares the header it read and writes only the owner record, the usage and its event (#73 design:173-178). It never writes the header.
- **The race:** AssignOwner reads k and commits `bytes` = (1 + k) × size. A mark that read the owner record as absent before that commit then passes its header compare and raises the count to k + 1 with no quota charge.
- **The result:** the tenant is under-charged by one size until deletion. This breaks the round-2 rule that snapshots taken while unassigned "are counted once assigned".
- **Precedent:** #70's step compares `owners/<id>` "at the revision read, absent included" and tests that hook (#70 design:96-100, `TestExpandChargesQuota`).

**Fix:** once #73 is active, the mark always compares `owners/<id>` (absence included) and `tenant/authorization`. That is 4 compares for an unassigned or system owner. Add to `TestRequestSnapshotCharges` an `interleaveKV` hook after the mark's read of `owners/<id>` that assigns the volume. The mark must then read again and charge the quota once.

### R3-C3 — Low
**A charge batch that finds a node without a valid report or a readable ledger has no stated outcome. The resulting stuck state is not named.**
- **What is specified:** a batch "rechecks `Room` at `SoftPPM`", and only "a batch without room" has an answer (`ResourceExhausted`, contracts.md:81-85). S2 and the named risk cover only "no room after the check, or a crash" (spec S2; design, Risks; decisions.md, round 2).
- **What is not:** between the check and a batch, a node can go down, or InitCapacity can mark its ledger. `Room` then cannot be computed, and a node down until `ForgetNode` keeps the snapshot `CHARGING` indefinitely.
- **The consequence:** every later request of the volume must complete the charge first, so the volume cannot be snapshotted at all until that node reports again or is forgotten. Its count and its quota stay held meanwhile.

**Fix:** state that a batch answers `FailedPrecondition` (naming the node) with earlier batches kept and the snapshot `CHARGING`. Name this case in S2's sentence and in the named risk. Test it in `TestRequestSnapshotCharges`: a report expiring after the check, and a ledger marked by InitCapacity between batches.

## Nits
- contracts.md:48-59: "Refused before any write" conflicts with completing a `CHARGING` snapshot "before that request's own checks", which lets an invalid or stale request write ledgers. Run the stateless checks (id, name, `expected_first_epoch`) before the completion.
- design.md:250 (S24): `TestSnapshotCommands` prints no Pending snapshot as ready. Say "no Pending or Charging snapshot".

Reviewed commit: 4d45c35f0b7360a99d036dd7dc4faf297a9cb4a2
