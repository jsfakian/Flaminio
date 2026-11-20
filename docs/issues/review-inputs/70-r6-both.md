# Issue #70, plan review round 6 (incremental), design-code and design-security

Scope: `git diff e729522..942d1eb -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70` (HEAD b5a942b adds only report.md). Line numbers are at HEAD.
Dependency plans read at: #72 559eef3, #73 ac399a6, #75 02853e8 (and 4663835, which #70 pins).

## Earlier findings

- R5-C1: fixed. No flags file is read. `csi_expand` expands through each instance (design.md:221–225). Its probe id now conflicts with #75's later text (R6-C1).
- R5-C2: fixed for #72 (design.md:47–50). #73's reads are not placed (R6-C2).
- R5-C3: fixed (design.md:207).
- R5-S1: fixed in form. There is one charge rule (design.md:74–89) and `TestExpandChargesQuota`. #73's keys count in the 120 bound, and #73's spec S4 states the rule. Two gaps are new (R6-S1, R6-S2).
- R5-S2: fixed. #72's spec S3 now lists growth (#72 spec.md:60–61, 8c718f4).
- R5-S3: resolved by removing release (b) (coordinator's choice).
- R5-S4: resolved by removing release (a) (coordinator's choice).
- R5-S5: fixed (same as R5-C1).
- R5-S6: fixed. NodeUnstage's `umount` is now in design.md:277–278, spec S6:79–80 and decisions.md:35–36. The citations match: node.go:135 `n.mu.Lock()`, :137 `n.unmount`, mount.go:81–84 `umount`.
- R5-S7: resolved by removing the narrowing; no text depends on it.
- R5-S8: fixed (design.md:208: slots alone, left-out nodes, a preferred node short of room).
- R5-S9: fixed (design.md:99–100).

## Findings

### R6-B1 — High

**Step 1 refuses every call, but the plan does not say how this step is then tested, checked or shipped if #70 lands first.** The plan expects that order ("the ledger's lander also removes step 1's refusal", design.md:77).
- **The tests cannot pass.** Step 1 answers `FailedPrecondition` before the flag, validation and any read (design.md:41–44). `TestExpandAppendsChunks`, `TestExpandSteps`, `TestExpandFaults`, `TestExpandLegacy`, `TestExpandRaces` and most cases of `TestExpandRefusals` (flag off, InvalidArgument, NotFound, #80, `ErrPlacement`) never get past step 1 (design.md:208–209).
- **The CLI test fails too.** `main_test.go`'s "`expand` ... prints the volume" (design.md:214) runs on the real `service.New` behind TLS (`meta/cmd/flaminio-meta/main_test.go` fixture), which refuses as well.
- **Only one test is adjusted.** The only text about the period before #72 is "`TestExpandRefusals` has step 1's refusal". No test seam is named, so S2 and S3 ship with no runnable test and with code behind step 1 that never runs. `make test`, which the design names as the proof (design.md:216), would fail.
- **The lab check fails.** `make csi-expand` fails at its probe on every instance (design.md:224, "an instance that refuses ... fails the check").
- **The intent's "Done when, here" cannot be met.** It needs PVCs that "grow from a resize request" (intent.md:31).
- **The advertising breaks S9.** `EXPAND_VOLUME`, `VolumeExpansion OFFLINE` and `allowVolumeExpansion: true` (design.md:120–123) are still advertised while no configuration can grow a volume. S9 says "Advertise only what is supported" (spec.md:93). The settled answer to advertise while the flag is off assumed the operator could turn the flag on (decisions.md:56–57).

**Fix:** pick one of the following and state it in the design, the spec's Tests and the intent.
- (a) #70 lands after #72. Drop step 1, and drop "whichever lands second" for the ledger.
- (b) Keep step 1 behind an unexported, test-only switch (for example, set from `export_test.go`; never an `Options` field or a serve flag). The S2, S3 and CLI tests run with it. Say that `csi-resizer`, `allowVolumeExpansion`, the capabilities, the README and lab.md claims, and the `make csi-expand` run all wait for the ledger's lander, and that this step's Done-when is met only then.

### R6-B2 — Medium

**Step 1 is checked before the flag, so a refusal while the flag is off no longer names the flag.** This contradicts a settled answer and S9.
- **The settled answer:** "a request while the metadata flag is off gets `FailedPrecondition` naming the flag" (decisions.md:56–57).
- **S9:** "every refusal then names the flag" (spec.md:94). The design's Risks repeat it: "refusals name the flag" (design.md:293–294).
- **The new order:** step 1 now answers `volume expansion needs growth to charge the capacity ledger (#72)` "whatever the flag" (design.md:41–42).
- **Not recorded:** the round-5 change is not listed among the changes made after approval for this answer.

**Fix:**
- Check `!s.opts.VolumeExpansion` first, then step 1's refusal. A flag-off refusal then names the flag, and its test runs before #72 lands.
- Or record the change to that settled answer and to S9.

### R6-S1 — Medium

**"A volume without an owner charges nothing, as at creation" lets a tenant grow an unassigned volume past #73's quota.**
- **Two kinds of volume have no charge:**
  - At creation, a volume without an owner is a system volume. Its claim writes `owners/<id>` `{system: true}` (#73 design.md:103–104).
  - Volumes that exist at activation are unassigned: they have no owner record at all (#73 spec.md:172–175).
- **#73 blocks deleting them:** it refuses to delete an unassigned volume (#73 contracts.md:283).
- **#73 charges them only at assignment,** "even past the limit" (#73 spec.md:174–175; AssignOwner checks overflow, not the limit, #73 design.md:161–166).
- **The bypass:** as written, a tenant resizes an unassigned PVC (pvc-<uid>) up to #80's cap with no charge. AssignOwner later charges the grown size past the limit. #73 calls its service check "the authority on every path" (#73 spec.md:72).
- **The Risks text is wrong too.** "once both land, #73's quota does" (design.md:283–284) leaves out activation and unassigned volumes.

**Fix:**
- Once tenancy is active, refuse a step on a volume with no owner record with `FailedPrecondition` `unassigned`, as #73's DeleteVolume does.
- A system owner record charges nothing. The step compares `owners/<id>` at its revision, or absent.
- Add the unassigned case and the system case to `TestExpandChargesQuota`.
- Reword the Risks text: "once both land and tenancy is active, #73's quota bounds growth of tenant-owned volumes; unassigned volumes do not grow".

### R6-S2 — Medium

**`ExpandVolume` has no row in #73's role table, and the Charges rule does not tell the second lander to add one.**
- **Today:** `ExpandVolume` joins `auth.go`'s administrator-only case (design.md:194; `meta/service/auth.go:64`, verified).
- **Once #73 is active:** Authorize lets an administrator through only with a role "that the method table allows", otherwise `PermissionDenied` (#73 design.md:45–55). The table has no `ExpandVolume` (#73 contracts.md:31–51).
- **The likely result:** once active, every CSI resize fails `PermissionDenied`.
- **The other result:** a lander who keeps the administrator-name rule lets any administrator certificate grow any tenant's volume and spend its quota. That includes `audit-reader` and `deletion-override` names.
- **What the plan covers:** "whichever of #70 and #73 lands second" adds only the quota charge (design.md:75–77).

**Fix:**
- Add `ExpandVolume` to the second lander's work: administrator name before activation. Once active, the role of the owner's kind, as DeleteVolume has it: `csi-controller` for a tenant owner, `system-provisioner` for a system one.
- Add an `auth_test.go` case for each role.
- Record the row #73 needs in the batch log.

### R6-C1 — Low

**The lab probe uses a fixed id, which a pending tombstone blocks. #75's current rule forbids that.**
- `csi_expand` creates `csi-expand-probe` on every run (design.md:222).
- `createVolume` refuses an id that still has a tombstone (`meta/service/service.go:373–378`). The id stays taken until every placed node's sweep confirms it (`control/csi/tombstones.go:9`, every 5 s).
- If one data node's sweep is stuck, every later run fails at `create`. That failure is not one of the cases the design names ("refuses or lacks `ExpandVolume`").
- #75's text after 4663835 (02853e8, "The rule for every lab check of such a flag") requires per-run ids `<prefix>-<vm>-<unix time>` checked with `valid_id`. It also requires a trap, set before the first create, that deletes every id passed to `create` (`NotFound` counts as deleted).
- #70 pins 4663835, so it now cites the older text.
- The S9 test checks deletion only when all instances pass (design.md:214). "Deleted whatever happens" is not tested when the check fails.

**Fix:**
- Use `csi-expand-probe-<unix time>`, checked with `valid_id`, under #75's trap rules.
- Add a test case: w refuses, and the probe's `delete` is still recorded.
- Pin #75 at 02853e8, or drop the pin.

### R6-C2 — Low

**Step 3's read order places #72's reads but not #73's.**
- The attempt reads the gate, the header, `s.nodes()` with the reports, then the ledgers (design.md:47–50).
- The quota bullet compares `owners/<id>`, `quota/<ns>/default` and the policy (design.md:82–84; #73 design.md:88–93), but the read order does not say when they are read.
- This is the gap R5-C2 raised for the ledgers. An implementer may read them after the placement is decided, or skip the policy read.

**Fix:** in step 3, add "then, for a tenant owner, `owners/<id>`, the policy and `quota/<ns>/<class>` (the revisions the transaction compares)".

## Nits

- **R6-C3 — Nit.** decisions.md:134–137 (round 3) still says the release path "goes into `README.md`'s upgrade note and `docs/followups.md`". decisions.md:154–158 (round 4) still describes the narrowing and the release paths as current choices. Neither is marked as removed by round 5 (decisions.md:173–177). Add "(removed in round 5)" to both entries.

Reviewed commit: b5a942b8c6aa9e44830ce16dfdb920abd7b095ff
