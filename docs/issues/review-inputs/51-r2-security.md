# Issue #51: plan review, round 2 (incremental), design-security focus

Scope: `git diff c8908eb..2997983 -- docs/issues/51/ ':!docs/issues/51/report.md'`, in
/home/jsfakian/Documents/src/flaminio-51.

## Earlier findings
- R1-S1: fixed. Contracts' Roles table (RequestSnapshot: administrator name, then `cluster-admin`; ListSnapshots: an open read; CommitSnapshot: the target's data plane). Insecure behaviour stated, matching #73 contracts (Insecure instances) and `auth.go:42-44,80-81` / `service.go:798-802`. `TestSnapshotRoles` added. One wording Nit below.
- R1-S2: partly fixed. Data space is charged (1 + k) through #72's ledger and #73's quota. The page pool is still only named as a risk, and it can be closed simply: R2-S1. AssignOwner is missing from the multiplier sites: R2-S2.
- R1-S3: fixed for the rule. S2, contracts (RequestSnapshot) and design (Catalog) refuse on #50's field 20 or the header mark, and `TestRequestSnapshot` covers both. The order in which #50 and #51 land is not covered: R2-S3.
- R1-S4: fixed. A follow-up in intent (Out of scope), S24, the contracts' Roles section and the design's files.
- R1-S5: fixed. Tags 72 and 73. No clear plan claims either as a frame tag (checked: origin/main #12–#18, #43, #44, #70–#86; #6, #9, #45–#50). `reply_tags_not_confused` added.
- R1-S6: fixed. `deleteChunks` (`service.go:481`), the header-absent branch (`:525-531`) and the claim (`:441`) clear `snapshots/<volume>/` under the same compares. Reads use only the `<first_epoch>` prefix. `first_epoch` is the claim's etcd revision (`service.go:6`), so it grows strictly with each incarnation. `ValidID` excludes `/` (`domain.go:87`). Tested by `TestDeleteClearsCatalog`.
- R1-S7: fixed. Lookup by `flaminio_node::volume_uuid` (`flaminio-node/src/lib.rs:21`), and a chunk fenced below E is refused. This is `Store::delete_volume`'s own `fenced_epoch < b` (`store.rs:1247`). "Below, not at" is right. Covered by `snapshot-read-test.sh`.
- Nits (R1): both fixed. The `snapshot-v2` gate now names its callers as #50's gate does. design.md's Lab section no longer calls the settled questions open.

Checked and sound, no finding:
- `Refused` ending the session (`frame.rs:110-113`, `replica/mod.rs:11-12`). Only the current session's target can send h < t, after the epoch check.
- The completeness evidence for `SourceProofV1`:
  - The fields match #52's draft (`flaminio-52/.../contracts.md:24`).
  - ChangeIsr does not write the header (`service.go:806-812`), but a cut does. So the header-to-header window fixes the committed set, and an ISR member at any revision in it is either continuous across the cut (S8 drain) or a join (S18).
  - A ChangeIsr prepared before a cut fails its header compare.
- The (1 + k) bound on data space. A layer holds at most a chunk size. Every site that names a copy (VolumeCharges, MoveReplica, ConfirmRelease, growth) compares the header that carries k, and the tombstone releases with the k of the header it deletes.
- The request's header writes. At most 63 per incarnation, and idempotent and refused requests write nothing, so a racing ChangeIsr or Attach retries a bounded number of times.

### R2-S1 — Medium
**The page pool can be bounded with one more multiplier. The plan leaves it as a named risk instead, and that risk is real only for small chunks.**
- How the pool is sized: `pool_pairs = ceil(N/E)×2 + chunk_slots×2` (format §2.1, `on-disk-format-v1.md:62`). Each (slot, layer) has one page table (§5.4, `:205`), and each layer has at most `ceil(chunk_clusters/E)` map pages.
- Where the gap is: with the bytes charge alone, a disk holds layers L ≤ T/c. For c ≥ 32 MiB this gives L ≤ S = `chunk_slots` (≈ T/32 MiB). The pool then already fits: N/E + 2L ≤ 2N/E + 2S.
- Below that the pool overflows. Chunks are allowed down to 1 MiB (`domain.go:180`; CSI `chunkSize`, `controller.go:71`). At 1 MiB with 64 KiB clusters, worst-case layer use is about 4× the pool. New map pages of other volumes on that disk then fail with OutOfSpace, the cross-volume DoS that R1-S2 raised.
- The design keeps "Slots are unchanged: a layer takes none" (Roles, audit and charges). #72's own premise is "Metadata pages are not a resource… the pool is sized for… every slot's page tables" (origin/main `docs/issues/72/spec.md:27-28`). Uncharged slots per layer break that premise.

**Fix:** apply the same (1 + k) multiplier to slots: each layer takes one slot, in the request, `VolumeCharges`, MoveReplica, ConfirmRelease, growth and the tombstone.
- Then L ≤ S, and the pool holds by the format's sizing: ≤ N/E + 2S ≤ 2N/E + 2S.
- Write that arithmetic in design.md and drop the named risk and its follow-up.
- The slot charge only binds for chunks below 32 MiB.
- Tests: `TestRequestSnapshotCharges` refusing on slots for 1 MiB chunks, and a store test that fills slots × layers without exhausting the pool.

### R2-S2 — Low
**The quota multiplier misses #73's AssignOwner, so snapshots taken while a volume is unassigned are never counted against the tenant's quota.**
- Before activation, and once active for an unassigned volume, a request charges no quota: it charges "for a tenant owner" only (contracts, Retention charge).
- #73's owner `bytes` is "a tenant's size at its claim or assignment plus every growth step's" (origin/main `docs/issues/73/contracts.md:153-155`).
- #51's multiplier sites (design, Roles, audit and charges: VolumeCharges, MoveReplica, ConfirmRelease, growth) leave out AssignOwner.
- So a legacy or pre-activation volume with k snapshots, once assigned, charges its size only. Its quota is under-counted by k × size for good, and the deletion releases the same smaller figure.

**Fix:** AssignOwner of a tenant charges (1 + `snapshot_count`) × size, comparing the header it already reads. Name it among the multiplier sites with the same "whichever lands later" clause, and test an assignment after k snapshots.

### R2-S3 — Low
**If #51 lands before #50, nobody is assigned to add the encrypted-volume refusal.**
- The refusal relies on #50's field 20 and header mark (contracts, RequestSnapshot; design, Catalog).
- Every other cross-plan part carries a "whichever lands second adds it" clause. This one does not, and #51 does not depend on #50 (batch-2026-11-12.md:148 lists no dependency).
- #50's clear plan says only that no encrypted snapshot is admitted (`flaminio-50/docs/issues/50/contracts.md:166-170`). It names no RequestSnapshot check.
- So if #50 lands second, RequestSnapshot can admit an encrypted volume, against #50's rule, and `TestRequestSnapshot`'s encrypted cases cannot be written when #51 lands first.

**Fix:** add "whichever of #50 and #51 lands second adds the refusal and its two test cases", or make #50 a prerequisite of #51's metadata step.

## Nits
- "Whichever of #72, #73 and #51 lands second" (contracts, Retention charge) and "whichever of #13, #70, #72, #73 and #51 lands second" (design): with three or more plans, the third plan's part has no owner. Write "of each pair, the later one".
- Contracts, Roles: say which of #73's Authorize classes the methods join. RequestSnapshot joins the "existing methods" (`inactiveRules`), not #73's "new methods", which inactive stores and insecure instances refuse (origin/main `docs/issues/73/design.md:45-53`). ListSnapshots joins the open reads.

Reviewed commit: da886c541f0072fd2494775510b46374d492ce21
