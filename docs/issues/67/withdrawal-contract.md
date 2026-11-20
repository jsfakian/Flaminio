# #67 consumer withdrawal — proposed review correction

Proposed and unapproved, 2026-11-20; SEC67-R1-01. Supplements the [design](design.md).
The accepted node-local tunnel and single static-claim design remain; this adds a host mount
helper gate, not a CSI driver, Kubernetes controller or gateway API credential.

## Owned host mount gate

Before publishing the consumer PV, the operator installs an owned dispatcher around the
actual kubelet NFS mount-helper entry points on both allowed workload hosts. An immutable PV
source is `(127.0.0.1, port, /flaminio/<export UUID>/<consumer PVC UID>)`; the UID is the actual
API-assigned consumer UID, not a name, annotation or current-state lookup substituted later.
The physical exported directory remains `/srv/flaminio/export`. Export_Id, FileSystem_ID and
NFS server owner/scope stay bound to the permanent export UUID; only the NFS pseudo-path and
mount admission lifetime include the consumer UID. Same-consumer process restart retains both
identities. A later consumer changes its source without changing the backing filesystem identity.

All loopback sources under the reserved `/flaminio/` prefix are owned inputs, including old,
unknown or malformed export/consumer IDs and ports. Canonically parse the exact path, compare
both IDs and endpoint against the durable current Serving record, and reject any mismatch
before helper launch. Never delegate a stale/unknown owned source as unrelated NFS. Only
sources outside this reserved owned class delegate unchanged to the retained real helper with
an argv vector, never shell text. Preflight checks wrapper ownership/path and actual kubelet
helper selection. Qualification mounts through the kubelet and asserts this gate was entered;
a host whose mounter can bypass it refuses publication. No pod gets this helper's authority.

Each host keeps a root-owned, no-follow state directory for the export. Its fsynced record
contains export UUID, consumer PVC UID, endpoint, host boot identity and monotonic phase
`Preparing -> Serving -> Withdrawing -> Retired` for that consumer UID; a per-export file
lock serializes local transitions. A retired/withdrawing UID can never return to Serving.
Before launching the real helper, the dispatcher takes the lock, checks Serving and exact
source consumer UID/endpoint/options, then fsyncs a unique invocation token. The token binds
this export, consumer UID, boot,
target/Pod UID and wrapper/helper process start identities. It releases the lock and invokes
the real helper in foreground mode; `bg` and untracked helper variants are refused for this
export. Completion removes the token under the lock only after the child has exited/reaped
and any resulting mount is visible to the host mount inventory. A launch/identity/cleanup
ambiguity retains the token; wrapper death never silently releases admission ownership.
This uses write-ahead ownership, not the lifetime of an inherited flock descriptor.
At most 8 invocation tokens of at most 4KiB each exist per host/export; reserve a token slot
before launch and refuse further calls on exhaustion. The host gate/probe and forwarded
helpers enter the owned per-export service containment, not an unrestricted kubelet cgroup;
record finite task/fd/CPU/memory limits in qualification. Lock/probe attempts have finite
command deadlines; expiry leaves ownership held rather than deleting an invocation token.

The host probe covers the actual kubelet mount namespace, owned pod/runtime bind mounts and
other visible namespaces retaining this export. It reports current owned mounts plus pending
invocation tokens. Unknown/unreadable namespace or process ownership is not an empty result.
Host-local state changes are atomic replace+directory fsync; the dispatcher rejects missing,
malformed, foreign-owned or stale-boot Serving records. Reboot cannot silently reactivate an
export; recovery must reconcile its persisted Withdrawing/Retired phase before any mount.

## First open and replacement consumer

