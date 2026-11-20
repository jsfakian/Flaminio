# Issue #91 — synchronous admission and durable ownership contract

Proposed, unapproved. This contract replaces dual-PDB accounting; it creates no PDBs.
The webhook/controller is one binary `flaminio-disruption-guard` with HTTPS admission serving
and bounded reconciliation. No Lease is required; CAS is the fence.

## Pinned ordering and API

Kubernetes `v1.36.5`, k3s `v1.36.5+k3s1` only. Source facts and exact lifecycle follow below;
real API fixtures must verify k3s composition before runtime qualification.

Two `admissionregistration.k8s.io/v1` configurations are installed by the deployment identity:

- Mutating webhook `eviction-uid.disruption.flaminio.io`: operations CREATE, apiGroups [""],
  apiVersions ["v1"], resources ["pods/eviction"], scope Namespaced; `/mutate-eviction`.
- Validating webhook `eviction.disruption.flaminio.io`: same rule; `/validate-eviction`.
- Validating webhook `enrollment.disruption.flaminio.io`: CREATE,UPDATE on core/v1 `nodes`,
  scope Cluster; `/validate-node`. `nodes/status` is excluded.

All use `matchPolicy: Equivalent`, `failurePolicy: Fail`, `timeoutSeconds: 30`,
`admissionReviewVersions: [v1]`, `sideEffects: NoneOnDryRun`, no namespaceSelector,
objectSelector or matchConditions. Mutator has `reinvocationPolicy: Never`. Service
`flaminio-disruption-guard` in namespace `flaminio-disruption-guard`, port443 to container9443,
trusted injected caBundle; TLS validates that exact Service DNS. AdmissionReview UID is request
correlation only, NEVER a Pod or Node UID. Dry-run has no durable writes; it returns evaluated
hold/allow without reserving capacity, and cannot qualify a real drain.

## Durable objects and anchor

One precreated ConfigMap `flaminio-disruption-guard-state`, maximum16KiB JSON, contains
`schema:1`, random immutable `installationID`, monotonic `generation`,
`profilePhase: Bootstrapping|Active`, `allocatedEnrollments:uint32<=4096` and `slot:null|{nodeName,
nodeUID,anchorUID,anchorName,enrollmentUID,token,observationDigest,phase,metadataID,
candidateClass,retirementStores}`. `token` is random128bit;
phase Reserved|Retiring. No deadline/expiry returns capacity. Every update sends current
metadata.uid and resourceVersion; conflict restarts evaluation, never blind overwrites.

Per Node UID, `enrollment-<uid>` ConfigMap (≤4KiB including bounded status) records
installationID, name, NodeUID, state Staging|Enrolled|Deleted, monotonically increasing
anchorGeneration and current anchorUID/name, plus `metadataID`, sticky `everStorage` and
`observedStores` under [identity-contract](identity-contract.md). Limit4096 enrollment allocations, including
unresolved Staging/deleted-but-admitted records; allocation first increments the singleton
counter by CAS, then creates the enrollment with a random allocation token. Incomplete
allocation can leak a count but never exceed the bound. Counter never decreases automatically;
Deleted enrollment tombstones remain bounded and runtime has no ConfigMap delete right.
Same-UID retries resume the actual record. To bound concurrency and leaked
allocations, Node enrollment handler permits one pending allocation per serving process and
returns429 to other new UID requests, which Kubernetes clients may retry. Counter exhaustion
fails closed. The4096 lifetime allocation bound may require explicit privileged reinstallation
in a maintenance window; this is not automatic safe reclamation or a runtime reset API.
Names/UIDs pass Kubernetes grammar and bounded128byte identifier checks.
Records belong to this installation; matching labels alone cannot establish ownership.

Each enrollment owns one ordinary Pod `anchor-<uid>-<generation>` with `apiVersion:v1`,
`kind:ConfigMap`, exact enrollment name/UID, `controller:true`, `blockOwnerDeletion:false`
in its ownerReference. The guard implements that custom controller; no StatefulSet,
Deployment, ReplicaSet, DaemonSet or PDB accounting is involved. Pinned kubectl accepts this
actual controllerReference without a force flag; it does not require a scalable built-in
controller for drain. Pod has `spec.nodeName: nodeName`, immutable annotation
`flaminio.io/guard-node-uid`, finalizer `flaminio.io/disruption-anchor`, restricted idle
container, token automount false, no emptyDir/data credentials/host volumes. Explicit
nodeName is supported placement, not UID proof. Live Node GET plus durable enrollment
establish identity when admission occurs.

