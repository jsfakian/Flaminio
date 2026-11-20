# #73 plan review, round 9 (incremental; design-code and design-security)

Scope: `git diff ae0b629..3aaec96 -- docs/issues/73/ ':!docs/issues/73/report.md'`. HEAD `9eb18b4` adds only report.md. Checked against contracts "Activation and the authorization record", "Records" and "Budget", the AssignOwner rules (design.md:173–178, contracts.md:308), `meta/kv/kv.go:33` and `meta/kv/etcd.go:91–92` (120 operations), and the issue's "Done when".

No service transaction changes. The lost-key repair is one etcd write made outside the service, so the 120-operation bound is untouched. The repair gives no party a power that an etcd-level writer lacks.

## Earlier findings

- R8-B1: fixed as raised. The `claimRef` step is removed, and every remaining path is an AssignOwner the contracts admit, with the refusals unchanged. One gap R8-B1 listed is still open: the designated namespace may be any namespace. It is carried as R9-B2.
- R8-B2: fixed. The residual is stated the same way in the contracts, spec S9, design Risks, the ADR's Consequences and decisions, the repair is defined, and the decisions entry is renamed. No cross-reference to the old name remains. The repair's definition opens R9-B1.
- R8-C1: fixed (design.md:256–261; S11 test reworded; decisions.md:173–175).
- R8-N1: fixed (spec.md:231–233; design.md:242).

### R9-B1 — Low: the lost-key repair contradicts `changed_by`, is an unconditional read-modify-write, and has no test

**Summary.** contracts.md:115–118 says the operator writes a new `receiver_key` "keeping every other field. The write leaves no event and no `changed_by`."
- **`changed_by` is wrong after the repair.**
  - Keeping every field keeps the previous `changed_by`. contracts.md:145 defines that field as the "`{actor, role, cert_serial, peer, time}` of the record's last write", and decisions.md:187 says "each record keeps `changed_by`".
  - After the repair, the record therefore names the last service writer, a `security-admin`, as the author of a key it never set.
  - "Leaves no `changed_by`" can also be read as removing the field. The two readings disagree.
- **The key change is the only one with no attributable record.** The issue's "Done when" requires "key operations have durable, attributable audit records".
  - The only traces are the new fingerprint in TenantStatus's `Authorization.receiver_key` and in the next `audit.pruned`.
  - A `cfg` prune through `next − 1` then erases the last `authorization.receiver` event. After that, only the logs show that the key changed out of band.
- **The write is a read-modify-write.** The operator reads the record, keeps its fields and writes it back, with no compare. That is contrary to AGENTS.md's Principle that etcd writes depending on a read are conditional transactions.
  - Under the stated precondition, the overdraft is spent, so no PutAuthorization can commit and the write is safe.
  - Nothing makes the operator check that precondition first. A repair made while a revocation can still commit would silently undo that revocation.
- **No test.** The S7–S9 rows (design.md:242; spec.md:227–236) name none. The service-side claims are therefore unproven: a record written outside the service parses and is honoured (no cache, no cross-check against `cfg`), and "the next prune's `audit.pruned` names the new key's fingerprint" holds.

**Fix.**
- **Contracts:** say that `changed_by` keeps the last service write's value and does not attribute the new key. The runbook records who made the repair and when. A verifier finds the repair as a fingerprint that no `authorization.receiver` event or logged record sets.
- **Conditional write:** make the write an `etcdctl txn` that compares the record's mod revision as read, and check the precondition (`cfg`'s `ChainStatus` at `cap + 1024`) before it.
- **Test:** add to `audit_test.go` on `backends`:
  1. Spend the overdraft with key changes.
  2. Rewrite the record through the store with a new key and every other field kept.
  3. Make a prune with the new key: it is admitted, and its `audit.pruned` names the new fingerprint.
  4. Make a PutAuthorization at the revision from before the repair: it is refused as stale.
- Mirror the test in spec S7–S9.

### R9-B2 — Low: Q3's designated namespace is unconstrained, so a live or re-created tenant can still be given a volume it never owned

**Summary.** With the PVC gone, the owner is "a namespace the administrator designates, with its own policy" (decisions.md:66–68 and 75–80; spec.md:280–281; design.md:303–305).
- **The service admits any namespace.** AssignOwner admits a tenant owner for any namespace UID that has a policy and a `default` limit (design.md:173–178; contracts.md:308).
- **"Its own policy" does not mean a dedicated namespace.** Every namespace that has a policy has its own.
- **The obvious candidate is a name.** The runbook gives no rule for which namespace to choose. The one an administrator will see is the PV's `claimRef` namespace, which is the name-based inference round 8 removed, now without even the PVC-UID check.
- **What a wrong designation costs the namespace:**
  - its tenant is charged "even past the limit" (spec S11), which can refuse its own creations;
  - its policy sets the volume's protection;
  - the deletion's two events spend its chain's allowance;
  - if the namespace was re-created under the old name, it never owned the volume. That is R8-B1's outcome.
- **The round-8 resolution is silent on this.** R8-B1 listed it ("Step 3 lets the administrator designate any namespace, a live tenant's included"), and report.md's round-8 resolution of R8-B1 does not address it.

**Fix.**
- **Where:** the Q3 answer in spec, decisions (both Q3 entries) and design Risks, and the usage comment's upgrade order (design.md:257–259).
- **The rule:** the designated namespace is a dedicated one, created to hold legacy volumes, with its own UID, policy and `default` limit. Never a tenant's namespace, and never one chosen from a PV's `claimRef`.
- **Since Q3 deletes the volume next,** charging the holding namespace costs nothing.

## Nits

1. **Step 1's order cannot be followed as written** (spec.md:278–280; decisions.md:65–66; design.md:302–303). The text says "read from the Kubernetes API, the namespace first, as the CSI controller reads them".
   - The CSI controller knows both names from CreateVolume's parameters. The runbook finds the PVC by UID, so it learns the namespace's name only from the PVC.
   - Read in that order, the namespace UID is taken by name after the PVC read, which leaves the re-created-namespace window open.
   - Reword: "find the PVC by its UID; read its namespace's UID; read the PVC again by namespace and name, and require the id's UID".
2. **A misread antecedent** (spec.md:158–162). "A receiver key removal is not such a change" now follows the etcd-level write, so "such a change" reads as that write. Move the sentence after the overdraft sentence, or write "A receiver key removal does not use the overdraft".
3. **A stale round reference** (decisions.md:46). It still says "restated in round 7". Round 8 narrowed the answer, and spec Q3 and design Risks say "rounds 7 and 8".

Reviewed commit: 9eb18b4a82990c41c01dab6cf3ad2097ca3b0658
