# Issue #50 plan review: round 4, design-code and design-security

Scope: `git diff b87df55..ed2facc -- docs/issues/50/ ':!docs/issues/50/report.md'` in
/home/jsfakian/Documents/src/flaminio-50. Checked against that worktree's code and the clear
plans #13, #73 and #76 on origin/main.

These were checked and are not findings:
- **The ListVolumes marker that was not taken (R3-S2).** The reasoning holds. The first rollout
  puts every metadata instance ahead of every client, `flaminio-meta` included. An older CSI
  controller still refuses `encryption` (`controller.go:157`), and an older `flaminio-meta` has
  no `--encrypted`. So no encrypted volume can exist next to an older instance unless the order
  is broken. The rollback reverses that order, so when an older instance first appears the drain
  binary (`flaminio-meta drain`, `main.go:433-440`) is already older and could not check a
  marker. A marker would only help a new drain against an older instance, which takes two order
  violations. The named risk, the restored "delete the encrypted volumes first", the narrowed
  "fails closed" and S12 cover it.
- **`readVolume`'s extra Get.** `readVolume` is called only by GetVolume (`service.go:502`) and
  by Attach (`:717`, `:736`). ListVolumes (`:845-858`), ChangeIsr and MoveReplica do not call it.
  On the data plane, GetVolume is sent at attach (`engine.rs:668`) and by the target's refresh
  (`:1782`), never per I/O. `drain.Step` pays one extra etcd Get per listed volume per pass,
  which is fine. Records are keyed by first epoch, so the Get cannot see another incarnation's
  record. A damaged record now stalls a drain pass with an error rather than skipping the volume,
  which is fail-closed.
- **"Every header write keeps the mark", against today's writers.** The only `volumes/<id>`
  writers are:
  - the claim (`:414-417`, a new `Header`, unmarked by design);
  - the publication (`:466-468`, marked by design);
  - DeleteVolume's mark (`:540-542`, which re-marshals the header it decoded);
  - Attach (`:723-725`, the same).

  ChangeIsr only compares the header (`:774`), and MoveReplica writes chunks (`:904-908`). No
  other Go, Rust or script code touches the key.
