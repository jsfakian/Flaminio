# Issue #50 — plan review, round 3, focus: design-security

Scope: `git diff f2857b5..f5075f7 -- docs/issues/50/ ':!docs/issues/50/report.md'`, checked
against the code in /home/jsfakian/Documents/src/flaminio-50 and #13 and #15 on origin/main.

Checked and not findings:
- Older paths on a marked header. `readHeader` returns the decode error (`service.go:282-284`),
  and `errStatus` maps it to `Unavailable` (`:151`), so GetVolume, Attach, ChangeIsr and
  MoveReplica fail. An older createVolume's failed claim gets that error, not `ErrNotFound`, so
  it answers `ErrExists` and recreates nothing (`:425-431`). DeleteVolume removes the header
  without a mark and still tombstones the chunk replicas (`:534-573`). Both clients move on for
  `Unavailable` (`meta.go:74`, `lib.rs:148-153`), as decisions.md says. An older node plugin
  reaches `mkfs` only after a data-plane attach (`node.go:70-116`). An older instance cannot
  serve that attach, and a new instance gates the older data plane. With `--insecure` no token
  is issued, so the data plane refuses (`main.go:97`, `engine.rs:1613`).
- The release's fence. `MetaWait::Attach` already filters by `attempt`
  (`engine.rs:1452-1462`), and `detach` removes the attaching entry (`:1858`). With #15, the
  release goes before the mark and is fenced by #15's generation (`origin/main` #15
  design.md:543-547). A release before the mark gives the key to no party that the mark would
  exclude. #13's `fenced` lists release holders, not a deposed target (#13 design.md:63), so it
  is right that the release's re-read ignores it.
- Leftover key records. A record is reached only at the header's first epoch, so a leftover is
  unreachable by GetVolume or the release. Its cleanup is conditional on the header being absent,
  or on the claim, so it cannot delete a newer incarnation's record. The contract states the
  remaining limit ("Until then it stays in etcd").

## Earlier findings
- R2-S1 (High): fixed. The mark makes every pre-#50 instance fail closed on serving. The node
  plugin refuses an answer without field 20, and the risk text names the reformat. See R3-S1
  and R3-S2 for what the mark leaves open.
- R2-S2 (Medium): fixed. `cmp -s` under sudo, refuse on a difference, and an
  `encryption-test.sh` case. See R3-S3.
- R2-S3 (Low): fixed. Cleanup by listing `volkeys/<id>/` in DeleteVolume and in creation, the
  limit stated in the contract, and `TestDeleteRemovesKeyRecord` extended.
- R2-S4 (Low): fixed. #15's token verification is a named prerequisite, with what holds before it.
- Nit 1 (`sudo test -f`): fixed. Nit 2 (`PR_SET_DUMPABLE` before the KEK is read): fixed in the
  design and S11.

### R3-S1 — Medium
**The mark now decides whether a volume is encrypted, but nothing checks it against the record
and no test pins it. The plan still says the record decides.**
- design.md:49-61: `readVolume` says "a plain header is plain, and no record is read". Round 1
  had the opposite (decisions.md:66-70: "the key record decides", with no plain reading while a
  record exists). That guarantee is gone in one direction. A new-version header write that loses
  `Profile` now serves an encrypted volume as plain, with no error. Writes that could lose it:
  Attach's epoch bump, DeleteVolume's `Deleting` mark, #13's `fenced`, #76's finalization, the
  wrap undo, and any writer that builds a `Header` anew. The data plane then takes `none`, and a
  node plugin formats the ciphertext (`node.go:107-116`): data loss.
- No test checks that a new-version rewrite keeps the mark. `TestOldHeaderRewriteKeepsKey` was
  removed, and `TestCreateEncryptedVolume` still asserts "header JSON as today"
  (design.md:312), which is now wrong.
- design.md:134 and contracts.md:153 still say "the record decides".
- What happens when the mark's `profile` disagrees with the record's is unspecified.

**Fix:**
- In `readVolume`, also check the record for a plain header: one Get at `volkeys/<id>/<first
  epoch>`, which is not on the I/O path. A record under a plain header is `Internal`, so the mark
  and the record must agree. A mark whose profile differs from the record's is `Internal` too.
- Fix the "record decides" wording, or say "the mark and the record, which must agree".
- Correct `TestCreateEncryptedVolume` to expect a marked header.
- Add `TestHeaderWritesKeepMark`: Attach, the deletion mark, and each later writer's
  round-trip keep the object `id`. A record under a plain header, or a profile mismatch, gives
  `Internal`.

### R3-S2 — Medium
**"Lists no encrypted volume" is not fail-closed. An older instance's ListVolumes lets a drain
end with encrypted replicas still on the node.**
- The mark makes an older ListVolumes skip encrypted volumes and answer `OK`
  (`service.go:853`).
- `drain.Step` builds its work from ListVolumes (`control/drain/drain.go:50`). `Run` returns
  "drained" at `Remaining == 0` (`:181`), so it reports a node empty while it still holds
  replicas of every encrypted volume. The operator then removes or wipes the node. Over a
  rolling decommission, that is replica loss and then volume loss.
- A new drain controller is exposed too: its ListVolumes can reach an older instance, and no
  client moves on for `OK`.
- design.md:146-153 counts "lists no encrypted volume" among the ways the rollback "fails
  closed whatever the older instance is started with". decisions.md withdraws round 1's risk on
  that basis.

**Fix:**
- Give `ListVolumesResponse` the same version marker as field 20 (say `bool profiles = 2`, set
  by this version). The new drain controller and `flaminio-meta` refuse an answer without it.
- Name the limit for older drain binaries in the rollback text, S12 and the docs: no drain while
  an older instance runs and an encrypted volume exists.
- Add `control/drain` test `TestDrainRefusesOldListing`.

### R3-S3 — Low
**The KEK install checks content and mode, but not length, ordering across VMs, or concurrent
runs.**
design.md:341-347 covers these steps: write `kek.new`, `cmp -s` against an existing `kek`,
`mv -f`, then `stat`. Gaps:
- **Length.** A short or empty workstation `kek` (an interrupted `pki init`) is installed as
  the cluster's first `kek`, because nothing checks for 32 bytes. Every later correct install
  is then refused with "the workstation's KEK is not the cluster's". The service fails closed
  (`TestStaticFileChecks`), but the message misleads, and recovery is manual.
- **Ordering across VMs.** `cmd_pki issue` installs VM by VM (`scripts/lab.sh:575-588`), and
  the refusal stops it mid-loop. A rebuilt VM processed before the refusing one has already
  taken a different KEK, and has new CA files too. The pin keeps this fail-closed, but later
  runs refuse that VM.
- **Concurrent runs.** The fixed name `kek.new` lets two concurrent runs interleave their writes.
- **Durability.** No `sync` before `mv`.

**Fix:**
- Before any write, check the workstation `kek` is exactly 32 bytes.
- Run a compare-only pass over fl-a, fl-b and fl-w before installing anything.
- Remotely, write `mktemp` in the directory, check `wc -c` = 32, `sync` it, then compare and
  `mv`.
- Add an `encryption-test.sh` case for a 31-byte workstation KEK (refused, no VM file written)
  and one for a second VM refusing (no VM changed).

## Nits
- design.md:195-197: the release's re-read refuses when the header is "gone". Say "absent or
  `Deleting` (readHeader's `NotFound`)". As written, a re-read that compares only target, epoch
  and first epoch would release a key for a volume whose deletion has begun.

Reviewed commit: b87df55153e7f3148f041fd219a43423d541c0b6
