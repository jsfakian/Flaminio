# Issue #50 plan review: round 3, design-code focus

Scope: `git diff f2857b5..f5075f7 -- docs/issues/50/ ':!docs/issues/50/report.md'` in
/home/jsfakian/Documents/src/flaminio-50. Checked against that worktree's code and the clear plans
on origin/main (#13, #15, #72, #73, #75, #76, #80).

These were checked and are not findings:
- **The mark against every header decoder.** `domain.Header.ID` is a `string`
  (`meta/domain/domain.go:38`). Go returns an `UnmarshalTypeError` for an object, and all three
  decode sites check the error: `readHeader` `service.go:282`, DeleteVolume `:539`,
  ListVolumes `:853`. Every per-volume call goes through `readHeader`, and its error maps to
  `Unavailable` (`:151`). Both clients move on for that code (`control/csi/meta.go:74`,
  `flaminio-metaclient/src/lib.rs:146-157`).
- **Other paths that could break.** An older createVolume on a marked id answers `ErrExists`
  (`:431`), not plain. No Rust code or script reads etcd's header JSON: the data plane sees only
  protobuf. The ISR vectors carry no header JSON. `domain.Volume` is never JSON-encoded, so a
  codec on the embedded `Header` changes nothing there. #80's `B` bounds protobuf, not etcd
  JSON. #76's 8192-byte cap has room for the mark's ~35 bytes.
- **The claim header.** It stays unmarked until publication, so a crash leaves today's hidden
  header for every version.
- **Key-record cleanup.** The listing deletes are conditional on the header being absent or on
  the claim's revision, as `deleteChunks` does (`:481-498`).
- **The engine split.** It matches `engine.rs`. `MetaWait::Attach{part, attempt}` is at
  `:440-445`. The `attempt` fence is at `:1459-1465`. `detach` removes `attaching` at `:1858`. The
  checks are at `:1596-1621`. With #15, a release before `MarkEpoch` keeps #15's rule that a
  failed attach writes no mark (#15 design S11).
- **The rollout order and the S1 narrowing.** The rollout order is stated in the design, S12
  and decisions, and the S1 narrowing is consistent with it. The mark means that an answer
  without field 20 always comes from a plain volume.

## Earlier findings

- R2-C1: fixed for today's code (the claim is undone, and the pin is checked before the claim).
  The interaction with #73 and #76 is open: see R3-C3.
- R2-C2: fixed (split, `part: 2` under `attempt`, ordering with #15, two engine tests).
- R2-C3: fixed (first rollout ordered; a plain create takes an answer without field 20).
- R2-C4: fixed (no revision compares; the release re-checks target, epoch and first epoch).
- R2-C5: fixed (re-read on a failed create, before the claim, cached).
- R2-C6: fixed (`volkeys/<id>/` listed after the commit, in the header-absent branch, and under
  the claim).
- Nits 1–3: fixed.

### R3-C1 — Medium

**"Fails closed whatever the older instance is started with" is false for listings. An older
instance hides every encrypted volume from ListVolumes, so a drain finishes with their replicas
still on the node.**
- **What the design claims.** design.md:144-155 says an older instance "serves and lists no
  encrypted volume", and "in every mix" serves none. S12 says the volume is "left unserved,
  never plain".
- **Why that fails.** ListVolumes drops any header that does not decode
  (`service.go:851-854`). `drain.Step` only visits the ids it lists (`control/drain/drain.go:50-63`),
  and `flaminio-meta drain` uses it (`main.go:433-440`). So against an older instance, a drain
  of a node holding encrypted replicas reports "0 chunk replicas left", and the operator retires
  the node. Each such volume loses a replica, or all of them at RF 1.
- **Capacity on deletion.** With #72, an older binary's deletion of an unparseable header
  releases no capacity ("unparseable header writes none", #72 design:174).
- **A dropped instruction.** The revision also dropped round 1's "the only safe one deletes them
  first" (the old design.md:121-122).
- Fix:
  - Name the exception in the design, S12 and the docs: while an older instance serves, its
    listings omit encrypted volumes, so no drain or node removal is safe.
  - Restore the instruction that a rollback deletes the encrypted volumes first.
  - Narrow "fails closed" to per-volume calls.

### R3-C2 — Low

**The creation test asserts the opposite of the mark, and nothing checks that a new instance
writes the mark.**
- **The stale assertion.** design.md:312 keeps `TestCreateEncryptedVolume` "(… header JSON as
  today …)". The revision touched this row, yet an encrypted header now writes `id` as an object
  (design.md:49-55, contracts.md "The header's mark").
- **The gap.** `TestOldDecoderRefusesEncryptedHeader` decodes an encrypted header with a copy of
  today's `Header`, so a hand-built fixture satisfies it. No test fails if the service publishes
  an unmarked encrypted header, which is the one write that keeps older instances away.
- Fix: `TestCreateEncryptedVolume` asserts three things:
  - the published header's `id` is `{"name","profile"}`;
  - the claim header is unmarked;
  - after an Attach and a DeleteVolume mark, the header is still marked.

  `TestOldDecoderRefusesEncryptedHeader` decodes the header the service actually wrote.

### R3-C3 — Low

**The claim undo (R2-C1) is specified only against today's code. #73 and #76, both clear,
change what the claim writes and who may delete a creating header.**
- **With #73.** The claim also writes `owners/<id>` and the namespace usage (#73
  design:95-97). The undo deletes the chunks and the header only, which leaves a headerless
  owner record with its charge. #73 then answers the next creation `Aborted` and releases the
  record there (#73 design:88-92). The quota stays charged until a retry, and "the id is free,
  a retry wraps a new key" does not hold on the first retry. The undo also appends no
  `delete.committed` event.
- **With #76.** A creating header is deleted only by #76's rollback, and its root goes with it
  (#76 design:49, :62-81). Helpers resume the operation, and a helper wraps at finalization
  (design.md:83-85). A direct header delete leaves the root behind and bypasses that
  protocol.
- Fix: add "whichever lands second wires it" to the undo.
  - **With #73:** the undo's header delete also deletes the owner record and releases the charge,
    with #73's event.
  - **With #76:** a failed wrap leaves the operation for a retry or a helper, with no undo, or
    goes through #76's rollback.
  - Extend `TestCreateEncryptedCrashPoints` to cover both.

## Nits

1. design.md:53-55 and contracts.md "The header's mark" do not say what the decoder does with a
   mark whose `profile` is not `xts-aes256-v1`, or whose `name` is not the key's id. Say
   `Internal`, never plain and never this profile, so that a later step's profile cannot be read
   as XTS.

Reviewed commit: b87df55153e7f3148f041fd219a43423d541c0b6