The finalizer is mandatory before the first Pod create. PodGC, kubelet or controller DELETE
may mark it deleting but cannot remove the interlock from API listings. The guard removes
this finalizer only for the exact slot-owned anchor UID using a UID/resourceVersion checked
update; only its own finalizer may be changed. Terminal/unready anchors remain covered and
can be evicted only through the same admission. **Every** image refresh, terminal replacement
or repair which deletes an existing anchor must first reserve the same single slot using
fresh eligibility. Otherwise an in-flight drain's old Pod snapshot could finish without ever
seeing its replacement. Readiness and duplicate/lookalike pods create zero credits. No
controller-driven deletion is assumed to respect PDBs.

## Automatic enrollment

Authenticate the API-server mTLS client under [security](security-contract.md) before parsing
an actionable review or allocating anything. `/validate-node` sees the generated Node UID
before CREATE's storage-existence check; a new UID in a POST is not evidence of a new node.
The following branches are mandatory, in this order, using actual uncached GETs:

1. **Existing name on CREATE:** GET Node(name). If present, ensure/resume coverage only for
   its actual existing UID, then deny this new CREATE with AdmissionResponse allowed=false,
   status code409/reason AlreadyExists/details `{group:"",kind:"nodes",name:<name>}`. Do not
   allocate the request's doomed new UID, Pod, claim or lifetime counter. It must emerge from
   the API server as `apierrors.IsAlreadyExists`, preserving stock kubelet's GET/reconcile
   path. If the node disappears concurrently, returning AlreadyExists is still conservative:
   kubelet GET sees NotFound and retries registration. Never allow an uncovered replacement
   CREATE merely because an earlier GET saw its predecessor.
2. **Absent name on CREATE:** use durable `nameclaim-<sha256(exactName)>` ConfigMap (≤1KiB,
   ≤4096 total), fields schema/installationID/exactName/requestedNodeUID/state Reserved|Observed.
   The unique ConfigMap name is the same-name arbiter; compare stored exactName against hash
   collisions. Allocate under the singleton lifetime counter before claim creation. Only the
   successful unique claim owner may create its UID enrollment/anchor. Concurrent losers
   return429 `NodeCreationInProgress`, create no Pod/enrollment and never return allowed=true.
   A losing counter increment can leak capacity but never adds an interlock/permission.
   Same exact requested UID resumes its owned claim by UID/resourceVersion; another requested
   UID cannot take it over while Reserved, even after timeout/cancellation/restart. If the
   first CREATE is delayed before commit, every contender holds until it commits. If it never
   commits, explicit maintenance is needed; no time-based reclaim is claimed.
3. **New owner preparation:** create/resume that requested UID's Staging enrollment and its
   ordinary anchor with exact enrollment ConfigMap controller ownerReference, explicit
   nodeName and finalizer. Node need not yet exist. Verify actual Pod UID/owner/spec/finalizer
   and persist them before allow. Re-read actual Node(name) immediately before allow: present
   means return AlreadyExists with no second-UID adoption; absent retains precommit coverage.
   Mark claim Observed and enrollment Enrolled only after actual Node GET matches that UID.
4. **Name reuse:** claim may advance by CAS to a new Reserved UID only after its prior UID
   was actually Observed and then actual GET proves it deleted/replaced. Preserve the old
   UID enrollment/anchor/slot for exact-store retirement, never transfer its grant. A Staging
   unobserved UID cannot be declared deleted from absence. Check actual current Node again;
   an existing name always uses branch1. Thus normal overlapping POSTs cannot plant doomed
   anchors on a committed node. Name claims contain no mutable identity authority.
5. **UPDATE/bootstrap:** use the actual persisted UID, resume/install only its owned coverage
   (or retained admission occupancy), never a newly generated UID. A Node UPDATE cannot
   release a slot. Initial existing-node coverage also creates the name claim in Observed
   state. The exact metadataID join/classification comes from [identity](identity-contract.md).

Cover every Node, including currently unlabeled nodes, so later storage registration creates
no empty-node drain gap. Do not attach a Node ownerReference before commit: garbage collection
could erase coverage while the Node is not visible. Claims/enrollments are bounded monotonic
lifetime allocations; routine kubelet POST against an existing name consumes **zero** new
allocations. Staging requests which may still commit remain protected indefinitely.

Initial installation first runs this same reconciliation for all existing Nodes. Deployment
reports Active only after bounded complete Node LIST plus enrollment/anchor verification and
both webhook configurations installed, then a second complete verification. Until Active,
webhooks deny anchor evictions; installation is not certified coverage for an existing empty
node before its first anchor. Installing into a running unguarded storage cluster therefore
requires the explicit installation precondition in design.md; no readiness label erases it.

## Eviction and slot lifecycle

