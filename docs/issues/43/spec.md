# Issue #43: The operator — specification

The first part of the issue's step 1 ([intent](intent.md)). Approved 2026-11-13; not implemented:
[decisions](decisions.md). How: [design](design.md). Issue #12 is taken as built, as planned.

## Terms

- **The resource:** the cluster's one `FlaminioCluster` (`storage.flaminio.io/v1alpha1`,
  cluster-scoped, ARCHITECTURE §17.2).
- **Operator:** the Deployment `flaminio-operator`, and the one instance of it that reconciles.
- **Managed objects:** what the operator makes from the resource: the CSIDriver; the CSI
  controller and the failover controller (Deployments); the CSI node plugin and the data plane
  (DaemonSets); the StorageClass `flaminio`. The four in the middle are **workloads**.
- **Install manifest:** what an administrator applies before the resource, in two files.
  - **The workloads' accounts:** the namespace `flaminio-system`, the workloads' service
    accounts and their rights (`deploy/csi/rbac.yaml`, with #12's account).
  - **The operator's manifest:** the resource's definition, the namespace `flaminio-operator`,
    the operator's account and rights, its Deployment.
- **Inputs:** what the resource says: the metadata service's endpoints, the trust domain, the
  two images, the claim's disk serial pattern.
- **Converged:** every managed object matches the resource, and every workload's pods are
  ready and at the newest revision.
- **Attached:** a VolumeAttachment of `csi.flaminio.io` exists.

## The invariant

The operator is on no path that I/O, an attach or a failover takes: stopped, partitioned or
wrong, it leaves what is installed running as it is. And it never stops a data plane: it deletes
nothing, and no change to the resource, its deletion included, removes or restarts a data-plane
pod. Replacing data-plane pods stays `make node-upgrade`'s (#12, S5).

## Behaviour

### The resource

**S1. One resource.** Only a `FlaminioCluster` named `flaminio` can be created; any other name
is refused when written.

**S2. What it says.** The inputs, and nothing else. Names, the namespace `flaminio-system`,
ports, probes, tolerations and the StorageClass's parameters are fixed, as the manifests have
them today. So are the identities: the controllers' Secret, under its name, and the nodes'
files. The serial pattern is what `flaminio-agent claim --serial` takes, a regular expression
matched anywhere in a disk's serial. Only letters, digits, `_`, `:` and `-` are allowed, so it
reads "the serial contains".

**S3. Validation.** A resource whose form shows that it cannot be applied (an input missing,
malformed or over its bound) is refused when written. Every value that reaches a manifest or a
command line is validated first.
- **Before it writes anything,** the operator asks the API server whether each managed object
  would be accepted. If any is refused, no managed object is written, and the status names
  each.
- **What shows only later** (an image not on a node, the Secret missing, a write that fails
  midway) is reported for its object. What was applied stays, and the next pass goes on.

**S4. Status.** Summarized and low-frequency (§2, principle 1): the generation it reflects;
whether the cluster is converged; for each workload, the pods wanted, ready and at the newest
revision; what the operator waits for or is stopped by (S3, S6, S7, S9), in words an
administrator can act on. Data-plane pods behind are counted as waiting for `make node-upgrade`.
A workload that Kubernetes has not yet reported on since its last change counts as rolling,
whatever its numbers. The status is as of the operator's last pass (S6): whoever reads it
polls. It carries no volume, chunk or ISR state, and nothing from the metadata service.

### Reconciling

**S5. Install.** From the resource alone, the operator creates the managed objects. For the same
inputs they run what `make csi-deploy` applies today (with #12's data plane): the same
containers, arguments, mounts, privileges, selectors and service accounts.

**S6. Repair.** A managed object deleted, or a field the operator sets changed by anyone else,
is put back by the first pass of the operator that starts after the change. A pass starts at
the latest 30 s after the one before it ended. Fields it does not set are left alone. Two
exceptions:
- **The StorageClass** is put back only when deleted (S7).
- **A data-plane DaemonSet whose node selector, affinity or tolerations differ** is not written
  at all: putting them back can remove a pod. The status names the fields, and an administrator
  restores them.

**S7. Change.** A change to the resource reaches the managed objects. So does a new build of the
operator whose templates differ (a sidecar's version, a flag).
- **The controllers and the node plugin** roll as they do today.
- **The data plane's DaemonSet** is updated, and none of its pods is deleted or restarted.
- **The StorageClass** is created when missing, and never changed or deleted afterwards:
  PersistentVolumes name it.

**S8. Nothing without cause.** Reconciling a converged cluster writes nothing and restarts
nothing. Neither does a restart of the operator, or a new instance of the same build.

**S9. Adoption.** Objects that `make csi-deploy` created are taken over under the names they
have, whoever set their fields, and brought to the resource by S7's rules. No workload is
deleted and created again. An object that cannot be taken over without that (another selector,
another controller as its owner, an immutable field that differs) stops the operator, which
names it (S3). One limit: a field that `make csi-deploy` set and that a later build of the
operator no longer sets stays on an adopted object, until the cluster is uninstalled (S23, so
with every volume detached) and installed anew.

**S10. One instance reconciles** at a time, by a Lease. A second instance writes nothing while
the first holds it. A write that an instance sent before it lost the Lease may still land: the
next pass puts the object back (S6). No safety rule rests on the Lease.

**S11. Deleting the resource** removes nothing (question 5). The operator keeps no hold on it:
it goes at once, the managed objects stay and run, and nothing reconciles them until a resource
exists again, which takes them over (S9). Uninstalling is a lab command (S23).

**S12. Removing the operator,** the resource, its definition or the whole operator's manifest,
in any order, removes or restarts no workload. Installed again, the operator converges without
restarting one. Removing the workloads' accounts removes `flaminio-system` and all it holds, as
today: the docs say so.

**S13. Independence.** With the operator stopped, or the API server out of its reach, I/O,
attach, detach and failover go on as before. It makes no metadata call.

### Security

**S14. No key.** The operator holds no CA key, no Flaminio identity and no token key. Its only
credential is its service account's token. It reads no Secret.

**S15. Its rights,** all of them:
- **Read, by name:** the resource and the six managed objects. It also lists and watches
  `FlaminioCluster`s.
- **Change, by name:** the resource's status; the CSIDriver; the four workloads.
- **Create, under any name** (Kubernetes cannot limit a creation by name): a CSIDriver, a
  StorageClass; a Deployment or a DaemonSet in `flaminio-system`; in `flaminio-operator`, a
  Lease, and events, which it may also change.
- **Its own Lease:** read and update.

It may not: delete anything; write the resource itself; change the StorageClass; read or write
Secrets; read pods; write service accounts, roles, bindings, namespaces, nodes or resource
definitions; evict or exec into a pod.

**S16. What those rights amount to** is stated, in the docs and in an ADR (question 4).
- **The operator's account** can run a privileged pod on any node, the witness included:
  `flaminio-system` admits one, and the witness's taint can be tolerated. That is root on every
  node, with the API server, its datastore and the Secrets' encryption key. It can also run a
  pod under any service account of `flaminio-system`, or one that mounts the controllers'
  Secret. It is a cluster administrator, and an administrator of the metadata service.
- **Whoever may write the resource** chooses the images that run privileged on every data node
  and that mount the Secret: the same. The install manifest lets only cluster administrators.

**S17. Its pod** runs in its own namespace, `flaminio-operator` (question 3), under Pod
Security `restricted`; on the witness, beside the CSI controller, where no privileged Flaminio
pod runs; not as root, with a read-only root filesystem, no capability, no host mount and no
host network.

**S18. Rights are granted by the install manifest only.** Its two files hold every service
account and its rights. Those of the managed workloads are what `deploy/csi/rbac.yaml` and #12
grant today. The operator grants none.

### The lab

**S19. Installing.** One lab command, `make operator-deploy`, applies the install manifest and
one `FlaminioCluster`; run again, it updates the resource. It still builds and imports the
images, issues the identities and the Secret, and prepares and labels the nodes, as
`make csi-deploy` does. Then `make csi-test`, `make csi-failover` and `make node-upgrade` pass
unchanged.

**S20. One installer per cluster.** `make csi-deploy` and `make csi-undeploy` refuse on a
cluster that has a `FlaminioCluster`, and when they cannot tell. A cluster without the
definition, or with the definition alone, has none. The new commands refuse in the Mac demo,
which is unchanged.

**S21. Unchanged.** etcd and the metadata service, as host units; fl-c; the benchmarks and
checks on the units; certificates (`make pki-issue`, #16's renewal); the managed workloads'
behaviour, identities and rights; the metadata API; `make node-upgrade` and #12's refusals.

**S22. Docs.** `docs/lab.md` and the Makefile's help: the new commands, what they still do by
script, how to uninstall, what removing each file of the install manifest does (S12), what a new
build of the operator rolls (S7) and cannot remove (S9), and that `make operator-deploy` is the
way back after the resource was deleted. README and ARCHITECTURE §3, §15.2, §17.2, §18: what exists,
and what is still planned. Follow-ups: the steps left. The ADR of questions 1 to 5.

**S23. Uninstalling.** One lab command, `make operator-undeploy`, in this order:
1. it removes the test namespace, then refuses as #12's `make csi-undeploy` does (a volume
   attached), by the same code, before it changes anything else;
2. it removes the operator's Deployment, waits until its pod is gone, then removes the resource;
3. it removes what `make csi-undeploy` removes, the same way (#12).

It leaves the install manifest's other objects, the Secret, PersistentVolumes, disks, metadata
and certificates.

## Tests

- **S1–S3:** unit, and against an API server: each refusal and each bound, and what a valid
  resource holds. A refusal before writing leaves no managed object written (counted); a write
  that fails midway leaves what was applied, is reported, and the next pass completes it.
- **S5:** unit: the objects made from the lab's inputs equal the manifests `make csi-deploy`
  renders from the same inputs. One source, so that they cannot drift.
- **S4, S6–S12:** against an API server, without nodes: install; repair of each object, whoever
  changed it, and S6's two exceptions; each change of S7, a new build included; status at each
  stage; a second pass and a restart write nothing (counted); adoption of objects that another
  tool applied with other values, and the object that stops it; deleting the resource, the
  operator, the definition and the operator's manifest, in each order, then installing again;
  two instances, and a late write. No managed object has an owner (no garbage collector runs
  in these tests). Each injected bug fails them: an operator that deletes the data plane's
  DaemonSet; one that changes its update strategy; one that writes it under S6's exception.
  Without nodes there is no pod: the checks are on the requests.
- **S14, S15, S17, S18:** on the install manifest: the account's rules are exactly S15's; run
  under that account, the operator converges with no request refused, and each thing S15 forbids
  is refused; the pod is admitted under `restricted`; no Secret mounted; the workloads' rights
  equal today's.
- **S19, S20, S23:** script tests with the VMs faked (`make lint`): the order, and each refusal.
- **The lab, a later step.** A check that reads the status, or waits for a repair, polls for
  120 s: two passes taken as 15 s each at most, the 30 s between them, one change of leader
  (17 s), and a margin of 43 s, which also holds the pod's restart.
  1. **From the script to the operator,** with a volume attached: the workloads are adopted, no
     data-plane pod restarts, the pod's I/O sees no error. `make csi-test` passes.
  2. **Repair:** the node plugin's DaemonSet and the CSIDriver, deleted, are back within that
     time. The data plane's DaemonSet, its image edited, is put back with no pod restarted.
  3. **A new image in the resource:** the controllers roll; data-plane pods wait, counted in the
     status; `make node-upgrade` replaces them; the status is converged.
  4. **The operator stopped (S13):** `make csi-test` and `make csi-failover` pass.
  5. **Rights:** `kubectl auth can-i` as the operator's account, for each line of S15.
  6. **Deleting the resource while attached** removes nothing, and the pod's I/O sees no error;
     created again, it is converged with no pod restarted. `make operator-undeploy` refuses
     while attached; once detached, it uninstalls.

## Open questions

The rules above follow each recommendation. Approved 2026-11-13; not implemented
([decisions](decisions.md)).

1. **The framework** (a new dependency; needs an ADR).
   - **(a) Recommended:** controller-runtime with client-go, which ADR 0001 names. It brings
     watches, caches and leader election, which the later steps need too. Cost: a large
     dependency tree in the module that also builds the metadata service, and an API server
     binary for the tests in CI.
   - **(b)** Grow the client written by hand (`control/failover/kube.go`): no dependency; the
     watch and apply logic is ours to get right.
   - Settled at approval, 2026-11-13: (a), controller-runtime with client-go (see decisions.md, Settled at approval).
2. **The resource API** (a new public surface; needs an ADR). Recommended: the group and
   version of §17.2; `v1alpha1` with no compatibility promise; S2's fields as the first ones;
   the definition shipped in the install manifest, not written by the operator.
   Settled at approval, 2026-11-13: `storage.flaminio.io/v1alpha1`, as recommended (see decisions.md, Settled at approval).
3. **Where the operator runs, and where the CA's key will live** (step 2; it fixes S17 now).
   - **(a) Recommended:** its own namespace. The key becomes a Secret there, which only the
     operator's account reads. That protects the key from mounts only: pods of
     `flaminio-system` cannot mount it, but in this lab whoever can create a workload there is
     root on the witness (S16) and reads it, as the API server's administrators do. Compact
     mode (#55) must not undo this.
   - **(b)** In `flaminio-system`: every account able to run a pod there mounts the key.
   - **(c)** Outside Kubernetes (a KMS, or an offline root and an intermediate): strongest, and
     another dependency.
   - Settled at approval, 2026-11-13: (a), its own namespace (see decisions.md, Settled at approval).
4. **The operator's rights** (S16).
   - **(a) Recommended:** accept them for this step, with S15, S17 and S18, in an ADR beside
     #12's ADR 0007.
   - **(b)** Also an admission policy that holds its account to the known pod templates: more to
     build, and to keep in step with the manifests.
   - Settled at approval, 2026-11-13: (a), the rights accepted for this step, in the ADR proposal (see decisions.md, Settled at approval).
5. **Deleting the resource** (S11).
   - **(a)** The operator uninstalls, once nothing is attached. It needs a hold on the resource
     (a finalizer, so the right to write the resource, and a definition that cannot go while
     the operator is gone), and an order that excludes a new attach.
   - **(b) Recommended** since plan review round 1: it removes nothing; uninstalling is a lab
     command (S23).
   - Settled at approval, 2026-11-13: (b), it removes nothing (see decisions.md, Settled at approval).
6. **The order of the steps left.** Recommended: the CA next (it unblocks #16's step 1), then
   the metadata service in Kubernetes, then ordered upgrades. If #12 is late, this step can ship
   without the data plane's DaemonSet, and add it after.
   Settled at approval, 2026-11-13: the CA next, then the metadata service in Kubernetes, then ordered upgrades (see decisions.md, Settled at approval).
