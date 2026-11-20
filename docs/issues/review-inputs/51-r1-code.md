# #51 plan review, round 1 (design-code focus)

### R1-C1 — High
**The snapshot-capable byte bound contradicts #13 (clear), and the catalog allowance is not derived.**
design.md "Budget, rollout, files and tests" and decisions.md (budget eligibility) use
`32768 + N*(288+264*(R+1))`, caps 5099/3853/3096/2588/2223. That is #80's original per-chunk term.
#13 (clear, origin/main `docs/issues/13/design.md:65-85`) changes #80's bound to
`4160 + N*(288+264*(R+1)+132)`, caps 4419/3457/2838/2408/2090: a worst chunk carries one release
beside a move, and the header carries `fenced` (16 names). Take a volume at #13's RF1 cap (N=4419).
It passes #51's check (32768+4419*816 = 3,638,672). Its worst reply is 4160 + ~12.6 KB of catalog
(63 Snapshot entries of ≤199 B, plus field 30) + 4419*948 ≈ 4.205 MB, above 4,194,304. Its
GetVolume and Attach would then fail. The plan never derives the 30,720 B it adds to the constant.
This is the "#80 byte-bound/admission proof" the batch flagged.
**Fix:** start from #13's B. Derive the catalog allowance exactly (63 × the maximal encoded
`Snapshot`, plus `snapshot_generation`) and add it to 4160. Recompute the caps (RF1 ≈ 4389).
Extend #13/#80's worst-view fixtures with a full catalog, #13's worst chunk and #50's descriptor.
State that whichever of #13, #80 and #51 lands second applies it.

