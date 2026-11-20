# Issue #91: Shared storage disruption guard — design

[spec](spec.md) · [decisions](decisions.md) · [Proposed ADR](adr-proposal.md).
Unapproved technical proposal. No implementation or executed qualification is claimed.

## The facts it starts from

`control/failover/kube.go:20` is a small authenticated REST client with node-write authority;
guard must use a separate identity. No client-go dependency exists. Metadata list silently
skips malformed JSON (`meta/service/service.go:845-857`); #77 only fixes named volume views.
#69/#77/#90 and #83 are reviewed proposed dependencies, not installed merely because planned.
The exact [reader](reader-contract.md), [security](security-contract.md),
[admission](admission-contract.md), [identity](identity-contract.md) and
[qualification](qualification.md) and [resource lifecycle](resource-lifecycle-contract.md) contracts are normative.

## Synchronous coverage and admission (S1, S3, S4, S5, S7)

Replace the unsafe dual-PDB candidate with the exact three-webhook contract. Install an
ordinary controlled finalizer-bearing anchor for **every** Node before Node CREATE becomes
visible; this covers nodes that later register storage without a label. A ConfigMap is its
real controller owner, managed by this process. Placement is explicit supported nodeName;
actual Node UID is independently checked. No invented UID affinity or node writes.

Kubernetes generic store fills the Node UID before validating CREATE and commits afterwards.
The webhook first GETs the actual name: existing-node POST returns exact AlreadyExists,
allocating no doomed request UID. For an absent name, durable nameclaim CAS/unique creation
allows only one requested UID to prepare its anchor; overlapping UIDs hold without Pods.
This preserves pinned kubelet POST/AlreadyExists/GET reconciliation. A delayed original create
retains its claim; another request cannot take it over by timeout.
Anchor finalizer prevents autonomous PodGC/controller deletion from creating an empty-node
window. Uncommitted creates remain Staging indefinitely if necessary; a timer cannot prove a
paused API request will never commit. Cap4096 allocated enrollments; exhaustion holds.

Stock kubectl omits deleteOptions UID preconditions. The mutator adds the actual anchor UID;
validator checks it, makes a complete fresh metadata pass and CAS-reserves the single durable
slot before allow. Different nodes cannot reserve the same empty resourceVersion. No Ready
pod count, surge, global PDB integer, Lease timeout or status flag grants permission. Existing
admitted node stays unavailable. Each same-node retry also needs fresh metadata; a replacement
Pod UID cannot reuse that grant. Pending/terminal/deleting anchors use the same admission path.

Finalizer removal is an exact slot-owned operation; a lost response retains occupancy. Recovery
retires the old admitted Pod UID, then creates a fresh finalizer-bearing Ready anchor and
requires the same Node UID schedulable/Ready. Storage additionally requires a live current
store/full relevant earned ISR; ProvenNonStorage re-proves absence of permanent store history,
current registration and refs instead. Later registration latches everStorage and forces
Storage/Unknown handling. Deleted UID retirement uses saved exact metadataID/store UUIDs
and explicit return/move-away predicates, all defined once in [identity](identity-contract.md). Only
then may CAS release the slot. Any paused old request is constrained to the now-absent old Pod
UID. Every controller image/terminal refresh consumes the same slot, because kubectl uses a
Pod snapshot and might otherwise finish when an unadmitted old marker is replaced.

This serializes more than strictly necessary, intentionally. A node which cannot meet the
minimum may retain a terminal anchor rather than refresh it. An admitted in-flight operation
can finish after metadata/controller failure; no webhook can revoke API-server work already
admitted. No new admission can use a persisted permissive marker while the process is absent.

## Read observation and reducer (S2)

Implement `control/disruption/reader.go` against strict enumeration, #77 coherent named reads
and #90 exact full/paged earned membership. Runtime identity is the exact #69 Reader URI.
Last-to-land composition extends only its page-read allowlist; no admin fallback or etcd client.
The appendix owns additive proto fields, backend bounded enumeration, budget arithmetic and
service-to-real-mTLS fixtures. Unsupported/partial/malformed/stale input yields Unknown.

`guard.go` reducer takes complete views, live/current exact stores, Node UIDs/cordons and the
single occupied slot. Deduplicate survivors by node, exclude candidate/dead/draining/cordoned/
admitted/ambiguous nodes, include exact move endpoints and conservatively compare every
relevant chunk to minISR. External nodes remain eligible survivors under validated identity.
The supported join is exact metadataID==Kubernetes Node.name with separate enrolled UID;
no address/label aliases. Persist bounded sticky everStorage/observedStores before grants;
slot copies immutable retirementStores. ACTIVE identity-v4 and strict history probe are required
for admission, so a formerly registered missing node cannot become harmless nonstorage.
No cross-volume transaction is claimed; subsequent failures and metadata changes remain
availability risks protected by existing data authority. The guard mutates none of that state.

