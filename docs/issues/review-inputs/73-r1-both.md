# Issue #73 plan review — round 1 (design-code + design-security, plan checklists on intent and spec)

Scope: `docs/issues/73/{intent,spec,design,adr-proposal}.md` in `/tmp/flaminio-plans-2026-11-12/73`,
checked against the code of that worktree and the draft plans of #70, #72, #76, #78, #80.
Not checked: the issue's own "Done when" text. It is not in the worktree and reading it needs a
network call, which the brief forbids; the goal was judged from intent.md and ARCHITECTURE §15.3.

### R1-B1 — High: "exclude old writers" and "policy-enabled mode" have no mechanism

Spec S11 says "Exclude old writers"; the design repeats it once (Risks) and makes enforcement a
per-process flag (`--authorization-policy FILE`, "policy-enabled mode"). Nothing durable says the
cluster is in that mode, and the code has no version gate at all.
- Several metadata instances serve one store (`deploy/csi/driver.yaml:49`, `--meta={{META_ENDPOINTS}}`).
  An instance that is older, or simply started without the flag, authorizes on the Admin name alone
  (`meta/service/auth.go:57,64–65`) and deletes with no protection check
  (`meta/service/service.go:516–582`).
- Headers are rewritten through the struct: `json.Unmarshal` then `json.Marshal`
  (`service.go:539–541` delete mark, `:723–725` Attach). An old instance's Attach therefore strips
  the new owner/protection fields from the header. A routine attach during a rolling upgrade removes
  protection (S5) and turns a tenant volume into "unknown owner".
- `Authorize` returns before any identity when `TrustDomain == ""` (`auth.go:42–44`); the design does
  not say that policy mode refuses `--insecure`.
Fix: design the gate. A durable activation record in etcd; new binaries refuse to serve tenant-owned
state without a policy file once it exists, and refuse `--insecure` with it; activation checks that no
older instance is live (a versioned instance record) and the procedure says what stops an old binary
afterwards; say what a new instance does with a header that lost its owner (fail closed, not
"system"). Add the tests: an old-format writer rewrites a protected header; an instance without the
flag after activation.

### R1-B2 — High: denied attempts are an unbounded, shared, durable write path

"Authenticated denied attempts append refusal before responding" and "Denials use remaining
unreserved budget" (design, Audit storage). Every node certificate is an authenticated identity, and
the CSI sidecar retries a refused call forever.
- Any node identity can loop DeleteVolume/CreateVolume and be refused. Each refusal is an etcd
  transaction and up to 8 KiB of the 256 MiB budget. When the unreserved budget is gone, "reject new
  auditable work" (S9) stops provisioning and deletion for the whole cluster.
- A tenant needs no certificate: deleting a PVC whose volume is protected (the design's default,
  `requiredProtection=true`) makes external-provisioner retry `DeleteVolume` indefinitely
  (`control/csi/controller.go:261–269` passes the error through), one durable refusal per retry (S5).
- Refusals and mutations compare the same `audit/tail/<p>` revision. A refusal stream makes
  legitimate transactions in that partition lose their compare; after `retries = 32`
  (`service.go:45`) they return Aborted.
- Partitions hash owners together, so one tenant's backlog blocks unrelated tenants in its partition.
"Transport retries ... do not create unbounded mandatory attempt events" covers admitted operations
only. The test table has no abuse case.
Fix: a separate, capped denial budget that cannot take mutation budget; coalesce refusals per
(identity, method, resource, reason) into a counter record with bounded cardinality; rate-limit per
identity; keep refusals off the mutation tail (their own chain) or give mutations precedence; say
what is recorded when the denial cap is hit. Tests: a node identity and a retrying sidecar flood
refusals while another tenant creates and deletes.

### R1-B3 — High: audit capacity, 90-day retention and etcd space do not fit together

- The design proposes "total local backlog+reserved budget 256 MiB" and also "after minimum
  retention (proposed 90 d) and verified durable export, an admin retention operation ... prunes".
  It never says whether exported-but-retained events count against the 256 MiB. If they do, a full
  budget stops all auditable operations until events are 90 days old and an administrator prunes by
  hand. If they do not, 90 days of events are unbounded in etcd, which contradicts S9's "bounded
  audit capacity".
