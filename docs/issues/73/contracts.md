# Issue #73: contracts — accepted with the plan, 2026-11-13

Normative appendix to the [design](design.md): the one place for every format, rule table and
code below; the spec and design defer to it. Everything here is Accepted with the plan,
2026-11-13, and changed after approval in review rounds 3 to 11 (decisions.md, Approvals).
Protobuf notation: `name:tag:type`; a response not listed is an empty message.

## Identifiers

Checked before any key is built, any record read or any URL built; a failure is
`InvalidArgument`.

| Value | Format |
|---|---|
| volume id, node id, device class | `domain.ValidID`: `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`; the class is `default` in this step |
| namespace UID, PVC UID | `^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$` |
| tenant volume id | `pvc-` followed by its PVC UID |
| Kubernetes namespace, PVC name | a DNS-1123 label (at most 63 bytes), a DNS-1123 subdomain (at most 253 bytes) |
| operation id | 32 lowercase hex digits; drawn by the service (128 random bits), never by a caller |
| reason | 1–256 bytes, each 0x20–0x7e |
| chain | `sys-00` to `sys-15`; `cfg`; `ns-<namespace UID>-<k>`, `k` the canonical decimal of a u64 of at least 1 (no leading zero) |
| chain hash, receipt, receiver key | exactly 32, 64 and 32 bytes |
| bound SPIFFE ID | an administrator ID, at most 255 bytes, equal to `pki.ParseID(id).String()` |
| CSI controller's ID | `spiffe://<trust domain>/flaminio/admin/csi-controller`, as the lab issues it (`scripts/lab.sh:1004`) |

## Methods and roles

Roles: `csi-controller`, `system-provisioner`, `cluster-admin`, `deletion-override`,
`audit-reader`, `security-admin`. "Administrator name": today's rule, any identity with an Admin
SAN (`meta/service/auth.go:57`). Node, agent and target rules are today's in both columns.

