# Issue #73: Tenant policy and destructive-operation audit — specification

[Intent](intent.md). [Design](design.md). [Contracts](contracts.md). [Decisions](decisions.md).
Approved 2026-11-13; not implemented. It specifies the issue's first step, the metadata
service's authority (intent, "This step"); the issue stays open after it.

## Terms

- **Active:** tenancy has been activated in the metadata store, by writing its authorization
  and allocation records. It is permanent. Until then a binary of this step behaves as today's.
- **Authorization record:** the one record, in the metadata store, that binds roles to exact
  administrator SPIFFE IDs and holds the receiver key. Every instance reads it; none has roles
  of its own. A security administrator changes it.
- **Role:** a named set of methods (contracts, "Methods and roles").
- **Owner:** a volume's owner record: a tenant (namespace UID and PVC UID) or `system`.
  **Unassigned:** a volume with no owner record. Names and labels are never owners.
- **Published:** a volume whose creation has finished. Only a published volume can be attached
  or hold data. A header counts as still being created only by #76's test (contracts, Records);
  any other header counts as published.
- **Tenant policy:** an administrator's record per namespace UID: RF=1 allowed, unencrypted
  allowed, protection required, the namespace's audit allowance, and a byte and a volume limit
  per device class.
- **Device class:** #72's pool name; `default` for every volume until #72 gives requests one.
- **Quota:** logical volume bytes and volume count charged per namespace UID and device class;
  distinct from #72 physical capacity/reserve.
- **Protection:** owner-record state fixed when the record is written; deleting the published
  volume then needs the override.
