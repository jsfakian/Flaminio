# #79 ordinary unmount coordination — R2 contract accepted with the plan, 2026-11-13

Linux6.6 cannot atomically compare a mount ID and unmount it. The parent-FD pathname procedure
in [design](design.md) therefore supports the following concrete kubelet/deployment ownership
profile. This is a verified source contract and implementation requirements accepted with the plan, 2026-11-13, not a
claim that the current deployment or runtime tests already meet them.

## Verified source ordering

Initial profile is upstream Kubernetes **v1.35.0**, Persistent CSI volumes, attachRequired=true
and STAGE_UNSTAGE_VOLUME enabled. Source inspection at that tag establishes:

- [operation_executor.go](https://raw.githubusercontent.com/kubernetes/kubernetes/v1.35.0/pkg/volume/util/operationexecutor/operation_executor.go)
  MountVolume lines743–779 uses a volume-wide pending key for this attachable/device-mountable
  driver. UnmountVolume lines781–808 uses volume+pod; UnmountDevice lines810–837 uses volume-wide.
- [nestedpendingoperations.go](https://raw.githubusercontent.com/kubernetes/kubernetes/v1.35.0/pkg/volume/util/nestedpendingoperations/nestedpendingoperations.go)
  lines242–275 treats an empty pod/node key as a wildcard. Thus mount/map/device work conflicts
  with every pod unmount; same-pod unmounts conflict. Different-pod unmounts may run concurrently,
  but target different leaves and CSI's Node.mu serializes their actual mount effects.
- [csi_mounter.go](https://raw.githubusercontent.com/kubernetes/kubernetes/v1.35.0/pkg/volume/csi/csi_mounter.go)
  TearDownAt lines396–427 waits for NodeUnpublish success before removing mount directories;
  an RPC error/deadline returns without that removal. Setup creates parents before NodePublish.
- [csi_attacher.go](https://raw.githubusercontent.com/kubernetes/kubernetes/v1.35.0/pkg/volume/csi/csi_attacher.go)
  UnmountDevice lines521–535 waits for NodeUnstage success before global path cleanup.
  makeDeviceMountPath lines543–565 derives its globalmount path from driver and SHA256(handle).
- [csi_block.go](https://raw.githubusercontent.com/kubernetes/kubernetes/v1.35.0/pkg/volume/csi/csi_block.go)
  lines14–56 specifies MapBlockVolume only after NodePublish; UnmapBlockVolume removes kubelet's
  extra block-map binds/loop references before NodeUnpublish, and NodeUnstage follows device
  unreference. CSI does not recursively tear down these kubelet-owned external paths.

A client deadline does not release Node.mu in a still-running server handler. The next CSI call
waits for it; kubelet post-publish mapping requires successful publish response, while unsuccessful
unpublish/unstage does not remove filesystem parents. Test these exact cases; do not equate client
request cancellation with completion of a server's mount effect.

## Paths and deployment enforcement (S1, S6–S8)

Restrict stage/publish to these templates under configured ROOT, not arbitrary descendant paths:

- Filesystem stage: `ROOT/plugins/kubernetes.io/csi/csi.flaminio.io/SHA256(volume)/globalmount`.
- Filesystem publish: `ROOT/pods/POD_UID/volumes/kubernetes.io~csi/PV_NAME/mount`.
- Block stage: `ROOT/plugins/kubernetes.io/csi/volumeDevices/staging/PV_NAME`.
- Block publish: `ROOT/plugins/kubernetes.io/csi/volumeDevices/publish/PV_NAME/POD_UID`.

Require Kubernetes UID grammar and escaped PV name grammar; controller/node verify PV_NAME maps
to this CSI driver/volume, and POD_UID references that PV on this node using read-only API lookup.
Store the mapping in durable owner records for deletion retries after API objects disappear;
a new path cannot be adopted from a stale API cache. Hash filesystem staging volume handle exactly
as kubelet does; no caller-controlled driver component. Node SA gets only get/list/watch PVs/Pods
and get its Node for mapping/profile, no secret or workload mutation privileges.

Deployment supplies `--kubelet-profile=kubernetes-v1.35.0`; node checks its Node status kubeletVersion,
root path config, root ownership, sole process flock, mount namespace and admission-policy identity
before serving. Installer requires administrator-verified upstream kubelet binary/config inventory
and one kubelet per host/root. A vendor-patched binary with the same version is not this profile.
Unsupported versions/profile drift refuse startup; upgrade drains volumes and requalifies pinned
sources + ordering tests before changing inventory. Existing lab k3s versions are not inferred to
match. This is a concrete initially supported configuration, not an unspecified cooperation gate.

Ship a fail-closed ValidatingAdmissionPolicy/binding (failurePolicy=Fail, Deny) forbidding tenant
Pods with privileged containers, hostPID, SYS_ADMIN/SYS_PTRACE or any hostPath, and forbidding
mountPropagation Bidirectional. Enforce all namespaces; only administrator-controlled, exact
Flaminio node DaemonSet service account + controlled namespace is exempt. Deny other Pods using
that identity; RBAC forbids tenants creating/updating Pods/workloads/SA/policy in that namespace.
Apply equivalent rules to ephemeral containers and all init/container security contexts. Trusted
system mount agents must have explicit audited exemptions and must not access these allocated CSI
paths. The install validator rejects a competing writer exemption or overlapping hostPath manager;
no second driver socket/process or manual mount manager may operate these paths. Host root and
cluster administrators remain trusted; a root injector violating the contract is not atomically
fenced by umount2 and must never be represented as covered by an identity recheck.

The verified kubelet is the only external writer. Driver effects run under process flock/Node.mu;
mount propagation duplicates effects into that kubelet namespace, not an independent worker.
During cleanup retain the trusted parent FD, verify unique current mount/source and no children,
close every mount-root FD, umount2(parentfd/basename,UMOUNT_NOFOLLOW), and inspect absence before
success. Kubelet-owned empty/preexisting leaves can remain for kubelet's post-success removal;
CSI's ambiguous-leaf preservation rule remains unchanged. No lazy/force unmount or magical fd API.

## Required checks

- `control/csi` ordering harness mirrors the pinned executor/map/unmap wrappers: normal ext4 and
  writable block stage/publish/unpublish/unstage fully clean up, including extra block references.
- Barrier races: same-pod publish/unpublish and volume-wide stage/unstage never overlap kernel
  effects; different-pod unmounts are distinct and serialize in Node.mu. Deadline after RPC entry
  retains server serialization; error path does not remove parents; successful retry cleans up.
- Admission integration rejects a concurrently submitted external privileged/hostPath/propagation
  writer in each container class; unauthorized SA impersonation/policy edits fail. Non-root direct
  namespace/mount mutation fails permission checks. Authorized kubelet operations still succeed.
- Built CSI-image disposable ext4-loop test checks actual native normal umount success after
  inspection-root FDs close, no leaked mount/loop refs, and busy/stacked cases refuse. Record pinned
  kubelet profile, image digest/kernel/tools. No such runtime/admission tests ran in this draft.
