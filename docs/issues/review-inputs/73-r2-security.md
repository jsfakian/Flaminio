# Issue #73 plan review — round 2 (incremental, design-security)

Scope: `git diff 094005f..ca5ee3a -- docs/issues/73/ ':!docs/issues/73/report.md'` in
`/tmp/flaminio-plans-2026-11-12/73` (HEAD 9180f47 adds only report.md), checked against the code of
that worktree.

## Earlier findings

- R1-B1: fixed (durable activation, records apart from the header, insecure refused; older binaries left as open Q4).
- R1-B2: fixed (a refusal writes nothing); the remaining shared-budget exposure is R2-S1.
- R1-B3: fixed (budget counts every event, reservation released by commit, etcd runbook); the cap's range is part of R2-S1.
- R1-B4: fixed (method table for all 23 methods; failover calls only ListNodes, `control/cmd/flaminio-csi/main.go:103–110`, confirmed).
- R1-B5: fixed.
- R1-B6: fixed.
- R1-B7: fixed (owner bound to `pvc-<PVC UID>`); residual input-order issue is R2-S5.
- R1-B8: fixed.
- R1-B9: fixed (server-side receipt and chain-hash checks); key handling is R2-S3.
- R1-B10: fixed.
- R1-B11: fixed.
- R1-B12: fixed.
- R1-B13: fixed (cut).
- R1-B14: fixed.
- R1-B15: fixed (cut).
- R1-B16: fixed.
- R1-B17: fixed.
- R1-B18: fixed.
- R1-B19: fixed.
- R1-B20: fixed.
- Nits 1–4: fixed (line references re-checked against the code; all cited lines match).

### R2-S1 — High: one namespace can spend the shared, never-pruned audit budget, locking every tenant and the administrator out

With no receiver in this step nothing is ever pruned (spec Q2), so the 16 × `cap` events are a
lifetime budget. Creations write no event, deletions write two, and PVC UIDs spread them over all 16
chains. One namespace churning PVCs within its quota (a CI namespace) spends about 65 000 deletions
at the default cap; then:
- every namespace's `DeleteVolume` is refused (`audit chain <pp> is full`), so charges are never
  released and creations stop at their limits too;
- "any other event: admitted when `present + reserved + 1 <= cap`" (contracts, Budget) also refuses
  `policy.put`, `policy.deleted`, `owner.assigned` and `confirm.forced` in that chain: the
  administrator cannot onboard a namespace whose UID hashes there, assign a legacy owner, or force a
  dead node's confirmation.
The only way out is restarting every instance with a larger `--audit-chain-events`, which the plan
never states, and whose range (up to 1 048 576, i.e. 32 GiB at 2 KiB a slot, contracts "serve")
exceeds what an etcd quota can hold, so that recovery can trip the space alarm that stops Attach and
ChangeIsr (design, Risks "etcd space"). Q2 acknowledges a bound on deletions, not that one tenant
spends it for all or that administrative writes share it.
Evidence: contracts "Budget" and `--audit-chain-events`; spec S9, Q2; design Risks.
Fix: reserve per-chain headroom for administrative events that `delete.admitted` cannot use; bound
each namespace's unpruned deletion events (e.g. a counter in `quota/<ns>/<class>`, admitted against
a policy field, lowered by the sweep) or state that cross-tenant exhaustion is part of Q2; state the
recovery and bound the cap so 2 × 16 × cap × 2 KiB fits etcd's maximum quota. Test: one namespace
churns until refused while another namespace deletes and `cluster-admin` puts a policy in the same
chain.

### R2-S2 — High: per-instance role files can disagree, and one disagreeing instance stops the CSI controller's Attach

Roles are read from each instance's file at start only (contracts "serve"); nothing in the store
records them, and `tenant activate` checks only that the caller holds `cluster-admin` on each listed
server (contracts "CLI"). After activation:
- An instance whose file lacks or mistypes the CSI controller's binding, or one left out of
  `--servers` and running without a file, answers PermissionDenied or FailedPrecondition. The CSI
  client moves to another instance only on Unavailable, DeadlineExceeded or Canceled and otherwise
  makes the answering instance its preferred one (`control/csi/meta.go:64–83`, `:76`, `:82`). Every
  later CreateVolume, DeleteVolume and ControllerPublish (Attach) then fails there, including the
  attach that moves a pod off a lost node.
- Revoking a binding needs every instance restarted; until then the caller picks an instance
  (`--meta` lists them all) where the old binding works.
- "Override and audit reading each bound alone" is checked per file only: one ID bound to
  `cluster-admin` on instance A and to `deletion-override` on instance B holds both.
No status shows another identity's roles or a file's digest, and no test covers two instances.
Evidence: contracts "serve" (line 57), "CLI" (lines 200–204), "Methods and roles" (lines 49–52);
spec S1, S11.
Fix: a durable digest of the bindings (in the activation record, changed only by an audited RPC);
an instance whose file digest differs answers Unavailable to role-gated calls so clients fail over,
and TenantStatus reports it; `tenant activate` requires equal digests and a `csi-controller` binding;
the runbook states revocation. Tests: two instances with different files (activation refused; after
activation the divergent one answers Unavailable and CSI Attach commits through the other); one ID
bound to different exclusive roles on two instances.

### R2-S3 — Medium: the receiver key is per-instance, unrecorded and unaudited, and a prune keeps no evidence of its receipt

`--audit-receiver-key FILE` is read by each instance (contracts "serve"); nothing in the store binds
it, instances may hold different keys, rotation is not described, and `audit.pruned` records only
`<first>-<last>` (contracts "Event", `reason`). So:
- Whoever can change one instance's arguments can install a key of their own and prune with
  self-signed receipts through that instance. The design's "an administrator [must not] forge a
  receipt" (Risks, Security) holds only if `cluster-admin` holders cannot change the metadata
  service's deployment; the plan does not state that boundary.