| Method | Before activation | Once active | Event |
|---|---|---|---|
| `GetVolume`, `ListNodes`, `ListVolumes` | any identity | any identity | – |
| `Attach` | administrator name; the agent with `only_if_target` | the CSI controller's ID, by name; the agent with `only_if_target` | – |
| `RegisterNode`, `NodeHeartbeat` | the node; administrator name | the node; `cluster-admin` | – |
| `ListTombstones` | the agent for itself; administrator name | the agent for itself; `cluster-admin` | – |
| `ConfirmDeletion` | the agent for itself; administrator name | the agent for itself; `cluster-admin` | `confirm.forced`: `cluster-admin`, a node newly confirmed |
| `ChangeIsr` | the target's data plane | the target's data plane | – |
| `CreateVolume` | administrator name; an owner is refused | with an owner `csi-controller`; without, `system-provisioner` | – |
| `DeleteVolume` | administrator name | `csi-controller` or `system-provisioner`; the method requires the one of the owner's kind | `delete.admitted`, `delete.committed` |
| `DrainNode`, `MoveReplica` | administrator name | `cluster-admin` | – |
| `TenantStatus` | administrator name | `cluster-admin`, `security-admin`, `audit-reader` | – |
| `ActivateTenancy` | administrator name | `security-admin`: `AlreadyExists`, nothing written | `authorization.binding`, `authorization.receiver` |
| `PutAuthorization` | refused | `security-admin` | `authorization.binding`, `authorization.receiver` |
| `PutTenantPolicy`, `DeleteTenantPolicy` | refused | `cluster-admin` | `policy.put`, `policy.deleted` |
| `GetTenantPolicy` | refused | `cluster-admin` | – |
| `GetVolumeOwner` | refused | `cluster-admin`, `deletion-override` | – |
| `AssignOwner` | refused | `cluster-admin` | `owner.assigned` |
| `OverrideDelete` | refused | `deletion-override` | `delete.admitted`, `delete.committed` |
| `ListAudit` | refused | `audit-reader` | – |
| `PruneAudit` | refused | `cluster-admin`; for a full `cfg`, also `security-admin` | `audit.pruned` |
| `ExpandVolume` (#70's) | administrator name | `csi-controller` or `system-provisioner`; the method requires the one of the owner's kind; an unassigned volume is refused | – |

- **Roles come from the store:** once active, a caller's roles are those the authorization
  record binds to its exact SPIFFE ID, read when Authorize runs for a call that needs one. No
  instance holds roles of its own, so every instance answers alike; a binding removed is refused
  from the caller's next call (a call already authorized finishes).
- **Attach is decided by the certificate:** once active, only the CSI controller's ID and the
  agent's own restart; every other administrator, `cluster-admin`, `system-provisioner`,
  `deletion-override` and `audit-reader` included, is refused. A non-administrator is decided by
  the agent rule with no read; an administrator's Attach reads the activation state until the
  instance has seen it active, then nothing. A deployment must name its CSI controller's
  certificate with that ID, and the record may bind `csi-controller` to no other ID.
- **The cost, a change after approval; Settled 2026-11-13 (user's instruction): accepted (see
  decisions.md):** today's administrators lose Attach at
  activation, so manual attaches and attaches of system volumes need the CSI controller's
  certificate, which also carries `csi-controller` (the lab's `scripts/lab.sh:624,629,715,824`
  among them); any certificate the CA issues for that ID attaches, and a revoked CSI binding
  does not stop its Attach; its certificate's expiry does (#16).
- **No read for the rest:** the open reads, and calls a node, agent or target rule allows, read
  no tenant or authorization record. An identity that is not an administrator, and that no such
  rule allows, is refused before any store read.
- **Insecure instances** (`--insecure`) apply their rule first: they refuse every new method in
  both columns, and once active (cached as for Attach) every call but the open reads, Attach,
  ChangeIsr, heartbeats and registrations included.
- **Today's callers once active:** the CSI controller `csi-controller` and Attach by name;
  `flaminio-meta drain` `cluster-admin`; node agents and data planes their own rules. The
  failover controller calls only `ListNodes` (`control/cmd/flaminio-csi/main.go:103–110`) but
  mounts the CSI controller's certificate (`deploy/csi/driver.yaml:208,220`), so it holds
  `csi-controller` and Attach; a certificate of its own, bound to no role, is a follow-up.

## Activation and the authorization record

- **Active:** `/flaminio/v1/tenant/authorization` or `/flaminio/v1/tenant/allocation` exists.
  `ActivateTenancy` writes both, the allocation at zero; nothing in the service deletes either.
- **Incomplete:** one without the other is `Internal` for every role-gated call on every
  instance. Attach, the open reads and the node, agent and target rules keep serving, and
  `serve` starts, logging the state. Both absent is an inactive store, as after a restore from
  before activation; an etcd-level deletion is outside the threat model. Repair (runbook): an
  etcd-level write restoring the missing record, the authorization record from its last logged
  value, the allocation's `events` and `policies` summed from the policies and its `chains` set
  to the store's current revision, which is above every `k` ever used; both absent, `tenant
  activate` again.
- **Record:** `{"schema":1,"bindings":[{"id":"<SPIFFE ID>","roles":["<role>",…]},…],
  "receiver_key":"<64 hex digits>","changed_by":{…}}`; `receiver_key` may be absent;
  `changed_by` is written by the service. `PutAuthorization` replaces it under
  `expected_revision`.
- **Checks,** `InvalidArgument` naming the entry: an unknown field or role; an empty or repeated
  role; a repeated ID; an ID that is not a bound SPIFFE ID (Identifiers) or is outside the
  instance's trust domain; more than 64 bindings; a write that changes more than 64 bindings;
  `deletion-override`, `audit-reader` or `security-admin` with any other role;
  `csi-controller` bound to any ID but the CSI controller's; no `cluster-admin` binding; a
  receiver key that is not 32 bytes. `ActivateTenancy` needs the caller's own ID bound to
  `security-admin` in the record it writes, and `PutAuthorization` a record that keeps it so: a
  handover takes two writes, as does replacing the last `cluster-admin`.
- **Recovery write:** a `PutAuthorization` that grants no ID a role it lacked: it only removes
  roles or bindings, or sets or replaces the receiver key. A write that removes one role and
  grants another, or replaces an ID, is a grant; one that removes the key is neither, and is
  refused on a full `cfg`.
- **Digest and fingerprint:** SHA-256 of the record's stored bytes; SHA-256 of the key's 32
  bytes.
- **A record that does not parse** fails every role-gated call, its own replacement included; the
  operator restores a valid one with an etcd-level write (runbook).
- **A lost receiver key:** with the private half of the record's key lost after the `cfg` overdraft
  is spent, the operator first finishes any pending `cfg` sweep (a PruneAudit of `cfg` through its
  `pruned`, which answers `pruned` and needs no key, with any 32-byte hash and 64-byte receipt, e.g.
  zeros). It then reads `audit/tail/cfg`, `audit/sweep/cfg` and the authorization record with
  `etcdctl get -w json`, the later reads at the first response's header revision (`--rev`);
  requires, on that tail value, `next − 1 − pruned` = `cap + 1024`, and no `audit/sweep/cfg`, so no
  PutAuthorization can set a key (one that changes nothing still commits, with no event); and builds
  the new record from that record value, with a new `receiver_key` and every other field kept. It
  writes it with an `etcdctl txn` that compares the record's and the tail's `mod_revision`s from
  those reads and `audit/sweep/cfg` absent: a prune ending, a write of the record, or a sweep
  starting after the reads makes it fail. `cfg`'s `ChainStatus` is a cross-check only. `changed_by`
  keeps the service's last write and does not attribute the new key; the operator writes who made
  the repair, when, and the new key's fingerprint in the runbook's operator log. The write leaves no
  event: a verifier finds the repair as a fingerprint that no `authorization.receiver` event or
  logged record sets. The next prune's `audit.pruned` names the new key's fingerprint (runbook).

## `flaminio-meta serve`

No new flag. At start, exit 1 on an active store with `--insecure`: `tenancy is active in this
store: --insecure is refused`.

## Records

Under `/flaminio/v1/tenant/`. Values are JSON with `"schema":1`. A record that does not parse,
or has another schema, fails the method with `Internal`, and nothing is written.

| Key | Value |
|---|---|
| `authorization` | above |
| `allocation` | `{schema, events, policies, chains}`: the sum of every policy's `max_audit_events`, their number, and the count of namespace chains ever made |
| `policies/<namespace UID>` | `{schema, allow_rf1, allow_unencrypted, required_protection, max_audit_events, classes, chain, changed_by}`; `max_audit_events` 2–65536; `classes` maps a device class to `{max_bytes, max_volumes}`, at most 4, either limit may be 0; `chain` is set at the policy's creation |
| `quota/<namespace UID>/<class>` | `{schema, bytes, volumes}`; deleted when both reach 0 |
| `owners/<volume id>` | `{schema, system, namespace_uid, pvc_uid, class, bytes, protected, deleting_op, deleting_chain}`; a system owner has empty UIDs and class, 0 bytes, not protected |
| `audit/tail/<chain>` | `{schema, next, hash, reserved, pruned}` |
| `audit/events/<chain>/<sequence>` | one event; never rewritten |
| `audit/sweep/<chain>` | `{schema, from, through}`; present only while a prune deletes events |

- `<sequence>`: 20 decimal digits, from 1. An owner record's revision is the store revision of
  its last write. `deleting_op` and `deleting_chain` are set by an admitted deletion's mark.
  `bytes` is the volume's current charge: a tenant's size at its claim or assignment plus every
  growth step's; 0 for `system`.
- `changed_by`: `{actor, role, cert_serial, peer, time}` of the service's last write of the record,
  as in an event; `role` is `admin-name` for `ActivateTenancy`, which no role guards.
- **Being created:** a header that parses with `creating` set, `epoch` and `first_epoch` 0 and
  no `target`, which is #76's unpublished test. Any other header, one that does not parse
  included, counts as published.

**Configuration writes.** `ActivateTenancy` and `PutAuthorization` write the authorization
record (and, for activation, the allocation); `PutTenantPolicy` writes a policy and the
allocation; `DeleteTenantPolicy` deletes a policy and its chain's tail and rewrites the
allocation. Each sets `changed_by` on the records it writes, appends its events to `cfg` in the
same transaction, and logs one `slog.Info` line with the method, the caller and the full new
record (none holds a secret), or the deleted policy.

**Event:** at most 2048 bytes, these fields in this order: `schema, chain, sequence, prev, time,
kind, operation, actor, role, cert_serial, peer, method, resource, owner, outcome, reason`.
- `prev`: the previous chain hash in hex; 64 zeros for sequence 1. An event's chain hash is
  SHA-256(previous chain hash ‖ the event's stored bytes).
- `actor`: the SPIFFE ID. `role`: the role the call used (`admin-name` for `ActivateTenancy`).
  `cert_serial`, `peer`: as `logConfirmation` reads them (`meta/service/service.go:683–691`).
- `operation`: the deletion's operation id; empty for other kinds.
- `resource`: a volume id, a namespace UID, a bound SPIFFE ID, `receiver`, or a chain. `owner`:
  for a volume, `system`, `unassigned` or `<namespace UID>/<PVC UID>`; else empty.
- `time`: RFC 3339 UTC from the instance's clock. Information only: nothing orders by it.
- `reason`: at most 256 bytes for the override's reason, at most 1024 for the rest, printable
  with no quote or backslash:
  - `authorization.binding`: `<old roles or none> -> <new roles or none>`, roles comma-joined;
  - `authorization.receiver`: `<old fingerprint or none> -> <new fingerprint or none>`;
  - `policy.put`, `policy.deleted`: `rf1=<bool> unencrypted=<bool> protection=<bool>
    audit_events=<n> chain=<chain> classes=<class>:<max_bytes>:<max_volumes>,…`, the new or the
    deleted policy;
  - `audit.pruned`: `<chain> <first>-<last> key <fingerprint> receipt <receipt hex>`;
  - else empty.

| Kind | Committed with | Outcome |
|---|---|---|
| `authorization.binding` | the authorization record: one per binding added, removed or changed | `ok` |
| `authorization.receiver` | the authorization record, when its receiver key changes | `ok` |
| `policy.put`, `policy.deleted` | the policy's write or deletion | `ok` |
| `owner.assigned` | the owner record and its charge | `ok` |
| `delete.admitted` | the mark | `ok`, `override` |
| `delete.committed` | the header's deletion, tombstone, owner record's deletion and release; or, with the header already gone, the owner record's deletion and release | `ok`, `marked-unassigned`, `header-missing` |
| `confirm.forced` | the tombstone's update | `ok` |
| `audit.pruned` | the prune's first transaction | `ok` |

**Chain of an event:**
- a tenant volume's deletion by `DeleteVolume`: its policy's `chain`, recorded in
  `deleting_chain` for the commit;
- `authorization.*`, `policy.*`: `cfg`;
- `audit.pruned` of a `sys-` chain or `cfg`: that chain; of an `ns-` chain:
  `sys-<SHA-256(namespace UID)[0] & 15>`;
- any other: `sys-<SHA-256(volume id)[0] & 15>`, as two digits.
No tenant's action writes a `sys-` or the `cfg` chain, except deleting a volume with no owner
record, which is one left from before activation, with one event.

**Budget.** Every event present counts once, exported or not. Per chain: `present = next − 1 −
pruned`; `cap` is 8192 for a `sys-` chain and for `cfg`, and for an `ns-` chain its policy's
`max_audit_events`. A tenant deletion always has a policy: every tenant owner record is counted
in its namespace's usage, and a policy with usage cannot be deleted.
- `delete.admitted`: admitted when `present + reserved + 2 <= cap`; adds 1 to `reserved`. A
  header that does not parse is deleted in one transaction with both events, under the same
  test, and `reserved` is unchanged.
- `delete.committed` of a deletion with a `deleting_op` (including `header-missing` after one):
  always admitted, in `deleting_chain`; takes 1 from `reserved`. Without one (`marked-unassigned`,
  or `header-missing` of an unmarked owner record): as any other event.
- `audit.pruned` of a `sys-` chain or `cfg`: always admitted. Of an `ns-` chain: as any other
  event in its `sys-` chain, so it is refused while that chain is full, until it is pruned.
- A recovery write's `k` events: admitted when `present + k <= cap + 1024`, the `cfg` overdraft,
  which nothing else uses. Any other configuration write's `k` events: `present + k <= cap`.
- Any other event: admitted when `present + reserved + 1 <= cap`.
- Not admitted: `ResourceExhausted`, `audit chain <chain> is full`, nothing written. A full `cfg`
  refuses policy writes and deletions and grants, until a prune of `cfg`; `security-admin` can make
  that prune with its own calls (a recovery write setting the receiver key, the chain hash from
  TenantStatus, and PruneAudit of the full `cfg` through `next − 1`) as long as it holds the private
  half of the record's key or the overdraft has room to set one. Key replacements can spend the
  overdraft, so the runbook requires `security-admin` to keep the private half of the last key it
  sets there. With the last key's private half lost after the overdraft is spent, no chain can be
  pruned and `cfg` refuses every write, until an etcd-level write of the authorization record's
  `receiver_key` (runbook); the lost-key repair above. A full `ns-` chain refuses only its
  namespace's deletions; `sys-` chains take no tenant's events.
- **Self-prune, a change after approval; Settled 2026-11-13 (user's instruction): accepted (see
  decisions.md):** with a key whose private half it holds,
  `security-admin` alone can prune a full `cfg`, erasing configuration events no receiver has
  received; its `audit.pruned` event keeps that key's fingerprint and the receipt, and the
  logs keep every record. A `cfg` that is not full only `cluster-admin` prunes.
- **Growth (#70):** each `ExpandVolume` step of a tenant volume raises `quota/<ns>/<class>.bytes`
  and the owner record's `bytes` by the step's bytes, in the step's transaction, comparing both,
  the policy and the authorization record; past the class's `max_bytes` (checked), the quota's
  `ResourceExhausted`, earlier steps kept. A system owner record charges nothing; a step writes
  no event. Whichever of #70 and #73 lands second adds it, with `TestExpandChargesQuota`.
- **Pruning raises `pruned` only when its sweep ends:** until then the swept events still count.
  A sweep transaction deletes at most 118 events and writes the sweep key; the last one also
  writes the tail, compared at the revision it read: at most 120 operations, as
  `meta/kv/etcd.go:91–92` enforces.
  So `present + reserved <= cap` holds for every chain, plus one pending `audit.pruned` in a
  `sys-` chain or `cfg`, and plus the overdraft in `cfg`. A policy's `max_audit_events` cannot
  be set below its chain's `present + reserved`.
- **Allocation:** `PutTenantPolicy` and `DeleteTenantPolicy` update `allocation` in their
  transaction, compared at the revision read. A policy that would take `events` past 524288 or
  `policies` past 4096 is refused. A new policy's chain is `ns-<namespace UID>-<k>`, `k` the
  incremented `chains`, so no chain name or sequence is ever reused. `DeleteTenantPolicy` is
  refused while its namespace has usage, or its chain has events present, reservations or a
  sweep; it deletes the chain's tail and frees the policy's allowance and slot.
- **Space:** the store holds at most 16 × 8193 + 8192 + 1025 + 524288 events (`allocated_events`
  in TenantStatus is that with `allocation.events` for 524288), each at most 2048 bytes of value
  and a key of at most 114 bytes: about 1.34 GiB of keys and values, and at most 4096 policy
  records. etcd's own overhead comes on top; the runbook requires an etcd quota at least 3 GiB
  above current use, and automatic compaction.
- **Without a receiver key** nothing is pruned: each `sys-` chain and `cfg` take 8192 events for
  the life of the store (`cfg` then admits only recovery writes, one of which can set a key);
  each namespace `max_audit_events / 2` deletions, after which only its own deletions are
  refused until the administrator raises its allowance, within the allocation and while `cfg`
  has room; and a namespace whose chain holds events keeps its policy and its allocation.

## Protobuf

`Owner{namespace_uid:1:string, pvc_uid:2:string}`. `CreateVolumeRequest.owner:30:Owner`; absent
means a system volume. Tag 30 is chosen high to stay clear of fields other open plans add to
this message; if one lands with 30 first, this one renumbers before `make proto`. No existing
message changes otherwise.

| RPC | Request | Response |
|---|---|---|
| `TenantStatus` | `{}` | `active:1:bool, caller_roles:2:repeated string, chains:3:repeated ChainStatus, unassigned:4:u64, unassigned_ids:5:repeated string, deleting:6:u64, deleting_ids:7:repeated string, orphan_owners:8:u64, orphan_owner_ids:9:repeated string, authorization:10:Authorization, authorization_revision:11:u64, allocated_events:12:u64, policies:13:u64` |
| `ActivateTenancy` | `authorization:1:Authorization` | |
| `PutAuthorization` | `expected_revision:1:u64, authorization:2:Authorization` | `revision:1:u64` |
| `PutTenantPolicy` | `namespace_uid:1:string, expected_revision:2:u64, policy:3:TenantPolicy` | `revision:1:u64` |
| `GetTenantPolicy` | `namespace_uid:1:string` | `policy:1:TenantPolicy, revision:2:u64, usage:3:repeated ClassUsage, chain:4:string` |
| `DeleteTenantPolicy` | `namespace_uid:1:string, expected_revision:2:u64` | |
| `GetVolumeOwner` | `volume:1:string` | `assigned:1:bool, system:2:bool, owner:3:Owner, device_class:4:string, bytes:5:u64, protected:6:bool, deleting:7:bool, revision:8:u64` |
| `AssignOwner` | `volume:1:string, system:2:bool, owner:3:Owner` | `revision:1:u64` |
| `OverrideDelete` | `volume:1:string, owner_revision:2:u64, reason:3:string` | `operation:1:string` |
| `ListAudit` | `chain:1:string, after_sequence:2:u64, limit:3:u32` | `records:1:repeated AuditRecord, next:2:u64, pruned:3:u64` |
| `PruneAudit` | `chain:1:string, through_sequence:2:u64, chain_hash:3:bytes, receipt:4:bytes` | `pruned_through:1:u64` |

- `Authorization{bindings:1:repeated Binding, receiver_key:2:bytes}`, `Binding{id:1:string,
  roles:2:repeated string}`. `changed_by` is not in the message.
- `ChainStatus{chain:1:string, next:2:u64, pruned:3:u64, reserved:4:u64, cap:5:u64,
  hash:6:bytes}`, one per chain with a tail; `hash` is the chain hash at `next − 1`, empty for
  an empty chain.
- `TenantStatus`: `unassigned` counts headers with no owner record, `deleting` owner records
  with a `deleting_op`, `orphan_owners` owner records with no header; each id list holds at
  most 100 ids, sorted; `policies` is `allocation.policies`. On an inactive store only `active`
  is set.
- `TenantPolicy{allow_rf1:1:bool, allow_unencrypted:2:bool, required_protection:3:bool,
  classes:4:repeated ClassQuota, max_audit_events:5:u64}`; `ClassQuota{device_class:1:string,
  max_bytes:2:u64, max_volumes:3:u64}`; `ClassUsage{device_class:1:string, bytes:2:u64,
  volumes:3:u64}`.
- `expected_revision` 0 means the record must not exist.
- `AssignOwner`: exactly one of `system` and `owner`.
- `AuditRecord{sequence:1:u64, value:2:bytes, hash:3:bytes}`: the stored bytes and the chain
  hash at that sequence. `limit` 1–256; 0 means 64. `next` is the tail's next sequence. From
  `max(after_sequence, pruned, a pending sweep's through) + 1`; `after_sequence >= next − 1` is
  an empty page.
- **Receipt:** the receiver's Ed25519 signature over `"flaminio-audit-receipt-v1"` ‖ the chain's
  length (one byte) ‖ the chain ‖ through_sequence (u64, big-endian) ‖ chain_hash.

**Errors.**

| Code | When |
|---|---|
| `InvalidArgument` | a malformed identifier or reason; a tenant volume id that is not `pvc-<PVC UID>`; a system creation whose id has the tenant form; an authorization record that fails its checks; a policy out of its bounds; `AssignOwner` with both or neither of `system` and `owner`; `limit` above 256 |
| `PermissionDenied` | no role for the method; `PruneAudit` by `security-admin` of a chain other than `cfg`, before any read; Attach once active by any identity but the CSI controller's and the agent's own restart; `DeleteVolume` or `ExpandVolume` by the role of the other owner kind; a creation with no policy for its namespace, or that its policy forbids (RF=1, unencrypted, no limit for its class); a creation whose id holds a volume of another owner; a receipt that does not verify |
| `ResourceExhausted` | over quota, or a charge that overflows (the message names that namespace's limit, usage and the request only); a full audit chain; a policy past the allocation |
| `FailedPrecondition` | deleting a protected or unassigned volume that is not being created; an owner on `CreateVolume`, or a new method other than `TenantStatus` and `ActivateTenancy`, on an inactive store; a call an insecure instance refuses; a creation whose id has a headerless owner record of another owner and with no `deleting_op` (`orphan owner record: the override removes it`); `AssignOwner` of an assigned volume, of a deleting one, `system` for a tenant-form id whose header parses, or, for a tenant, of one being created or whose header does not parse, or for a namespace with no policy or no limit for `default`; `OverrideDelete` of an unassigned volume; an allowance below its chain's use; deleting a policy while its namespace has usage or its chain holds anything; `PruneAudit` with no receiver key, or a chain hash that is not the chain's, or by `security-admin` of a `cfg` that is not full; `ExpandVolume` of an unassigned volume |
| `AlreadyExists` | as today, for a creation whose id holds a volume of the same owner; `ActivateTenancy` on an active store |
| `OutOfRange` | `PruneAudit` at or beyond the tail's next sequence |
| `NotFound` | `GetVolumeOwner`, `AssignOwner` or `OverrideDelete` of a volume with neither header nor owner record; a policy that does not exist |
| `Aborted` | a stale `expected_revision` or `owner_revision`; contention, as today; `tenancy was activated during this call: retry` |
| `Unavailable` | as today, a store error, including reading the authorization record: the CSI client then tries another instance (`control/csi/meta.go:75–80`) |
| `Internal` | a tenant record that does not parse; an incomplete activation (above); an event missing below the tail; a release larger than its usage |

`CreateVolume`'s policy refusals are `PermissionDenied`, never `FailedPrecondition`, which the
CSI controller turns into `ResourceExhausted` (`control/csi/controller.go:248–249`). Every other
code reaches the CSI controller unchanged, from any instance alike, and the provisioner retries.

**Refusal log.** Each `PermissionDenied`, `ResourceExhausted`, `FailedPrecondition` and `OutOfRange`
above logs one `slog.Warn("refused", "caller", …, "method", …, "reason", …)` and writes nothing to
the store. `caller` is the SPIFFE ID or `unauthenticated`; `reason` is one of `no-role`,
`wrong-kind`, `wrong-owner`, `orphan`, `inactive`, `insecure`, `no-policy`, `policy`, `quota`,
`protected`, `unassigned`, `assigned`, `audit-full`, `allocation`, `below-use`, `not-full`,
`receipt`, `chain-hash`, `beyond-tail`, `usage`. At most 10 lines per caller per second per
instance; past that the caller's refusals are counted. The table holds at most 1024 callers, callers
past that sharing one entry, `others`; each second it logs every nonzero count (`"refusals
suppressed", "caller", …, "count", n`) and is cleared.

## CLI

`flaminio-meta` gains two commands. Each prints the response as JSON (`audit list`: one record
per line) and exits 0, or prints `flaminio-meta: <error>` and exits 1.

```
tenant status
tenant activate --servers HOST:PORT,... --authorization FILE
tenant authorization-put --expect-revision N --authorization FILE
tenant policy-get <namespace-uid>
tenant policy-put <namespace-uid> --expect-revision N --audit-events N [--allow-rf1]
                  [--allow-unencrypted] [--no-protection] --class NAME=MAX_BYTES:MAX_VOLUMES ...
tenant policy-delete <namespace-uid> --expect-revision N
tenant owner <volume>
tenant assign <volume> (--system | --namespace-uid U --pvc-uid P)
tenant override-delete <volume> --owner-revision N --reason TEXT
audit list <chain> [--after N] [--limit N]
audit prune <chain> <through-sequence> <chain-hash-hex> <receipt-hex>
```

`FILE` is the authorization record's JSON, without `changed_by`. `MAX_BYTES` takes the suffixes
`create` takes. `--expect-revision` comes from `tenant status` (`authorization_revision`) or
`tenant policy-get` (`revision`). `tenant activate` calls `TenantStatus` on every server of
`--servers` and refuses, naming the server, unless each answers with `active` false; any error
refuses. An older binary answers `Unimplemented`: gRPC refuses a method it does not know before
any interceptor runs, and `flaminio-meta` sets no handler for unknown methods. Then it calls
`ActivateTenancy` on the first. `--servers` is required and is not defaulted from `--server`.

## CSI controller

- `flaminio-csi controller --tenant-owners`: off by default, which is today's behaviour. On,
  `CreateVolume` reads `csi.storage.k8s.io/pvc/namespace` and `csi.storage.k8s.io/pvc/name` from
  the request's parameters, checks them, then `GET /api/v1/namespaces/<ns>` and then
  `GET /api/v1/namespaces/<ns>/persistentvolumeclaims/<name>`, each segment through
  `url.PathEscape`, and sends `Owner{namespace UID, PVC UID}`. The namespace is read first: a PVC
  that exists after that read was created in the namespace read, since a namespace's deletion
  removes its PVCs first.
- `failover.APIClient`'s requests return `*failover.APIError{Code int}` for a non-2xx answer; its
  `Error()` is today's text (`control/failover/kube.go:82`), and a caller that wraps it uses `%w`.
- Its refusals: a parameter missing, `InvalidArgument` `the provisioner must run with
  --extra-create-metadata`; a malformed namespace or name, or a volume name that is not
  `pvc-<UUID>`, `InvalidArgument`; the PVC's UID not the name's, `Aborted` `PVC <ns>/<name> was
  replaced`; an `APIError` 404 for either, `NotFound`; any other API failure, `Unavailable`. No
  metadata call is made then.
- Activation needs, beyond the flag: `csi-provisioner --extra-create-metadata` with its default
  volume name prefix and UUID length, and `get` on `namespaces` in `ClusterRole
  flaminio-csi-controller`. This step changes no manifest.