## Process, installation and failure (S1, S4, S6, S7)

Proposed command: `flaminio-disruption-guard --namespace flaminio-disruption-guard --meta
ENDPOINTS --cert PATH --key PATH --ca PATH --trust-domain TD --listen :9443 --tls-cert PATH
--tls-key PATH [--external-node NODE_ID ...]`. Namespace is fixed to the separate guard namespace; a different value refuses.
One installer-precreated Deployment, replicas1, strategy Recreate; optional concurrent instances
are still safe against durable CAS. Reconciliation every5s, one bounded pass at a time, ≤30s;
HTTP admission max8 active requests, reject excess429, request body≤1MiB and no unbounded queue.
One process-wide nonblocking collector semaphore covers admission and reconcile: Busy holds.
One absolute header-completion+24s deadline covers body/decode, initial reads, reader/API work and reply/unwind; action cutoff is+23s. Reader≤20s and API/RPC≤2s subdeadlines are clamped to the remaining actionable budget, never added to it. Four RPCs and64MiB application-controlled credits are process-wide. The exact formulas and no-late-CAS rule are in [admission](admission-contract.md). Stock gRPC/runtime retention is not covered by a false64MiB whole-process proof; the separate verified cgroup memory.max≤256MiB/swap0 containment and fail-closed recycle protocol are defined in [resource lifecycle](resource-lifecycle-contract.md).
Listener mTLS authentication, fixed mounted admission-client trust/URI allowlist and exact
k3s credential selection/deadlines are canonical in [security](security-contract.md); no
unauthenticated request can allocate claims or enrollments. Cancel work with request context;
unknown write outcomes hold. Kube token and serving/reader certificates reload by file replacement.

Resource exhaustion or a known terminal reader failure enters Quiescing, refuses new admission and ends the process under the bounded supervisor contract. No cleanup timer forces Free, releases a durable slot or treats an uncertain remote write as failed. A fresh process re-reads durable state before allowing anything. Stock caches disappear with actual process termination; healthy-call recovery is qualified across that replacement, not falsely claimed within the old process. Repeated restarts/held drains are an explicit Proposed cost, not human acceptance.

**Initial install has an unavoidable boundary:** no webhook can retroactively intercept a
client that already listed an empty node or a drain that bypasses the eviction API. The exact
installer precondition is a maintenance change window with no existing/in-flight node drains
or node removals while installing; it installs fail-closed webhooks, covers all existing Node
UIDs, verifies bounded complete coverage twice, then CAS profilePhase Active. No per-node
Flaminio command or candidate label is required. The administrator must arrange the installation
window just as for installing any interlock; after Active new Node CREATE is synchronous.
Failing preflight leaves profile Bootstrapping and reports unmet installation, never success.
This is an installation boundary, not a deferred choice of runtime admission mechanism.

Serving pods live on preexisting nonstorage management nodes; install the serving Deployment
before webhooks, bootstrap coverage, then activate. Losing that management node holds evictions
and new Node admission by failurePolicy Fail, while nodes/status remains available. Recovery
uses the existing management node and installed serving artifacts. Complete management-node
loss may require explicit privileged repair of admission configuration; that repair is a
bypass, logged/documented, never issued automatically by this controller. Uninstall likewise
requires a maintenance window, no occupied slot and explicit administrator removal of webhook
configs/finalizers; runtime credentials cannot disable the policy.

## Status and authority (S6)

[security-contract](security-contract.md) defines exact precreated state/status/serving objects,
runtime ConfigMap/Pod/Event verbs, separate Restricted namespace and credentials. Runtime has
no StatefulSet/Deployment/PDB/node/tenant-write or webhook-configuration permission. Enrollment
status gives bounded volume/chunk reasons; singleton status summarizes omitted nodes. Logs and
rate-limited Events contain no key/token/payload. Failure to publish is observable and cannot
turn an admission error into allow. No metadata mutation or data-plane credential exists.

## Files touched

- `control/disruption/{guard,reader,kube,admission,enrollment,status}.go` and focused tests:
  reducer, bounded REST transport, HTTPS AdmissionReview, JSONPatch and CAS/finalizer lifecycle.
