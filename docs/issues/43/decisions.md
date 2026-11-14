# Issue #43: The operator — decisions

Choices of the [spec](spec.md) and the [design](design.md). Each line gives the choice, the
reason, and the review finding that raised it.

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

**The spec's open questions** (`spec.md`, Open questions), each as recommended:
- **1. The framework:** (a), controller-runtime with client-go. `spec.md`, question 1; here,
  Proposed choices; the [ADR proposal](adr-proposal.md).
- **2. The resource API:** `storage.flaminio.io/v1alpha1`, no compatibility promise, S2's fields
  first, the definition shipped in the install manifest. `spec.md`, question 2; here, Proposed
  choices; the ADR proposal.
- **3. Where the operator runs, and where the CA's key will live:** (a), its own namespace,
  `flaminio-operator`; a future CA key a Secret there, protected from mounts only. `spec.md`,
  question 3 and S17; here, Proposed choices; the ADR proposal.
- **4. The operator's rights:** (a), accepted for this step, with S15, S17 and S18, in the ADR
  proposal; no admission policy. `spec.md`, question 4 and S16; here, Proposed choices;
  `design.md`, Risks (Security).
- **5. Deleting the resource:** (b), it removes nothing; uninstalling is a lab command (S23).
  `spec.md`, question 5 and S11; here, Proposed choices.
- **6. The order of the steps left:** the CA next, then the metadata service in Kubernetes, then
  ordered upgrades. `spec.md`, question 6; here, Proposed choices.