At HTTP request-header completion record H once: reply/unwind deadline D=H+24s and
last actionable point A=H+23s. Body read/decode, initial object reads, reader work and all
later API/CAS attempts consume this same budget; no stage resets it. The last≤1s is reserved
for bounded reply/unwind. Clamp actual body/reader/RPC/API contexts and connection deadlines
as [security](security-contract.md) requires. Never dispatch a durable create/update/CAS at or
after A, including retries and Node-enrollment writes; a write submitted before A may still
commit after cancellation and its unknown outcome remains held, never reset or assumed failed.

An eviction outside `flaminio-disruption-guard` passes immediately without reading its Pod;
Pod namespaces are immutable and all anchors are in that fixed namespace. Inside the guard
namespace, only exact owned anchors are eligible. Anchor is the required final drain interlock. A lookalike/foreign object in the guard namespace denies rather than passes.

1. Mutator GETs the actual named Pod, verifies actual controlled anchor ownership/enrollment/finalizer,
   and adds `deleteOptions.preconditions.uid = Pod.metadata.uid` if absent. If already present,
   require equal; retain caller resourceVersion/grace/dryRun. It creates missing parent JSON
   objects with JSONPatch. Validating webhook checks this precondition again. It prevents a
   delayed admitted request deleting a same-name replacement; admission request UID cannot.
2. Validator GETs Node, enrollment, Pod and singleton state, validates exact-name join and
   classifies Storage|ProvenNonStorage|Unknown under [identity](identity-contract.md). Require exact NodeUID/owner/spec,
   committed enrollment, profile Active and a live serving process; otherwise return allow=false
   Status code429/reason `TooManyRequests`, message `FlaminioGuard:<boundedReason>`.
   Real API fixture must prove this AdmissionResponse status survives webhook dispatcher
   conversion as HTTP429 and stock kubectl retries the same pending drain; a wrapped403
   or generic transport failure is not a passing held-then-progress result.
3. Acquire the nonblocking process-wide collector semaphore (one holder across admissions
   and reconciliation); Busy returns429. Reader deadline is min(readerStart+20s,A), not20s
   added after earlier work. Each metadata RPC deadline is min(RPCStart+2s,readerDeadline);
   each initial/later Kubernetes read or CAS deadline is min(operationStart+2s,A). Use actual
   remaining time and stop/reply hold if it is exhausted; no later API/CAS allowance is added.
   Check A immediately before dispatching each durable write/retry. No cached pass authorizes.
   Unknown/unsafe returns429. Cancellation aborts actual work; an already-submitted CAS with
   unknown outcome retains any possible reservation even if A or D passes.
4. Before allow persist sticky everStorage/observedStores and copy candidateClass/metadataID/
   retirementStores into the slot. If slot null, CAS Reserved for exact NodeUID/anchorUID,
   then return allow. If already
   same node AND same anchor UID, recheck metadata and return allow without changing credit.
   Other slot owner or anchor generation returns429 `OtherDrainInProgress`. CAS conflict
   re-reads the owner and restarts the entire pass, at most once within remaining budget.
5. Reservation persists even if another webhook/PDB rejects, response is lost or client cancels.
   No attempt to infer success from HTTP response or elapsed time. Kubernetes PDBs may further
   refuse; they cannot grant another slot. Same-node retries still require a fresh reader.
6. After admission, wait for deletionTimestamp on the exact slot-owned Pod, then remove only
   this finalizer using UID/resourceVersion update. A crash before this step leaves drain
   waiting; a crash after it leaves durable slot occupancy. Never remove an unreserved anchor's
   finalizer, including terminal/GC cases. While node cordoned/dead/draining, retain slot and
   do not create a replacement that returns credit. The completed drain may leave no anchor:
   its durable slot covers that admitted node until recovery. Recovery requires Node
   same UID schedulable+Ready, live metadata store and full relevant earned ISR, plus fresh
   complete collection under the Storage branch in [identity](identity-contract.md). A
   ProvenNonStorage candidate instead re-proves no-store-history/no-reference predicates;
   it requires no store liveness/full ISR. Appearance of storage latches everStorage and
   switches to Storage/Unknown conservatively. CAS Reserved→Retiring before deleting the exact old anchor with UID
   precondition and
   removing its finalizer if it still exists. Observe old UID absent, increment enrollment
   anchorGeneration by CAS, create/read back a matching finalizer-bearing replacement, then
   require it Ready and Node still the same UID before CAS same token Retiring→null. If
   a Node UPDATE already created that owned replacement, resume it rather than incrementing
   again; never replace a foreign generation or reuse its permission. Cancellation
   follows this same automatic path after administrator uncordons; no Flaminio CLI/timeout reset.
   Deleting this owned anchor retires the admitted UID; it is not eviction of tenant workloads.
