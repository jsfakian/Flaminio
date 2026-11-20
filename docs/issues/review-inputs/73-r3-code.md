# #73 round 3, design-code review (diff ca5ee3a..7a91186)

## Round 2 status
R2-C1 fixed in #73 (DeleteVolume deletes a still-creating header; test row added) but see F2 (#76 does not do it yet).
R2-C2 fixed (header-missing takes the slot; test). R2-C3 fixed (orphan, FailedPrecondition at once; test row).
R2-C4 fixed (Unimplemented, stand-in). R2-C5 mostly fixed (typed APIError, escaping, 404/500 tests); F5 is a leftover.
R2-C6 stated in S11 (accepted wording). R2-C7 fixed (insecure refuses new methods; in TestRoleTable).
R2-C8 fixed (rows added). R2-CN1-3 fixed.

Checked and fine: Authorize reads the record only for calls no node/agent/target rule allows, so ChangeIsr,
heartbeats and agent Attach stay read-free (auth.go:63-84 shape is preserved); the data plane's steady state
is untouched. Per-call cost is one etcd Get for CSI/admin control calls. Transaction sizes: mark about 8 ops,
finishDeletion about 10, prune 3 compares + 3 writes, sweep batch 119 + 1, activation 2 compares + 3 writes; all under 120.
Bootstrap is coherent: absent record means the administrator-name rule for ActivateTenancy, and the record must
bind a security-admin.

## Findings

1. **High: a full `sys-00` chain blocks revocation and receiver-key setting, and nothing can free it.**
   Evidence: contracts "Chain of an event" puts `authorization.put` (and `tenancy.activated`) in `sys-00`;
   "Budget" admits it only as "any other event" (`present + reserved + 1 <= cap`, cap 8192), refusing with
   ResourceExhausted. `audit.pruned` is always admitted but needs a receiver key (design PruneAudit), and the
   key is set only by PutAuthorization or the first ActivateTenancy. `sys-00` also takes 1/16 of every
   system event (policy.put, owner.assigned, system-volume deletes, confirm.forced). With no receiver, each sys chain
   fills for life (contracts "Without a receiver key"). Then the `security-admin` cannot remove a compromised
   binding or install the key that would let pruning start. Today's admin-name revocation is a restart away;
   this is a revocation that audit capacity can deny, against S9 "an exporter or receiver outage changes no authorization"
   and the stated invariant. Also `policy.put` in a full sys chain stops the administrator from raising a namespace's
   `max_audit_events`, which is the one remedy S9 offers for a full ns chain (cap raise is the policy write; if
   its sys chain is full it cannot be written).
   Fix: give `authorization.put` (and `policy.put`) always-admitted status with a bounded overdraft, or a reserved
   headroom above cap (e.g. 64 slots) usable only by them; add a test: fill `sys-00` with no receiver key, then
   PutAuthorization (revoke, add key) and PutTenantPolicy commit, and the key then lets a prune free space.

2. **Medium: #76's rollback does not do what #73 requires, and the conflict is resolved only by assertion.**
   Evidence: design "#76" says the rollback "must then do what `finishDeletion` does here". #76 design.md:80-95 and
   spec S7-S8: the unpublished-header rollback is one delete-only Txn that "deletes only that header",
   "apply all existing authorization/protection preconditions", and must work under NOSPACE. #73's version also
   deletes the owner record, lowers the quota, writes `delete.committed` and, for the mark, `delete.admitted`; those are
   writes that grow the store and need audit budget, so they can neither be delete-only nor run under etcd NOSPACE.
   A header cleaned by #76 alone leaves a charge and owner record for ever (S4 violated); a header cleaned by #73's
   path under a full chain is refused although it was never attachable. #76's "protection preconditions" would also
   refuse the very case R2-C1 exempts. Neither plan's text meets the other.
   Fix: decide in this plan the order and interface: either `finishDeletion` for an unpublished header is exempt
   from the audit budget (always admitted, no `delete.admitted` reserve) and #76 calls it (record the cross-plan
   dependency in both plans), or the owner record and charge are released by a separate repair that #76's rollback
   cannot skip. Add a test of the interaction (rollback with an owner record present).

