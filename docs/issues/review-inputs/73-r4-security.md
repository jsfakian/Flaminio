# Issue #73 plan review — round 4 (incremental, design-security)

Scope: `git diff 7a91186..98c1242 -- docs/issues/73/ ':!docs/issues/73/report.md'` in
`/tmp/flaminio-plans-2026-11-12/73` (the round-3 fix, made after approval). Checked against
the code in that worktree and, where the plan relies on it, #76's approved design.

## Earlier findings

- R3-S1: fixed (configuration writes leave the chains, so nothing can lock them out). The fix
  drops configuration changes from the audit (R4-S1).
- R3-S2: fixed (allocation, 4096 policies, floor at the chain's use). The allocation can never be
  freed (R4-S2), and the space bound leaves out pending sweeps (R4-S7).
- R3-S3: fixed (Attach reads no record). It overshoots: any administrator name can attach (R4-S3).
- R3-S4: fixed (the activating caller is bound as `security-admin`; no write drops the writer's
  own binding; a handover takes two writes).
- R3-S5: fixed (two records; Internal). `serve` exiting 1 widens the outage (R4-S4).
- R3-S6, R3-S7, R3-S8, R3-S9, R3-S10, R3-S11, R3-S12, R3-S13: fixed.
- R3-SN1: fixed. The contracts header now overclaims instead (Nit 3).
- R3-C1: fixed (same cause as R3-S1). The allocation can now refuse a raised allowance (R4-S2).
- R3-C2: partly fixed. The interface is stated, but the order and the return are not (R4-S5).
- R3-C3, R3-C4, R3-C5, R3-C6, R3-C7, R3-C8: fixed.
- R3-CN1: fixed in the contracts. The design's Authorize order contradicts it for Attach (R4-S6).
- R3-CN2: fixed. R3-CN3: deferred to clearance, as intended.

### R4-S1 — High: role, receiver-key and policy changes no longer leave a durable, attributable record

The changed design keeps, for each configuration write, only `changed_by` (the last writer) and
one `slog.Info` line holding the new record's digest (contracts:112–113, 118–121; spec S7;
design:232–233).
- A compromised `security-admin` can bind an identity to `deletion-override`, use it, then unbind
  it. Afterwards the record shows only that `security-admin` wrote it last, and the logs show two
  digests that cannot be reversed. The same holds for a receiver key that is swapped in and then
  swapped back.
- A compromised `cluster-admin` can clear `required_protection` (or allow RF=1 or unencrypted),
  let the CSI controller create volumes, then restore the policy. Those volumes stay unprotected
  for life (protection is fixed when the owner record is written), and nothing durable shows the
  flip.
- `DeleteTenantPolicy` deletes the record together with its `changed_by`, so a destructive
  operation leaves no durable trace at all.

This contradicts:
- the approved intent, which audits "policy, owner and pruning changes" (intent.md:33–34) and was
  not amended;
- the issue's own premise that diagnostic log lines are not an audit trail;
- the Done-when condition "destructive and key operations have durable, attributable audit
  records".

The choice is a round-3 decision (decisions.md:125) made after the approval. No test covers the
grant-use-revoke or the flip-flop path.

Fix:
- Either restore events for configuration writes in chains no other role writes, each with a
  stated budget and a statement of what a full one blocks. For example, a `sec` chain for the
  authorization record, written only by `security-admin`, and a `cfg` chain for policies, written
  only by `cluster-admin`, which a prune frees.
- Or narrow the intent and spec, with the approver, to exclude configuration changes, and say so
  against Done when.
- Either way, log the full new record (it holds no secret), not only its digest.
- Tests: grant, use and revoke; a protection flip-flop; a policy deletion. Each leaves a durable
  trace.

### R4-S2 — High: every namespace that ever deleted a volume keeps its policy and allocation for life, receiver or not

`audit.pruned` is appended to the chain it prunes (contracts:137). A prune covers at most
`next − 1` (design:159–160) and then appends one event, so after any prune `present ≥ 1`.
`DeleteTenantPolicy` is refused while the chain "holds events" (contracts:169–170). An `ns-`
chain that has ever received an event therefore never empties. Its policy (one of 4096) and at
least 2 events of the allocation (the floor is `present + reserved`, and the minimum allowance
is 2) are held forever, even with a receiver key.

Contracts:174–177 imply that pruning frees them; it does not.

Consequences:
- Under namespace churn (CI or per-PR namespaces, each deleting a PVC), PutTenantPolicy
  eventually answers `allocation` for every new namespace. Tenant creations in new namespaces
  are then refused for the life of the store. Only etcd surgery recovers.
- Once the allocation is used, S9's remedy for a full chain ("raises its allowance, within the
  allocation") is gone.
- The invariant "no configuration write is refused for audit space" (spec:41–42, 133) is false
  for PutTenantPolicy (`allocation`, `below-use`).
- The test "new-UID churn stopping at the allocation" (design:196) encodes the lock as expected
  behaviour.

Fix:
- Let a fully exported chain be retired. Either write an `ns-` chain's `audit.pruned` to its
  namespace's `sys-` chain, so a pruned `ns-` chain can be empty; or let DeleteTenantPolicy
  retire a chain whose only present event is its own last `audit.pruned`, recording the final
  sequence and hash in a `sys-` event. In both cases never reuse a chain's sequence numbers.
- Reword the invariant to except allocation refusals.
- Test: deletions, a prune, then DeleteTenantPolicy succeeds and frees its slot; 4097 namespaces
  over time each get a policy.

### R4-S3 — High: once active, any administrator name can Attach, including the audit reader, the override and revoked identities

