# Issue #73: Tenant policy and destructive-operation audit — decisions

Choices the plan reviews settled, each with the finding that raised it. Reviewers treat these
as given; none is an approval.

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status:
needs_incremental_review).

- **After approval:** the round-3 review fixes (commit `Address the review of the design of #73,
  round 3`) came after the approval; the choices they settle are marked "round 3" below.
- **After approval:** the round-4 review fixes (commit `Address the review of the design of #73,
  round 4`) came after the approval; the choices they settle are marked "round 4" below.
- **After approval:** the round-5 review fixes (commit `Address the review of the design of #73,
  round 5`) came after the approval; the choices they settle are marked "round 5" below.
- **After approval:** the round-6 review fixes (commit `Address the review of the design of #73,
  round 6`) came after the approval; the choices they settle are marked "round 6" below.
- **After approval:** the round-7 review fixes (commit `Address the review of the design of #73,
  round 7`) came after the approval, and after the settle commit; the choices they settle are
  marked "round 7" below, including a restated Q3 answer.
- **After approval:** the round-8 review fixes (commit `Address the review of the design of #73,
  round 8`) came after the approval; the choices they settle are marked "round 8" below, including
  a narrowed Q3 answer.
- **After approval:** the round-9 review fixes (commit `Address the review of the design of #73,
  round 9`) came after the approval; the choices they settle are marked "round 9" below.
- **After approval:** the round-10 review fixes (commit `Address the review of the design of
  #73, round 10`) came after the approval; the choices they settle are marked "round 10" below.
- **After approval:** the round-11 review fixes (commit `Address the review of the design of
  #73, round 11`) came after the approval; the choices they settle are marked "round 11" below.
- **After approval:** the round-12 review fix (commit `Address the review of the design of #73,
  round 12`) came after the approval; the choice it settles is marked "round 12" below.

## Settled at approval

- **Q1, fail-closed dependencies** (spec, Open questions): settled as proposed: a creation
  without a policy, and a deletion without audit room, are refused.
- **Q2, budget sizes** (spec, Open questions; contracts, Budget): settled: 8192 events per
  system chain, and each policy's `max_audit_events`, 2–65536.
- **Q2, the receiver's signature as the only gate to pruning, with no local time minimum**
  (spec, Open questions; contracts, Budget and Protobuf): settled as proposed.
- **Q2, pruning by hand** (spec, Open questions; design, Audit chains): settled: `cluster-admin`
  prunes with `audit prune`. Changed after approval in round 6 (R6-S1, R7-S1): `security-admin`
  also prunes a full `cfg`; accepted at the user's instruction (see Settled at the user's
  instruction of 2026-11-13, the self-prune residual).
- **Q3, who assigns legacy owners** (spec, Open questions; design, Deletion, protection and
  override): settled: `cluster-admin`, through `AssignOwner`.
- **Q3, from what evidence legacy owners are assigned** (spec, Open questions): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction), restated in rounds 7 to 10:
  see Settled at the user's instruction of 2026-11-13.
