# #73 plan review, round 6 (incremental; design-code and design-security)

Scope: `git diff f262e1f..e2e87a7 -- docs/issues/73/ ':!docs/issues/73/report.md'` (HEAD `ac399a6` adds only
report.md). Checked against `meta/service/{auth,service}.go`, `meta/kv/{kv,etcd}.go`, `pkg/pki/pki.go`,
`control/csi/controller.go`, `scripts/lab.sh`, and the #70 and #76 plans where this diff relies on them.

## Earlier findings

- R5-B1 (`cfg` freeze): partly fixed. Revoking the sole `cluster-admin` is now refused, and `security-admin` may prune `cfg`. But the escape needs a chain hash that only `audit-reader` can read, and the fix weakens the pruning separation (R6-B1, R6-S1).
- R5-C1 (121-op sweep, blind tail write): fixed. Non-last sweep transactions are at most 118 deletes + 1 sweep put; the last is at most 118 + sweep delete + tail put = 120 ops with 2 compares (`meta/kv/etcd.go:91–92`). The tail is compared, and the etcd tests at 118 × n and 119 × n can fail without the change.
- R5-C2 (CSI name not tied to the binding): fixed (contracts.md:62–63, 101; test in roles_test.go).
- R5-S1 (least-privilege cost of Attach): not changed. It is recorded as a change after approval, not accepted (contracts.md:64–68), so no finding.
- R5-C3 (`ns-` prune refused while its `sys-` chain is full): fixed (spec S9; test).
- R5-C4 (overdraft bound and classifier untested): fixed.
- R5-S2 (Attach read before refusing a non-administrator): fixed (design.md:47–50; counting-store test).
- R5-C5 (#76 rollbacks outside DeleteVolume): partly fixed. #76's legacy rollback leaves a record that the same PVC's retry can never clear (R6-C1).
- R5-N1 (key size), R5-N2 (canonical `k`), R5-N3 (uses of `active`): fixed.
- R5-N4 (no-header branches): fixed. The new wording leaves a third case unnamed (Nit 1).
- R5-N5 (repair of `chains`; raised allowance on a full `cfg`): partly fixed (Nit 2).

Checked, no finding:
- The `createVolume` release of a headerless owner record cannot release a live volume's record. It compares the header key absent (ModRevision 0, not `readHeader`'s view, `service.go:275–290`). While the owner record exists, every active claim fails its `owners/<id>`-absent compare. Two concurrent releasers are serialized by the tail compare of the same `deleting_chain`.
- Every changed transaction fits within 120 compares and 120 ops.
- The 114-byte key and the 1.34 GiB figure are correct (664593 events × 2162 bytes).
- The new tests for the sole `cluster-admin`, `csi-controller` → `admin/csi` and `system-provisioner`'s Attach can each fail without their rule.

### R6-B1 — High: without an `audit-reader`, a sole `cluster-admin` that fills `cfg` cannot be revoked, and `cfg` freezes for life

The round-5 escape (contracts.md:205–208, spec S9 spec.md:148–151, decisions.md:172–174) is "`security-admin` sets a key and prunes `cfg` itself". That prune needs `chain_hash`, the chain hash of event `through` (design.md:181–184). Only ListAudit returns it (contracts.md:51, 269). Its role is `audit-reader`, which `security-admin` cannot also hold (contracts.md:100). TenantStatus's `ChainStatus` has no hash (contracts.md:257).

Nothing requires an `audit-reader` binding (contracts.md:97–104), and this step ships no exporter (intent.md:51–52). So a deployment can easily have none.

Sequence:
1. The sole `cluster-admin` (compromised, or simply leaving) fills `cfg` with no-op PutTenantPolicy writes. On an unpruned store, normal use fills it too.
2. Its revocation is now refused (InvalidArgument: no `cluster-admin`).
3. Binding a replacement is a grant, refused on a full `cfg`.
4. Binding an `audit-reader` is a grant too.
5. The prune cannot be built without the hash.

That `cluster-admin` keeps its role (forced ConfirmDeletion, AssignOwner, drain, moves) for the life of the store. No policy can be written again. The only remedy is etcd surgery.

