# #73 plan review, round 10 (incremental; design-code and design-security)

Scope: `git diff 9eb18b4..72396a6 -- docs/issues/73/ ':!docs/issues/73/report.md'`. HEAD `cc1ca57` adds only report.md. Checked against contracts "Activation and the authorization record", "Records", "Budget" (contracts.md:203–244) and "Protobuf" (`ChainStatus`, contracts.md:281–282); design PutAuthorization (design.md:59–61), PruneAudit (design.md:193–203) and AssignOwner (design.md:173–178); `meta/kv/kv.go:38–52` and `meta/kv/etcd.go:67,96` (mod revisions); and the issue's "Done when".

Q3's step 1 is sound. Suppose PVC `U` still exists at the third read. Its namespace cannot have been deleted and re-created since the first read, because a namespace is removed only after its contents. The UID from the second read is therefore the original namespace's. Suppose instead that the namespace or the PVC was re-created between the reads. The PVC `U` is then gone (UIDs are never reused), and the third read refuses.

## Earlier findings

- R9-B1: fixed as raised. The repair paragraph now states `changed_by`, the conditional txn, the precondition and the out-of-band record. The four-step `audit_test.go` case is in design S7–S9 and spec S7–S9. Its step 3 fails if the service caches or cross-checks the key, and its step 4 fails if the stale compare does not use the store's mod revision. The precondition opens R10-B1. One wording residual is left (Nit 1).
- R9-B2: fixed as raised, and consistent in spec Q3, both decisions entries, design Risks and the usage comment. The holding namespace still admits creations (R10-B2).
- R9-N1: fixed. The order is sound (above).
- R9-N2: fixed (spec.md:161).
- R9-N3: fixed (decisions.md:48).

### R10-B1 — Low: the repair's precondition does not establish what it claims, and the txn does not hold it

**Summary.** contracts.md:115–118 says to check that "`cfg`'s `ChainStatus` shows `present` at `cap + 1024`, so no PutAuthorization can commit", then write with a txn that compares only the record's mod revision.
- **Not every PutAuthorization is stopped.** A PutAuthorization that changes nothing is a recovery write with `k = 0`. It is admitted at `present + 0 <= cap + 1024` (contracts.md:215), so it commits on a spent overdraft. It is harmless, because the txn's compare catches it, but the claim is false as written.
- **A pending `cfg` sweep defeats the test:**
  - `present` still counts swept events until the sweep ends (contracts.md:238–244). It can also be `cap + 1025`, with one pending `audit.pruned`.
  - Anyone's PruneAudit of `cfg` finishes that sweep with no key (design.md:194). After that, `cfg` has room, and `security-admin` can set a key through the audited path.
  - Read as "at least", the precondition makes an unaudited repair that was not needed. Read as "equal", it is mis-stated at `cap + 1025`.
  - `ChainStatus` has no sweep field (contracts.md:281–282), so the operator cannot see the sweep.
- **The repair can run on a `cfg` with room.** The txn compares only the authorization record (contracts.md:118), so nothing ties the earlier check to the write. Its result is a key change with no event, made when the audited path was available.
- **`ChainStatus` has no `present`.** Its fields are `next`, `pruned`, `reserved`, `cap` and `hash`.

**Fix.**
- **The precondition** (contracts.md:115–118 and the usage comment's runbook):
  1. Finish any pending sweep first: a PruneAudit of `cfg` through its `pruned`, which answers `pruned` and needs no key.
  2. Then require `next − 1 − pruned = cap + 1024`, from `ChainStatus` or `audit/tail/cfg`, and `audit/sweep/cfg` absent.
- **The txn:** besides the record's mod revision, compare `audit/tail/cfg`'s mod revision as read and the absence of `audit/sweep/cfg` (`version(...) = 0`), so the precondition still holds when the write commits.
- **The claim:** reword "so no PutAuthorization can commit" to "so no PutAuthorization can set a key".

### R10-B2 — Low: the holding namespace is a tenant namespace to every component, and its policy admits creations

**Summary.** Q3 makes the holding namespace "a dedicated namespace created to hold legacy volumes, with its own UID, policy and `default` limit … never a tenant's namespace … since the volume is deleted next, charging that namespace costs nothing" (spec.md:283–286; decisions.md:69–72, 85–88; design.md:305–308).
- **Nothing makes it different from a tenant namespace.** AssignOwner needs "a limit for `default`" (design.md:176; contracts.md:313). A limit that admits assignments also admits creations, so anyone with PVC rights there gets tenant volumes under that policy. The CSI controller and the service cannot tell it from a tenant namespace.
- **"Never a tenant's" is a promise with no means.** Nothing in the policy or the runbook enforces it.
- **"Costs nothing" is true only of the quota charge.** Each legacy deletion still spends two events of the holding namespace's chain (contracts.md:207). With no receiver in this step, that chain cannot be pruned, so its policy and allowance stay allocated for the store's life (contracts.md:257–264). The runbook gives no size for its `max_audit_events`.
- **A verifier cannot tell these deletions from a tenant's.** It sees them as tenant deletions, owner `<holding UID>/<PVC UID>`, unless it knows the holding UID.

**Fix.**
- **The policy** (spec Q3, decisions, design Risks, usage comment):
  - Give the holding namespace's policy a `default` limit of 0 volumes. An assignment is charged past the limit (design.md:177), and every creation is refused.
  - State that the class bounds admit 0.
  - Set `max_audit_events` to at least twice the number of volumes it will hold.
- **The runbook:** grant no one PVC rights in that namespace, and record its UID.
- **The S11 test:** make the existing "PVC is gone" assignment target a namespace whose `default` allows 0 volumes, then show that a creation there is refused.

## Nits

1. **`changed_by`'s definition still says "the record's last write"** (contracts.md:149). After the repair (contracts.md:119), and after the other etcd-level restores (contracts.md:90–91, 113–114), that is no longer true. Write "of the service's last write of the record".
2. **"The runbook records who made the repair and when"** (contracts.md:119–120; decisions.md:189; design.md:261–262). A usage comment can record nothing, and this out-of-band note is the repair's only attribution under "Done when". Name who writes the note and where it is kept, for example "the operator records who and when in <the place the runbook names>".

Reviewed commit: cc1ca570d84ef9b40e08eda8f0961316c73cdb20