- **The #76 undo choice.** No undo with #76 matches #76: only `stepUnpublishedRollback` deletes
  a creating header (#76 design, "Deletion classification"), and helpers resume finalization.
- **Release Nit wording.** `readHeader` returns `NotFound` for absent, `Creating` and `Deleting`
  (`:286-288`).

## Earlier findings
- R3-C1: fixed. "Fails closed" is narrowed to per-volume calls, the listing exception is named
  in the design, S12, decisions and docs, and "delete the encrypted volumes first" is restored.
- R3-C2: fixed. `TestCreateEncryptedVolume` checks the claim and the published `id`,
  `TestHeaderWritesKeepMark` is added, and the old-decoder test uses the service's header.
- R3-C3: partly fixed. The #76 branch is right. The #73 branch is under-specified (R4-C1).
- R3-S1: fixed. Mark/record agreement is checked in `readVolume`, "record decides" is reworded,
  and `TestKeyRecordDamaged` is extended. The #76 finalization wording is new drift (R4-C2).
- R3-S2: fixed by the named risk and the restored instruction. The marker is not needed (above).
- R3-S3: mostly fixed (length, compare-only pass, `mktemp`, `sync`, tests). It introduces
  R4-B1. A compare/`mv -f` race is left (Nit 1).
- Code Nit 1 (a mismatched mark): fixed, `Internal`.
- Security Nit 1 (absent or `Deleting`): fixed in the design and contracts.

### R4-C1 — Low
**The #73 wiring of the failed-wrap undo does not say what it compares or what happens when
that compare fails. Lowering the usage under only the claim's compare is a read-modify-write.**
- **What the design says.** design.md "Creation" puts it all in one transaction: the transaction
  "that deletes the header also deletes `owners/<id>`, lowers the namespace usage the claim
  charged and appends #73's `delete.committed`". That transaction runs "under the same compare",
  the claim's.
- **What #73 requires.** Every #73 transaction that lowers `quota/<ns>/<class>` or appends to a
  chain also compares the quota's and the tail's revisions. Examples are `finishDeletion` and
  the header-absent release (#73 design.md:127-141). A concurrent creation in the same namespace
  moves the quota's revision, so the undo either loses that charge or must retry. Neither is
  stated.
- **Unnamed details.** The event's outcome, chain and operation id are not named. #73's release
  writes `header-missing`, which is wrong for a header this call deletes. What a full chain or a
  failed compare leaves is not named either.
- **Fix:** with #73, the undo deletes the header under the claim's compare, as today. It then
  releases the creation's own headerless owner record through #73's existing release, with that
  release's compares, retry and event. Alternatively, it is `finishDeletion` with a named outcome
  in #73's retry loop. Add to `TestCreateEncryptedCrashPoints` a failed wrap racing a second
  creation in the same namespace; the usage must stay exact.

### R4-C2 — Low
**With #76, the finalization is the publication, so it must add the mark, not keep it. The
design says it writes "the header they decoded", which is the unmarked creating header.**
- **Where it says so.** design.md "The header's mark and the key record" says: "the claim header
  stays unmarked until the publication marks it; #13's `fenced` and #76's finalization write the
  header they decoded".
- **What #76 does.** #76 keeps the creating header "byte-for-byte at C until visibility". Its
  finalization "atomically writes visible header" (#76 design, "Conditional helpers").
- **What is missing.** The "With #76" bullet names only the key record in that transaction, not
  the mark. `TestHeaderWritesKeepMark` expects the `id` "still the object" after the
  finalization, but nothing marked it before.
- **What a literal reading does.** A literal implementation publishes a plain header with a
  record, which `readVolume` now answers `Internal`, for every encrypted volume created through
  #76.
- **Fix:** say that #76's finalization marks the header it publishes, in the same transaction
  as the key record (it is this plan's publication), whether a creator or a helper runs it. List
  it with the publication rather than with the writers that keep the mark. In the test: after a
  finalization by the creator and by a helper, the `id` is the object.

### R4-B1 — Low
**The new pre-check stops every `pki issue` on an existing lab, including runs that reach no
metadata VM, because `pki issue` never makes a missing `kek`.**
- **The cause.** `pki issue` runs `pki init` only when `flaminio/ca.crt` is missing
  (`scripts/lab.sh:574`). A workstation that already has its CAs has no `kek` until someone runs
  `pki init` by hand.
- **The new rule.** The design (Lab, "The KEK") checks that the workstation's `kek` is exactly
  32 bytes "before it writes anything to any VM, CA files included".
- **What breaks.** That check stops `cmd_pki issue w` from `cmd_partition` (`:807`) and
  `cmd_pki issue c` from `cmd_data_node_up` (`:1228`). The fl-c run installs no KEK at all. It
  also stops any rebuilt VM's identities.
- **Fix:** `pki issue` makes a missing `kek` as `pki init` does, each piece only if missing. A
  fresh `kek` against a cluster that has one is still refused by the compare-only pass. Apply
  the length check and compare only to runs that reach a metadata VM. Add an
  `encryption-test.sh` case: a state dir with CAs and no `kek`, then `pki issue c` and
  `pki issue` both succeed.

## Nits
1. design.md "The KEK": "compares it again with an existing `kek` … otherwise `mv -f`". This
   leaves a window between the compare and the rename. Two first installs of different KEKs can
   both pass, and the last one wins. It also does not say that the `mktemp` file is removed when
   the `wc -c` check fails. Use `ln "$t" kek` (it fails if `kek` exists) and then compare, and
   remove the file on every failure.

Reviewed commit: e067381127a09cf5e7caee25f46a4289480bc8ba