Contracts:33, spec S1 and design:42 keep Attach on "administrator name". Attach is a write: it
increments the epoch and sets the target (`meta/domain/domain.go:217–220`,
`meta/service/service.go:723–725`), which fences the running target.

So all of these can move any tenant volume's target to any node, or bounce it repeatedly to deny
service to every volume:
- the `audit-reader` certificate, which an exporter outside the core will hold;
- the `deletion-override` certificate;
- an administrator whose bindings were all removed.

With a node it controls, such a caller can also steer a target onto that node.

There is no CRL (`pkg/pki` has none), so the binding is the only revocation, and it no longer
covers Attach. This contradicts:
- S1's opening "an administrator name alone grants nothing" (spec:49);
- design Risks, "the audit reader, any write" (design:229);
- intent "What we want" 1;
- the approved Attach row (`csi-controller`, `system-provisioner`, `cluster-admin`).

TestRoleTable will encode the widened rule. No abuse test exists.

Fix:
- Restore the approved Attach roles.
- Meet R3-S3 by refusing an authorization record with no `csi-controller` binding, the one way
  the service itself could lose it. A record that does not parse comes only from an etcd-level
  write and is repaired there.
- Tests: Attach by `audit-reader`, by `deletion-override` and by a revoked identity is refused;
  a PutAuthorization without a `csi-controller` binding is refused.

### R4-S4 — Medium: `serve` exits 1 on incomplete activation records, so a restart takes failover down

When one activation record is missing, `serve` exits 1 (contracts:68–71, 90–92; design:55). A
running instance already answers Internal for role-gated calls in that state (design:46–47), so
exiting adds nothing to safety. It only means that after any restart, no instance serves
ChangeIsr, heartbeats, Attach or the open reads.

No service caller can trigger this: no path deletes either record. But a partial restore, or a
botched etcd-level repair (the runbook's own remedy, contracts:84–86), then blocks failover
cluster-wide. That contradicts design:219–221 ("a bad record … cannot block failover") and
decisions.md:132.

Fix:
- Start anyway, log the incomplete state, and serve the node, target, Attach and open-read calls;
  role-gated calls answer Internal, as they do at runtime.
- Test: incomplete records at start; ChangeIsr and NodeHeartbeat commit and role-gated calls
  answer Internal.

### R4-S5 — Medium: the #76 interface leaves the order and the return unpinned, so a tenant charge and slot can leak

Design:123–127 requires only that the rollback runs "after the checks above" and leaves the owner
record and quota alone. Two things are left open.

(a) Before or after #73's mark?
- If before, a tenant volume's header is deleted with no event in or before its transaction,
  against S7.
- The later `header-missing` then has no `deleting_chain`, so it falls to a `sys-` chain ("Any
  other", contracts:136–140), against "No tenant's action writes a `sys-` chain".

(b) Does DeleteVolume re-enter its loop?
- #76's `stepUnpublishedRollback` returns after its delete-only transaction (#76 design:80–87).
- The CSI controller treats success and NotFound as done (`control/csi/controller.go:265`).
- If the call that ran the rollback answers before the release, the "next DeleteVolume" of S5
  (spec:95) never comes. The owner record, the charge and, after a mark, a reserved slot stay
  forever, which also pins the policy (R4-S2).

The test row (design:197) checks only a separate later call.

Fix:
- Require the mark first, so `delete.admitted` is durable and the slot is reserved.
- The rollback replaces finishDeletion's transaction. The same call then loops and releases the
  owner record with `delete.committed` in `deleting_chain`, and answers success only after that;
  otherwise it returns an error so the provisioner retries.
- State the S7 exception for a rolled-back header, and that the mark is a write, so under
  NOSPACE this path waits for space. Record both in #76.
- Tests: the release in the same call; a release that fails, then the retry.

### R4-S6 — Low: Attach passes before the insecure check

Design:42 passes "the open reads and Attach … by today's rules" before the insecure rule. Today's
rule on an insecure instance passes everything (`meta/service/auth.go:42–44`). An insecure
instance still running across activation would therefore accept unauthenticated Attach. The
contracts say such an instance refuses every call but the open reads (contracts:58–59), and their
Attach row's "no store read" cannot hold there, because refusing once active needs `active`.

Fix:
- Apply the insecure rule to Attach first, and say that it reads `active`, which is cached.
- Add a TestRoleTable row: an insecure instance across activation refuses Attach.

### R4-S7 — Low: the space bound leaves out a pending sweep's events

`present = next − 1 − pruned` (contracts:150). Events `from..through` still exist until their
sweep ends, and a sweep that crashed stays until the next PruneAudit of that chain
(design:158–165). A chain can therefore physically hold up to twice its cap. "At most 16 × 8192 +
524288 events" (contracts:171–173) and "cannot be reached through the audit records"
(design:223–227) do not hold. The 3 GiB margin also leaves out etcd's per-key overhead.

Fix:
- While a sweep is pending, count from `sweep.from − 1` instead of `pruned`; the bound then holds
  exactly.
- Size the runbook's margin with etcd's per-key overhead, or soften the claim.

## Nits

1. `changed_by.role` is undefined for ActivateTenancy, which no role guards. The removed event
   text said `admin-name` (contracts:112–113).
2. "Each overwrites or deletes one record … logs … the new record's digest" (contracts:118–121)
   is inaccurate. ActivateTenancy and PutTenantPolicy write two records, DeleteTenantPolicy
   deletes two and rewrites `allocation`, and a deletion has no new record.
3. "Everything here is Accepted with the plan, 2026-11-13" (contracts:1–5) also covers round-3
   content written after the approval, which decisions.md:16–17 marks as such.

Reviewed commit: e9f0ba75deb22f83729d9535d0b269287c7f831a