Before round 5, the revocation was a recovery write, always admitted. The new "no `cluster-admin` left" rule removes that. design.md:258–259 ("a revocation … always possible") and decisions.md:132–134 ("can always revoke a binding") are now false for the sole `cluster-admin`.

The planned test ("a `cluster-admin` flooding `cfg` … `security-admin` … pruning `cfg`", design Tests S7–S9) can take the hash from the store or an `audit-reader`, so it passes while a deployment cannot do this.

Fix:
- Give `security-admin` the hash without a grant. Either add the tail's chain hash to `ChainStatus` (`hash:6:bytes`, so it can prune `cfg` through `next − 1`), or let `security-admin` call ListAudit on `cfg`.
- And/or admit, in the overdraft, a write that replaces the sole `cluster-admin`'s ID.
- Test with no `audit-reader` bound: the `cluster-admin` fills `cfg`; `security-admin` replaces it and prunes `cfg` using only its own calls.

### R6-S1 — Medium: `security-admin` alone can now prune `cfg` with a key it holds, erasing unexported configuration history; the plan does not state this

Before round 5, the role that sets the receiver key (`security-admin`) and the role that prunes (`cluster-admin`, approved answer Q2, decisions.md:31–32) were different. So deleting `cfg` events needed either the real receiver's receipt or two colluding people.

Now `security-admin` may prune `cfg` (contracts.md:52; spec S8 spec.md:132–133). It can:
- set a receiver key whose private half it holds (a recovery write);
- sign its own receipt;
- prune `cfg` through its latest event.

That deletes, never exported, the `authorization.binding` events of its own grants. This includes a grant to an identity it controls (Q3), and now needs no CA access. What remains is `audit.pruned`, with the fingerprint of the self-set key, and the `slog` lines.

Several texts still say otherwise:
- the ADR proposal: "an independent receiver's Ed25519 signature" (adr-proposal.md:45–46);
- the receiver-key definition: "independent of the metadata service" (spec.md:34);
- intent: "pruned only once a receiver has signed for them" (intent.md:78);
- design "Security" (design.md:255–268) does not list this power;
- Q2's approved answer is not marked as changed.

No test covers it.

Fix, one of:
- Restrict `security-admin`'s `cfg` prune to a full `cfg` (`present >= cap`), the only case the lock-out argument needs. State the residual in the ADR proposal's consequences and in design "Security" as a change after approval, and mark Q2. Test: a `security-admin` prune of a non-full `cfg` is refused, and the surviving `audit.pruned` carries the key's fingerprint.
- Or drop the self-prune: admit the sole `cluster-admin`'s replacement in the overdraft, so the key setter and the pruner stay different roles. R6-B1's hash access is still needed.

### R6-C1 — Medium: after #76's legacy rollback, the same PVC's retry is refused as an orphan for good

The diff says #76's legacy rollback may leave a headerless owner record (design.md:147–150). Only one with a `deleting_op` is released by a creation; one without is an orphan (design.md:97–99; spec S5 spec.md:102–105; contracts.md:283).

A legacy rollback deletes an unmarked creating header, which has no `deleting_op`. The record exists when #73 lands before #76 (design.md:147, "Until #76 lands"): #73's binary writes owner records on headers without a create root.

#76's rollback runs inside the creation retry, which then "freshly admit[s]" (#76 design.md:80–84, 96).

#73's claim then fails on `owners/<id>`:
- the result is FailedPrecondition `orphan`;
- the CSI controller turns that into ResourceExhausted (`control/csi/controller.go:248–249`);
- the provisioner retries it forever;
- the provisioner never calls DeleteVolume for an unprovisioned PVC, so "DeleteVolume's pass" never comes.

The record's stale charge also counts in the quota check, which runs before the claim (design.md:88–91). So at the namespace's limit the retry is refused before it reaches the claim, the `deleting_op` case included. The PVC stays stuck and the charge is held until a `deletion-override` acts. That contradicts S4: "keeps it until the creation is resumed (#76) or the volume deleted".