- After a prune the chain shows neither which key verified it nor the receipt, so neither the
  receiver nor a later audit can check that the deletion of history was authorized.
Evidence: contracts lines 62, 96, 154–155; design PruneAudit (135–141), Risks (200–204).
Fix: write the key's SHA-256 fingerprint durably (an audited `audit.receiver-set` event, or the
activation record) and refuse PruneAudit on an instance whose key differs; put the receipt and the
fingerprint in the `audit.pruned` event; state rotation (new event, old receipts no longer
accepted) and the trust boundary. Tests: an instance with another key refuses; the pruned event
verifies against the recorded key.

### R2-S4 — Low: on an insecure inactive instance the new methods pass with no identity

Authorize: "Insecure: while inactive pass, marked `inactiveRules`" comes before the role checks, and
a handler's own "needs a role and finds none" check is defined only "on an active store" (design,
Activation and roles). So `ActivateTenancy`, and `PutTenantPolicy` and the other new methods, reach
their handlers from an unauthenticated peer, contradicting the table (`cluster-admin`, or
"refused" before activation) and leaving the event's actor and role empty.
Fix: an insecure instance refuses every new method, or every role-gated handler refuses without a
role in its context whatever the activation state; add the case to `TestRoleTable`.

### R2-S5 — Low: reading the PVC before its namespace lets a replaced namespace become the owner

Contracts "CSI controller": GET the PVC, then the namespace. If the namespace is deleted (its
pending PVC with it) and a namespace of the same name is created between the two reads, the owner
is the new namespace's UID with the old PVC's UID, charged and protected under another tenant's
policy. Reading the namespace first, then the PVC by name and checking its UID, closes it: a PVC
that still exists after the namespace read lives in that namespace.
Fix: specify namespace-then-PVC (or refuse when the namespace's `creationTimestamp` is after the
PVC's); add the interleaving to the `httptest` cases.

### R2-S6 — Low: a tenant creation's retry adopts an existing volume of another owner

A failed claim with a header present "takes today's path" (design, CreateVolume), i.e.
AlreadyExists, and the CSI controller then accepts the volume if its geometry matches
(`control/csi/controller.go:239–247`); it cannot read owners (`GetVolumeOwner` is not its role).
Nothing stops `system-provisioner` creating a `pvc-<UUID>` id, and a creation committed before
activation whose reply was lost is retried after the flag: both bind the PVC to a system or
unassigned volume, uncharged and unprotected.
Fix: for a creation with an owner, answer AlreadyExists only when `owners/<id>` holds that owner,
else PermissionDenied; refuse a system creation whose id has the tenant form. Test both.

### R2-S7 — Low: a refused creation still pays for placement, on every retry

"Spec validation, the tombstone check and placement run once, as today. Then ... read the policy
and refuse" (design, CreateVolume). Placement Lists every chunk record (`meta/service/service.go:385`),
and the provisioner retries a refusal forever (design, facts). A tenant with many PVCs its policy
forbids turns each retry into a full chunk listing. The "refusal costs the store nothing" goal
(intent, What we want 3) holds for writes only.
Fix: read the policy and quota (one Get each) and refuse before placement; the claim still compares
their revisions.

### R2-S8 — Low: the global refusal-log limit lets one caller hide everyone else's refusals

"At most 10 lines per second per instance; past that refusals are counted" (contracts, Refusal
log). A node identity, or a retried deletion of a protected volume, keeps the limiter saturated, so
another caller's refusals (a compromised `csi-controller` trying protected deletions) appear only
as an anonymous count. Refusal lines are the only record of refused attempts in this step (S7).
Fix: limit per (caller, reason) with a bounded table, and name the suppressed callers in the
summary line; test that a flood from one identity does not suppress another's first refusal.

### R2-S9 — Low: the failover controller holds the `csi-controller` identity

Contracts: "the failover controller none". It mounts `flaminio-csi-controller-tls`
(`deploy/csi/driver.yaml:208,220`), so once active the failover pod can state any owner, delete
tenant volumes and attach: the party Q3 trusts is two Deployments.
Fix: say so in the method table and Risks, and record a follow-up for a failover certificate with no
role.

### R2-S10 — Low: an older binary answers Unimplemented, not PermissionDenied

Contracts "CLI": "an older binary answers PermissionDenied: its interceptor knows no such method".
gRPC answers Unimplemented for an unregistered method before any interceptor runs, and
`flaminio-meta` sets no unknown-service handler. "Any error refuses" keeps the CLI correct, but the
design's S11 test uses "a stand-in answering PermissionDenied", which does not model an older binary.
Fix: correct the sentence; the stand-in answers Unimplemented (keep a PermissionDenied case for an
unbound caller).

### R2-S11 — Low: a legacy volume whose header does not parse can never be deleted

DeleteVolume refuses it as unassigned (design, Deletion), OverrideDelete refuses unassigned
volumes, and AssignOwner refuses "one whose header does not parse" (contracts, Errors). Its id and
replicas stay forever.
Fix: let AssignOwner assign `system` to such a header (nothing to charge), or let OverrideDelete
delete an unassigned unparseable header; test it.

### R2-S12 — Low: ListAudit's start wraps at the top of `after_sequence`

"Point reads from `max(after, pruned) + 1`" (design, ListAudit) with `after_sequence` a u64: at
2^64−1 the start wraps to 0, reads a missing sequence and answers Internal ("an event missing below
the tail"), which looks like a broken chain.
Fix: `after >= next − 1` returns an empty page; add it to the S10 tables.

Reviewed commit: 9180f47bba154f88b28cee53822bf93c6d612427