- **Audit event:** an immutable, hash-linked record committed in the transaction of the mutation
  it describes, in a chain. **Audited method:** one that writes an event (contracts, "Methods and
  roles"). **Chain:** a sequence of events with its own budget: one per namespace for its
  tenant deletions, one configuration chain, and 16 system chains for the rest. **Configuration
  write:** activation, a change of the authorization record, a policy's write or deletion; its
  events go to the configuration chain, which no tenant writes.
- **Receiver key:** the public key of an export receiver, set by the security administrator;
  independent of the metadata service unless that administrator holds its private half
  (contracts, "Self-prune").

## The invariant

Once active: a tenant volume exists only with an owner the CSI controller verified, a policy
that permitted it and its quota charge. No request path to the metadata service deletes a
published volume that is protected or unassigned, except the override of that exact protected
volume. Every audited mutation commits with its event. No tenant's activity can refuse another
namespace's or an administrator's audited operation. A revocation, or setting or replacing the
receiver key, is refused for audit space only past a fixed overdraft; other configuration
writes, a key removal included, only when the configuration chain is full, and policy writes
also by the allocation. The audit records' space
is bounded. No failure of an exporter, a receiver, a requester or a metadata instance authorizes
an operation or loses an event. A refusal writes
nothing to the store. Events never hold credentials or data.

## Behaviour

**S1. Roles.** Once active, an administrator name alone grants nothing: every method other than the
three open reads, Attach, and the node, agent and target rules needs a role that the authorization
record binds to the caller's exact SPIFFE ID, as the contracts' method table lists for every method.
Attach, once active, is decided from the certificate alone: the CSI controller's identity by name,
or an agent restarting itself; every other administrator loses it at activation, a cost the
contracts state as a change after approval. Settled 2026-11-13 (user's instruction): accepted
(see decisions.md). Once active, Attach, the open reads and
the node rules read no tenant or authorization record, and an identity that is not an administrator
is refused before any store read. Every instance reads the same record, so all answer alike, and a
removed binding is refused from the caller's next call. Activation binds the activating
administrator as security administrator; no write can remove the writer's own security binding,
leave no cluster administrator, or bind the CSI controller's role to another identity than its fixed
one. The CSI controller, system provisioning, cluster administration, deletion override, audit
reading and security administration are separate roles; the override, audit reading and security
administration are each bound to an identity with no other role. Node self-only rules and
ChangeIsr's target and epoch check are unchanged. The Kubernetes RBAC split of admin resources is
not in this step.

**S2. Namespace restrictions.** A tenant creation is refused unless its namespace has a policy,
RF=1 is allowed if it asks for one replica, unencrypted is allowed if it is unencrypted (every
volume is, until #50), and its device class has a limit. A missing or unreadable policy refuses,
before any placement work. A refusal names only the caller's own namespace, limit and usage.
Kubernetes admission is not in this step: the service's check is the authority on every path.

**S3. Owner.** The CSI controller reads the namespace and then the PVC from the Kubernetes API,
checks that the PVC's UID is the one in the provisioner's volume name, and states the owner. The
service accepts a tenant owner from the CSI controller's role only, requires the volume id to be
`pvc-<PVC UID>`, and writes the owner record in the creation's claim; a retried creation
succeeds only if the existing volume has that same owner. A creation without an owner makes a
system volume, needs the system provisioning role, and cannot take a tenant volume id. A
decision compares the policy revision it read; a policy change applies to later decisions only.
No I/O, attach or ISR change consults a policy, an owner or the authorization record.

**S4. Race-safe quotas.** The claim charges the volume's bytes and one volume to its namespace and
device class, in the same transaction, against the usage it read: concurrent creations cannot spend
the same quota, and checked arithmetic refuses overflow. The charge is released only in the
transaction that deletes that volume's header. A crash, a lost reply, an interrupted creation, a
namespace recreated under the same name or a physical report never releases or moves a charge; an
interrupted creation keeps it until the creation is resumed (#76) or the volume deleted. Lowering a
limit refuses new creations and deletes nothing. Each growth step charges the owner's namespace and
class quota in the same transaction as #72's ledger, and raises the owner record's charge with it,
so the deletion releases the grown size; whichever of #70 and #73 lands second adds that charge and
its test (contracts, "Growth"). Snapshots and backups are charged by the issues that add them.

**S5. Protection.** A volume created under a policy that requires protection is protected once
published. DeleteVolume refuses a published volume that is protected or unassigned before any
write, whoever calls; a header that does not parse counts as published. A volume still being
created was never attachable and holds no data: DeleteVolume deletes it, with its events and
release, whatever its protection or assignment. Nothing a tenant can change in Kubernetes
(fields, finalizers, namespace deletion) reaches the owner record. A refused deletion writes no
mark, and nothing else (S7). A deletion already admitted, or marked while the volume had no
owner record, is finished by any later call allowed to delete it; a mark an older binary left on
an owned volume is checked again. An owner record whose header another path already removed
(an older binary, or #76's rollback run outside DeleteVolume) is deleted, and its charge
released, by the next DeleteVolume of that id by its kind's role, or by a creation of that id,
before its quota check, if it carries an admitted deletion or the creation's own owner (the same
PVC, or `system`); it is refused as an orphan only otherwise. #76's rollback
of a header being created runs after this plan's mark, inside the same DeleteVolume, which
releases the owner record before it answers.

**S6. Explicit override.** The deletion-override role deletes one assigned volume by naming its
id, its owner record's revision and a reason; the service draws the operation id and returns
it. A stale revision (the volume was replaced or changed) is refused; a retry after the deletion
began finishes it. The override performs the deletion itself, through the same mark, tombstone
and quota release as DeleteVolume; it changes no policy and no other volume, and cannot
authorize a later recreation's deletion. Its events name the caller and the reason. Tombstones
and epoch fencing apply as for any deletion.

**S7. Required audit.** Every audited method commits its event in the transaction of its mutation:
identity, role, certificate serial, peer address, method, resource, owner, operation id, outcome and
reason. A deletion writes one event with its mark and one with its tombstone, under one operation
id; a header that does not parse gets both in its one transaction, a volume with no owner record
one, with its tombstone, and a header #76's rollback deletes its two in the mark's and the release's
transactions, none in the rollback's. A retry adds no event and deletes nothing twice. A refusal
writes nothing to the store, so no caller can grow it or delay a mutation by being refused: refusals
are logged, at a bounded rate per caller, with identity, method and a fixed reason word. Durable
records of refused attempts are not in this step. Configuration writes are audited too: one event
per role binding changed, per receiver-key change and per policy write or deletion, in the
configuration chain; each is also logged in full. Key operations and snapshot or backup deletions
are audited by the issues that add them (#50–#53).

**S8. Append-only, export and pruning.** No role can change an event, and only pruning deletes
one. Each chain's order and hash linkage expose a gap, a reordering or an altered event. Audit
readers read pages. The cluster administrator prunes any chain, and the security administrator
the configuration chain when it is full, through a sequence only with a signature,
by the receiver key in the authorization record, over that sequence and the chain's hash there;
the service checks both against its own chain, and the prune's event records the receipt and
the key's fingerprint. The cluster administrator cannot change the receiver key. Without a
receiver key nothing is pruned. etcd or root access can rewrite history that no receiver has
signed: the chain alone is not called immutable against that adversary.

**S9. Capacity and failure.** Events held in the metadata store, exported or not, count against
their chain's fixed budget, with room reserved to finish every admitted deletion. A namespace's
tenant deletions write only to its own chain, whose budget its policy sets; configuration events
only to the configuration chain; other administrative and system events only to the system chains.
At a budget, new audited operations in that chain are refused; admitted deletions finish; creations,
attaches, ISR changes, moves, heartbeats and pruning are not refused, except a namespace chain's
prune while the system chain its event goes to is full, until that chain is pruned. A reservation is
released by its deletion's completion, never by time. An event that cannot be written refuses its
mutation. An exporter or receiver outage changes no authorization. The security administrator's
revocations and receiver-key changes have an overdraft no other write uses, and the security
administrator can prune a full configuration chain itself, with the chain hash the status gives it,
so a full one can be emptied by the role that cannot be locked out as long as that administrator
holds the private half of the record's key or the overdraft has room to set one; the runbook
requires it to keep the private half of the last key it sets in the overdraft. With the last key's
private half lost after the overdraft is spent, no chain can be pruned and the configuration chain
refuses every write, until an etcd-level write of the authorization record's `receiver_key`
(runbook). A receiver key removal does not use the overdraft. The namespaces' budgets have a fixed
total and the policies a fixed number, and a fully pruned namespace chain is retired with its policy
(contracts, "Budget"), so the audit records' space is bounded; the store's own space limit is a
deployment requirement the design states. With no receiver key, the budgets bound the store's
audited operations for its life.

**S10. Data minimization and identifiers.** Events hold only their listed fields: no request
bodies, tokens, credentials, keys or payloads. Every identifier that enters a key, an event, a
URL or a log line has a fixed format and length, checked before use; reasons are bounded
printable text. Audit reads need the reader role, which changes nothing.

**S11. Activation and legacy volumes.** Activation is one conditional write of the
authorization and allocation records, made only after every metadata instance the administrator
lists answers as this step's binary. One missing afterwards fails every role-gated call
closed, while Attach, ChangeIsr, heartbeats and the open reads keep serving; the repair is in
the contracts. After it, an instance refuses to start with `--insecure`, and an insecure
instance already running refuses every call but the open reads. A call authorized under the old
rules that would write an event once active is refused if activation commits during it; a
creation, move, drain or attach authorized just before activation may still commit just after
it, and a volume it creates is unassigned. Owner, policy, quota and audit records are separate
from the volume header, so a writer that does not know them cannot strip them. A binary older
than this step cannot be stopped by the store: running one after activation is unsupported, and
what it leaves behind is reported. Volumes that exist at activation are unassigned: they attach
and serve as before, and, unless their deletion was already marked or their creation never
finished, are deleted only after an administrator assigns an owner, which is audited and, for a
tenant, charged, even past the limit. A header that does not parse can be assigned only to
`system`, and a parsed `pvc-<UUID>` volume only to a tenant. No owner is guessed from a name.

## Tests

- **S1:** every method by every role, by node identities, by an unbound identity and with no
  identity, on secure and insecure instances, inactive and active; the CSI controller's, the
  failover controller's and the drain command's calls with only their roles; a binding removed
  and added while two instances serve; Attach by `audit-reader`, `deletion-override`,
  `cluster-admin`, `system-provisioner` and an unbound administrator refused once active, a node
  identity's refused Attach reading nothing, by the CSI controller and an
  agent committing, with a record that does not parse or is missing; ChangeIsr and heartbeats
  with incomplete records, at start and at runtime; a node identity's refused calls read nothing;
  activation racing on two instances; PutAuthorization stale, dropping the writer's own security
  binding, leaving no `cluster-admin`, binding `csi-controller` to another ID, with a repeated or
  non-canonical ID, and during a prune; a missing record after
  activation; the CLI's `tenant authorization-put`; a grant, its use and its revocation, and a
  policy's protection cleared and restored, each leaving its events in the configuration chain.
- **S2–S4:** on both backends: forbidden RF=1, unencrypted and class, a missing policy, refused
  before placement, with a message naming only that namespace; quota at the limit and one byte over,
  overflow, a growth step charged to the namespace and the owner record, over the limit refused with
  earlier steps kept, and the deletion releasing the grown size, two creators racing for the last
  bytes (exactly one wins), a policy change between read and claim, a lost reply and its retry (one
  charge), a retry against a volume of another owner, an orphan owner record, a stand-in for #76's
  legacy rollback of an unmarked creating header then the same PVC's creation, also at the
  namespace's limit (released, then claimed), delete then recreate, a deletion whose tombstone a
  dead node never confirms (released at once); a policy past the allocation, an allowance below its
  chain's use, policy deletion refused with chain events, then allowed once the chain is pruned,
  freeing its allowance and slot; more than 4096 namespaces over time, each getting a policy. CSI
  owner lookup: UID mismatch, a PVC recreated under the same name, a namespace replaced between the
  two reads, malformed names, missing provisioner metadata, a 404 and a 500, the old error text kept
  through a wrapper.
- **S5–S6:** a protected deletion by each role, with a header that does not parse, of an
  unassigned volume; a creation interrupted after its claim, then deleted (charge released); a
  protected creating header with a nonzero epoch, refused; a stand-in for #76's rollback
  deleting the header inside DeleteVolume after the mark, the release in the same call, and a
  release that fails, then the retry; a header removed alone outside DeleteVolume, then its owner
  record released by DeleteVolume and, with a `deleting_op`, by a creation of that id; the
  override with a stale
  revision, against a replacement, retried after each step; authorization records refused by
  validation.
- **S7–S9:** a failure and a lost reply at each transaction of a deletion; chain verification
  catches an altered, missing and reordered event; prunes that are stale, replayed, forged, beyond
  the tail, with a wrong chain hash, with no receiver key, or after the key changed; pruning removes
  only events through the signed sequence, records its receipt, and finishes after a crash mid-way,
  even once the key is removed; one namespace churns until its chain is full while another namespace
  deletes; the configuration chain full: grants (one that also revokes, or replaces an ID, included)
  and policy writes refused, a revocation and a receiver-key change admitted in the overdraft, one
  admitted at exactly its end and the next refused, a key removal refused; the overdraft spent by
  key changes: the next key change refused; a prune with the last key admitted; a key change
  admitted after it; the lost-key repair: the overdraft spent by key changes, the record rewritten
  through the store with a new key and every other field kept, a prune with the new key admitted and
  its `audit.pruned` naming the new fingerprint, a PutAuthorization at the revision from before the
  repair refused as stale; the repair's txn, from reads at `cap + 1024`, refused by one compare
  alone in each of three cases: a prune of `cfg` by a key holder ending before the txn (the tail
  compare; the record's revision unchanged and the sweep key absent), a no-op PutAuthorization after
  the reads (the record compare; the tail's revision unchanged and the sweep key absent), and reads
  taken while a sweep is pending (the sweep-absence compare; the tail's and the record's revisions
  unchanged); the security administrator's prune retried after a lost reply and after a crash
  mid-sweep, answered as pruned; with no `audit-reader` bound, a sole `cluster-admin` flooding
  policy writes, then the security administrator, with its own calls only, setting a key, reading
  the hash, pruning the configuration chain, binding a new `cluster-admin` and revoking the old; the
  security administrator's prune of a configuration chain that is not full, or of a system or
  namespace chain, refused; a namespace chain's prune refused while its system chain is full, then
  admitted once that chain is pruned; a prune of exactly 118 × n and 119 × n events on the etcd
  store, and an append racing its last sweep transaction; every system chain full: Attach and
  ChangeIsr commit; a pending sweep's events counted until it ends; a refusal flood from one
  identity leaves another identity's first refusal logged, a burst then silence still logs its
  count, 1025 callers share `others`, no store write comes from refusals; each configuration write's
  log line holds the full record.
- **S10:** identifier and reason tables (malformed, oversized, newlines, key separators, the top
  `after_sequence`); events hold only allowlisted fields; audit reads by the wrong roles.
- **S11:** activation refused while a listed instance is older or insecure; an insecure instance
  after activation; a deletion that read the store inactive while activation commits; a creation
  authorized before activation and claimed after it (unassigned, uncharged, reported, deletable only
  after assignment); a header rewritten in the old format keeps its protection; a legacy volume
  attaches, its deletion is refused, then succeeds after assignment; a legacy header that does not
  parse, assigned to `system`, then deleted; AssignOwner admitting a tenant owner for a `pvc-` id
  whose PVC is gone, into a namespace whose `default` allows 0 volumes, and a creation there refused
  (the evidence order is a runbook step the service cannot check), then the volume deleted; `system`
  refused for a parsed `pvc-<UUID>` volume; a deletion by an old binary is reported; TenantStatus
  counts and lists.

## Open questions

- **Q1:** creation now depends on a policy record and deletion on audit space. Fail-closed
  behaviour is proposed; no availability trade-off is accepted.
  Settled at approval, 2026-11-13: fail closed (see decisions.md, Settled at approval).
- **Q2:** the budgets' sizes, the receiver's signature as the only gate to pruning (no local time
  minimum) and pruning by hand need administrator agreement. No receiver is in this step: until
  one exists, the budgets bound how many audited operations an active store can make. A
  receiver's signature is not WORM storage.
  Settled at approval, 2026-11-13: the contracts' budgets, the receiver's signature as the only
  gate, pruning by hand by `cluster-admin` (see decisions.md, Settled at approval). Changed after
  approval in round 6: `security-admin` also prunes a full configuration chain; Settled
  2026-11-13 (user's instruction): accepted (see decisions.md).
- **Q3:** who assigns legacy owners and from what evidence, the separately credentialed override and
  security roles, the CSI controller as the one party trusted to state owners, and the first
  activation by any administrator name are Proposed. Settled at approval, 2026-11-13:
  `cluster-admin` assigns legacy owners; the roles, the CSI controller's trust and the first
  activation as proposed (see decisions.md, Settled at approval). Settled 2026-11-13 (user's
  instruction), restated after approval in rounds 7 to 10: the owner is the PVC whose UID the volume
  id carries: find the PVC by that UID, read its namespace's UID, read the PVC again by namespace
  and name, and require the id's UID. With that PVC gone, the owner is a dedicated namespace created
  to hold legacy volumes, never a tenant's namespace and never one taken from a PV's `claimRef`, and
  the PVC UID the id carries. Its policy has a `default` limit of 0 volumes, so every creation there
  is refused while an assignment is charged past the limit, and a `max_audit_events` of at least
  twice the volumes it will hold; no one is granted PVC rights there, and its UID is in the
  runbook's operator log. It costs its chain two events per volume, which stay allocated while no
  receiver prunes them. DeleteVolume or the override then deletes it. `system` stays for ids not of
  the tenant form and headers that do not parse (see decisions.md, Settled at the user's instruction
  of 2026-11-13).
- **Q4:** nothing in the store stops a binary older than this step after activation. The plan
  states it as unsupported and reports what it leaves. Settled 2026-11-13 (user's instruction):
  that risk is accepted as the plan states it (see decisions.md).
  Settled at approval, 2026-11-13: unsupported, and reported (see decisions.md, Settled at
  approval).