Fix:
- In the failed claim, a headerless owner record without a `deleting_op` whose owner is the request's (the same namespace and PVC UIDs) is that PVC's own interrupted creation. Release it as DeleteVolume's pass does (`header-missing`), then answer Aborted.
- Run this release before the quota check, or leave the record's own charge out of it.
- Or require #76's legacy rollback of a header with an owner record to go through #73's mark.
- Test: a stand-in for #76's legacy rollback of an unmarked creating header, then the same PVC's CreateVolume, also at the namespace's limit.

### R6-C2 — Medium: spec S4's new growth charge has no approach or test in #73, and the owner record's `bytes` is never said to grow

Spec S4 (spec.md:89–91) now says each growth step charges the namespace and class quota, and that "whichever of #70 and #73 lands second adds that charge and its test" (decisions.md:180–181). #73's design has no approach for it: nothing in Files touched, Tests S2–S4 or contracts.

#70's plan writes "its owner record and class usage" and "delete releases the grown size" (#70 design.md:82–89). That only works if `owners/<id>.bytes` rises with each step. Otherwise `finishDeletion` releases only the creation's bytes and leaks the rest (design.md:117–121). If the usage is raised but the record is not, the release can also fall "below the charge" and give Internal.

Contracts "Records" (contracts.md:129) defines `bytes` as the claim's charge and says nothing of a later write. A growth write also changes the owner record's revision, which OverrideDelete compares.

Fix: if #73 lands second, a growth step:
- raises `quota/<ns>/<class>.bytes` and `owners/<id>.bytes` by the step's bytes, comparing both and the policy;
- refuses over the limit with the contracts' ResourceExhausted message;
- charges nothing for a volume with no owner record.

State in contracts "Records" that `bytes` is the current charge. Name the test (#70's `TestExpandChargesQuota`), with deletion releasing the grown size.

### R6-C3 — Low: the overdraft can end with no receiver key, which freezes `cfg`; the "always" claim is wrong

Removing the receiver key is a recovery write (contracts.md:105–106). So once `cfg` is full, a set/remove sequence by `security-admin` can use up the 1024-event overdraft and leave no key. Then:
- PruneAudit answers FailedPrecondition (no key);
- every further write, a key-setting one included, is refused;
- `cfg` is frozen for life.

So "`security-admin` can always make that prune" (contracts.md:206–208) is false. Only `security-admin` itself (compromised, or a script) reaches this state. No test covers it.

Fix:
- Do not admit a key removal past `cap`, or always admit a write that sets a key when none is set.
- Test: on a full `cfg`, a key removal in the overdraft is refused.

### R6-C4 — Low: `security-admin`'s PruneAudit of a non-`cfg` chain has no stated refusal point and no test

The PruneAudit row is request-dependent: "`cluster-admin`; for `cfg`, also `security-admin`" (contracts.md:52). Design "Authorize, in order" (design.md:53–54) puts the method table's roles into the context and does not say where the chain is checked. The only test shows the allowed side (`security-admin` pruning `cfg`, design Tests S7–S9). TestRoleTable's "every method by each role" uses one request per method.

Fix:
- State that Authorize, or the handler before any read or sweep, refuses `security-admin` for any chain but `cfg` (PermissionDenied `no-role`).
- Test it with a `sys-` and an `ns-` chain.

## Nits

1. design.md:126–130 now covers "no header at the loop's first read" and "after this call's own #76 rollback". A later read with no header is unnamed: a lost race to another deleter, or another path deleting the header between reads. Today's loop answers NotFound there (`service.go:527–533`). Say it takes the first-read branch.
2. contracts.md:89–91 restores `chains` to "at least the largest `k` of any policy's chain or `audit/tail/ns-…` key". A deleted policy's chain has neither, because DeleteTenantPolicy deletes its tail (contracts.md:221–222), so its name can be reused. Take the bound from the `ns-` names in the `sys-` chains' `audit.pruned` events, or use the store's current revision, which bounds every `k`.

Reviewed commit: ac399a63c5450af7ed53360c777a5af938270097
