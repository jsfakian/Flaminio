# #73 plan review, round 8 (incremental; design-code and design-security)

Scope: `git diff c80ae2f..b124ff2 -- docs/issues/73/ ':!docs/issues/73/report.md'` (HEAD `ae0b629` adds only report.md). Checked against `control/csi/controller.go:231–256` (the volume id, PV name and handle are all `req.GetName()`), `meta/kv/etcd.go:91–92`, and the unchanged contracts (AssignOwner, Budget, Errors).

No transaction changes in this revision. PruneAudit's reorder writes nothing new, and the Q3 restatement adds no service path. The 120-op bound and the conditional writes are unchanged.

## Earlier findings

- R7-C1: fixed as raised. Every step is now a tenant AssignOwner, which the contracts admit, and the refusals are unchanged. Step 2 opens R8-B1.
- R7-B1: fixed. Every "always" now carries the precondition, spec.md:44–47 and design.md:275–277 treat a removal as an ordinary write, and a test is added. I found no other sequence that freezes `cfg` or locks out both roles. The residual is understated and stated in one place only (R8-B2).
- R7-C2: fixed. The replay check runs after the sweep's finish and before `not-full`. It writes nothing and answers only a `through` already pruned, so it admits no prune that was not done. It is tested for `security-admin`, and that test fails under the old order.
- R7-C3: fixed (design.md:159–162, decisions.md:236–239).
- R7-S1: fixed (adr-proposal.md:45–48; decisions.md:36–39; spec Q2).
- Nits 1–4: fixed.

### R8-B1 — Medium: Q3 step 2 takes the owner's namespace from a name, so a re-created namespace receives a volume it never owned

**Summary.** Step 2 assigns "the namespace in the `claimRef` of the PV named by the volume id (its UID read from the API)" (decisions.md:63–65; spec.md:274–276; design.md:300–302).
- A `claimRef` holds the namespace's name, not its UID. Reading the UID now returns whichever namespace holds that name today.
- The usual reason a legacy `pvc-` volume has no PVC is that its namespace was deleted, since a namespace's deletion removes its PVCs. Re-creating a namespace under the same name is common.
- When that happens, the step assigns the volume to a tenant who never owned it. That tenant is charged "even past the limit" (spec S11), which can refuse its own creations. Its policy also sets the volume's protection, and the deletion's events land in its chain.

**What it contradicts.**
- spec.md:184: "No owner is guessed from a name".
- decisions.md:68: "never guesses an owner from a name".
- intent.md:15: a namespace string does not establish ownership.

**Steps the text leaves out.**
- Step 2 never checks that `claimRef.uid` equals the UID the id carries.
- Step 2 never checks that the PV is Flaminio's, with volume handle equal to the id.
- It does not say what happens when no PV exists, which is the usual leftover once a Retain PV is deleted.
- Step 3 lets the administrator designate any namespace, a live tenant's included, and charge it.
- Step 1 no longer says the namespace is read before the PVC, as the CSI controller reads them.

**Fix.**
- Accept the `claimRef` namespace only if all three hold:
  - the PV's CSI driver is Flaminio's and its handle equals the id;
  - `claimRef.uid` equals the id's UID;
  - the namespace's `creationTimestamp` is not later than the PV's (a re-created namespace is newer than the PV).
- Otherwise, including when no PV exists, go to step 3. Alternatively, drop step 2.
- Make step 3 a dedicated namespace UID that no tenant uses, with its own policy.
- In step 1, read the namespace first.
- Make the change in all three places, and reword "never guesses an owner from a name".
- Name the re-created-namespace case in the runbook; see R8-C1 for its test.

### R8-B2 — Low: the "key lost after the overdraft is spent" residual is understated, its repair is undefined, and only the contracts state it

**The impact is wider than `cfg`.** contracts.md:215–217 says such a key "leaves `cfg` full until an etcd-level repair".
- Every PruneAudit must verify against the record's one key (design.md:193–198).
- With that key's private half lost and no overdraft left to replace it, no chain can be pruned at all.
- Every `sys-` and `ns-` chain then fills as in "Without a receiver key" (contracts.md:247–251).
- Allowances cannot be raised either, because `cfg` is full. So deletions stop across the store, not only configuration writes.

**The repair is not defined.** The contracts define repairs for a missing record and for one that does not parse (contracts.md:90–93, 113–114), but not this one. They do not say which record is written, with what, or what trace the write leaves.

**The residual appears only in the contracts.**
- decisions.md:217 still has the heading "Configuration cannot freeze".
- spec S9 (spec.md:155–158) and design Risks (design.md:275–278) give the precondition but not the residual.
- The ADR Consequences (adr-proposal.md:82–85) list the self-prune residual but not this one.

**Fix.**
- State it as: "no chain can be pruned, and `cfg` refuses every write, until an etcd-level write of the authorization record's `receiver_key` (runbook)".
- Define that write next to the record's repair.
- Rename the decisions.md:217 entry, and add the residual to spec S9, design Risks and the ADR Consequences.

### R8-C1 — Low: round 7's runbook duties have no home in the Docs plan, and the evidence order is checked nowhere

**Two new duties.**
- The evidence order for legacy owners (Q3).
- `security-admin` keeping the private half of the last key it sets in the overdraft (contracts.md:215–216; spec.md:157–158).

**Where they would go.** The Docs plan's list for `flaminio-meta`'s usage comment (design.md:256–258) covers `tenant`, `audit`, the upgrade order, etcd space and the record's repair. The upgrade order (design.md:69–72) mentions only "legacy assignments".

**The S11 test.** Its new case (design.md:244; spec.md:251–252) runs in `tenant_test.go`, and the service never sees a PV or a PVC. The case is "assigned to its PV's `claimRef` namespace, or to a designated namespace". It passes with any namespace that has a policy, and it passed before round 7. It cannot fail on the evidence order.

**Fix.**
- Add both duties, and R8-B2's repair, to the usage comment's list: the evidence order under the upgrade order, the key duty under `audit`.
- Describe the S11 case as "AssignOwner admits a tenant owner for a `pvc-` id whose PVC is gone".
- State that the service cannot check the evidence order, which is a runbook step.

## Nits

1. design.md:242 and spec.md:231–232 say "the overdraft spent by key changes, then a prune with the last key admitted and the next key change refused". Read in order, the key change comes after the prune, and by then it is admitted because `cfg` has room. Reword: "the overdraft spent by key changes: the next key change refused; a prune with the last key admitted; a key change admitted after it".

Reviewed commit: ae0b6296770960fce8ea5b4a8242d6641b21133f
