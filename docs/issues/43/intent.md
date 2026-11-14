# Issue #43: The operator — intent

[Issue #43](https://github.com/jsfakian/Flaminio/issues/43), from ARCHITECTURE §3, §15.2, §17.2
and §18. This change is the first part of the issue's step 1: the `FlaminioCluster` resource, the
operator's Deployment, and the reconciliation of what runs in Kubernetes (the CSI driver, the
failover controller, and the data-plane DaemonSet of issue #12). The metadata service, the CA and
ordered upgrades are later steps: issue #43 stays open after this change.

## Why

1. **There is no operator.**
   - `control/` holds `agent`, `csi`, `drain`, `failover` and `cmd` (`flaminio-agent`,
     `flaminio-ca`, `flaminio-csi`). `api/` holds `proto` and `gen`: no resource types.
   - `go.mod` has no Kubernetes client: no `client-go`, no `controller-runtime`. The one
     controller that talks to the API server, `control/failover`, uses a client written by hand
     for nodes only (`control/failover/kube.go:21`).
   - ARCHITECTURE describes it as built (§3's table, §15.2, §17.2, §18), and `flaminio-ca` says
     "in a cluster the operator holds the CA" (`control/cmd/flaminio-ca/main.go:2`).
2. **What runs in Kubernetes is applied once, by a script.** `make csi-deploy` (`cmd_csi`,
   `scripts/lab.sh:991`):
   - applies `deploy/csi/rbac.yaml`: the namespace `flaminio-system` (Pod Security
     `privileged`), three service accounts and their rights;
   - issues the controllers' identity and puts it in a Secret, only when the Secret is missing
     (`:1003`);
   - builds the image, imports it into each node's containerd under a dated tag (no registry),
     and applies `deploy/csi/driver.yaml` with `{{CSI_IMAGE}}` and `{{META_ENDPOINTS}}` filled
     in: the CSIDriver, the CSI controller, the node plugin's DaemonSet, the failover
     controller, the StorageClass.
   - Issue #12 adds `deploy/dataplane/dataplane.yaml` the same way: the DaemonSet
     `flaminio-dataplane` (`OnDelete`), replaced node by node by `make node-upgrade`.
3. **Nothing reconciles.** An object deleted or edited stays so until someone runs
   `make csi-deploy` again. Nothing says in one place what is installed, at which image, and
   whether it is ready. `csi undeploy` deletes by rendering the template again with dummy values
   (`render_csi_placeholder`).
4. **The rest runs outside Kubernetes.** etcd and the metadata service are host units on fl-a,
   fl-b and fl-w (`cmd_etcd`, `cmd_meta`; `scripts/guest/etcd.sh`, `meta.sh`). fl-c has no
   kubelet (docs/issues/14). The CAs and the token key are on the workstation (`.lab/pki`,
   `cmd_pki`), and every certificate is issued from there (`make pki-issue`).
5. **Work waits for "the operator".** Issue #16's step 1 (the CA issues on CSR, after a
   TokenReview), #17 (draining unattached volumes), #55 (compact mode: the control plane in one
   Deployment), #69 (the UI it serves). Each needs the resource and the Deployment first.
6. **Whoever can create a workload in `flaminio-system` holds the cluster.** This bounds what an
   operator's service account can be.
   - The namespace admits privileged pods (`deploy/csi/rbac.yaml:11`). A DaemonSet there is root
     on every node it selects, the witness included: its taint can be tolerated and its label
     selected (`scripts/guest/k3s.sh:40-41`). In the lab, the data nodes hold the token key and
     the metadata service's keys (docs/issues/12, intent, Why 8).
   - The controllers' Secret holds `spiffe://<domain>/flaminio/admin/csi-controller`
     (`scripts/lab.sh:1004`). An `admin` identity may call every metadata method but
     `ChangeIsr` (`meta/service/auth.go:57`, `:81-82`). A pod that mounts the Secret is an
     administrator.
   - Secrets are encrypted at rest in the lab (`secrets-encryption`, `scripts/guest/k3s.sh:39`);
     the key is on fl-w, with the API server.
7. **A framework and a resource are new.** ADR 0001 fixes Go and names controller-runtime and
   client-go as what the control plane may use; neither is a dependency yet. No ADR covers the
   resource API (`storage.flaminio.io/v1alpha1`, §17.2) or the operator's rights.

## What we want

1. One `FlaminioCluster` resource says what is installed in Kubernetes: the images, the metadata
   service's endpoints, the trust domain and the claim's disk serial pattern. The identities
   stay where they are, under fixed names.
2. An operator, as a Deployment, makes the cluster match it and keeps it so: the CSI driver, the
   failover controller, the data-plane DaemonSet, the StorageClass. Its status says, in summary,
   what is ready and what is behind.
3. It can only make things safer than the script did. It is on no data path. It never stops a
   data plane: issue #12's ordered upgrade keeps replacing data-plane pods, and deleting the
   resource removes nothing.
4. Its rights are written down, as small as Kubernetes allows, and what they still amount to is
   stated. It holds no key in this step. Where a CA key will live is settled now, as it fixes
   where the operator runs.
5. The Kubernetes lab is installed by applying the operator and one `FlaminioCluster`.
   `make csi-test`, `make csi-failover` and `make node-upgrade` pass on it.

## Out of scope

- **The metadata service and its etcd in Kubernetes** (the rest of step 1; ARCHITECTURE §3, a
  StatefulSet). They stay host units, and the resource names their endpoints. It needs its own
  decisions: where etcd's data lives, the witness, its certificates.
- **Step 2: CA bootstrap,** and issuing on CSR (#16 step 1). Certificates stay `make pki-issue`
  and #16's renewal. How a node without a kubelet (fl-c) gets one is #16's question 2.
- **Step 3: ordered upgrades by the operator** (§18), and the storage disruption guard (§10.4).
  `make node-upgrade` (#12) stays the only thing that replaces a data-plane pod.
- **Step 4: replacing a lost metadata member** (§5.4, §11.1; phase1-plan, "What stays out of
  phase 1").
- **The other resources of §17.2** (`DevicePolicy`, `Device`, `Disk`, `StorageNode`, `Volume`,
  …), admission policy (§15.3), and `kubectl-flaminio`.
- **Status from the metadata service:** volumes, ISRs, node liveness. The operator makes no
  metadata call in this step, so it needs no identity.
- **Building and publishing images:** a registry, signing (§15.6). The lab still builds and
  imports them.
- **Compact mode** (#55), **the UI** (#69), **draining unattached volumes** (#17).
- **Nodes outside Kubernetes** (fl-c), the benchmarks and checks that drive the units, and the
  Mac demo: unchanged.
- **Separating metadata members from data nodes,** and the keys with them (docs/issues/12).
