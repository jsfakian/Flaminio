# Accepted with the plan, 2026-11-13: the first operator manages Kubernetes workloads without owning storage lifecycle

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12, revised 2026-11-13 after plan review round 1
- **Issue:** [#43](https://github.com/jsfakian/Flaminio/issues/43)

## Context

The [intent](intent.md) and [spec](spec.md) scope the first installer/reconciler step: CSI,
failover and #12's data-plane workloads. Metadata/etcd, CA bootstrap, operator-directed upgrades
and member replacement are later steps. Nothing in this issue's folder is approved
([decisions](decisions.md)).
Settled at approval, 2026-11-13: the spec and design are approved (see decisions.md, Settled at approval).

Go and Kubernetes-facing libraries fit accepted ADR 0001; choosing the dependency, the API and
the rights still requires an explicit decision. The data-plane privilege boundary and the
deployment identity are #12's and are preserved. The audit's proposed ADR 0007 is a different
topic and is not an accepted replacement for #12's branch-local privilege proposal.

## Decisions proposed

Settled at approval, 2026-11-13: each decision below, as stated (see decisions.md, Settled at approval).

- **The framework:** controller-runtime and client-go, pinned as a compatible pair for the
  lab's Kubernetes minor, not a larger hand-written client. Tests run against an API server.
- **The resource:** cluster-scoped `storage.flaminio.io/v1alpha1` `FlaminioCluster`, named only
  `flaminio`, with no compatibility promise. It holds the installation's inputs and a summarized
  status. Names, pod shapes and identities are fixed: it cannot supply a pod spec or a command.
- **One source of manifests:** the script and the operator render the same template files.
- **The operator owns the fields it renders, and nothing else.** It applies them server-side
  and takes them from any other manager. It adds no label, owner reference or finalizer, so
  nothing ties a workload's life to the resource, the definition or the operator.
- **It never deletes.** It has no delete right. Deleting the resource, the definition or the
  operator leaves every workload running; a new resource takes them over by name. Uninstalling
  is an administrator's command, with #12's refusals. The data plane stays `OnDelete`, and
  replacing its pods stays #12's `make node-upgrade`.
- **Where it runs:** its own namespace, under Pod Security `restricted`, on the witness, with
  no Flaminio key and no Secret access.
- **What its rights amount to:** its account can create any pod in `flaminio-system`, which
  admits privileged pods on every node, the witness included. It is a cluster administrator,
  and so is whoever may write the resource. This is a proposed boundary, not an accepted risk.
  Settled at approval, 2026-11-13: the boundary is accepted for this step (spec question 4 (a)) (see decisions.md, Settled at approval).
- **One reconciler at a time, by a Lease,** which is not a fence. Its writes are computed from
  the resource and are put back by the next pass. No storage safety rests on it.

## Options not taken

- **The operator uninstalls when the resource is deleted.** It needs a finalizer, so the right
  to write the resource and a definition that cannot go while the operator is gone, and an
  order that excludes a new attach (the CSI controller stopped first). Left to a later step.
- **An admission policy** that holds the operator's account to the known pod templates: more
  to build and to keep in step with the manifests. A future option, not claimed protection.
- **Naming the pods behind in the status** needs read rights on pods and ControllerRevisions.
  The status counts them; `make node-upgrade` finds them.

## Still open

- **Versions:** the framework pair for k3s 1.36, and how CI gets the API server's binaries.
  Settled 2026-11-13 (user's instruction): client-go v0.36.x and the controller-runtime release
  built on it; `setup-envtest` from `make test` and `make race` (see decisions.md).
- **The API's lifecycle:** no conversion between versions is planned in this step.
  Settled 2026-11-13 (user's instruction): `v1alpha1`, no compatibility promise and no conversion;
  the step that adds a second version plans its conversion, in a superseding ADR (see
  decisions.md).
- **A future CA key** (step 2): a Secret in the operator's namespace is protected from mounts
  only (spec, question 3). This step creates and reads none.
  Settled at approval, 2026-11-13: its own namespace (spec question 3 (a)) (see decisions.md, Settled at approval).

## Consequences

A compromised operator account remains powerful despite its restricted pod and its lack of
Secret access. Loss of the operator or of the API server stops reconciliation and adds no
dependency to live storage; a global API loss still prevents Kubernetes rescheduling.

Later #43 steps must implement learner-based, quorum-checked, serialized and restartable etcd
member replacement, and metadata-backed ownership for storage controllers across API outages.
This step does not satisfy those requirements, bootstrap a CA, order upgrades or fix #82.

## Confirmation

The [design](design.md) maps S1–S23 to tests against an API server, on the manifests and on
the lab's scripts, and to later lab checks. The tests count requests: no delete, and no change
to the data plane's strategy, selector or scheduling fields, with three injected bugs that must
fail. No runtime test, implementation, accepted risk or approval is recorded here.
Settled at approval, 2026-11-13: accepted with the plan; nothing is implemented (see decisions.md, Settled at approval).
