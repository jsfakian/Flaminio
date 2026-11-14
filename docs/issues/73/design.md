# Issue #73: Tenant policy and destructive-operation audit — design

How the [spec](spec.md)'s rules are built; every format, table and code is in
[contracts](contracts.md). Settled choices: [decisions](decisions.md).
Approved 2026-11-13; not implemented. Changed after approval in review rounds 3 to 12
(decisions.md, Approvals).

The facts it starts from:
- `Authorize` grants CreateVolume, DeleteVolume, MoveReplica and DrainNode to any Admin name, and
  the admin branch of Attach, RegisterNode, NodeHeartbeat, ListTombstones and ConfirmDeletion
  (`meta/service/auth.go:57,64–78`). With no trust domain it returns before any identity
  (`:42–44`). gRPC answers a method a binary does not know with Unimplemented, before any
  interceptor.
- The CSI controller creates, deletes and publishes (`control/csi/controller.go:231–236,265,281`);
  its client moves to another instance only on Unavailable, DeadlineExceeded or Canceled
  (`control/csi/meta.go:75–82`). The failover controller calls only `ListNodes`
  (`control/cmd/flaminio-csi/main.go:103–110`) with the CSI controller's certificate
  (`deploy/csi/driver.yaml:208,220`); drain moves and drains (`control/drain/drain.go:124,172`).
- `createVolume` lists every chunk to place (`meta/service/service.go:385`), claims the id with a
  header marked creating (`:413–417`), and clears the mark last (`:466`); `readHeader` hides a
  creating header, so it is never attached (`:287`). A failed claim finishes another caller's
  marked deletion through `DeleteVolume` (`:421–433`). `DeleteVolume` marks the header, except
  one that does not parse (`:535–551`), then deletes it and writes the tombstone in one
  transaction (`:565–570`). The mark and Attach rewrite the header through the struct
  (`:539–541`, `:723–725`): an older binary drops fields it does not know.
- `logConfirmation` logs caller, peer and serial (`:675–694`). `kv.KV` has Get, List of a
  prefix, Txn and TxnRev, at most 120 operations each (`meta/kv/kv.go:33,38–52`).
- The CSI controller turns CreateVolume's FailedPrecondition into ResourceExhausted, ignores
  `csi.storage.k8s.io/` parameters and returns DeleteVolume's errors, so the provisioner retries
  a refusal forever (`control/csi/controller.go:248–249,157–161,265–267`). The one Kubernetes
  client is hand-written HTTP (`control/failover/kube.go:21–88`), already used by `control/csi`
  (`control/csi/topology.go:35`); a non-2xx answer is an untyped error (`kube.go:81–83`).
- `serve --insecure` turns off TLS and identities (`meta/cmd/flaminio-meta/main.go:97,112–130`).
  The lab's etcd has no space quota or compaction flag (`scripts/guest/etcd.sh:45–52`). Tests
  run on both stores (`backends`) and inject writes (`interleaveKV`, `service_test.go:47,619`).

## Activation and roles (S1, S11)

- **Authorization record** (`meta/service/roles.go`): `ValidateAuthorization(rec, caller,
  trustDomain)` applies the contracts' checks. `Service.active(ctx)` reads both activation
  records until it finds one, then caches true; Attach, the insecure rule and the race's retry
  loop use it. Roles
  are not cached: Authorize reads both records for each call that needs a role, so every
  instance sees a change at the caller's next call.
- **Authorize, in order:** the open reads pass. Insecure: the new methods are refused; other
  calls, Attach included, pass while `active` is false, marked `inactiveRules`, and are refused
  once it is true. Identity as today; a node, agent or target rule that allows the call passes,
  and an identity that is not an administrator is refused, both with no store read, Attach
  included. An administrator's Attach: while `active` is false, today's rule; once true, the CSI
  controller's ID, else PermissionDenied; nothing else is read. Otherwise read both records:
  one without the other, Internal; neither, existing methods, `TenantStatus` and
  `ActivateTenancy` take the administrator-name rule, marked `inactiveRules`, and other new
  methods are refused; both, the roles the record binds to the caller's ID that the method table
  allows go into the context, or PermissionDenied. A handler that needs a role and finds none in
  its context refuses, active or not (a test calling it directly).
