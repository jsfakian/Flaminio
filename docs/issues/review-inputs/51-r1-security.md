# Issue #51 — plan review, round 1, design-security focus

### R1-S1 — High
**Under #73 the three new methods have no role, so any administrator name can burn a tenant volume's 63 lifetime snapshots.**
#73 is clear and binding. Its "Methods and roles" table (origin/main `docs/issues/73/contracts.md`, the table and "Roles come from the store") gives every method a rule for "Before activation" and one for "Once active". Once tenancy is active, an administrator name alone no longer grants admin methods: MoveReplica and DrainNode need `cluster-admin`, and DeleteVolume needs the role of the owner's kind. #51 never mentions #73. Its rules are "Requesting: an administrator" (spec S4) and "RequestSnapshot: Admin only" (contracts.md, Metadata and lifecycle), today's `admin := id.Admin != ""` (`meta/service/auth.go:57`). Taken literally, any identity with an Admin SAN could request snapshots on any tenant's volume once tenancy is active, `audit-reader` and `deletion-override` included. Each request permanently uses up one of the 63 slots, because no deletion exists (S2, Q6). Each cut also holds the volume's client writes (S8, S9) and pins uncharged space (R1-S2). Nothing places ListSnapshots among #73's open reads or CommitSnapshot under the target rule either.
**Fix:** add #73 rows. RequestSnapshot: an administrator name before activation; once active, `cluster-admin` (or the owner-kind rule DeleteVolume uses). ListSnapshots joins the open reads. CommitSnapshot takes the target's data plane rule, with no record read. State the insecure-instance behaviour for each. Add the role tests to the S1–S6 row: an `audit-reader` or `deletion-override` caller refused once active.

### R1-S2 — High
**Retained layers break #72's capacity bound. Other volumes on the same disks can then fail their writes with OutOfSpace.**
The store allocates thinly, from bitmaps shared by every volume on the disk: data clusters (`store.rs:1765,1806`) and the metadata page pool (`state.rs:226,504`). Without a snapshot, an overwrite of a mapped cluster stays in place. After each cut, the first write to a cluster allocates a new cluster (64 KiB COW, `store.rs:1790`) and possibly new map pages, and nothing frees the old one until the volume is deleted. So a copy can use up to about 64 times its chunk size. #72 is clear: it charges one chunk size per copy and states "L bounds its physical use plus its unwritten commitments" (origin/main `docs/issues/72/spec.md:15,36`). #73's quota counts the volume's bytes. #51 charges neither (no #72 or #73 reference anywhere in the plan), and it measures COW as a cost only (intent, Out of scope). When the disk or pool fills, S13 fails writes to *other* volumes on that disk, possibly other tenants'. Those writes return OutOfSpace exactly as S13 says a refused layer push does.
**Fix:** bound the retention in #72's terms. For example, RequestSnapshot charges one more chunk size per named copy in its own transaction, refuses at #72's soft limit, and the tombstone releases the charge. Or explicitly accept the gap as admin-only for this step, with a follow-up and an operator warning in README and `docs/lab.md`. Add a test either way.

### R1-S3 — Medium
**The refusal of encrypted volumes, which #50 requires, is in the design only. The spec and contracts leave it out or name it vaguely, and no test covers it.**
#50 is clear and states: "No encrypted snapshots, clones or backups are admitted until their catalog pins this descriptor, its source version and any required keys" (`flaminio-50/docs/issues/50/contracts.md:166-170`). With AES-XTS and a tweak of `LE128(volume_offset/4096)`, retained layers also keep several ciphertexts under one tweak. The design does reject encrypted snapshots (design.md, Catalog, "Encrypted snapshots are rejected…"). But spec S2's refusal list omits them. contracts.md says only "reject count/budget/incompatible profile/hydrating state", and "profile" there could mean the wire profile. The S1–S6 test row has no encrypted case.
**Fix:** add to S2 and RequestSnapshot: FailedPrecondition before any write for a volume whose `Volume.encryption.profile` (#50 field 20) is not `none`, or whose header carries #50's key mark. Add the unit test and a vector.

### R1-S4 — Medium
**#73's hand-off on auditing snapshot operations is not taken.**
batch-2026-11-12.md:222 says #50 and #51–#53 "should either audit key, snapshot and backup operations through #73's event append or record that as a follow-up". #50 records its follow-up (`flaminio-50/docs/issues/50/intent.md`, Out of scope, "Audit events for key operations"). #51 does neither: RequestSnapshot and CommitSnapshot are not audited, and no `docs/followups.md` entry is planned (design.md, files list).
**Fix:** either add events (for example `snapshot.requested`, `snapshot.committed`) to #73's Event column within the 120-operation limit, or record a `docs/followups.md` item in S24 and in the design's files.

### R1-S5 — Medium
**Frame tag 0x47 (71) collides with #13's `NOT_HELD` reply tag 71.**
contracts.md (Wire-v2 bytes) assigns `0x47 Layers` and `0x48 Changes`. #13 is clear and claims tag 71 for `NOT_HELD` (origin/main `docs/issues/13/design.md:940`; `decisions.md:70`). Today's tags end at 70 (`flaminio-net/src/frame.rs:130-148`). A shared tag means one reply would be decoded as the other, against the plan's own rule "never interpret one format as another".
**Fix:** move Layers and Changes to free reply tags (for example 72 and 73, after checking the other plans' claims), and add the decode test.

### R1-S6 — Low
**How snapshot records are cleaned up after a DeleteVolume that dies part-way is unspecified.**
The design deletes "this incarnation's ≤63 catalog records in bounded conditional batches" (design.md, Catalog) and does not say whether that happens before or after the header is deleted. Today's header-absent retry (`service.go:527-533`) and a re-creation's cleanup of leftovers ("a creation that claims the id meanwhile deletes the leftovers itself", `service.go:575-576`) handle only chunk records. Records left under `snapshots/<volume>/<old first_epoch>/` would then pile up without bound across delete-and-recreate cycles. Nothing states that ListSnapshots, the Volume replies and the count list only the current `first_epoch` prefix, not `snapshots/<volume>/`.
**Fix:** delete the records while the header is marked, before the header-deleting transaction. Also let the header-absent retry and the creation's cleanup remove `snapshots/<volume>/` under the header-absent compare. State that every read uses the current incarnation's prefix. Test the crash between the batches and the re-creation that follows.

### R1-S7 — Low
**`snapshot-read --first-epoch E` does not say how E is checked against the store.**
A stopped replica that missed a delete and re-create still holds the earlier volume's chunks under the same UUID (a hash of the name). #30's drop happens only at the new volume's first `Hello` (`flaminio-node/src/replica/mod.rs:193-211`). The tool takes `--first-epoch` (contracts.md, Offline seam) but no rule refuses such chunks. The lab check could then compare an earlier volume's layers as this volume's snapshot.
**Fix:** refuse unless the volume's stored fence, and each read chunk's `fenced_epoch`, is above E (#30's rule). Add a script test with leftovers from an earlier volume.

## Nits
- contracts.md, `accepted_snapshot_profile`: name which callers the gate applies to, as #50's gate does ("an `io` caller whose node is the record's target"); "io must send" is ambiguous for agent, CSI and metactl reads.
- design.md:171 still says "Q1–Q8, budget eligibility and below-top historical mismatch remain open"; the settled line after it contradicts this.

Reviewed commit: c8908ebd6c23a4120a12e17d129241c436e39c54