- **Q3, separately credentialed override and security roles** (spec, Open questions; contracts,
  Methods and roles; design, Risks): settled as proposed: `deletion-override`, `audit-reader`
  and `security-admin` are each bound alone, and certificate issuance (#16) bounds that
  separation.
- **Q3, the CSI controller as the one party trusted to state owners** (spec, Open questions;
  design, Risks): settled as proposed.
- **Q3, the first activation by any administrator name** (spec, Open questions; design, Risks):
  settled as proposed.
- **Q4, binaries older than this step after activation** (spec, Open questions; design, Risks):
  settled with the plan's answer: unsupported, and `TenantStatus` reports what they leave.
- **The ADR proposal** ([adr-proposal.md](adr-proposal.md), Status): accepted with the plan.
- **The contracts, labelled Proposed** ([contracts.md](contracts.md), header): settled as
  written.

## Settled at the user's instruction of 2026-11-13

- **Q3, from what evidence an administrator assigns a legacy owner** (spec, Open questions; design,
  Risks): for a legacy `pvc-` volume, the owner is the PVC whose UID the volume id carries: find the
  PVC by that UID, read its namespace's UID, read the PVC again by namespace and name, and require
  the id's UID. With that PVC gone, the owner is a dedicated namespace created to hold legacy
  volumes, never a tenant's namespace and never one taken from a PV's `claimRef`, and the PVC UID
  the id carries. Its policy has a `default` limit of 0 volumes, so every creation there is refused
  while an assignment is charged past the limit, and a `max_audit_events` of at least twice the
  volumes it will hold; no one is granted PVC rights there, and its UID is in the runbook's operator
  log. It costs its chain two events per volume, which stay allocated while no receiver prunes them.
  DeleteVolume or the override then deletes it. `system` stays for ids not of the tenant form and
  headers that do not parse. Recommended by Claude, no recommendation in the plan: it reuses the
  verification the plan already trusts for owners, adds no mechanism, and never guesses an owner
  from a name. **Changed after approval, round 7 (R7-C1):** the first answer sent a `pvc-` volume
  with no PVC to `system` or the override, which the contracts refuse for a parsed tenant-form id
  and for an unassigned volume, so a Retain PV whose PVC was deleted had no path. The restated
  answer uses only paths the contracts admit (a tenant assignment, then DeleteVolume or the
  override) and narrows no intent or spec rule. **Changed after approval, round 8 (R8-B1):** the
  round-7 step taking the namespace from the PV's `claimRef` is removed. A `claimRef` holds a
  namespace's name, not its UID, so a namespace re-created under that name would receive a volume it
  never owned. With the PVC gone, the owner is now a namespace the administrator designates, with
  its own policy; nothing is inferred from a name. The order is a runbook step the service cannot
  check. **Changed after approval, round 9 (R9-B2, R9-N1):** the designated namespace is a dedicated
  one, created to hold legacy volumes, with its own UID, policy and `default` limit; never a
  tenant's namespace and never one taken from a `claimRef`. That is enough because Q3 deletes the
  volume next (round 10: what it costs, below). Step 1 finds the PVC by its UID, reads its
  namespace's UID, then reads the PVC again by namespace and name and requires the id's UID.
- **The Attach least-privilege cost** (contracts, "The cost"; spec S1; design, Risks; ADR
  proposal, Consequences): once active, manual and system-volume attaches need the CSI
  controller's certificate. Accepted under this instruction, a change after approval, as the
  plan's current text states it.
- **The self-prune residual** (contracts, "Self-prune"; design, Risks; ADR proposal,
  Consequences): a `security-admin` holding the receiver key's private half can prune a full
  `cfg` alone. Accepted under this instruction, a change after approval, as the plan's current
  text states it.
- **Q4, an older binary after activation** (spec, Open questions; ADR proposal, Consequences):
  unsupported, and what it leaves is reported; the residual risk is accepted. The plan's
  recommendation.

## Choices

- **This step is the metadata service's authority** (R1-B13, R1-B14, R1-B15): the Kubernetes RBAC
  split, the admission webhook and policy resource, and the key, snapshot and backup hooks of
  #50–#53 stay open (intent, "This step"); each later issue owns its hook and test, recorded in
  `docs/followups.md`.
- **Durable one-way activation** (R1-B1): writing the authorization and allocation records
  activates the store; nothing deletes either, and one missing afterwards fails role-gated calls
  closed (round 3, R3-S5), while Attach, ChangeIsr, heartbeats and the open reads keep serving
  and `serve` starts; the repair is an etcd-level restore, or `tenant activate` when both are
  gone (round 4, R4-S4, R4-C4). Insecure instances refuse to start on an active store, and
  refuse the new methods always (R2-C7, R2-S4).
- **One authorization record in the store** (R2-S2, R2-S3): the role bindings and the receiver
  key live in etcd, read by every instance on each call that needs them; no instance has roles
  or a key of its own, so instances cannot disagree, and a revocation applies from the caller's
  next call (round 3, R3-C4, R3-S6). A separate `security-admin`, bound alone, writes it;
  `cluster-admin` cannot. No per-instance file, digest or consistency check.
- **No instance version record** (R1-B1): an older binary writes none, so its absence proves
  nothing. `tenant activate` probes the servers the administrator lists; an older binary answers
  Unimplemented (R2-C4, R2-S10); after activation it is unsupported (spec Q4).
- **Owner, policy and quota records apart from the header** (R1-B1): an older writer's header
  rewrite cannot strip them; a volume with no owner record is unassigned and fails closed.
- **Six roles and one method table** (R1-B4, R1-B6): in `contracts.md`, for every existing and
  new method. The CSI controller is `csi-controller`; drain is `cluster-admin`.
- **Where R1-B4 does not apply:** the failover controller calls no Attach. Its only metadata call
  is `ListNodes` (`control/cmd/flaminio-csi/main.go:103–110`); Attach comes from the CSI
  controller's publish (`control/csi/controller.go:281`) and agents' own restarts.
- **The failover pod holds `csi-controller`** (R2-S9): it mounts the CSI controller's
  certificate; a certificate of its own, bound to no role, is a follow-up.
- **Refusals are logged, never stored** (R1-B2), at most 10 lines a second per caller, with
  suppressed counts named per caller (R2-S8); durable refusal records stay open.
- **Per-namespace chains** (R2-S1): a namespace's tenant deletions write only to its own chain,
  whose budget its policy's `max_audit_events` sets (2–65536); everything else writes to 16
  system chains of a fixed 8192 events. One tenant fills only its own chain; no tenant writes a
  system chain, so administrative events cannot be blocked by tenants. No budget flag.
- **The budget counts every event present at one slot of 2 KiB** (R1-B3), exported or not, with
  one slot reserved per admitted deletion and released only by its commit, `header-missing`
  included (R2-C2). With no receiver nothing is pruned, and the budgets bound the store's life;
  etcd quota, compaction and defragmentation are runbook requirements of activation.
- **Pruning needs a receiver's signature over the chain hash** (R1-B9), by the key in the
  authorization record; the prune's event keeps the receipt and the key's fingerprint (R2-S3).
  No separate acknowledgement, cursor or exporter role: readers only read.
- **Device class is #72's pool name, `default` in this step** (R1-B8): RF=1 and unencrypted
  permissions are per namespace and checked on the request; no StorageClass UID is used.
- **The owner is bound to the volume id `pvc-<PVC UID>`** (R1-B7, R1-B16): no intents, operation
  records or spec hash. The CSI controller reads the namespace, then the PVC, and verifies the
  PVC UID against the name (R2-S5); names are checked and escaped, and a non-2xx answer is a
  typed error (R2-C5).
- **A retried creation is checked against the owner record** (R2-S6, R2-C3): another owner is
  PermissionDenied; an owner record with no header is FailedPrecondition at once; a system
  creation cannot take a tenant id.
- **Policy and quota are read before placement** (R2-S7), and compared again in the claim.
- **Records end with their subject** (R1-B12): owner records with the header; quota records at
  zero usage; a policy only by `DeleteTenantPolicy`, refused while its namespace has usage.
- **Quota is released in the header's deletion** (R1-B11), not at the last confirmation, so a
  dead node holds no charge. A creator finishing a marked deletion is the commit event's actor.
- **Protection is read from the owner record only** (R1-B10): a header that does not parse counts
  as published and is refused alike.
- **Protection guards published volumes** (R2-C1): a header still being created was never
  attachable, so DeleteVolume deletes it whatever its protection or assignment. "Being created"
  is #76's unpublished test, not the flag alone (round 3, R3-S11).
- **Protection is fixed when the owner record is written** (R1-B6): no protection update RPC.
- **The override performs the deletion itself** (R1-B18); the service draws its operation id
  (R1-B16).
- **One policy writer, `cluster-admin`** (R1-B19): no policy controller in this step.
- **Legacy volumes are unassigned** (R1-B5): they attach as before; deletion needs an
  assignment, which charges past the limit; a header that does not parse can be assigned
  `system` (R2-S11), a parsed tenant-form id cannot (round 3, R3-S9). The override uses the owner
  record's revision, not `first_epoch`.
- **The activation window is stated, not closed** (R2-C6): a creation, move, drain or attach
  authorized just before activation may commit just after it; audited writes are refused.
- **Point reads of 20-digit sequence keys** (R1-B17): `kv.KV` is unchanged. An `after_sequence`
  at or past the tail is an empty page (R2-S12).
- **The hand-written Kubernetes client** (R1-B14): `failover.APIClient`, tested with
  `httptest`; no client-go. `--tenant-owners` is off by default and no manifest changes.
- **MoveReplica and DrainNode are not audited** (R1-B4): a move drops its source only once the
  destination is in the ISR (`docs/replication-protocol.md` §7, Moves).
- **No Rust change** (Nit 3): `flaminio-metaclient` uses none of the changed messages.
- **Policies need `allow_unencrypted` until #50** (Nit 4): stated in the upgrade order.
- **No tenant QoS ceilings in this step** (cross-plan, #75).
- **The runbook carries round 7's duties** (round 8, R8-C1): the evidence order for a legacy
  owner, the key duty and the lost-key repair are in the usage comment's plan; the S11 test
  checks only that AssignOwner admits a tenant owner for a `pvc-` id whose PVC is gone.
- **The overdraft test reads in order** (round 8, R8-N1).
- **The lost-key repair is conditional and attributed by the runbook** (round 9, R9-B1): the
  operator checks the precondition, then writes the key with an `etcdctl txn` comparing the record's
  mod revision; `changed_by` keeps the last service write and does not credit the new key; the
  operator writes who repaired and when in the runbook's operator log; an `audit_test.go` case on
  `backends` covers it.
- **The lost-key repair holds its precondition** (round 10, R10-B1): it first finishes any pending
  `cfg` sweep, requires `next − 1 − pruned` = `cap + 1024` and no sweep, and its `etcdctl txn` also
  compares `audit/tail/cfg` and the sweep's absence, so the precondition still holds when it commits
  (round 11: the precondition, not the txn, refuses a `cfg` with room); a PutAuthorization that
  changes nothing still commits, which the record compare catches.
- **The legacy-holding namespace refuses creations** (round 10, R10-B2): its `default` limit is 0
  volumes, its `max_audit_events` at least twice the volumes it will hold, no one has PVC rights
  there, and its UID is in the operator log. It costs its chain two events per volume, allocated
  while no receiver prunes them.
- **Attribution wording** (round 10, R10-N1, R10-N2): `changed_by` is the service's last write of
  the record; the operator writes the repair's who, when and fingerprint in the runbook's
  operator log.
- **The lost-key repair reads once and compares what it read** (round 11, R11-B1): `etcdctl get
  -w json` of the tail, the sweep key and the record, the later reads at the first one's header
  revision; the precondition on that tail, the new record from that record, the txn on those
  `mod_revision`s; `ChainStatus` a cross-check only.
- **Each of the repair's three compares has its own test** (round 11, R11-B2): a prune ending
  before the txn, a no-op PutAuthorization after the reads, reads taken during a sweep.
- **A no-op PutAuthorization leaves `cfg`'s tail unwritten, and each repair case checks the
  other compares** (round 12, R12-B1): with no events `appendEvents` returns only the tail compare;
  each of the three cases also asserts that the other two compares still hold.
- **Wording** (round 11, R11-N1, R11-N2): the sweep-finishing prune takes any 32-byte hash and
  64-byte receipt; the operator log also holds the new key's fingerprint; the holding-namespace
  clause points to spec Q3.
- **Wording** (round 9, R9-N2, R9-N3): a key removal "does not use the overdraft"; the Q3 marker
  here says rounds 7 to 9.
- **A `security-admin` prune is a replay before it is `not-full`** (round 7, R7-C2): a retry
  after a lost reply or a crash mid-sweep answers `pruned`, for either role.
- **Q2 and the ADR's Decision say who else prunes** (round 7, R7-S1): `security-admin` prunes a
  full `cfg` with a key it holds, as the Consequences already stated.
- **Wording** (round 7, R7-N1–N4): the inactive-creation sentence is back under "Without an
  owner"; `bytes` is a tenant's size at its claim or assignment plus growth, 0 for `system`;
  the refused prune reads only Authorize's two records; this file's references point to
  "Settled at the user's instruction of 2026-11-13".
- **Configuration writes are audited in `cfg`** (round 4, R4-S1, replacing round 3's choice for
  R3-C1, R3-S1): one event per binding changed, per receiver-key change and per policy write or
  deletion, in a chain no tenant writes; each record keeps `changed_by`, and each write logs the
  full record. A recovery write (one that grants nothing) has a 1024-event overdraft no other write
  uses, so the security administrator can set the key that lets `cfg` be pruned and revoke a binding
  (the sole `cluster-admin` after binding a replacement, round 6) while the overdraft has room; it
  must keep the private half of the last key it sets there (round 7, R7-B1); a key removal is not a
  recovery write (round 6, R6-C3); a full `cfg` refuses grants, policy writes and policy deletions.
  Policies hold at most 4 classes, so a policy event fits its 2048 bytes.
- **A fixed store-wide allocation** (round 3, R3-S2): the namespaces' allowances sum to at most
  524288 events and there are at most 4096 policies; an allowance cannot fall below its chain's
  use, and a policy whose chain holds events cannot be deleted. Constants, not configuration.
- **A pruned namespace chain is retired with its policy** (round 4, R4-S2, R4-C1): an `ns-`
  chain's `audit.pruned` goes to its namespace's `sys-` chain, so a fully pruned chain is empty
  and its policy, allowance and slot can be freed; chain names carry a counter, so no name or
  sequence is reused.
- **Pruning raises `pruned` when its sweep ends** (round 4, R4-S7): unswept events count, so the
  space bound holds.
- **Attach is decided from the certificate** (round 4, R4-S3, replacing round 3's choice for R3-S3):
  once active, only the CSI controller's ID by name and agents restarting themselves; no record is
  read, so failover never depends on it. Today's other administrators lose Attach at activation. A
  non-administrator identity is refused before any store read (R3-S7), Attach included (round 5,
  R5-S2). The insecure rule comes first (R4-S6). The record may bind `csi-controller` only to the
  CSI controller's fixed ID, which deployments must use; the cost, manual and system-volume attaches
  needing that certificate, is stated as a change after approval (round 5, R5-C2, R5-S1). Settled
  2026-11-13 (user's instruction): accepted (see Settled at the user's instruction of 2026-11-13).
- **A `security-admin` cannot remove its own binding** (round 3, R3-S4): activation binds the
  activating administrator; a handover takes two writes.
- **Binding IDs are canonical and unique** (round 3, R3-S10): equal to
  `pki.ParseID(id).String()`; the record and its file are a list, so a repeated ID is refused.
- **What #73 needs from #76** (round 3, R3-C2; round 4, R4-S5, R4-C2): its unpublished rollback
  runs inside DeleteVolume after #73's checks and mark, replaces `finishDeletion`'s transaction,
  and leaves the owner record and quota alone; it may stay delete-only. The same DeleteVolume
  call then releases the owner record and answers only after that.
- **`APIError` keeps today's text** (round 3, R3-C5); wrappers use `%w`.
- **A record that does not parse is repaired at the etcd level** (round 3, R3-C6), by the
  operator; nothing in the service writes around it.
- **A tenant deletion always has a policy** (round 3, R3-C8): its owner record is in usage, and a
  policy with usage cannot be deleted; no separate refusal word.
- **Suppressed refusal counts are logged each second** (round 3, R3-S8).
- **A pending sweep is finished before the key check** (round 3, R3-S12).
- **Text written after approval says so** (round 4, R4-C5, R4-SN3) in the contracts and design
  headers.
- **A full `cfg` can be emptied while `security-admin` keeps its key** (round 5, R5-B1; renamed in
  round 8, R8-B2, from "Configuration cannot freeze"): no write may leave no `cluster-admin`, and
  `security-admin` may prune `cfg` itself, so a full `cfg` can be emptied by the role that cannot be
  locked out, as long as it holds the private half of the record's key or the overdraft has room to
  set one (round 7, R7-B1: key replacements can spend the overdraft, so the claim is stated with
  that precondition; no rule changes). With that key lost after the overdraft is spent, no chain can
  be pruned and `cfg` refuses every write until an etcd-level write of the record's `receiver_key`;
  the contracts define that repair (round 8, R8-B2). It reads the chain hash from
  `ChainStatus.hash`, with no `audit-reader` needed (round 6, R6-B1), and only a full `cfg` (round
  6, R6-S1, R6-C4: other chains are PermissionDenied before any read). The residual, a
  `security-admin` holding the key's private half erasing unexported configuration events, is stated
  after approval. Settled 2026-11-13 (user's instruction): accepted (see Settled at the user's
  instruction of 2026-11-13).
- **Sweep transactions hold at most 118 deletions** (round 5, R5-C1), so the last, with the sweep
  key and the tail compared at its revision, stays within etcd's 120 operations.
- **A creation releases its own leftover owner record** (round 6, R6-C1): before its quota
  check, a headerless owner record of the same owner, or with a `deleting_op`, is released, then
  Aborted; only another owner's, without one, is an orphan.
- **Growth raises the owner record's `bytes` with the usage** (round 6, R6-C2), so deletion
  releases the grown size; `ExpandVolume` writes no event; #70's role-table row is in the
  contracts.
- **`chains` is repaired to the store's current revision** (round 6, Nit 2); a later headerless
  read in DeleteVolume takes the first-read branch (Nit 1).
- **#76's rollbacks outside DeleteVolume** (round 5, R5-C5; corrected in round 7, R7-C3): a
  headerless owner record they leave is released by DeleteVolume's pass, or, with a
  `deleting_op` or of the creation's own owner, by a creation of that id (round 6, R6-C1); only
  another owner's, without one, is an orphan.
- **Growth is charged per step with #72's ledger** (round 5, cross-plan #70): whichever of #70
  and #73 lands second adds the charge and its test (spec S4).
- **A namespace chain's prune waits for room in its `sys-` chain** (round 5, R5-C3), as S9 now
  says.
- **Chain counters are canonical, keys are counted at 114 bytes, and the allocation's repair
  restores `chains`** (round 5, Nits).