- **ActivateTenancy** compares both records and `cfg`'s tail absent, and writes the record,
  with `changed_by`, the allocation at zero, and an `authorization.binding` event per binding
  (and `authorization.receiver` for a key) as `cfg`'s first events; present, AlreadyExists.
  **PutAuthorization** compares `expected_revision` and `cfg`'s tail, and writes the record and
  its events, under the recovery overdraft when it grants nothing (contracts, Budget). Both
  refuse a record that leaves no `cluster-admin` or binds `csi-controller` to another ID than the
  CSI controller's. **Start:**
  `serve` exits 1 only when insecure on an active store; incomplete records are logged, and
  served as the contracts say.
- **The race:** a call marked `inactiveRules` compares the record absent in each transaction that
  would carry an event once active: DeleteVolume's mark and header deletion, an administrator's
  ConfirmDeletion. Its retry loop checks `active` before each attempt and answers Aborted once
  true. Creations, moves, drains and attaches in that window are the exception S11 states.
- **Upgrade order** (runbook): every instance on this binary; `tenant activate` with every binding,
  the caller's own as `security-admin`; a policy per tenant namespace, with `--audit-events` and
  `--allow-unencrypted` (until #50 every volume is unencrypted); the CSI flag and manifests; legacy
  assignments. Until the flag, tenant creations are refused; until assigned, legacy deletions are.
  `tenant authorization-put` revokes a binding at the next call on every instance. The CSI
  controller's certificate must carry its fixed ID; from activation on, manual and system-volume
  attaches need that certificate (contracts, "The cost").

## Owner, policy and quota (S2, S3, S4)

- **Policies** (`meta/service/tenant.go`): PutTenantPolicy writes `policies/<ns>` under
  `expected_revision`, updates `allocation` (a new policy takes the next `chains` for its chain
  name), and appends `policy.put` to `cfg`; it reads the namespace's chain tail for the
  allowance's floor and compares the policy, the allocation, both tails. GetTenantPolicy adds
  usage from a List of `quota/<ns>/`. DeleteTenantPolicy refuses while that List is not empty or
  the chain has events present, reservations or a sweep; it compares every policy class's quota
  key absent, the chain's tail, its sweep key, the allocation and `cfg`'s tail, deletes the policy
  and the chain's tail, frees the allowance and slot, and appends `policy.deleted`.
  `cluster-admin` is the one writer: there is no policy controller in this step.
- **CreateVolume once active, with an owner:** UIDs and `id == "pvc-" + pvc_uid` first. Then
  `owners/<id>` and the header: a headerless owner record with a `deleting_op` or of the
  request's own owner (the same UIDs; `system` for a system creation) is released as
  DeleteVolume's no-header branch does, before the quota check, and the call answers Aborted so
  the retry claims a free id; any other headerless record is FailedPrecondition `orphan`. Then
  one Get each of the policy and `quota/<ns>/default` (absent is zero), refused as the contracts say
  (none, RF=1, unencrypted, no limit for `default`, over a limit or an overflow) before the
  tombstone check and placement, which then run once, as today. The claim also compares
  `owners/<id>` absent and the policy's and quota's revisions read, and writes the owner record
  (`class` `default`, `bytes` the size, `protected` from `required_protection`) and the usage.
- **A failed claim** reads the header, the tombstone and the owner record. A header: a marked
  deletion is finished as below, else the owner record must be the request's (the same UIDs, or
  `system`) for AlreadyExists, or PermissionDenied `wrong-owner`. A tombstone: AlreadyExists, as
  today. An owner record alone (another path removed the header meanwhile): the check above,
  again. None: the policy, quota or owner
  key changed; re-read and re-check them, keep the placement, claim again; after `retries`
  attempts with `backoff`, Aborted. Of two creators racing for the last bytes, the second's
  re-read refuses it.
- **Without an owner,** active: an id of the tenant form is refused; the same headerless check;
  the claim writes `{system: true}`, no charge. Inactive: today's claim, no owner record; an
  owner is refused.