- The events live in the same etcd as headers, epochs and ISRs. etcd refuses every write after its
  space alarm, including Attach and ChangeIsr. S9's "fencing and safety-critical membership changes
  must not depend on SIEM availability" is argued only for the exporter, not for the space an
  exporter outage consumes. The lab starts etcd with no quota or compaction flag
  (`scripts/guest/etcd.sh:45`); pruned keys free nothing until compaction and defragmentation (#76's
  draft states the same, design line 116).
- When a reservation (64 KiB per admitted operation) is released is not stated. "No timeout frees a
  hidden creator's quota" suggests never for an abandoned operation; 256 such operations fill one
  16 MiB partition.
Fix: define what the budget counts; bound retained events against a stated etcd quota with headroom
that fencing writes can always use (or say why the alarm cannot be reached); state compaction as a
requirement; state when a reservation is released and who recovers an abandoned one. Test: budget
full with exporter down, then Attach and ChangeIsr still commit.

### R1-B4 — High: the role list covers none of today's Admin callers, and the audited methods are not listed

"An Admin name alone grants nothing in policy-enabled mode ... deny unbound identities". The roles
named are human-admin, provisioning-broker, policy-controller, deletion-override, audit-reader,
audit-exporter. Today's Admin callers are the CSI controller (CreateVolume, DeleteVolume, Attach),
the failover controller (Attach) and the drain controller (DrainNode, MoveReplica)
(`api/proto/flaminio/meta/v1/meta.proto:26–60`, `control/cmd/flaminio-csi/main.go:7–8`), plus the
`admin ||` branch of RegisterNode, NodeHeartbeat, ListTombstones and ConfirmDeletion
(`auth.go:66–78`). With the six roles as written, the failover controller's Attach, which is the
fencing path, is either refused or bound to human-admin, which is the "unrestricted administrator
name" S1 forbids. The design also never says which process holds provisioning-broker
(`control/policy` verifies UIDs; `control/csi` is listed as "trusted intent broker context").
The design also never enumerates which RPCs are "destructive/key" (S7). ConfirmDeletion by an
administrator for another node frees a volume id for reuse and is what `logConfirmation` records
today (`service.go:675–694`, docs/issues/14 S4), with the certificate serial and peer address; event
schema 1 has neither. MoveReplica removes a copy.
Fix: one table, method x role x audited, for all 13 existing RPCs and each new one; name the roles
of the CSI, failover and drain controllers; keep serial and peer in the event or say why not; test
that failover Attach and drain still work in policy mode with only their own roles.

### R1-B5 — Medium: S11 has no approach, and the design cites a rule S12 that does not exist

The spec ends at S11. The design's first section is titled "(S1–S3, S12)" and its test row is
"S11/S12". S11 (legacy inventory, quarantine, administrator assignment) has no design section: no
record of a quarantined owner, no statement of which operations a quarantined volume allows (Attach
of an existing workload after the upgrade matters), no assignment RPC. `Header.FirstEpoch` is 0 for
older volumes (`meta/domain/domain.go:44–47`), so the override's incarnation check
(`first_epoch`) cannot tell two legacy incarnations apart; not addressed.
Fix: write the S11 section (state, allowed operations, assignment RPC and its role, first_epoch 0)
and remove or define S12.

### R1-B6 — Medium: operations the design relies on have no RPC or auth rule

`AGENTS.md`: an `auth.go` rule per new RPC. The proto list has PutTenantPolicy, PutTenantIntent,
OverrideDelete, ListAudit, AckAuditExport. Missing, though the text depends on them:
- deletion-protection update ("admin-only, quota-neutral and audited");
- the retention/prune operation and its manifest (S8 "authorized/audited");
- the `system-provision` path ("role/reason/owner mode") and its role, absent from the role list;
- legacy owner assignment (S11);
- any read of policy, quota or intent, and removal of a policy;
- S10's "tenant status discloses its own decisions only" has no approach at all;
- DeleteVolume: `TenantContext` is added "on create/grow/snapshot/backup" only, so the "deletion
  intent" has no carrier.
Fix: add each message with its role, or drop the behaviour.

### R1-B7 — Medium: how an intent reaches CreateVolume is unspecified, and CSI has no UID to look it up with