7. On deletion/recreation, use saved exact metadataID/retirementStores under
   [identity](identity-contract.md), never query metadata for a Kubernetes UID it lacks.
   Old-anchor absence plus either returned-exact-store recovery or moved-away old-store
   predicates permits release; unknown/old active refs retain occupancy. New same-name UID
   needs independent anchor coverage and fresh admission. Old nonadmitted anchor cleanup
   also consumes the slot; old enrollment tombstone and exact name claim remain retained.

The admission linearization point is the durable slot CAS; allow sent before a subsequent
failure can finish afterwards. There is no way to revoke an already executing API operation
by killing the webhook. Admission observations remain availability observations, not an atomic
transaction with subsequent metadata changes or physical failures.

## Race proof and required counterexamples

Distinct nodes cannot both change null at the same ConfigMap resourceVersion. Retries/overlap
cannot add credit: only one slot exists. No Ready count or denominator changes that result.
A stale release fails CAS; a resumed old allow can touch only its preconditioned old Pod UID,
which must already be absent before release. A stale new-admission attempt revalidates actual
Node/anchor identities and cannot reuse a retired UID. Missing metadata/process yields no new
allow; durable Reserved is occupancy, not reusable permission for a different Node/anchor.

Required real API fixtures: RF2 A/B concurrent evictions while unrelated C has two Ready
lookalikes/rollout pods; Pending/Succeeded/Failed/deleting anchors; same-name new Node UID with
old enrollment/anchor surviving; routine kubelet repeated POST returns AlreadyExists with
zero allocation; concurrent different-UID POST holds without a ghost anchor; Node CREATE
paused before commit; outage after reservation; mutation
UID injection from stock kubectl; pause old eviction after webhook allow until retirement and
replacement, then resume (replacement survives); dropped CAS/allow responses; stale controller
release after takeover; client cancellation/uncordon; all existing-node installation preconditions.

## Pinned primary-source evidence

- [Generic create store](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/registry/generic/registry/store.go#L445-L490):
  system UID assignment precedes validating admission and storage commit.
- [Create handler](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/endpoints/handlers/create.go#L166-L192):
  mutation runs before the storage-specific validating admission callback.
- [Eviction implementation](https://github.com/kubernetes/kubernetes/blob/v1.36.5/pkg/registry/core/pod/storage/eviction.go#L131-L187):
  validating callback precedes Pod fetch/PDB-bypass branch. [Final delete](https://github.com/kubernetes/kubernetes/blob/v1.36.5/pkg/registry/core/pod/storage/eviction.go#L288-L340)
  preserves client UID preconditions; generic storage applies them to actual deletion.
- [kubectl drain](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/kubectl/pkg/drain/drain.go#L121-L161)
  constructs eviction without UID precondition. Its [evict/wait path](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/kubectl/pkg/drain/drain.go#L247-L328)
  retries429, treats404 as done, then [waits](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/kubectl/pkg/drain/drain.go#L386-L418)
  for original UID disappearance/change without relisting newly created Pods. Therefore no
  unadmitted anchor deletion/replacement is safe, including custom-controller image refresh.
- [Drain filters](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/kubectl/pkg/drain/filters.go#L217-L241):
  a controller ownerReference is accepted; terminal Pods remain evictable. Deleting Pods are
  skipped only with the explicit skip-wait override. No filter identifies ConfigMap-owned
  anchors as DaemonSets or mirror pods.
- [PodGC](https://github.com/kubernetes/kubernetes/blob/v1.36.5/pkg/controller/podgc/gc_controller.go#L318-L340)
  uses ordinary grace-zero DELETE, retaining custom finalizers; [generic deletion](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/registry/generic/registry/store.go#L1056-L1127)
  honors finalizers and UID preconditions rather than granting finalizer-removal authority.
- [Validating webhook dispatcher](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/admission/plugin/webhook/validating/dispatcher.go)
  bounds calls with timeoutSeconds and handles Fail/Ignore policy; Fail and explicit denials
  are required here. A timeout is never evidence that a previously completed durable CAS failed.

These are pinned source facts, not claims that the required k3s API fixtures already passed.

[Kubelet registration source](https://github.com/kubernetes/kubernetes/blob/v1.36.5/pkg/kubelet/kubelet_node_status.go#L84-L130)
POSTs before GET and handles AlreadyExists by reconciling the existing node. Real API fixtures
must assert the custom409 maps to this exact client predicate, not wrapped Forbidden/Conflict,
and that routine kubelet restart leaves one usable interlock and ordinary drain progression.