### R1-C2 — Medium
**Growth (#70, clear) of a volume with snapshots is not addressed.** #70 checks a new size
only against #80's admission (origin/main `docs/issues/70/spec.md:32-42`), and it adds chunks.
#51 applies its snapshot-capable bound only "before accepting the first snapshot" (design.md,
Budget). A volume that has snapshots can therefore grow past that bound, and its full replies
then exceed 4 MiB. The plan also does not say what `Before(g)` means for a chunk added after cut g,
or how catch-up and the cut treat such chunks.
**Fix:** ExpandVolume of a volume with any snapshot applies the snapshot-capable bound (whichever
of #70 and #51 lands second adds it, with a test). State that a chunk added at generation g reads
as zeros in every snapshot below g, and test it.

### R1-C3 — Medium
**#73's hand-offs are not taken.** These are batch-2026-11-12.md:222 and #73 (clear):
- **Roles:** #73's method table (contracts.md:32-53) has no rows for RequestSnapshot,
  ListSnapshots or CommitSnapshot. Once tenancy is active, an administrator's call to a method not
  in the table gets PermissionDenied, and a non-administrator passes only an open read or a
  node/agent/target rule (#73 design, Activation and roles). So snapshots cannot be requested, and
  ListSnapshots ("any identity") is refused.
- **Audit:** #73 spec S7 (line 132) and decisions ("each later issue owns its hook and test,
  recorded in docs/followups.md") give snapshot operations to #51. The plan neither audits them
  nor records a follow-up.
- **Quota:** #73 spec S4 (line 96): "Snapshots and backups are charged by the issues that add
  them." Layers consume space, and #51 neither charges them nor records a follow-up.

**Fix:** add the three role rows (whichever of #51 and #73 lands second). Add ListSnapshots to
the open reads. Add audit events, or a followups entry, and the same for the quota charge.

### R1-C4 — Medium
**The deletion path for catalog records is unspecified and leaks on a crash.** design.md:36-40
says only "bounded conditional batches". It does not say which transaction, which compare, or
how a crash is recovered. Today's no-header branch clears only `chunks/<id>/`
(`meta/service/service.go:525-531`), and creation clears only chunk leftovers. A crash between the
header's deletion and the catalog batches therefore leaves `snapshots/<v>/<fe>/*` forever. A later
incarnation deletes "only this incarnation's" records, so it never clears them. With #73, the
header deletion is `finishDeletion`, which carries events, the owner record and the quota. The
plan counts no operations against the 120 limit (`meta/kv/kv.go:33`), which the batch flagged.
**Fix:** delete the catalog after the header's deletion, conditional on the header being absent,
as `deleteChunks` does. Clear `snapshots/<v>/` in the no-header branch and at the creation claim,
and count the operations with #73's. Test a crash between each pair of steps, and a re-creation.

### R1-C5 — Medium
**The #52/#58 coordination is not recorded, and the completeness evidence is undefined.**
contracts.md is titled "exact snapshot contracts (Accepted with the plan)". #58's settled decision
says these bytes are "fixed in one coordinated review with #51 and #52 before implementation"
(`flaminio-58/docs/issues/58/design.md:9`, contracts.md:65). #52's `SourceProofV1` needs "#51's
completeness evidence, a source replica holding the committed cut in full"
(`flaminio-52/docs/issues/52/decisions.md:58-62`). #51 defines no such evidence: S22 says a
replica does not know whether it is in sync, and design.md:116 says the durable top does not
prove completeness.
**Fix:** record the coordinated-review gate in contracts.md and decisions.md. State what evidence
#51 provides (for example, ISR membership at a chunk's `isr_ver` with the committed generation),
or hand it to #52 or #61 explicitly.

### R1-C6 — Medium
**S18 and the design's join/cut rules have no injected bug.** The spec's mutation table
(spec.md, Tests) covers S3, S8, S12, S15, S16, S17 and S20, but not S18 ("proposed for the ISR
only once it holds every snapshot"). It also omits the design's "if a new cut committed, recheck
catch-up frontier before proposing join" and "stage/session identities fence old replies"
(design.md:109,121). The "early_publish ... catalog checker" (design.md:154) is not one of the
spec's three checks. AGENTS.md: each new safety rule gets a simulator check and an injected bug.
**Fix:** add mutations `join_before_frontier` (join without rechecking after a cut) and
`stale_stage_reply` (an old stage's reply advances the frontier). Define the catalog checker in
the spec's check list.

### R1-C7 — Low
**S12's refusal has no wire reply, and contracts.md misstates `Refused`.** contracts.md
(Wire-v2): "Existing Data/Digests/Mappings/Ok/Stale/Refused correlate by req/session". But
`Frame::Refused` carries no `req` and closes the connection (`flaminio-net/src/frame.rs:110-113,
309-312`). A store failure closes the session (`flaminio-node/src/replica/mod.rs:11-12`).
`Response` has no per-request refusal (`flaminio-repl/src/msg.rs:83-101`).
**Fix:** state that h < t closes the session with `Refused` and the target treats the replica as
failed, or add a per-request reply tag. Test it.

### R1-C8 — Low
**CommitSnapshot's "wrong caller PermissionDenied" contradicts S3 for a deposed target.**
ChangeIsr answers an io that is not the target with `applied=false` and the current state, so a
deposed target learns it is deposed (`meta/service/service.go:794-800`). S3 and S9 say the same
of the cut. The contract's PermissionDenied would leave a deposed target holding writes until its
next poll.
**Fix:** only Authorize returns PermissionDenied (non-io identities). An io of another node gets
`applied=false` with the current epoch and generation. Test it.

### R1-C9 — Low
**RequestSnapshot writes the header.** That contradicts S2 ("changes nothing else"), and the
plan does not place the catalog in #77's validated read. The header counters (design.md:21-27,
contracts "request serializes count/order in header") change the header revision. That revision
is what #77's pinned Attach checks after its commit (Aborted on a change, origin/main
`docs/issues/77/design.md`, Pin Attach) and what ChangeIsr and MoveReplica compare. Volume
replies add the catalog, but the plan does not say it is listed within #77's header-to-header
window.
**Fix:** keep the counters in a catalog key, or state the effect and test the race between
RequestSnapshot and Attach. Read the catalog prefix inside #77's validated window.

### R1-C10 — Low
**`snapshot-read` does not say how it finds and checks the chunks.** Store keys come from
`flaminio_node::volume_uuid` (SHA-256 of the name, `flaminio-node/src/lib.rs:21`). The existing
offline tool uses the MVP name-padding `volume_uuid` (`flaminio-io/src/cli.rs:41-48`). The plan
does not say what `--first-epoch` checks. A chunk fenced below it belongs to an earlier
incarnation (#30 S5).
**Fix:** use the node's key derivation. Refuse a chunk whose `fenced_epoch` is below
`--first-epoch`, and test it.

### R1-C11 — Low
**The encrypted-volume and profile refusals have no mechanism or test.** The design rejects
snapshots of encrypted volumes (design.md:42-43), but S2, the RequestSnapshot refusal list
(contracts.md) and the test table omit it. "Incompatible profile" at request time has no source:
the service does not know the attached target's version.
**Fix:** refuse on #50's header descriptor (field 20) in S2 and the contract, with a test. Define
the profile refusal or drop it.

### R1-C12 — Low
**The test plan names no tests, and the lab check is not runnable.** The design's table lists
topics, not test names or files. The lab check (intent rule 8) has no script or `make` target
(design "scripts/checks/tools"). The implementation commands omit `make proto`,
`make meta-vectors` and `make hygiene`, although proto and vectors change.
**Fix:** name a test, with its file, for each rule and each mutation. Name the lab script and its
command. List `make proto meta-vectors` and the full pre-PR set.

### R1-C13 — Low
**The texts call settled and accepted things open.** spec.md:3 says "Nothing is decided yet";
design.md:170-171 says "without accepting it" and "Q1–Q8 ... remain open"; adr-proposal.md:349
says "No policy/ADR is accepted". Each sits beside "Settled at approval" and an ADR status of
Accepted.
**Fix:** reword these as settled.

## Nits
- design.md:38 "#76/#77 creator/deletion ownership": #77 is consistent reads; deletion ownership is #73's.
- contracts.md: the `<first_epoch>` key segment's encoding (decimal, padding) is unstated; chunk keys pad to 8 digits.

Reviewed commit: c8908ebd6c23a4120a12e17d129241c436e39c54