- `control/cmd/flaminio-disruption-guard/`: command, identity/TLS reload, bounded quiescing/exit watchdog and verified cgroup controls.
- `api/proto/flaminio/meta/v1/meta.proto`, generated bindings, metadata service/KV backends,
  Rust metadata client compatibility, `meta/service/auth.go`: exact reader appendix changes.
- `deploy/disruption/`: namespace/PSA, installer/runtime RBAC, reader/serving credentials,
  Service/Deployment and two webhook configurations; no PDB or per-node built-in workloads.
- `scripts/checks/disruption-guard.sh`, script fixtures, `scripts/lab.sh`, Makefile help:
  separate API qualification and authorized physical-host data acceptance dispatch.

## Contracts

Normative contracts include the six linked appendices, including the explicit resource lifecycle. No storage format, replication frame, metadata
mutation RPC or control-socket line changes. New metadata read fields require `make proto`,
Rust client compatibility and `make contracts`; preserve every method's authorization.
Owned ConfigMap/finalizer/AdmissionReview are availability state, never data fencing tokens.

## Tests

| Rules | Planned proof |
|---|---|
| S1/S7 | `enrollment_test.go`: Node precommit/AlreadyExists behavior, same-name claims and delayed overlap, bounds/recreation, actual ownership/finalizer and bootstrap coverage |
| S2 | `reader_test.go`, real service/mTLS integration: strict enumeration/history proof, all bounds, exact-name/UUID lifetimes and page predicates, nonstorage/old-store retirement, unknown refusal |
| S3/S7 | `admission_test.go`: CAS contention/lost reply, old Pod retirement, stale-process release and cancellation; real A/B plus C surge/readiness fixture |
| S4/S5 | Resource exhaustion/cgroup refusal, known terminal reader error and supervised replacement preserve durable state and fail closed; real API: stock kubectl UID patch, Pending/terminal/deleting/GC anchors, controller death before/after reservation, metadata loss, held/admitted standard drains |
| S6 | Actual authorizer positive status/Event path and negative node/tenant/Secrets/stronger namespace/credential paths, Restricted Pod admission |
| S8 | [qualification](qualification.md) evidence matrix and original authorized RF2/minISR1 stop/return/drain/data verification |

Future implementation checks: `make lint test race hygiene contracts`; bounded reducer and
ownership/CAS state-machine tests plus actual API-server fixtures are required. This adds no
replication/membership/recovery safety rule, so no new replication simulator rule is asserted.
Docs-only draft validation is link/whitespace checking, not a runtime test result.

## Lab

No lab action is authorized now. Proposed `make disruption-guard-api-test` runs disposable
pinned API fixtures; `make disruption-guard-test` requires the qualified physical profile,
prerecorded prior node/guard state, dedicated verified RF2 data and explicit stop/cordon consent.
Exact prerequisites, cleanup/refusal and outcomes are in [qualification](qualification.md).

## Docs

Update README §10.4 fraction and actual installation/bypass contract, `docs/lab.md` qualification
matrix/commands, `docs/followups.md` shared ownership and Makefile help when implemented.
#63 remains the original-slice composition wrapper and waits for this owner's actual clear plan;
#48 later composes it. No autoscaler stage is added or completion inferred from moving text.

## Risks

Availability: synchronous webhook failure blocks all eviction requests and Node creates/updates,
a never-committing first same-name create and staging/lifetime4096 saturation hold new nodes
until privileged maintenance; ordinary existing-name kubelet restarts allocate no new state,
the global slot is intentionally conservative, finalizers
can retain deleting anchors and management-node loss needs privileged repair. Security: dynamic
namespace Pod/ConfigMap create is namespace authority; isolation and absence of stronger accounts
or credentials is essential. Compatibility: only the pinned API/k3s version is claimed after
actual fixture qualification. Storage: stale observations can cost availability, never change
write acknowledgement/fencing. Resource containment/recycling may also hold admissions or repeat process restarts under hostile/broken peers; it does not promise uninterrupted maintenance availability. These costs are concretely Proposed, not human-accepted.

## ADRs

Accepted 0001–0004 remain consistent: Go/Kubernetes availability control, dedicated metadata
service, same kernel/target profiles. 0005 is not used to relax current takeover checks; proposed
0007+#83 clock/physical constraints remain. Accepted0006 minISR is read unchanged. No accepted
ADR is superseded. [Local Proposed ADR](adr-proposal.md) records this expensive-to-reverse
synchronous admission boundary; numbering/index move follows acceptance, not this draft.

## Parallelizable?

Reader/auth and reducer can proceed independently against exact appendices. Admission,
enrollment and finalizer lifecycle share the durable contracts and need one integrated owner;
real API qualification must review their combined behavior. No implementation is authorized.
