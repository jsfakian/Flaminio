# Issue #73 — report

## Review rounds

### Plan review round 1 — 094005fc7936e5cdbd41a1224c35b59165d4fb17

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b1 (design-code and design-security).

- **R1-B1 — High:** "Exclude old writers" and "policy-enabled mode" had no mechanism: a
  per-process flag, so instances could disagree, an older Attach could strip owner fields from
  the header, and `--insecure` was not refused. Evidence: `meta/service/auth.go:42–44,57,64–65`;
  `meta/service/service.go:516–582,539–541,723–725`; `deploy/csi/driver.yaml:49`.
- **R1-B2 — High:** Denied attempts were an unbounded, shared, durable write path that any node
  identity or a retrying provisioner could use to exhaust the budget and abort other
  transactions. Evidence: `control/csi/controller.go:261–269`; `service.go:45`.
- **R1-B3 — High:** Audit capacity, 90-day retention and etcd space did not fit; when a
  reservation is released was unstated. Evidence: `scripts/guest/etcd.sh:45`; #76's draft.
- **R1-B4 — High:** The role list covered none of today's Admin callers, and the audited methods
  were not listed; event schema 1 dropped serial and peer. Evidence:
  `api/proto/flaminio/meta/v1/meta.proto:26–60`; `auth.go:66–78`; `service.go:675–694`.
- **R1-B5 — Medium:** S11 had no approach, and the design cited an S12 that did not exist;
  `first_epoch` is 0 for older volumes. Evidence: `meta/domain/domain.go:44–47`.
- **R1-B6 — Medium:** Operations the design relied on had no RPC or auth rule (protection
  update, pruning, system provisioning, legacy assignment, policy reads, tenant status, a
  deletion intent carrier). Evidence: the draft's proto list.
- **R1-B7 — Medium:** How an intent reached CreateVolume was unspecified, and the CSI controller
  had no UID to look it up; `spec_hash` was undefined. Evidence: `deploy/csi/driver.yaml:67–68`;
  `control/csi/controller.go:233`.
- **R1-B8 — Medium:** "Class" meant both StorageClass UID and device class. Evidence:
  ARCHITECTURE §15.3 and :177; #72's draft.
- **R1-B9 — Medium:** AckAuditExport was trusted but gated pruning. Evidence: the draft's
  export paragraph.
- **R1-B10 — Medium:** DeleteVolume's unparseable-header path deletes without a protection
  check. Evidence: `service.go:535–551`.
