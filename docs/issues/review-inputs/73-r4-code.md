# #73 round 4, design-code review (diff 7a91186..98c1242)

## Round 3 status
R3-C1 fixed (configuration writes leave the chains; a test fills every sys chain then PutAuthorization/PutTenantPolicy/prune) but see F1.
R3-C2 fixed in #73's text, still one-sided: see F2. R3-C3 fixed in spec, design, ADR; contracts/design headers: F5.
R3-C4 fixed (wording "next call" plus a test). R3-C5 fixed (Error() keeps text, %w, tested). R3-C6 fixed as a runbook rule; the missing-record
case is not covered: F4. R3-C7 fixed (rows for racing activation, PutAuthorization cases, CLI, TenantStatus). R3-C8 fixed: the
unreachability argument holds (volumes >= 1, so every tenant owner record leaves a quota record; a policy with usage cannot be deleted).
R3-CN1-3 fixed.
Checked and fine: transaction sizes (policy delete about 20 compares, sweep 119+1 ops, mark 4 compares/4 writes, all under
meta/kv MaxTxnOps = 120; only the policy transactions compare `allocation`); every dependent write is a conditional
transaction; Attach keeps auth.go's `admin || only_if_target` rule with no read; `pki.ParseID`/`Identity.String` (pki.go:202,
209-238) support the canonical-ID check; the space bound 655360 x 2128 B is about 1.3 GiB.

## Findings

1. **Medium: a policy can never be deleted once its chain was pruned, so the allocation is a lifetime budget.**
   Evidence: contracts "Budget": `audit.pruned` raises `pruned` by at least one but is itself appended at `next`, and
   `through < next`, so after a prune `present = next - through >= 1`. "Allocation" refuses `DeleteTenantPolicy` while "its chain
   holds events". So any namespace that ever deleted a volume (chain nonempty) keeps its policy, its allowance in
   `allocation.events` and one of the 4096 slots for the life of the store, even when Kubernetes deleted the namespace and every
   event was exported. With namespace churn (CI tenants) the 4096 policies or 524288 events run out; new namespaces then get no
   policy and every creation is refused, with no remedy but an etcd write. The churn test only shows "stopping at the allocation".
   Fix: let DeleteTenantPolicy succeed when the only event present is a trailing `audit.pruned` and there is no sweep, and keep the
   tail (numbering continues if the UID returns; a tail is about 100 bytes) or state that a recreated chain restarts at 1; or
   release the allowance on namespace deletion some other way. Add a test: delete, prune, delete the policy, the allocation falls.

2. **Medium: the #76 interface is recorded only in #73, and #76's text can leave the charge in place with no next pass.**
   Evidence: design "#76" says "DeleteVolume's next pass finds no header and releases the owner record". #76 design.md:80-95 and
   spec S8 delete only the header, "page away leftovers without a new tombstone", "automatic legacy rollback" by a maintenance
   writer, and mention neither #73 nor roles ("Create/delete remain administrator-only"). If #76's step returns success from
   the DeleteVolume call, or runs without DeleteVolume, no second pass comes: the CSI controller stops retrying a successful
   DeleteVolume, the owner record and the namespace's charge stay (a `pvc-<uid>` id never recurs), and TenantStatus only
   reports an orphan. That is the S4 leak the rule exists to prevent. Also "after the checks": it is unsaid whether #76's step
   replaces the mark (then no `delete.admitted`, and the later `header-missing` of an unmarked owner is "any other event",
   refused on a full chain) or follows it.
   Fix: say that DeleteVolume re-reads and continues its own loop after #76's step returns (so the release runs in the same
   call), that the step runs before the mark (no event) or after it (stated), and that #76's automatic path leaves only
   orphan reports; record the dependency in #76's plan (follow-up row), and test DeleteVolume with a stand-in rollback that
   deletes the header inside the call.

3. **Low: nothing tests the new log lines and the refusal table's bounds.**
   Evidence: contracts "Configuration writes" (one `slog.Info` line with method, caller, digest) and "Refusal log" (1024 callers,
   shared `others` entry) have no row in design "Tests" (refuse_test lists flood, burst and silence only). Fix: add a test with a
   captured handler for the config line, and one with 1025 callers.

4. **Low: a record missing after activation has no stated recovery, and ActivateTenancy is refused by it.**
   Evidence: design "Authorize, in order": absent record on an instance that has seen active is Internal for all role-gated
   calls, ActivateTenancy included; `serve` exits 1 only on one record present without the other. A store restored from an
   older etcd snapshot (both records missing) leaves running instances Internal until restarted, and the contracts "Repair"
   covers only a record that does not parse. Fix: say the runbook's repair covers a missing record (restart, then
   `tenant activate`), and that an instance that has seen active re-reads when both are absent.

5. **Low: post-approval text is labelled approved.**
   Evidence: contracts header "accepted with the plan, 2026-11-13" and "Everything here is Accepted"; design header "Approved
   2026-11-13". The allocation, the configuration-writes rule and the `security-admin` binding came after the approval
   (decisions "After approval"), and decisions says the contracts were "settled as written". Only the ADR proposal says the
   round-3 changes came later. Fix: add the same sentence to the contracts and design headers.

## Nits
- Spec S1 says "an administrator name alone grants nothing" for every method other than the open reads, Attach and node rules; the
  contracts allow Attach to any administrator name, including the override and reader identities. The Risks section says so; say it
  in S1's sentence too.
- Contracts "Space" says "about 1.3 GiB" and etcd's default quota is 2 GiB; the runbook's "3 GiB above current use" is the real
  bound, so state it next to the figure.