`control/policy` writes `PutTenantIntent` and gets `intent:1:string` back; `control/csi` must send
`TenantContext{intent, operation}`. Nothing says how the CSI controller learns the intent id.
`--extra-create-metadata` (not set today, `deploy/csi/driver.yaml:67–68`) gives PVC name and
namespace, not UIDs; the only UID in a CSI CreateVolumeRequest is inside the volume name, which the
design calls a display name. A lookup by name lets a PVC deleted and recreated under the same name
with the same spec use the other incarnation's intent (S3 "another PVC's authorization").
Also undefined: the canonical encoding behind `spec_hash` and who recomputes it (metadata must, from
the request it receives); whether it covers `prefer_node`, which comes from the scheduler through
topology (`controller.go:233`) and is unknown to a controller reading the PVC; what CSI returns when
its call arrives before the intent exists.
Fix: bind the intent to the exact volume id the provisioner will use and to the PVC UID; define the
canonical field list and encoding in `pkg/tenant` and state that the service recomputes it; state
the not-yet-present error and retry.

### R1-B8 — Medium: "class" means two things; per-device-class quota is not what the design builds

ARCHITECTURE §15.3: restrict namespaces' use of "unencrypted or RF=1 classes" (StorageClasses) and
"enforce quotas per device class". A device class is a named pool (`fast`, `capacity`,
ARCHITECTURE:177); #72's draft carries it as `string device_class` with no UID
(`72/docs/issues/72/capacity-contracts.md:23`). The spec says "device-class UID"; the design keys
`quota/<namespaceUID>/<classUID>` and `Owner.class_uid` by StorageClass UID. Consequences: several
StorageClasses on one device class each get a full quota; recreating a StorageClass (its parameters
are immutable, so this is routine) gives a new UID, a refused policy lookup, and usage split across
two quota keys.
Fix: separate the two: permissions keyed by StorageClass UID, quota keyed by namespace UID and
#72's device-class name; fix the spec term.

### R1-B9 — Medium: AckAuditExport is trusted, but it gates pruning

"Advances a durable cursor only after receiver acknowledgement"; "after ... verified durable export
... prunes". The design does not say that the metadata service checks `batch_hash` against its own
chain at `last_sequence`, verifies `checkpoint` against a configured receiver key, bounds
`checkpoint`'s size, or refuses a cursor that moves backwards or past the tail. As written a faulty
or compromised exporter acknowledges batches nobody received, and the retention operation then
deletes the only copy. S10 says export credentials have no mutation privilege; this is one.
Fix: state the server-side checks, make the cursor write a conditional transaction, and add stale,
replayed, forged and beyond-tail acknowledgements to the tests.

### R1-B10 — Medium: the unparseable-header path in DeleteVolume deletes without a protection check

`service.go:535–551`: a header that does not parse skips the mark and goes straight to the
tombstone transaction. With owner and protection in the header, that path has neither, and the
design ("compares protection generation with its mark, before any side effect") assumes a mark
always happens. S5 needs this to fail closed or to require the override role.
Fix: say what DeleteVolume does with an unparseable header and test it.

### R1-B11 — Medium: when quota is released is left as a choice of three

"Once the exact incarnation's terminal cleanup/tombstone/reference transition commits." The header
and tombstone transaction (`service.go:565–570`), the last ConfirmDeletion (`:654–661`) and a
reference drop are different moments. Release at the last confirmation is blocked forever by a dead
node (docs/issues/14) and needs owner and bytes in `tombstoneRecord` and a quota write in a
transaction a node agent triggers; release at the tombstone does not. `createVolume` also finishes
another caller's marked deletion (`service.go:426–433`); who is recorded for that outcome is not
said.
Fix: pick one transition per resource kind, list the records it touches, test the dead-node case.

### R1-B12 — Medium: intents, operations and policies are never removed

`intents/<id>`, `operations/<op>` and `policies/...` have no end of life. The webhook "performs no
quota reservation", so PVCs over quota are persisted and each gets an intent; a tenant creating and
deleting PVCs grows etcd without limit, outside quota. Deleted namespaces leave policy and quota
keys.
Fix: state removal conditions (PVC gone and no claim; operation terminal and exported; namespace
gone and usage zero), bound intents per namespace, and test the churn case.

### R1-B13 — Medium: webhook scope and DELETE behaviour are unspecified