- **R1-B11 — Medium:** When quota is released was a choice of three; who is recorded when a
  creation finishes another's deletion was unsaid. Evidence: `service.go:426–433,565–570,
  654–661`; docs/issues/14.
- **R1-B12 — Medium:** Intents, operations and policies were never removed. Evidence: the
  draft's key list.
- **R1-B13 — Medium:** Webhook scope and DELETE behaviour were unspecified. Evidence: the
  draft's admission paragraph.
- **R1-B14 — Medium:** The Kubernetes pieces had no client or test harness in this repository.
  Evidence: `go.mod`; `control/failover/kube.go:21–27`.
- **R1-B15 — Medium:** Half the plan designed against #50–#53 code that does not exist.
  Evidence: the draft's KMS paragraph and S11/S12 test row.
- **R1-B16 — Medium:** Identifiers that become keys or idempotency handles were not validated
  or bound to the caller. Evidence: the draft's key list.
- **R1-B17 — Low:** Paging and pruning needed a KV operation the interface lacks. Evidence:
  `meta/kv/kv.go:41–42`.
- **R1-B18 — Low:** OverrideDelete did not say who performs the deletion. Evidence: the draft's
  override paragraph.
- **R1-B19 — Low:** Two writers of one policy key. Evidence: the draft's PutTenantPolicy rule.
- **R1-B20 — Low:** Test plan gaps for S8 pruning, S10 and S11. Evidence: the draft's tables.
- **Nits 1–4 — Nit:** stale line references; partition by owner or key; Rust files listed
  with no message; `allowUnencrypted=false` refuses every creation before #50.

Changes/resolution: the plan is narrowed to the issue's first step, the metadata service's
authority (intent, "This step"): the Kubernetes half, #50–#53 hooks, a receiver and durable
refusal records stay open. Exact formats, the method table and codes are in a new
`contracts.md`; settled choices in a new `decisions.md`.
- **R1-B1:** a durable one-way activation record; this step's binary refuses to start, and a
  running instance without roles or insecure refuses administrator calls, once it exists; a call
  authorized before activation is Aborted if activation commits during it. Owner, policy and
  quota records live apart from the header, so an older writer cannot strip them; an older
  binary is unsupported, its leftovers reported (spec Q4). No instance record: an older binary
  writes none (decisions).
- **R1-B2:** a refusal writes nothing to the store; it is logged at most 10 lines a second, with
  a fixed reason word. Spec S7 and a flood test.
- **R1-B3:** the budget counts every stored event at one 2 KiB slot, exported or not, with one
  slot reserved per admitted deletion, released only by its commit; abandoned deletions are
  listed. No time-based retention. The etcd quota, compaction and defragmentation are runbook
  requirements (design, Risks); test with a full chain and Attach and ChangeIsr committing.
- **R1-B4:** five roles and one method table for all 13 existing and 10 new methods, with the
  event each writes; events keep serial and peer. Partly does not apply: the failover controller
  calls only `ListNodes` (`control/cmd/flaminio-csi/main.go:103–110`), never Attach (decisions).
  MoveReplica and DrainNode are not audited, with the reason (decisions).
- **R1-B5:** S11 written (activation, unassigned legacy volumes, `AssignOwner`); no S12; the
  override uses the owner record's revision, not `first_epoch`.
- **R1-B6:** every remaining operation has an RPC and a role; protection update, tenant status
  and the deletion intent are cut (protection is fixed with the owner record).
- **R1-B7:** no intents or spec hash: the owner is bound to the volume id `pvc-<PVC UID>`, which
  the CSI controller checks against the PVC it reads; its refusals are listed.
- **R1-B8:** device class is #72's pool name, `default` in this step; RF=1 and unencrypted are
  per-namespace permissions checked on the request; no StorageClass UID.
- **R1-B9:** no acknowledgement RPC; pruning requires the receiver's Ed25519 signature over the
  chain hash at that sequence, checked against the service's own chain; replay, stale, forged,
  beyond-tail, wrong-hash and no-key tests.
- **R1-B10:** protection and ownership are read from the owner record only, so an unparseable
  header is refused alike; test.
- **R1-B11:** released in the header's deletion transaction; a dead node's tombstone holds no
  charge (test); a creator finishing a deletion is the commit event's actor under its operation
  id.
- **R1-B12:** intents and operation records cut; quota records go at zero; a policy is deleted
  only with no usage.
- **R1-B13:** resolved by the cut: no webhook in this step.
- **R1-B14:** the CSI lookup uses the existing hand-written client, tested with `httptest` and a
  fake; no client-go, no envtest.
- **R1-B15:** resolved by the cut: #50–#53 own their hooks and tests, recorded in
  `docs/followups.md`.
- **R1-B16:** an identifier table checked before any key; operation ids are drawn by the
  service; no operation records to collide on.
- **R1-B17:** point reads of 20-digit sequence keys; `kv.KV` unchanged.
- **R1-B18:** the override performs the deletion itself and is the audited actor; a retry
  finishes it.
- **R1-B19:** one writer, `cluster-admin`; no policy controller in this step.
- **R1-B20:** spec and design tests cover pruning; every row names its file.
- **Nits:** line references corrected and re-checked; partition by volume id or namespace UID;
  no Rust change (`pb.rs` holds none of the changed messages); the upgrade order requires
  `--allow-unencrypted` until #50.
- **Other plans:** this step provides no tenant QoS ceilings (#75); from #72 it assumes only a
  device class name.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 9180f47bba154f88b28cee53822bf93c6d612427

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73c2 (incremental design-code), r73s2 (incremental design-security). Both confirm
every round-1 finding fixed.

- **R2-C1 — High:** An interrupted creation of a protected volume could never be cleaned up:
  DeleteVolume refused it as protected and AssignOwner refused a creating header, so the charge
  stayed. Evidence: `meta/service/service.go:287,413,466`.
- **R2-S1 — High:** One namespace could spend the shared, never-pruned audit budget, refusing
  every tenant's deletions and the administrator's policy, assignment and confirmation events.
  Evidence: contracts "Budget"; spec S9, Q2; the cap's range up to 32 GiB.
- **R2-S2 — High:** Per-instance role files could disagree; the CSI client fails over only on
  Unavailable, so one divergent instance stops its Attach; revocation needed every restart; an
  ID could hold exclusive roles on two instances. Evidence: `control/csi/meta.go:64–83`.
- **R2-C2 — Medium:** `reserved` leaked when an older binary deleted the header of an admitted
  deletion. Evidence: contracts "Budget", `header-missing`.
- **R2-S3 — Medium:** The receiver key was per-instance, unrecorded and unaudited, and a prune kept
  no evidence of its receipt. Evidence: contracts "serve", "Event".
- **R2-C3 — Low:** An orphan owner record blocked recreating the id with 32 backoffs and Aborted.
  Evidence: design "A failed claim".
- **R2-C4, R2-S10 — Low:** An older binary answers Unimplemented, not PermissionDenied; the test
  stand-in did not model it. Evidence: gRPC refuses unknown methods before interceptors.
- **R2-C5 — Low:** The Kubernetes lookup had an untyped error and unvalidated, unescaped path
  segments. Evidence: `control/failover/kube.go:81–83`.
- **R2-C6 — Low:** The activation race guard covered only audited calls. Evidence: design "The
  race".
- **R2-C7, R2-S4 — Low:** On an insecure inactive instance the new methods, `ActivateTenancy`
  included, passed with no identity. Evidence: design "Authorize, in order".
- **R2-C8 — Low:** Missing tests: the insecure branch, the refusal-log limit, S2's message, the
  cached active bit. Evidence: design "Tests".
- **R2-S5 — Low:** Reading the PVC before its namespace let a replaced namespace become the owner.
  Evidence: contracts "CSI controller".
- **R2-S6 — Low:** A tenant creation's retry adopted an existing volume of another owner.
  Evidence: `control/csi/controller.go:239–247`.
- **R2-S7 — Low:** A refused creation still listed every chunk for placement on every retry.
  Evidence: `meta/service/service.go:385`.
- **R2-S8 — Low:** The global refusal-log limit let one caller hide everyone else's refusals.
  Evidence: contracts "Refusal log".
- **R2-S9 — Low:** The failover controller holds the `csi-controller` identity. Evidence:
  `deploy/csi/driver.yaml:208,220`.
- **R2-S11 — Low:** A legacy volume whose header does not parse could never be deleted. Evidence:
  contracts "Errors".
- **R2-S12 — Low:** ListAudit's start wrapped at the top of `after_sequence`. Evidence: design
  "ListAudit".
- **R2-CN1–CN3 — Nit:** `deleting_op` is in the owner record, not the header; S7's event count
  omitted two cases; `audit.pruned` "always admitted" next to the cap.

Changes/resolution:
- **R2-C1:** protection guards published volumes: a header still being created is deleted by
  DeleteVolume whatever its protection or assignment, with its events and release (spec S5,
  S4; design "Deletion"), which agrees with #76's resumption and its rollback after this plan's
  checks; test of a creation failed after its claim, then deleted, on both backends.
- **R2-S1:** per-namespace chains: a namespace's tenant deletions write only to `ns-<UID>`,
  budgeted by its policy's `max_audit_events` (2–65536); all other events go to 16 `sys-`
  chains of a fixed 8192, which no tenant writes. The cap flag is gone; contracts "Budget" states
  the space bound (`allocated_events`) and what happens with no receiver; test of one namespace
  churning while another deletes and a policy is written in the same `sys-` chain.
- **R2-S2:** roles live in one authorization record in the store, read by every instance on each
  call that needs them, written by a separate `security-admin`; no per-instance file. Every
  instance answers alike and revocation is immediate; the CSI client's behaviour on
  PermissionDenied is stated (contracts "Errors"); two-instance test.
- **R2-C2:** a `header-missing` commit after an admitted deletion takes its slot from
  `reserved`, in its recorded chain; test.
- **R2-S3:** the receiver key lives in the authorization record, which `cluster-admin` cannot
  change; the prune transaction compares the record and `audit.pruned` holds the receipt and the
  key's fingerprint; rotation is a new record, after which old receipts fail; trust boundary
  stated (design Risks, Q3).
- **R2-C3, R2-S6:** a failed claim reads the owner record: another owner is PermissionDenied, an
  owner record with no header FailedPrecondition at once; a system creation cannot take a tenant
  id; tests.
- **R2-C4, R2-S10:** contracts say Unimplemented; the stand-in answers it, with PermissionDenied
  kept for an unbound caller.
- **R2-C5:** names checked as DNS-1123 and escaped; `*failover.APIError` for non-2xx; 404 and 500
  tests.
- **R2-C6:** stated in S11 as the exception, not closed.
- **R2-C7, R2-S4:** an insecure instance refuses every new method; handlers refuse without a role
  in any state; in `TestRoleTable`.
- **R2-C8:** test rows added (insecure branch, `refuse_test.go`, message check, counting store).
- **R2-S5:** the namespace is read first, then the PVC; httptest case for a replaced namespace.
- **R2-S7:** policy and quota are read and refused before the tombstone check and placement.
- **R2-S8:** the limit is per caller, with suppressed counts named per caller and a bounded
  table.
- **R2-S9:** stated in the method table and Risks; follow-up in intent.
- **R2-S11:** AssignOwner takes `system` for a header that does not parse or is still creating;
  test.
- **R2-S12:** `after_sequence >= next − 1` is an empty page; in the S10 tables.
- **Nits:** "owner record" throughout; S7 lists the one- and two-event cases; contracts say
  `audit.pruned` raises `pruned` in its own transaction, so `present` does not grow.
- **Needs other plans:** #76's unpublished rollback must delete the owner record, release the
  charge and append this plan's events, so it is not delete-only once tenancy is active.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 7a911860ee5d0c0f3a181ba6e1594606b2ceb34d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73c3 (incremental design-code), r73s3 (incremental design-security). Both confirm
every round-2 finding fixed. The plan was approved at the reviewed commit; these fixes came after
the approval.

- **R3-C1, R3-S1 — High:** `authorization.put` and `policy.put` shared a system chain's budget,
  so filling it, by `policy.put` alone and with no receiver key never pruned, locked the security
  administrator out of revoking a binding or setting the key, and the administrator out of
  raising an allowance. Evidence: contracts "Chain of an event" and "Budget".
- **R3-C2 — Medium:** #76's unpublished rollback is delete-only and cannot do what this plan's
  `finishDeletion` does; the conflict was resolved only by assertion. Evidence: #76 design
  "Deletion classification and legacy rollback"; design "#76".
- **R3-C3, R3-SN1 — Medium:** the approval left spec Open questions, design Risks, the contracts
  header and the ADR proposal's section presenting settled questions as open. Evidence: decisions
  "Settled at approval".
- **R3-S2 — Medium:** nothing bounded the total audit space: the number of policies, the sum of
  their allowances and chains left after a policy's deletion. Evidence: contracts "Budget".
- **R3-S3 — Medium:** the CSI controller's Attach read the authorization record, against Risks.
  Evidence: contracts "Methods and roles"; design Risks.
- **R3-S4 — Medium:** nothing kept a usable `security-admin`. Evidence: contracts "Authorization
  record".
- **R3-C4, R3-S6 — Low:** "a removed binding is refused at once" did not hold for calls already
  authorized. Evidence: spec S1; design PruneAudit.
- **R3-C5 — Low:** `APIError` had to keep today's error text. Evidence:
  `control/failover/kube.go:82`.
- **R3-C6 — Low:** a corrupt authorization record locked out its own repair. Evidence: contracts
  "Records".
- **R3-C7 — Low:** tests were missing for the new RPCs, the CLI and TenantStatus. Evidence: design
  "Tests".
- **R3-C8 — Low:** a namespace chain's cap with no policy was unexplained. Evidence: contracts
  "Budget".
- **R3-S5 — Low:** an authorization record missing after activation fell back to the
  administrator-name rule. Evidence: design "Authorize, in order".
- **R3-S7 — Low:** a refused role-gated call cost a store read, for node identities too.
  Evidence: `meta/service/auth.go:62–85`.
- **R3-S8 — Low:** suppressed refusal counts were lost when the table was cleared. Evidence:
  contracts "Refusal log".
- **R3-S9 — Low:** AssignOwner `system` could take a tenant volume id. Evidence: design
  "AssignOwner".
- **R3-S10 — Low:** repeated or non-canonical binding IDs were not refused. Evidence:
  `pkg/pki/pki.go:222–238`.
- **R3-S11 — Low:** "still being created" used the `Creating` flag alone, which #76 calls a
  weakened classifier. Evidence: #76 spec S8.
- **R3-S12 — Low:** a pending sweep could not finish once the receiver key was removed. Evidence:
  design "PruneAudit".
- **R3-S13 — Low:** the activation-window exception had no test. Evidence: spec S11.
- **R3-CN1–CN3 — Nit:** the insecure active instance's refusals unnamed; `--expect-revision`'s
  source; the round in which the review clears.

Changes/resolution:
- **R3-C1, R3-S1:** configuration writes (activation, the authorization record, policies) are
  no longer audit events: bounded overwrites, never refused for audit space, each record keeping
  `changed_by` and each change logged (contracts "Configuration writes"). The chains hold
  destructive operations only. Test: every system chain full, then PutAuthorization and
  PutTenantPolicy commit and a prune follows.
- **R3-C2:** #73 needs from #76 only that its rollback runs inside DeleteVolume after #73's
  checks and leaves the owner record and quota alone; it may stay delete-only, since
  DeleteVolume's next pass releases an owner record whose header is gone. Test with a header
  deleted alone.
- **R3-C3, R3-SN1:** each settled question is marked in place in spec, design, the ADR proposal
  and the contracts header; the legacy-owner evidence is marked still open.
- **R3-S2:** a store-wide `allocation` record bounds the allowances' sum (524288 events) and the
  policies (4096); an allowance cannot fall below its chain's use; a policy whose chain holds
  anything cannot be deleted. Tests for each limit and for new-UID churn.
- **R3-S3:** Attach keeps today's rule and reads no record; tests with a bad record and a missing
  CSI binding.
- **R3-S4:** activation binds the caller as `security-admin`, and no write drops the writer's own
  binding; tests, including a two-step handover.
- **R3-C4, R3-S6:** stated as "from the caller's next call"; test that an authorized call
  finishes.
- **R3-C5:** `Error()` keeps today's text, wrappers use `%w`; tested.
- **R3-C6:** repair is an etcd-level write by the operator, in the runbook.
- **R3-C7:** test rows for racing activation, PutAuthorization (stale, self-removal, during a
  prune), the CLI and TenantStatus.
- **R3-C8:** shown unreachable: every tenant owner record is in usage, and a policy with usage
  cannot be deleted.
- **R3-S5:** activation writes two records; one missing, or a record absent on an instance that
  has seen the store active, is Internal, and `serve` exits 1; tested.
- **R3-S7:** a non-administrator identity is refused before any store read; tested with a
  counting store.
- **R3-S8:** every nonzero count is logged each second; tested.
- **R3-S9:** `system` refused for a parsed tenant-form id; tested.
- **R3-S10:** IDs must be canonical and unique; the record and its file are lists; tested.
- **R3-S11:** "being created" is #76's test (`creating`, epochs 0, no target); tested with a
  nonzero epoch.
- **R3-S12:** a pending sweep is finished before the key check; tested with the key removed.
- **R3-S13:** an `interleaveKV` case for the window.
- **R3-CN1, R3-CN2:** stated in the contracts. **R3-CN3:** applies when the review clears.
- **Needs other plans:** #76 must keep its unpublished rollback inside DeleteVolume after #73's
  checks and leave #73's owner record and quota alone.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — e9f0ba75deb22f83729d9535d0b269287c7f831a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73c4 (incremental design-code), r73s4 (incremental design-security). The reviewed
fix, 98c1242, and these fixes came after the plan's approval.

- **R4-S1 — High:** role, receiver-key and policy changes left no durable, attributable record
  (round 3 had cut them from the chains), against the approved intent. Evidence: contracts
  "Configuration writes"; intent "This step".
- **R4-S2, R4-C1 — High:** an `ns-` chain's own `audit.pruned` kept it from ever emptying, so
  its policy, allowance and slot were held for life. Evidence: contracts "Budget".
- **R4-S3 — High:** once active, any administrator name could Attach, the audit reader, the
  override and revoked identities included. Evidence: `meta/domain/domain.go:217–220`;
  `meta/service/service.go:723–725`.
- **R4-S4 — Medium:** `serve` exited on incomplete activation records, so a restart took
  failover down. Evidence: contracts "serve".
- **R4-S5, R4-C2 — Medium:** the #76 interface left its order (before or after the mark) and its
  return (same call or a later one) open, so a charge and slot could leak. Evidence: #76 design
  "Deletion classification and legacy rollback"; `control/csi/controller.go:265`.
- **R4-C3 — Low:** no test for the configuration log line or the refusal table's bound.
  Evidence: design "Tests".
- **R4-C4 — Low:** a record missing after activation had no stated recovery. Evidence: contracts
  "Repair".
- **R4-C5, R4-SN3 — Low:** text written after approval was labelled approved. Evidence:
  contracts and design headers.
- **R4-S6 — Low:** Attach passed before the insecure rule. Evidence: `meta/service/auth.go:42–44`.
- **R4-S7 — Low:** the space bound left out a pending sweep's events. Evidence: contracts
  "Budget".
- **R4-CN1, R4-CN2, R4-SN1, R4-SN2 — Nit:** S1's Attach sentence; the 3 GiB margin next to the
  space figure; `changed_by.role` for activation; the configuration writes' description.

Changes/resolution:
- **R4-S1:** configuration changes are events again, in one `cfg` chain no tenant writes: one
  event per binding changed, per receiver-key change and per policy write or deletion; each write
  also logs the full record. A recovery write (one that grants nothing) has a 1024-event
  overdraft no other write uses, so revoking a binding or setting the key is admitted when `cfg`
  is full; a full `cfg` refuses grants and policy writes and deletions until pruned. Policies hold
  at most 4 classes so a policy event fits. Tests: grant, use, revoke; a protection flip-flop;
  `cfg` full.
- **R4-S2, R4-C1:** an `ns-` chain's `audit.pruned` goes to its namespace's `sys-` chain, so a
  fully pruned chain is empty and DeleteTenantPolicy frees its allowance and slot; chain names
  carry a counter (`ns-<UID>-<k>`), so nothing is reused. Tests: deletion after a prune; 4097
  namespaces over time.
- **R4-S3:** once active, Attach is decided from the certificate alone: the CSI controller's ID
  by name and agents restarting themselves; others are refused, and the loss of manual attach at
  activation is stated. Tests for each refused identity.
- **R4-S4, R4-C4:** `serve` starts; incomplete records fail only role-gated calls; repair is an
  etcd-level restore, or `tenant activate` when both are gone. Tests at start and at runtime.
- **R4-S5, R4-C2:** the mark comes first; #76's rollback replaces the deletion's transaction; the
  same DeleteVolume call releases the owner record and answers only after that, or returns an
  error. Tests with a stand-in rollback.
- **R4-C3:** a captured-handler test and a 1025-caller test.
- **R4-C5, R4-SN3:** the headers say what changed after approval.
- **R4-S6:** the insecure rule comes first, Attach included; tested.
- **R4-S7:** `pruned` rises when the sweep ends, so unswept events count; the bound includes them.
- **Nits:** S1 states the Attach rule; the 3 GiB margin sits by the figure; `changed_by.role` is
  `admin-name` for activation; the configuration writes' description is exact.
- **Needs other plans:** #76 must run its unpublished rollback inside DeleteVolume after #73's
  mark, as the replacement of the deletion's transaction, leave #73's owner record and quota
  alone, and not answer the call itself.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — c69830e887cb703a5c47994724278f68c3ec8d54

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b5 (incremental, design-code and design-security). The reviewed fix, f262e1f, and
these fixes came after the plan's approval.

- **R5-B1 — High:** the `cfg` overdraft did not guarantee that a full `cfg` could be pruned:
  revoking the sole `cluster-admin`, or a `cluster-admin` flooding `cfg`, froze grants and policy
  writes for life. Evidence: contracts "Budget", "Methods and roles".
- **R5-C1 — High:** the sweep's last transaction could be 121 operations, over etcd's 120, and
  its tail write was not conditional. Evidence: `meta/kv/etcd.go:91–92`; design "PruneAudit".
- **R5-C2 — Medium:** Attach's fixed CSI name was not tied to the `csi-controller` binding.
  Evidence: `control/cmd/flaminio-csi/main.go:7–8`; `scripts/lab.sh:1004`.
- **R5-S1 — Medium:** once active, manual and system-volume attaches needed the CSI controller's
  certificate, a least-privilege cost. Evidence: `scripts/lab.sh:624,629,715,824`.
- **R5-C3 — Low:** an `ns-` chain's prune could be refused while its `sys-` chain was full,
  against S9, untested. Evidence: contracts "Budget".
- **R5-C4 — Low:** the overdraft's bound and the recovery-write classifier were untested.
  Evidence: design "Tests".
- **R5-S2 — Low:** Attach read `active` before refusing a non-administrator. Evidence: design
  "Authorize, in order".
- **R5-C5 — Low:** #76's rollbacks outside DeleteVolume were unstated (R4-C2 residual). Evidence:
  #76 design "Deletion classification and legacy rollback".
- **R5-N1–N5 — Nit:** the event key size; a non-canonical chain counter; the uses of `active`;
  DeleteVolume's two no-header branches; the allocation's repair and a raised allowance with a
  full `cfg`.

Changes/resolution:
- **R5-B1:** no write may leave no `cluster-admin` (as for `security-admin`), and
  `security-admin` may prune `cfg` itself; tests for the sole-`cluster-admin` revocation and for
  a `cluster-admin` flood followed by a key, a prune, a new binding and the old one's revocation.
- **R5-C1:** sweep transactions delete at most 118 events, and the last writes the tail compared
  at its revision: at most 120 operations; tests on the etcd store at 118 × n and 119 × n, and an
  append racing the last transaction.
- **R5-C2, R5-S1:** the fixed name stays; the record may bind `csi-controller` only to it, and
  the upgrade order requires it; the cost to manual and system-volume attaches is stated as a
  change after approval, not accepted. Tests: `csi-controller` bound to `admin/csi` refused;
  `system-provisioner`'s Attach refused.
- **R5-C3:** S9 states the exception; tested.
- **R5-C4:** tests for a revoke-and-grant and an ID replacement classed as grants, and for a
  recovery write at exactly `cap + 1024` and the next refused.
- **R5-S2:** the agent rule and the non-administrator refusal come before Attach reads `active`;
  tested.
- **R5-C5:** a headerless owner record left by #76's rollback outside DeleteVolume is released by
  DeleteVolume's pass, or, with a `deleting_op`, by a creation of that id; only one without is an
  orphan; tested.
- **Cross-plan #70:** spec S4 states that each growth step charges the owner's namespace and
  class quota with #72's ledger, added by whichever of #70 and #73 lands second.
- **Nits:** keys counted at 114 bytes (about 1.34 GiB); the chain counter is a canonical u64
  decimal; `active`'s uses named; both no-header branches named with their answers; the repair
  restores `chains`, and a raised allowance needs room in `cfg`.
- **Needs other plans:** #76 (unchanged from round 4, plus: its rollbacks outside DeleteVolume
  leave #73's owner record for #73 to release).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — ac399a63c5450af7ed53360c777a5af938270097

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b6 (incremental, design-code and design-security). The reviewed fix, e2e87a7, and
these fixes came after the plan's approval.

- **R6-B1 — High:** pruning `cfg` needed the chain hash, which only `audit-reader` could read,
  so with none bound a sole `cluster-admin` flooding `cfg` still froze it. Evidence: contracts
  "Protobuf" (`ChainStatus`), "Methods and roles".
- **R6-S1 — Medium:** `security-admin` alone could set a key it holds and prune `cfg`, erasing
  unexported configuration history; the plan did not say so. Evidence: contracts "Methods and
  roles"; spec S8; intent "What we want" 3.
- **R6-C1 — Medium:** after #76's legacy rollback, the same PVC's retry was refused as an orphan,
  or at the namespace's limit by its own stale charge. Evidence: design "A failed claim";
  `control/csi/controller.go:248–249`.
- **R6-C2 — Medium:** spec S4's growth charge had no approach or test in #73, and the owner
  record's `bytes` was never said to grow. Evidence: #70 design "Charges".
- **R6-C3 — Low:** the overdraft could end with no receiver key, freezing `cfg`. Evidence:
  contracts "Recovery write".
- **R6-C4 — Low:** `security-admin`'s PruneAudit of a non-`cfg` chain had no stated refusal point
  and no test. Evidence: contracts "Methods and roles".
- **R6-N1, R6-N2 — Nit:** a later headerless read in DeleteVolume was unnamed; repairing `chains`
  from existing names could reuse a deleted policy's chain name.

Changes/resolution:
- **R6-B1:** `ChainStatus` gains `hash`, the chain hash at `next − 1`, which TenantStatus gives
  `security-admin`; test with no `audit-reader` bound, the security administrator using only its
  own calls.
- **R6-S1:** `security-admin` may prune `cfg` only when it is full; the residual (a key whose
  private half it holds) is stated in the contracts, design, ADR proposal and intent as a change
  after approval, not accepted; tests for the refusal of a non-full `cfg`.
- **R6-C1:** before its quota check, a creation releases a headerless owner record of its own
  owner, or with a `deleting_op`, then answers Aborted; test with a stand-in legacy rollback,
  also at the namespace's limit.
- **R6-C2:** the growth step raises the owner record's `bytes` with the usage (contracts
  "Growth", "Records"); design approach and `TestExpandChargesQuota` named; #70's `ExpandVolume`
  row added to the role table, with no event.
- **R6-C3:** a key removal is not a recovery write, so it is refused on a full `cfg`; tested.
- **R6-C4:** `security-admin` on another chain is PermissionDenied before any read; tested with a
  `sys-` and an `ns-` chain.
- **Nits:** a later headerless read takes the first-read branch; `chains` is repaired to the
  store's current revision.
- **Needs other plans:** none new; #70's row and test are as #70 states them.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — c80ae2f67345a36bc9940d24d14ef12e77b5ff3f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b7 (incremental, design-code and design-security). The reviewed commits, the
round-6 fix and the settle commit, and these fixes came after the plan's approval.

- **R7-C1 — Medium:** the settled Q3 answer sent a `pvc-` volume with no PVC to `system` or the
  override, and the contracts refuse both. Evidence: `control/csi/controller.go:232`; contracts
  "Errors" (`system` for a parsed tenant-form id; `OverrideDelete` of an unassigned volume).
- **R7-B1 — Low:** key replacements could spend the `cfg` overdraft and leave a key nobody signs
  with, so the "always" claims were false; the invariant still listed a key removal under the
  overdraft. Evidence: contracts "Budget", "Recovery write"; spec invariant and S9.
- **R7-C2 — Low:** a `security-admin` prune retried after a lost reply or a crash answered
  `not-full`, not `pruned`. Evidence: design "PruneAudit".
- **R7-C3 — Low:** two passages still said only a `deleting_op` record is released by a
  creation. Evidence: design "#76"; decisions, round 5.
- **R7-S1 — Low:** Q2 was unmarked and the ADR's Decision still called the receiver
  independent. Evidence: decisions "Settled at approval"; ADR proposal "Decision".
- **R7-N1–N4 — Nit:** the inactive-creation sentence under Growth; `bytes` for assigned and
  system volumes; "nothing read" for a refused prune; "(see decisions.md)" inside decisions.md.

Changes/resolution:
- **R7-C1:** Q3 restated, after approval: the PVC whose UID the id carries, with its namespace;
  with no such PVC, the namespace in the PV's `claimRef` and the id's PVC UID; with that
  namespace gone, a namespace the administrator designates, with its own policy; then
  DeleteVolume or the override. `system` stays for ids not of the tenant form and headers that do
  not parse. No refusal is loosened and no rule narrowed; decisions.md records the change and
  why; tests added.
- **R7-B1:** the claims now state their precondition: `security-admin` must keep the private
  half of the last key it sets in the overdraft; a key lost after it is spent leaves `cfg` full
  until an etcd-level repair. The invariant says a key removal is an ordinary write. No rule
  changes and no lockout path opens; test: the overdraft spent by key changes, a prune with the
  last key admitted, the next key change refused.
- **R7-C2:** `through <= pruned` comes before `not-full`; both `security-admin` retries tested.
- **R7-C3:** both passages corrected.
- **R7-S1:** the ADR's Decision names the exception; Q2 marked in spec and decisions.
- **Nits:** each fixed as raised.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — ae0b6296770960fce8ea5b4a8242d6641b21133f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b8 (incremental, design-code and design-security). The reviewed fix, b124ff2, and
these fixes came after the plan's approval.

- **R8-B1 — Medium:** Q3's step 2 took the owner's namespace from the PV's `claimRef`, a name,
  so a namespace re-created under that name would receive a volume it never owned. Evidence:
  decisions "Settled at the user's instruction of 2026-11-13"; spec Q3; design Risks; spec S11
  ("No owner is guessed from a name").
- **R8-B2 — Low:** the lost-key residual was understated (no chain can be pruned, not only
  `cfg`), its repair undefined, and it was stated in the contracts only. Evidence: contracts
  "Budget"; decisions "Configuration cannot freeze".
- **R8-C1 — Low:** round 7's runbook duties had no home in the Docs plan, and the S11 test could
  not fail on the evidence order. Evidence: design "Docs", "Tests".
- **R8-N1 — Nit:** the overdraft test read out of order. Evidence: design and spec tests S7–S9.

Changes/resolution:
- **R8-B1:** step 2 removed. With the PVC gone, the owner is a namespace the administrator
  designates, with its own policy; step 1 reads the namespace first. Updated in decisions (both
  Q3 entries, with why), spec Q3 and design Risks; the test is reworded (R8-C1).
- **R8-B2:** the residual is stated exactly (no chain can be pruned and `cfg` refuses every write,
  until an etcd-level write of the record's `receiver_key`), in the contracts, spec S9, design
  Risks and the ADR's Consequences; the repair is defined next to the record's repairs; the
  decisions heading is renamed. No new mechanism.
- **R8-C1:** the usage comment's plan carries the evidence order (a runbook step the service
  cannot check), the key duty and the lost-key repair; the S11 test reads "AssignOwner admits a
  tenant owner for a `pvc-` id whose PVC is gone".
- **R8-N1:** reworded in the order the reviewer gives.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 9eb18b4a82990c41c01dab6cf3ad2097ca3b0658

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b9 (incremental, design-code and design-security). The reviewed fix, 3aaec96, and
these fixes came after the plan's approval.

- **R9-B1 — Low:** the lost-key repair contradicted `changed_by`, was an unconditional
  read-modify-write, and had no test. Evidence: contracts "Activation and the authorization
  record", "Records"; AGENTS.md (conditional transactions).
- **R9-B2 — Low:** Q3's designated namespace was unconstrained, so a live or re-created tenant
  could still be given a volume it never owned. Evidence: spec Q3; design "AssignOwner";
  contracts "Errors".
- **R9-N1 — Nit:** step 1's "namespace first" could not be followed when the PVC is found by UID.
- **R9-N2 — Nit:** "A receiver key removal is not such a change" read as referring to the
  etcd-level write. Evidence: spec S9.
- **R9-N3 — Nit:** decisions still said "restated in round 7".

Changes/resolution:
- **R9-B1:** the contracts say `changed_by` keeps the last service write's value and does not
  attribute the new key; the runbook records who repaired and when; a verifier finds the repair
  as a fingerprint no `authorization.receiver` event or logged record sets. The repair is an
  `etcdctl txn` comparing the record's mod revision as read, after checking that `cfg`'s `present`
  is at `cap + 1024`. A four-step `audit_test.go` case on `backends` (spend the overdraft with key
  changes; rewrite the record through the store; a prune with the new key admitted, naming its
  fingerprint; a PutAuthorization at the earlier revision refused as stale), mirrored in spec
  S7–S9.
- **R9-B2:** the designated namespace is a dedicated one created to hold legacy volumes, with its
  own UID, policy and `default` limit, never a tenant's and never one taken from a `claimRef`;
  enough because Q3 deletes the volume next. Updated in spec Q3, both decisions entries, design
  Risks and the usage comment's upgrade order.
- **R9-N1:** step 1 finds the PVC by its UID, reads its namespace's UID, reads the PVC again by
  namespace and name, and requires the id's UID.
- **R9-N2:** "A receiver key removal does not use the overdraft."
- **R9-N3:** the Q3 markers say rounds 7 to 9.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — cc1ca570d84ef9b40e08eda8f0961316c73cdb20

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b10 (incremental, design-code and design-security). The reviewed fix, 72396a6, and
these fixes came after the plan's approval.

- **R10-B1 — Low:** the lost-key repair's precondition named a `present` field `ChainStatus` does
  not have, ignored a pending `cfg` sweep and the no-op PutAuthorization, and its txn did not hold
  the precondition at commit. Evidence: contracts "Activation and the authorization record",
  "Budget", "Protobuf" (`ChainStatus`).
- **R10-B2 — Low:** the legacy-holding namespace was a tenant namespace to every component, its
  policy admitted creations, and "costs nothing" ignored its chain. Evidence: spec Q3; design
  "AssignOwner"; contracts "Budget".
- **R10-N1 — Nit:** `changed_by` was defined as "the record's last write". Evidence: contracts
  "Records".
- **R10-N2 — Nit:** "the runbook records who made the repair" named no writer or place.

Changes/resolution:
- **R10-B1:** the repair first finishes any pending `cfg` sweep (a PruneAudit through `pruned`,
  no key), then requires `next − 1 − pruned` = `cap + 1024` and no sweep, so no PutAuthorization
  can set a key (a no-op one still commits, which the record compare catches); its `etcdctl txn`
  also compares `audit/tail/cfg` and the sweep's absence. The test adds a repair refused while
  `cfg` has room or a sweep is pending.
- **R10-B2:** the holding namespace's policy has a `default` limit of 0 volumes (class limits may
  be 0), a `max_audit_events` of at least twice the volumes it will hold; no one has PVC rights
  there and its UID is in the operator log; it costs its chain two events per volume. The S11
  test assigns into it and shows a creation refused.
- **R10-N1:** `changed_by` is the service's last write of the record.
- **R10-N2:** the operator writes who repaired, when and the new fingerprint in the runbook's
  operator log.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 11 — d01d39af016b11c6c484f592705075e24894c7b8

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b11 (incremental, design-code and design-security). The reviewed fix, 39da093, and
these fixes came after the plan's approval.

- **R11-B1 — Low:** the repair checked its precondition on `ChainStatus`, which carries no
  revision, and compared revisions from a separate read, so `cfg` could gain room between the
  two. Evidence: contracts "Activation and the authorization record", "Protobuf".
- **R11-B2 — Low:** the repair's test cases did not isolate its three compares, and "while `cfg`
  has room" could not pass. Evidence: design and spec S7–S9.
- **R11-N1 — Nit:** the sweep-finishing PruneAudit still needs a well-formed 32-byte hash and
  64-byte receipt. Evidence: contracts "Identifiers", "CLI".
- **R11-N2 — Nit:** the usage comment's operator log omitted the new key's fingerprint, and its
  reference followed the holding-namespace clause. Evidence: design "Docs".

Changes/resolution:
- **R11-B1:** the repair reads `audit/tail/cfg`, `audit/sweep/cfg` and the record with `etcdctl
  get -w json`, the later reads at the first response's header revision (`--rev`); checks `next −
  1 − pruned` on that tail value; builds the new record from that record value; and its txn
  compares those reads' `mod_revision`s. `ChainStatus` is a cross-check only. Same in the
  usage comment's runbook.
- **R11-B2:** three named cases, each refused by one compare alone: a prune of `cfg` ending
  before the txn (tail), a no-op PutAuthorization after the reads (record), reads taken while a
  sweep is pending (sweep absence); "while `cfg` has room" dropped.
- **R11-N1:** "with any 32-byte hash and 64-byte receipt, e.g. zeros".
- **R11-N2:** the operator log holds the fingerprint too; the holding-namespace clause points to
  spec Q3.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 12 — 3b298c6c1d5e0fb265a5c133d16207878a090717

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r73b12 (incremental, design-code and design-security). The reviewed fix, 1cc9a08, and
this fix came after the plan's approval.

- **R12-B1 — Low:** the repair's case 2 isolated the record compare only if a PutAuthorization
  with no event leaves `audit/tail/cfg` unwritten, which the plan did not say, and no case
  checked that its compare refused alone. Evidence: design `appendEvents`, PutAuthorization;
  design and spec S7–S9.

Changes/resolution:
- **R12-B1:** the design states that with no events `appendEvents` returns only the tail compare
  and no tail write, so a PutAuthorization that changes nothing leaves `audit/tail/cfg` unwritten.
  In spec and design S7–S9, each repair case also asserts that the other two compares still hold:
  case 1, the record's revision unchanged and the sweep key absent; case 2, the tail's revision
  unchanged and the sweep key absent; case 3, the tail's and the record's revisions unchanged.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 13 — b7b0b821ee8a2e6a13702126a294576b09e47da5

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