**The choices from plan review rounds 1 to 9** (here, Proposed choices, "From plan review round
1"), each settled as it states: no teardown, no finalizer, no delete right; `make
operator-undeploy`'s order; two files in the install manifest; forced server-side apply of
exactly the rendered fields; kubectl's ownership of adopted objects left as it is; no marker;
S3's dry-run refusal; S6's DaemonSet whose scheduling fields differ, not written; S10's one
reconciler by a Lease; S15, exactly the design's table; the StorageClass by a plain create; a
workload's numbers counted only at its `observedGeneration`; one watch, on the resource, managed
objects read by name, uncached; the reconciler's own client; the pass written once, in the design;
time bounds stated in passes; no gate on a build that drops a field; S15 in the words RBAC can
enforce; the port's range in the pattern; objects decoded into their typed kinds; no right to
read pods, ControllerRevisions or VolumeAttachments; S16's cluster administrators; no Event
about the resource or a workload; `diskSerialPattern`; one character set for every input; three
injected bugs; a new build of the operator a change like any other; `runAsNonRoot: true`; the
Secret's fixed name; `operator deploy`'s order.

**No recommendation in the plan; these stayed open.** Settled 2026-11-13 (user's instruction): see
Settled at the user's instruction of 2026-11-13, below.
- **The controller-runtime and client-go releases for k3s 1.36.** `design.md`, Risks (Open);
  the ADR proposal, Still open (Versions).
- **How CI gets the API server's binaries.** `design.md`, Risks (Open); the ADR proposal, Still
  open (Versions).
- **#12's plan putting its ServiceAccount in `rbac.yaml`, and bounding its own values:** a
  dependency on #12's plan. `design.md`, Risks (Open).
- **The API's lifecycle** beyond this step, which plans no conversion between versions. The ADR
  proposal, Still open.

## Settled at the user's instruction of 2026-11-13

- **The controller-runtime and client-go releases for k3s 1.36:** client-go at the Kubernetes
  minor of `K3S_VERSION` (v0.36.x) and the controller-runtime release whose `go.mod` requires
  that minor, at their newest patches when the implementation pins them; the names written from
  memory are checked against them. Recommended by Claude, no recommendation in the plan: the ADR
  proposal asks for a compatible pair for the lab's Kubernetes minor, and matching the client to
  the server's minor is the supported skew, with no guessed release number.
- **How CI gets the API server's binaries:** controller-runtime's `setup-envtest`, at the pinned
  controller-runtime version, fetches the kube-apiserver and etcd of that Kubernetes minor into a
  cache from `make test` and `make race`, which set `KUBEBUILDER_ASSETS`; CI's `make race` job
  needs no step of its own, and a missing binary or failed fetch fails the tests, never skips
  them. Recommended by Claude, no recommendation in the plan: it is envtest's own path for the
  design's envtest tests, adds no CI job, and keeps the design's rule that a missing binary
  fails.
- **#12's ServiceAccount in `rbac.yaml`, and #12 bounding its own values:** #12 puts its
  ServiceAccount `flaminio-dataplane` in `deploy/csi/rbac.yaml` and bounds its own values, as
  the design requires; #43 is implemented on #12 as built, the renderer keeps refusing a
  ServiceAccount in a template, and `TestRender` runs on #12's real template. The plan's
  recommendation (`design.md`, Risks, "must" and "should").
- **The API's lifecycle beyond this step:** `v1alpha1` with no compatibility promise and no
  conversion in this step; the step that adds a second version plans its conversion, in an ADR
  that supersedes this one. Recommended by Claude, no recommendation in the plan: spec question 2
  already promises no compatibility for `v1alpha1`, so planning a conversion now would build for
  a version that does not exist, and deferring it weakens nothing.

## Proposed choices

**From the spec's open questions,** each as recommended there:
- **Question 1:** controller-runtime and client-go.
- **Question 2:** `storage.flaminio.io/v1alpha1`, the definition in the install manifest.
- **Question 3:** the operator in its own namespace. It protects a future CA key from mounts
  only (R1-S1).
- **Question 4:** the rights of S15, with S16's statement, in the [ADR proposal](adr-proposal.md).
- **Question 5 (b):** deleting the resource removes nothing. Changed in plan review round 1.
- **Question 6:** the CA next.

**From plan review round 1:**
- **No teardown, no finalizer, no delete right.** The operator never deletes; uninstalling is
  `make operator-undeploy`. This is the simplest answer to three findings: a teardown needs a
  rule that excludes a new attach (R1-B2), a finalizer blocks the definition's removal
  (R1-B3), and a terminating resource needs repair to go on (R1-B5).
- **`make operator-undeploy`:** the refusal first, then the operator, then the resource, then
  what `make csi-undeploy` removes, by #12's code and with #12's refusal (R1-B2, R1-B8).
- **Two files in the install manifest.** `deploy/csi/rbac.yaml` stays as it is, and holds
  `flaminio-system`. The operator's manifest can be deleted alone (R1-B8, nit 3).
- **Forced server-side apply of exactly the rendered fields.** A refusal is left only for
  identity: a controller owner, an immutable field (R1-B1).
- **kubectl's ownership of adopted objects is left as it is.** Rounds 1 and 2 added a step
  that moved it to the operator; no finding asked for it, and round 3 cut it (R3-B1, R3-C2):
  it patched the StorageClass, which the operator may not patch, and a DaemonSet that S6
  excepts. Instead the plan states the limit: the forced apply owns every field the operator
  renders, and a field that a later build drops stays on an adopted object until it is
  installed anew (S9). Taking the ownership is a follow-up.
- **No marker.** No label, annotation or owner reference: the objects are found by name, so a
  new resource takes them over (R1-B7, nit 4).
- **S3:** a refusal in the dry run leaves every managed object unwritten; what fails later is
  reported, and what was applied stays (R1-B6).
- **S6:** a data-plane DaemonSet whose scheduling fields differ is not written (R1-B6).
- **S10:** one reconciler by a Lease; a late write may land and is put back; no safety rests
  on it (R1-B6).
- **S15, exactly the design's table.** No right on the resource but reading it and patching
  its status. StorageClasses: read and create (R1-B4).
- **The StorageClass is made by a plain create,** never an apply: it is never updated, so
  `create` is the only write right it needs. Every other row of the table names what uses each
  verb (R2-B1, R2-C7).
- **A workload's numbers count only once its `observedGeneration` is its generation** (R2-B2).
- **One watch, on the resource; the managed objects are read by name in each pass,** uncached.
  So no informer needs a namespace, and every read of a managed object carries its name
  (R3-C4). No cache option is relied on (R4-C2, R5-2).
- **The reconciler has one client of its own,** built without a cache from a copy of the
  configuration that carries the request timeout. The manager's cached client is never given
  to it, and the manager's configuration has no timeout, so its watch and its election are
  not cut. The same function builds the operator for `main` and for the tests (R6-1, R6-4).
- **The pass is written once, as a numbered list in the design ("A pass"),** and nothing
  else restates it (R7-1 to R7-3). What it settles:
  - only a resource that is not found ends the chain of passes; every other outcome, a
    failed read of the resource included, asks for the next pass after 30 s, with no
    framework backoff (R4-C1, R7-1);
  - no pass deadline: round 4 added one that no finding asked for; a request that fails or
    does not answer is an error of its step (R5-1);
  - the inputs are checked and rendered before any managed object is read, and a failure of
    either is `InvalidInput` (R3-C4, R7-2);
  - the status is patched only when it differs from the one read (R6-2);
  - `Blocked` has one order of reasons, and its message lists every object found (or `spec`),
    each entry's text at most 512 bytes, so the patch always fits (R8-1, R9-1, R9-2);
  - the StorageClass, which is never changed or deleted, is not checked for an owner (round
    9, nit 2);
  - the watch ignores updates that leave the generation unchanged, so the operator's own
    status patch starts no pass (R7-3).
- **Time bounds are stated in passes:** a change is repaired, and in the status, once two more
  passes have ended. The one figure in seconds is the lab's 120 s poll, derived in the design
  with one change of leader and a margin that holds the pod's restart (R4-C3, R5-3, R6-3,
  R7-4).
- **No gate on a build that drops a field.** The docs of that build say what it needs: every
  volume detached, and the data plane removed until the reinstall. A check that fails such a
  build until the ownership follow-up exists is itself a follow-up: this step has no previous
  template to compare with (R4-C4).
- **S15 in the words RBAC can enforce:** by name where a verb can carry one, "any name" for
  creations (R3-C3).
- **The port's range is in the pattern,** so the schema and `Validate` hold the same bound
  (R2-C6).
- **Objects are decoded into their typed kinds,** so a value read as a bool or a number is an
  error, never another value (R2-S1).
- **No right to read pods, ControllerRevisions or VolumeAttachments.** The status counts the
  data-plane pods behind, from the DaemonSet's status, and no longer names them. R1-B4 proposed
  granting the revisions; not granting is smaller.
- **S16:** the account, and the resource's writers, are cluster administrators (R1-S1).
- **No Event about the resource or a workload:** only the election's, about the Lease (R1-C3).
- **`diskSerialPattern`,** letters, digits, `_`, `:` and `-` only: every character is literal,
  so it reads "the serial contains". An exact serial cannot be required: `^` and `$` are left
  out, as `$` is shell syntax in #12's fragments (R1-S2).
- **One character set for every input,** with bounds, checked by the schema and in Go. The
  templates are substituted as text, as the lab does. No IPv6 literal in this step (R1-S3).
- **Three injected bugs:** an operator that deletes and creates the data plane's DaemonSet,
  one that renders it `RollingUpdate`, and one without S6's exception. The tests count requests
  (R1-C4, R2-C4).
- **A new build of the operator** is a change like any other (S7); S8 holds for the same
  build (R1-C5).
- **`runAsNonRoot: true`** (R1-C6).
- **The Secret's name is fixed,** not a field (R1-C7).
- **`operator deploy`:** `rbac.yaml`, the Secret, the operator's manifest, the resource
  (R1-C1). A second run updates the resource. No definition means no resource (R1-C2).

## Kubernetes behaviour the plan depends on

None of it can be checked in this repository. Each line names the test that fails if it does
not hold (design, Tests).
- **A forced apply takes a field from another manager, and leaves the fields it does not
  declare:** `TestRepair`, `TestAdoption`.
- **A dry-run apply writes nothing, returns the object that would result, and refuses a change
  to an immutable field:** `TestPreflightRefusalWritesNothing`, `TestSecondPassWritesNothing`.
- **An applied field that the operator alone owns goes when it is dropped:** `TestNewBuild`.
  One that kubectl also owns stays: not tested, stated as S9's limit.
- **A `get` can be limited by `resourceNames`, and a manager that watches one kind lists and
  watches nothing else:** `TestRunsAsAccount`.
- **controller-runtime's and client-go's names** (`client.New`, `GetClient()`, the
  configuration's `Timeout`, a result that asks for a requeue after a time, a predicate on
  the generation) and the API group of the election's events (core) are written from
  memory: the modules are not in `go.mod` yet. They are to be checked against the versions
  the implementation pins.
  `TestRunsAsAccount` fails if a read is cached or an event is refused; `TestRepair` fails
  if a failed pass is not followed by the next one; `TestMidwayFailureResumes` fails if a
  request that does not answer hangs the pass.
- **A patch of the status subresource leaves the resource's generation unchanged:**
  `TestSecondPassWritesNothing` (no pass starts after a status patch).
- **Creating by apply needs `create` and `patch`; `resourceNames` limits `patch`; the election
  needs `get`, `create` and `update` on its Lease, and writes its events in the Lease's
  namespace:** `TestRunsAsAccount`.
- **Deleting a definition deletes its resources, and ends when none has a finalizer:**
  `TestRemovalOrders`.
- **Garbage collection removes only objects that have an owner reference:** no collector runs
  in the Go tests. They assert instead that no managed object has one and that the operator
  writes no other object (`TestRender`, `TestRemovalOrders`); the collector itself is seen in
  the spec's lab check 6 (R2-C5).
- **A workload's status describes its current template only when `observedGeneration` equals
  its generation:** `TestStatus` sets both; the real controllers are seen in lab check 3.
- **Pod Security `restricted` refuses a pod without `runAsNonRoot: true`:**
  `TestPodAdmittedRestricted`.
- **The tests' API server enforces RBAC and Pod Security:** the two tests above each need a
  refusal to happen.
- **A DaemonSet's `updatedNumberScheduled` counts the pods at its newest template under
  `OnDelete`:** no controller runs in the Go tests; the spec's lab check 3.
- **`kubectl delete --ignore-not-found` and `kubectl get -o name` succeed, printing nothing,
  when nothing matches:** the script tests fake `kubectl` (`undeploy-no-operator`). If the
  real one fails instead, `operator undeploy` dies before it removes a workload.
- **#12's template** is not in this tree: what the design says of it rests on #12's plan, and
  `TestRender` runs on the real file once it exists (R2-S1).
- **A DaemonSet's controller removes its pods from nodes that the template no longer selects:**
  the reason for S6's exception, not tested. If it does not hold, the exception is only
  stricter than needed.

No longer depended on: the rights a finalizer needs; where Events about cluster-scoped objects
are stored; that kubelet stages only after the VolumeAttachment is attached.

## Findings that do not apply as written

- **R1-B4, the revisions:** the right is not granted (above). The rest of R1-B4 is applied.
- **R1-B5:** with no finalizer there is no terminating state; the operator reconciles while
  the resource exists.