The webhook matches PVC and snapshot CREATE/UPDATE/DELETE "without tenant label exclusions",
`failurePolicy=Fail`. Nothing limits it to Flaminio's provisioner, so while it is down every PVC
write in the cluster fails, other drivers' included, and namespace deletion hangs on DELETE. What
it decides on DELETE is not said: refusing a protected PVC's DELETE blocks namespace deletion
forever; allowing it leaves the Released PV retrying into B2. "Immutable protection labels/owner
fields" on a PVC are not defined (who sets them; the spec says labels are not proof). The serving
certificate and `caBundle` source are not named.
Fix: scope by a non-tenant-controlled condition (StorageClass provisioner via `matchConditions`),
state the DELETE decision and the namespace-deletion outcome, define or drop the labels.

### R1-B14 — Medium: the Kubernetes pieces have no stated client or test harness in this repo

`go.mod` has no client-go or controller-runtime (only `k8s.io/utils`, indirect); the one Kubernetes
client is hand-written HTTP (`control/failover/kube.go:21–27`). The design needs watches on four
kinds, a CRD and an admission server, and its S1–S3 proof is "envtest/real API fixture"; the lab
plan is "disposable Kubernetes/API". Neither exists or is named, so the S1–S3 tests cannot be run
as written, and the dependency choice (and its `make hygiene` cost) is undecided.
Fix: name the client approach and the fixture (and how `make test` gets it without the network).

### R1-B15 — Medium: half of the plan designs against code that other issues have not planned

S11 keeps #73 open until hooks in #50–#53 pass; #52 and #53 have no spec, #50 and #51 no design.
The "External KMS/repository" paragraph and the test row "every #50–#53 destructive/key integration"
describe code with no file in this repository; those tests cannot be written here.
Fix: #73 delivers the guard and audit API with a fake provider and its crash tests; each of #50–#53
owns its hook and test, recorded in `docs/followups.md`. Move the rest out of the spec's rules.

### R1-B16 — Medium: identifiers that become keys or idempotency handles are not validated or bound

`namespace_uid`, `class_uid`, `resource_uid`, `cluster`, `intent`, `operation` and `kind` are free
strings; several become etcd key segments (`policies/<namespaceUID>/<classUID>`,
`operations/<op>`). No format or length is given (#76's draft fixes its op id as 128-bit lowercase
hex). `operations/<op>` is not bound to the caller or the request: "Retry reads the same durable
outcome" lets a second caller with the same id read or absorb another's outcome.
Fix: fixed formats and lengths, refused before any key is built; the operation record stores caller
identity and a request hash, and a mismatch is refused. Add malformed and colliding ids to the tests.

### R1-B17 — Low: paging and pruning need a KV operation the interface does not have

`kv.KV.List` returns every key under a prefix (`meta/kv/kv.go:41–42`, `etcd.go:70–72`). "256-event
pages" and "bounded conditional batches" need either point reads by sequence (then say so, and fix
the sequence key's width) or a ranged list in both stores, which touches ADR 0002's narrow
interface; `meta/kv` is not in "Files and contracts".

### R1-B18 — Low: OverrideDelete does not say who performs the deletion

It "writes that one delete decision ... under the protection CAS". Whether that marks the header
itself or leaves a decision that a later DeleteVolume by the CSI controller consumes (and how that
call finds it) is not stated; the two differ in who is the audited actor and in what a crash
between them leaves.

### R1-B19 — Low: two writers of one policy key

PutTenantPolicy is allowed to policy-controller and human-admin. The controller mirrors the CRD, so
it overwrites a direct administrator write on its next pass (or loops on `expected_revision`). Say
which is the source of truth.

### R1-B20 — Low: test plan gaps

The spec's Tests list has nothing for S8's retention minimum and authorized, audited pruning (the
design row mentions "prune" only). The design's rows for S10 and S11 name no test file or package,
unlike S4–S9.

## Nits

- Line references are stale: intent cites `control/csi/controller.go:246–252` and `:283–291`
  (the calls are at 231–236 and 261–269); design cites `auth.go:58–90` (the file has 88 lines) and
  `service.go:509–588` (DeleteVolume ends at 582).
- "16 partitions by SHA-256(resource owner/key)": owner or key; the two give different partitions
  for one resource.
- Files lists `flaminio-metaclient/src/{lib,pb}.rs` without a message that changes; `pb.rs` holds
  only the messages the data plane uses, and `Owner` is "retained internally" while "#80
  worst-response budget tests include exposed new fields". Say which fields are exposed.
- `allowUnencrypted=false` by default with no encryption before #50 means every tenant create is
  refused until an administrator sets it; state this in the rollout text.

Reviewed commit: 094005fc7936e5cdbd41a1224c35b59165d4fb17