Create the consumer PVC Pending with `volumeName` naming the not-yet-created owned static PV;
there is no matching PV to mount. Read and durably record its API UID, then construct the PV's
immutable UID-bearing source and exact-UID claimRef. Configure that pseudo-path and install
Preparing records for this UID on both hosts before creating the PV. Verify the resulting
PVC/PV UID binding before either host transitions this UID to Serving and before readiness.
If creation/UID ownership or a partial reopen is ambiguous, keep Preparing and refuse service;
retry validates those same UIDs/source rather than creating another serving lifetime by name.

After retirement, reopening the same retained backing export requires a newly created consumer
PVC with a different API UID and a new PV carrying that UID in its source. Preserve old lifetime
retirement while preparing the new one; replace current admission only under the host lock.
An old cached kubelet operation retains its captured old source even if its dispatcher call
was delayed until after reopening. Its old UID mismatches and is rejected before launch, with
no invocation/mount to carry into the new lifetime. This holds even after old detailed records
are pruned because unknown reserved owned sources fail closed. No extra sequence counter or
reused-name authority is introduced; missing/uncertain retirement still blocks reopening.

## Down and retry

1. Persist `Withdrawing` in the workstation UID inventory before changing either host. Seal
   each host gate by durably changing its matching-UID phase to Withdrawing under its lock,
   and acknowledge that state. Up/retry cannot turn it back to Serving. Keep the NFS server
   and existing connections alive while consumers drain.
2. Wait for both seals and every previously admitted invocation token to retire. A delayed
   kubelet invocation now sees Withdrawing and fails before calling the real helper. A helper
   admitted before a seal remains owned until reaped; down cannot outrun its mount result.
   Missing host, lost SSH, interrupted token or ambiguous process/mount state returns failure
   with resources held, never permission to retire them. No timeout, name-only check, PID-only
   check or forced token deletion proves quiescence.
3. Delete only the exact owned **consumer** PVC UID with a delete precondition; preserve the
   Retain PV claimRef and backing RWO PVC/data. The authorized caller stops all consumer Pods.
   Wait for PVC protection/deletion and no referencing Pods, then require both host probes to
   report no owned mount or invocation. API absence alone is insufficient. Recheck host boot
   identities and seals; a reboot/missing proof holds retirement pending reconciliation.
4. Only then close owned TLS connections/ingress, retire proxies and server and remove owned
   ephemeral objects/credentials. Persist Retired on both hosts and the UID inventory before
   releasing the endpoint allocation. No force unmount. Never reuse a name/port while its old
   phase, mount or invocation ownership is ambiguous; a replacement consumer has a new PVC UID
   in its immutable source and cannot adopt an old source, withdrawal token or claimRef. Retain the withdrawal tombstone until
   this retirement proof is durable; an interrupted down resumes it and never republishes.

Down is deliberately allowed to stop in Withdrawing with the old service retained. It cannot
promise bounded completion when consumers, helpers or host evidence are unavailable. This
proposed availability cost is explicit and unapproved; it does not delete backing data or
restart unrelated services. Operator recovery of an ambiguous token requires evidence that its
helper and all resulting mounts have retired, not elapsed time or a manual 'force' bypass.

## Future fixtures

Pause a real-helper stub before/after launch, create another authorized Pod between initial
inspection and each seal, and delay its kubelet mount until after sealing: it is either an
owned pre-seal operation that blocks retirement or rejected before real-helper invocation.
Kill the wrapper with a live child and verify its durable token blocks down. Interrupt/resume
after either host seal/PVC deletion. Also pause a saved old kubelet mount before dispatcher
entry, complete retirement, reopen the same export/name/port with a new PVC UID, then release
the old call: it must be refused while a new-source call succeeds. Interrupt after Pending
PVC creation, either Preparing record/PV binding or one Serving transition: retry must not
activate the old UID or delegate an unknown owned source. Attempt stale/malformed sources
with and without detailed retirement records; inject missing
host/boot change/mount namespace uncertainty. None may retire an ambiguous endpoint. A normal
successful drain proves helper reaping, consumer unmount and preserved backing data. These
are planned tests; no helper, API, mount, server or lab execution occurred in this review fix.