3. **Medium: the approval left the plan contradicting itself.** decisions.md "Settled at approval" settles Q1, Q2,
   Q3 (most), Q4 and the contracts, but spec "Open questions" still says "Fail-closed behaviour is proposed; no
   availability trade-off is accepted", "need administrator agreement", "are Proposed"; design Risks says "Q1-Q4 stay
   open; no risk is accepted"; contracts is headed "draft" and "Everything here is Proposed"; the ADR proposal
   header says Accepted but its section is still "Decision proposed for review"; design ADRs "Approving this design accepts it".
   Also decisions says the Q3 evidence for legacy owners stays open while spec S11 and design say `cluster-admin`
   assigns, and "Q2 ... pruning by hand: `cluster-admin`" is settled while the spec still lists it open.
   Fix: update spec Open questions, design Risks/Availability and the contracts header to the settled status
   (keep Q3 evidence and Q4 explicitly open); one sentence each.

4. **Low: the authorization read and the mutation are not tied.** Authorize reads the record once; the handler's
   transactions compare it only in ActivateTenancy (absent), PutAuthorization (revision) and PruneAudit. A binding
   revoked after Authorize can still finish one DeleteVolume/OverrideDelete/AssignOwner/PutTenantPolicy. Spec S1 says
   "a removed binding is refused at once". Fix: say "at the next call" (the design says that) in S1 and contracts,
   or add the record's revision compare to the transactions of the override and the audited admin mutations (one more compare;
   op counts allow it) and test with `interleaveKV` revoking between read and commit.

5. **Low: `APIError` must keep today's error text.** Design says `do` returns `*APIError`; today's message
   (`kube.go:81-83`, `%s %s: %s: %s`) reaches logs and the failover controller's callers (`StorageNodes`, `SetOutOfService`)
   unchanged only if `APIError.Error()` reproduces it, and callers wrap with `%w`. The design does not say so.
   Fix: state `APIError.Error()` is the current text and callers that wrap use `%w`; add to the 404/500 tests an
   `errors.As` through a wrapper and a check that the old text is unchanged.

6. **Low: a corrupt or newer-schema authorization record locks out every role-needing call, including its own repair.**
   Contracts "Records": an unparseable record fails the method with Internal; Authorize reads it for every
   such call, PutAuthorization included, and no path writes around it. etcd tampering is the only way in, but the
   plan states it as an integrity concern and the recovery is undefined. Fix: state the recovery (etcd edit by the
   operator, audited by nothing) in the runbook, or have PutAuthorization read the record raw and accept a replacement
   under `expected_revision` when it does not parse; test it.

7. **Low: tests missing for the new RPCs.** No row covers: two instances racing ActivateTenancy (one AlreadyExists, one
   event); PutAuthorization with a stale revision, removing the caller's own `security-admin`, or concurrent with a
   prune (the prune's record compare retrying); the `tenant authorization-put` CLI path (main_test lists only start and
   activate); TenantStatus counts and id lists (S11 mentions only orphans). Each rule needs a test that can fail.

8. **Low: ns chain cap with no policy.** Contracts "Budget": `cap` is "0 without a policy". A tenant deletion
   whose policy was removed (DeleteTenantPolicy is refused while quota exists, but an AssignOwner/owner record can outlive
   a zero quota record only if bytes are 0, and a `header-missing` or crash-time state is possible) would be refused
   as "chain full" with no hint. State the refusal word (`audit-full` vs `no-policy`) for a tenant deletion whose policy
   is absent, or prove the state unreachable and say so.

## Nits
- Design "Authorize, in order" mixes the insecure and secure paths in one paragraph; a two-row table (inactive, active)
  would show that an insecure active instance also refuses ChangeIsr, heartbeats and RegisterNode (S11 says "every call
  but the open reads"), which the runbook should name.
- Contracts "CLI": `tenant authorization-put` has no `--servers` or revision-read helper; say whether `--expect-revision`
  comes from `tenant status`.
- Decisions "Approvals" says "Approved before its plan review cleared"; add the round in which it clears when it does.