- **Growth (#70's `ExpandVolume`):** if #73 lands second, each step reads the owner record, the
  policy and `quota/<ns>/<class>`, and its transaction compares them and the authorization
  record and adds the step's bytes to the usage and the owner record's `bytes` (contracts,
  "Growth"); #70's step 3 and "Charges" fix the checks and their order, and its
  `TestExpandChargesQuota` the test, with the deletion releasing the grown size. A growth
  changes the owner record's revision: an override naming the old one is Aborted and reads
  again.
- **CSI controller:** `Controller.Owners OwnerLookup` (nil: off), `Owner(ctx, namespace, pvc)
  (nsUID, pvcUID string, err error)`; `failover.APIClient.Owner` checks both names, reads the
  namespace and then the PVC, and returns `*APIError` from `do` for a non-2xx answer;
  `flaminio-csi controller --tenant-owners` sets it from `failover.InCluster()`. CreateVolume
  checks the parameters and the name, calls `Owner`, compares the PVC UID with the name, and
  sends the owner.

## Deletion, protection and override (S5, S6)

- **Chain of a deletion:** DeleteVolume of a tenant volume uses its policy's chain; every other
  deletion its volume's `sys-` chain. The mark records it in `deleting_chain`; a tenant mark also
  compares the policy's revision, whose `max_audit_events` is the chain's cap.
- **`finishDeletion(ctx, id, op, outcome)`** (`service.go`): lists the chunks, reads the tail
  and, for a tenant, the quota. One transaction compares the header and the owner record at the
  revisions given (owner absent for `marked-unassigned`), the tail and the quota; deletes the
  header, writes the tombstone, deletes the owner record, lowers the usage (deleting the record
  at zero; below the charge is Internal) and appends `delete.committed`. Under `inactiveRules`:
  today's transaction, with the race compare. A failed compare re-enters the caller's loop;
  chunk records then go as today (`service.go:574–578`).
- **DeleteVolume** (inactive: today's, with the race compare), active, in its loop, gets the
  header and `owners/<id>`:
  - No header at the loop's first read, or at a later read after another deleter or path removed
    it: today's chunk cleanup. An owner record left by another
    path (an older binary, or #76's rollback outside DeleteVolume) is deleted by the role of its
    kind in one transaction comparing the header absent, with its charge released and
    `delete.committed` `header-missing`. Then NotFound, as today.
  - No header after this call's own #76 rollback (below): the same release, then OK.
  - Header marked and no owner record, or an unmarked header being created (contracts, Records)
    with no owner record (marked first, with no event): `finishDeletion`, `marked-unassigned`, a
    fresh operation id, `sys-` chain, by either deleting role.
  - An owner record with a `deleting_op`: `finishDeletion` with it, by the role of its kind.
  - Otherwise, a mark an older binary left included: no owner record, `unassigned`; the other
    kind's role, `wrong-kind`; `protected`, refused unless the header is being created. Then a
    header that parses is marked (compares header, owner record, tail; writes
    the marked header, `deleting_op`, `deleting_chain` and `delete.admitted`) and finished; one
    that does not parse is deleted in one transaction with both events.
- **#76:** for a header being created, #76's rollback replaces `finishDeletion`'s transaction,
  after this plan's checks and its mark, so `delete.admitted` is durable and the slot reserved.
  The rollback stays delete-only and touches neither the owner record nor the quota. DeleteVolume
  then loops in the same call: no header, an owner record with `deleting_op`, so it releases it
  with `delete.committed` `header-missing` in `deleting_chain` (always admitted), and answers
  success only after that; a failed release is an error, and the provisioner retries. The mark
  is a write: under etcd's space alarm this path waits for space. "Being created" is #76's own
  test. Until #76 lands, `finishDeletion` deletes such a header itself. #76's rollbacks outside
  DeleteVolume (its recovery of a creating and deleting header, its legacy rollback) may leave a
  headerless owner record; DeleteVolume's pass and, with a `deleting_op` or of the creation's own
  owner, a creation of that id release it; only another owner's, without one, is an orphan.
- **createVolume** finishes only a header already marked (no owner record, or one with a
  `deleting_op`), with `finishDeletion` directly, so its role is not matched to the owner's kind.
  The event names the creator under the deletion's operation id, which links it to the
  `delete.admitted` event naming who decided.
- **OverrideDelete:** no owner record: `unassigned` if the header exists, else NotFound. An owner
  record with a `deleting_op`: `finishDeletion` with it, whatever the revision, so a retry after
  a lost reply finishes. Another revision: Aborted. No header (an older binary deleted it): one
  transaction deletes the owner record, releases the charge and appends `delete.committed`,
  `header-missing`, in `deleting_chain` if set. Otherwise the mark, outcome `override` with the
  reason, then `finishDeletion`. It answers the operation id.
- **AssignOwner:** the header exists and is not deleting; `owners/<id>` is absent. `system` takes
  any such header, one that does not parse or is being created included, except a parsed one
  with a tenant-form id. A tenant needs a
  parsed, published header, the `pvc-` id, a policy and a limit for `default`; the charge is
  checked for overflow, not the limit; protection is the policy's. One transaction compares all
  it read and writes the owner record, the usage and `owner.assigned`.

## Audit chains (S7, S8, S9, S10)

- **`meta/service/audit.go`:** `chainOf`, `event` (the contracts' fields in order, through a
  `json.Encoder` with HTML escaping off), and `appendEvents(tail, cap, events)`, which applies the
  kinds' budget rule and returns the tail's revision compare and the writes; with no events it
  returns only the tail compare, and no tail write, so a PutAuthorization that changes nothing
  leaves `audit/tail/cfg` unwritten. Every audited transaction is built with it; a lost compare
  retries in the method's loop (`retries = 32`, `service.go:45`). Actor, role, serial and peer come
  from the context as `logConfirmation` reads them. With the ID and reason bounds an event stays
  under 2048 bytes; `appendEvents` checks it (Internal otherwise).
- **ConfirmDeletion** by `cluster-admin`, for a node not yet confirmed, also carries
  `confirm.forced`; `logConfirmation` stays.
- **ListAudit:** point reads from the contracts' start up to `next − 1`, at most `limit`; it
  reads the chain's sweep key to skip a pending sweep. No ranged List, so `kv.KV` is unchanged.
- **PruneAudit,** in order: `security-admin` on a chain other than `cfg`, PermissionDenied before
  any read; finish a pending sweep, which needs no key; `through <= pruned` answers `pruned` (a
  replay, a lost reply, or a retry after a crash mid-sweep, by either role); `security-admin` on a
  `cfg` that is not full (`present < cap`), FailedPrecondition `not-full`; the record's receiver
  key, else FailedPrecondition; `through >= next`, OutOfRange; the chain hash of event `through`,
  from its `prev` and bytes, must equal `chain_hash`; the receipt must verify with that key. One
  transaction compares the tails it writes, the authorization record's revision and the sweep key
  absent, appends `audit.pruned` (to the chain, or for an `ns-` chain to its namespace's `sys-`
  chain) and writes the sweep `{from: pruned + 1, through}`. The sweep deletes event keys in batches
  of at most 118, each advancing `from` under the sweep key's revision; the last deletes the sweep
  key and sets `pruned = through` in the chain's tail, compared at the tail's revision (an append
  meanwhile makes it read and retry): at most 120 operations, so the swept events count until they
  are gone. `security-admin` may prune `cfg` as `cluster-admin` may any chain.
- **Refusals:** `meta/service/refuse.go` `refuse(ctx, method, reason, err)` logs as the
  contracts' refusal log says and returns `err`. No refusal touches the store.
- **TenantStatus:** Lists `volumes/`, `owners/` and `audit/tail/`, and reads both activation
  records.
- **Configuration events:** activation and PutAuthorization diff the old and new records into
  one event per binding and one for the key; PutTenantPolicy and DeleteTenantPolicy append one.
  Each also logs the full new record.

## Files touched

- `meta/service/auth.go`: the order and the method table (S1, S11); `roles.go`, new: the record's
  checks (S1).
- `meta/service/tenant.go`, new: activation, authorization, allocation, policies, owners,
  AssignOwner, OverrideDelete, GetVolumeOwner, TenantStatus, identifier checks (S2–S6, S10, S11).
- `meta/service/service.go`: the claim, DeleteVolume, `finishDeletion`, ConfirmDeletion (S3–S7).
- `meta/service/expand.go` (#70's): the growth charge, if #73 lands second (S4).
- `meta/service/audit.go`, `refuse.go`, new: chains, budget, reads, pruning, refusals (S7–S10).
- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen` (`make proto`): the new messages.
- `meta/cmd/flaminio-meta/main.go`: the start check, `tenant`, `audit` (S1, S8, S11).
- `control/csi/controller.go`, `control/failover/kube.go`, `control/cmd/flaminio-csi/main.go`:
  the owner lookup, `APIError` and `--tenant-owners` (S3).
- No Rust change: `flaminio-metaclient/src/pb.rs` holds none of the changed messages, and the
  data plane's calls keep their node rules. No manifest, script or on-disk format change.

## Contracts

[contracts.md](contracts.md), every one: identifiers, methods and roles, the authorization
record, records, chains, events, budget, protobuf, errors, refusal log, CLI, CSI controller.

## Tests

| Rules | Concrete proof |
|---|---|
| S1 | `meta/service/auth_test.go` `TestRoleTable`: every method by each role, a node, an agent, a data plane, an unbound administrator, over mTLS and with no identity on an insecure service, inactive and active; the CSI controller's, failover's and drain's calls with only their roles; two services on one store: a binding removed by one is refused by both at their next call, and a call authorized before the removal finishes; two services racing ActivateTenancy: one record, one AlreadyExists; PutAuthorization stale, dropping the caller's own `security-admin`, and during a prune (the prune retries); Attach once active by `audit-reader`, `deletion-override`, `cluster-admin` and an unbound administrator refused, by the CSI controller and an agent committing; an insecure instance across activation refusing Attach; one record missing, at start and at runtime: role-gated calls answer Internal while Attach, ChangeIsr and NodeHeartbeat commit; a counting store shows `active` read until found and nothing read on Attach after, and no read for a node identity's refused calls, Attach included; `system-provisioner`'s Attach refused once active; a grant, its use and its revocation, and a protection flip-flop, each leaving its events in `cfg`. `roles_test.go`: every validation error, a repeated ID, a non-canonical ID, no `cluster-admin`, `csi-controller` bound to `admin/csi`, a two-step handover. |
| S2–S4 | `meta/service/tenant_test.go` on `backends`: forbidden RF=1, unencrypted, class, missing policy, each with no chunk List and a message naming only its namespace; quota at the limit and one byte over; overflow; `interleaveKV` races (two creators, a policy change before the claim); lost reply and retry; a retry against another owner's volume; an orphan owner record; a stand-in for #76's legacy rollback of an unmarked creating header, then the same PVC's creation, also at the namespace's limit: released before the quota check, Aborted, then claimed; `TestExpandChargesQuota` (#70's, by whichever lands second): a step charged to the usage and the owner record's `bytes`, over the limit refused with earlier steps kept, deletion releasing the grown size; delete then recreate; a dead node's tombstone with the charge released; a policy past either allocation limit, an allowance below its chain's use, policy deletion refused with chain events, then, once the chain is pruned, allowed with its allowance and slot freed and `policy.deleted` in `cfg`; 4097 namespaces over time each getting a policy. `control/csi/csi_test.go`: lookup refusals with a fake `OwnerLookup`; `control/failover/failover_test.go`: `Owner` against `httptest`: 404, 500, malformed names, a namespace replaced between the reads, `errors.As` through a wrapper with today's text kept. |
| S5–S6 | `tenant_test.go` on `backends`: protected and unassigned deletions by each role, a header that does not parse, an older binary's mark on a protected volume; a protected creation failed after its claim, then deleted by `csi-controller` with its charge released; a protected creating header with a nonzero epoch, refused; a stand-in for #76's rollback deleting the header inside DeleteVolume after the mark: the release in the same call, before the answer; a release that fails, then the retry; an older binary's header deletion, and a stand-in for #76's rollback outside DeleteVolume, each then released by the next DeleteVolume and, with a `deleting_op`, by a creation of that id (Aborted, then the claim); override stale, against a replacement, a failure after each transaction then a retry; createVolume finishing a marked deletion. |
| S7–S9 | `meta/service/audit_test.go` on `backends`: a failing store at each transaction and a lost reply; altered, missing and reordered events caught; prunes stale, replayed, forged, beyond the tail, with a wrong hash, with no key, with a receipt by a replaced key, crashed mid-sweep and retried after the key was removed; `audit.pruned` holds the receipt; `header-missing` after an admitted deletion releases its slot; one namespace's chain full while another namespace deletes; `cfg` full with no receiver key: grants (a revoke-and-grant and an ID replacement included) and policy writes refused, a revocation and a key change admitted in the overdraft, one admitted at exactly `cap + 1024` and the next refused; with no `audit-reader` bound, a sole `cluster-admin` flooding `cfg` with policy writes, then `security-admin`, with its own calls only, setting a key, reading `ChainStatus.hash`, pruning `cfg`, binding a new `cluster-admin` and revoking the old; `security-admin` pruning a `cfg` that is not full (`not-full`), a `sys-` and an `ns-` chain (PermissionDenied, nothing read beyond Authorize's two records); a key removal on a full `cfg` refused; the overdraft spent by key changes: the next key change refused; a prune with the last key admitted; a key change admitted after it; the lost-key repair: the overdraft spent by key changes, the record rewritten through the store with a new key and every other field kept, a prune with the new key admitted and its `audit.pruned` naming the new fingerprint, a PutAuthorization at the revision from before the repair refused as stale; the repair's txn, from reads at `cap + 1024`, refused by one compare alone in each of three cases: a prune of `cfg` by a key holder ending before the txn (the tail compare; the record's revision unchanged and the sweep key absent), a no-op PutAuthorization after the reads (the record compare; the tail's revision unchanged and the sweep key absent), and reads taken while a sweep is pending (the sweep-absence compare; the tail's and the record's revisions unchanged); `security-admin` retrying its prune after a lost reply and after a crash mid-sweep, each answered `pruned`; revoking the sole `cluster-admin` refused; an `ns-` prune refused while its `sys-` chain is full, then admitted after that chain is pruned; on the etcd store, prunes of exactly 118 × n and 119 × n events and an append racing the last sweep transaction; every `sys-` chain full: Attach and ChangeIsr commit; a sweep's events counted until it ends, and an `ns-` chain's prune event written to its `sys-` chain; `refuse_test.go`: one identity's flood leaves another's first refusal logged, a burst then silence logs its count at the second's end, 1025 callers share `others`, no store write; a captured handler shows each configuration write's line with the full record. |
| S10 | `tenant_test.go`: identifier and reason tables; `audit_test.go`: only the listed fields, the top `after_sequence`; ListAudit by the wrong roles. |
| S11 | `tenant_test.go`: activation; `interleaveKV` activating between a deletion's read and its mark, and between a creation's authorization and its claim (unassigned, uncharged, reported, deletable only after assignment); an old-format header write keeps the owner record; legacy attach, refused deletion, assignment, deletion; a legacy header that does not parse, assigned `system`, deleted; AssignOwner admitting a tenant owner for a `pvc-` id whose PVC is gone, into a namespace whose `default` allows 0 volumes, and a creation there refused (the evidence order is a runbook step the service cannot check), then the volume deleted; `system` refused for a parsed `pvc-` volume; TenantStatus counts and id lists; an orphan owner reported, then removed by the override. `meta/cmd/flaminio-meta/main_test.go`: the start refusals; `tenant authorization-put`; `tenant activate` against an insecure instance and a stand-in answering Unimplemented, and PermissionDenied for an unbound caller. |

`make test race lint hygiene contracts`. No `flaminio-repl` change and no replication,
membership or recovery rule: no simulator check is added. No result is claimed.

## Lab

None: this step runs nothing on the lab and deletes or reformats nothing; the lab stays
inactive and behaves as today. Lab activation is a follow-up.

## Docs

`docs/ARCHITECTURE.md` §15.3: what this step provides and leaves open. `docs/followups.md`: a row
for the rest of #73 (intent, "This step"). `flaminio-meta`'s usage comment: `tenant`; the upgrade
order, with the evidence order for a legacy owner's assignment and the dedicated holding namespace
(spec Q3), a runbook step the service cannot check; `audit`, with `security-admin`'s duty to keep
the private half of the last key it sets in the overdraft; etcd space; the record's repairs, the
lost receiver key's included, with its `etcdctl get -w json` reads at one revision, its precondition
on that tail, its conditional `etcdctl txn` on those `mod_revision`s, and the operator log where the
operator writes who repaired, when and the new key's fingerprint (contracts, "Activation and the
authorization record"); the legacy-holding namespace's policy, with no PVC rights and its UID in
that log (spec Q3). `docs/lab.md`, `docs/performance-plan.md`, `README.md`, benchmarks: no change.

## Risks

- **Safety:** no epoch, fence, token or ISR rule changes. Attach and ChangeIsr read no tenant or
  authorization record and write no event, so a full chain, a bad record or a receiver outage
  cannot block failover. A deletion still marks before listing chunks and tombstones with the
  header's deletion.
- **etcd space:** the audit records are bounded (contracts, Budget): about 1.34 GiB of keys and
  values, pending sweeps included, and 4096 policies at most. etcd's own overhead comes on top,
  and pruned events free space only after compaction and defragmentation. The runbook requires
  an etcd quota at least 3 GiB above current use, automatic compaction, and defragmentation after
  prunes; that margin is a deployment requirement, not something the service checks.
- **Security,** who must not: a tenant, anything, nor fill another namespace's or a `sys-` chain; a
  node identity, more than its node rules; the audit reader, any write; the override, more than one
  assigned volume per call; `cluster-admin`, forge a receipt, change a binding or the receiver key,
  or freeze configuration: setting or replacing the receiver key and a revocation are admitted
  within the overdraft (the sole `cluster-admin`'s after binding a replacement), a key removal only
  as an ordinary write, and `security-admin` can prune a full `cfg`, with the hash from
  TenantStatus, while it holds the private half of the record's key (contracts, Budget); no write
  leaves no `cluster-admin`. With the last key's private half lost after the overdraft is spent, no
  chain can be pruned and `cfg` refuses every write, until an etcd-level write of the authorization
  record's `receiver_key` (runbook). The self-prune residual (contracts) is a change after approval;
  Settled 2026-11-13 (user's instruction): accepted (see decisions.md). Once active, Attach is the
  CSI controller's and the agents' only: the override, the reader, `cluster-admin`,
  `system-provisioner` and revoked administrators are refused; a revoked CSI binding keeps Attach
  until its certificate expires (#16). The cost, manual and system-volume attaches needing the CSI
  controller's certificate, is a change after approval (contracts); Settled 2026-11-13 (user's
  instruction): accepted (see decisions.md). Configuration changes are events in `cfg`, and the full
  records are logged. Open (Q3): a `security-admin` who can get certificates issued (#16) can bind
  an identity of its own to any role; before activation every administrator name is trusted and the
  first activation wins; the failover pod holds `csi-controller`; a compromised `csi-controller` can
  state any owner for a `pvc-<UID>` id. etcd or root can rewrite unsigned history. Settled at
  approval, 2026-11-13: as proposed (see decisions.md, Settled at approval).
- **Compatibility:** the proto change is additive; before activation nothing changes. After it,
  an older binary is unsupported and unstoppable (Q4): its deletions skip protection, events and
  release; TenantStatus reports what it leaves, and its marks are checked again.
- **Availability:** creations need a policy, deletions audit room, pruning a receiver this step does
  not provide (Q1, Q2). Settled at approval, 2026-11-13: Q1, Q2, Q4 and Q3 as proposed; Settled
  2026-11-13 (user's instruction), restated after approval in rounds 7 to 10: for a legacy `pvc-`
  volume, the owner is the PVC whose UID the volume id carries: find the PVC by that UID, read its
  namespace's UID, read the PVC again by namespace and name, and require the id's UID. With that PVC
  gone, the owner is a dedicated namespace created to hold legacy volumes, never a tenant's
  namespace and never one taken from a PV's `claimRef`, and the PVC UID the id carries. Its policy
  has a `default` limit of 0 volumes, so every creation there is refused while an assignment is
  charged past the limit, and a `max_audit_events` of at least twice the volumes it will hold; no
  one is granted PVC rights there, and its UID is in the runbook's operator log. It costs its chain
  two events per volume, which stay allocated while no receiver prunes them. DeleteVolume or the
  override then deletes it. `system` stays for ids not of the tenant form and headers that do not
  parse (see decisions.md).
- **Not audited:** MoveReplica and DrainNode: a move drops its source only once the destination
  is in the ISR (`docs/replication-protocol.md` §7, Moves), so no copy is lost.

## ADRs

- **0002:** consistent; `kv.KV` is unchanged, audit reads are point reads. **0001:** consistent;
  no new dependency. **0005, 0006:** untouched.
- **Accepted with the plan, 2026-11-13:** [adr-proposal.md](adr-proposal.md): durable
  activation by records in the store, roles bound to exact identities, a hash-chained audit of
  destructive operations with per-namespace chains, pruned only against a receiver's signature.

## Parallelizable?

Yes. No Rust unit. The CSI unit (`control/csi`, `control/failover`, `flaminio-csi`) builds against
`CreateVolumeRequest.owner` and the contracts' CSI refusals; the metadata unit (`meta/service`,
`flaminio-meta`, the proto) against the rest. Neither needs the other's code.
