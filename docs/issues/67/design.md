# Issue #67: RWX through an NFS gateway — design

[spec](spec.md) · [decisions](decisions.md) · [ADR, Accepted with the plan, 2026-11-13](adr-proposal.md)

Approved 2026-11-13; not implemented.
First-step lab design only. No CSI RWX capability is advertised. Qualification gates
below must be met before exposure; dependency versions and privilege profile are unaccepted.
Settled at approval, 2026-11-13: the dependency and privilege approach as proposed, exact versions
and rules fixed by qualification (see decisions.md, Settled at approval).

## The facts it starts from

`control/csi/controller.go:104` accepts single-node modes only; `node.go:81` stages ext4.
`deploy/csi/driver.yaml:13` has one attachRequired driver. `scripts/dev/image.sh:11` supplies
no NFS server or TLS tunnel. `scripts/guest/k3s.sh:36` puts inter-node traffic on WireGuard,
which does not authorize consumer claims. No shipped gateway or namespace admission policy.
#79 finds unsafe mount retries; #82 finds unsafe failover taints. Those defects invalidate the
intent's inference that block fencing alone proves all gateway behavior. This increment pins
the gateway to one node; it never forces node replacement or old-client cache safety.

External facts checked 2026-11-12: Ganesha's [VFS documentation](https://github.com/nfs-ganesha/nfs-ganesha/wiki/VFS)
requires privileged handle syscalls; a user-space server is not automatically unprivileged.
Its [core configuration](https://github.com/nfs-ganesha/nfs-ganesha/blob/next/src/doc/man/ganesha-core-config.rst)
describes persistent recovery directories and grace settings. [Kubernetes Pod Security](https://kubernetes.io/docs/concepts/security/pod-security-standards/)
restricts direct NFS and host access in Restricted namespaces. Pin tested release artifacts;
these moving documentation branches are evidence, not dependency pins.

## Gateway and durable state (S1–S3)

Propose one Ganesha VFS server with NFSv4.1/TCP only, no UDP, v3, mountd, NLM, rquota or
management listener. A single Recreate Deployment is pinned to the selected node. The backing
PVC is in `flaminio-system`; mount at `/srv/flaminio`, export only `/srv/flaminio/export`.
Recovery lives at `/srv/flaminio/private/recovery`, outside the exported root. Initialize one
persisted export UUID before first readiness; derive fixed Export_Id/FileSystem_ID and NFS
server owner/scope from it. Never derive identities from pod UID, node name or ublk number.
Server boots with normal grace/reclaim and a fresh reboot verifier; no Graceless shortcut.
Settled at approval, 2026-11-13: this server as proposed (see decisions.md, Settled at approval).

A qualification fixture must establish the exact pinned release's FSAL configuration, sync
stable-write/COMMIT behavior, persistent recovery backend and durable shutdown-independent
ordering. No export is enabled until restart tests verify file handles, opens/locks and
stable data. Persistent record loss stops startup; never silently initialize a replacement
identity. Server-private state has restrictive ownership and is not reachable through NFS
pseudo-root traversal, symlinks or a second export.

Readiness tests the local NFS service, mounted backing volume identity and recovery startup.
Only one server process may mount/serve that export identity; restart policy cannot create an
overlapping rollout. Stop old pod before replacement; failed termination blocks restart.
RWO alone is not a process exclusion mechanism on the same node. A process-supervisor/file
lock on the mounted backing volume supplements Recreate; node-loss failover is a later design.

## Authenticated transport and claim isolation (S4–S6)

Propose per-export node-local TLS tunnels. The server's plaintext NFS listener binds loopback
inside its pod network namespace. A colocated tunnel exposes only authenticated TLS on the
Service. Node proxies bind host 127.0.0.1 on a dedicated port per export and verify the exact
server identity; server verifies the exact admitted node identity, using a dedicated NFS CA.
These are transport identities, never Flaminio metadata/replica identities. No pod receives
client keys. Use NFSv4.1 backchannel over the established connection; v4.0 callbacks unsupported.
A generic tunnel that merely accepts any CA-signed client is insufficient.
Settled at approval, 2026-11-13: these tunnels as proposed (see decisions.md, Settled at approval).

The trusted operator creates a static NFS PV pointing at node-loopback proxy and pre-binds it
with claimRef namespace/name/**UID** to the one consumer claim. `mountOptions` require
`vers=4.1,proto=tcp,hard,nosuid,nodev` and the assigned port. Retain reclaim policy. Proxies
exist only on the two allowed workload nodes; PV node affinity limits scheduling accordingly.
Applications mount via that PVC, with ordinary restricted Pod security. They never name a
server or port. Root squash and fixed export Unix permissions apply; sharing a claim grants
that filesystem trust boundary, not authentication of arbitrary AUTH_SYS uid claims.

The lab preflight must enforce Restricted admission in **all tenant-capable namespaces**,
not just the consumer namespace; deny tenant PV creation, policy/namespace-label mutation,
proxy Secrets, system namespace writes, privileged/host-network/hostPath pods and alternate
mount helpers. Inventory existing pods/controllers/PVs before exposure; preexisting bypasses
cause refusal rather than changing unrelated resources. Namespace admins who can relax these
controls are trusted administrators, outside the stated tenant boundary. Test policy denial
through the actual API server. A read-back of labels alone does not prove enforcement.

NetworkPolicy allows ingress only to TLS, and only from required node addresses; deny all
other gateway ingress/egress except required tunnel traffic. Host routing/firewall rules must
prevent pod-to-node proxy access and plaintext escape. Loopback binding alone does not prove
protection from hostNetwork, so admission/RBAC and live bypass tests are mandatory. CNI node
traffic exemptions must be tested, never assumed. The gateway cannot talk to metadata or
replica ports; node proxy has no Kubernetes API credential.

Fail closed on certificate expiry/unknown peer/identity mismatch. Refuse connection reuse
past configured certificate authorization validity; credential rotation closes old sessions
before replacing authorization. Revocation profile, tunnel implementation/version and
per-export port allocation are qualification gates in the proposal. Existing kernel NFS
connections must reconnect safely; no plaintext fallback on TLS failure.

## Privileges and lifecycle (S6, S7)

Ganesha VFS requires a measured explicit capability/seccomp profile for handle operations and
uid changes. Do not ship `privileged:true` as a convenient substitute. Candidate root process
with a minimal capability set must pass the fixture; if it cannot, return for design review.
Read-only rootfs, no host namespaces, service-account token automount false, no hostPath,
no host socket/device mounts. Tunnel process is nonroot with all capabilities dropped.
Secrets mount only in tunnel containers. Pin all image digests and record dependency licenses.
Settled at approval, 2026-11-13: this privilege profile as proposed, exact rules recorded in
`qualification.md` (see decisions.md, Settled at approval).

Proposed `make gateway-{up,status,down,test} GATEWAY=<id> NAMESPACE=<ns> CLAIM=<name>` dispatches
through `scripts/lab.sh` to `scripts/checks/gateway.sh`. `up` requires an explicit existing
backing PVC and selected node, performs all preflights before publication, and checkpoints
resource UIDs. It installs only labeled/UID-owned objects. Store inventory in workstation lab
state, excluding secret bytes; validate Kubernetes names and quote every shell argument.
Output names gateway, backing PVC, consumer claim UID, node, endpoint identity and readiness.
Exit 0 success, 1 configuration/runtime failure, 2 invalid arguments, 255 lost SSH.
`status` is read-only; on partial failure print owned stages and safe retry instructions.
Settled at approval, 2026-11-13: these commands as proposed (see decisions.md, Settled at approval).

`down` first refuses while live consumer pods/mounts exist; authorized caller stops them,
then repeats down. Disable TLS ingress, unmount proxies after kernel clients are gone, stop
server, and remove only matching-UID owned objects/credentials. Keep backing data and identity.
No force unmount, raw formatting or implicit PVC deletion. If ownership or lifecycle state is
ambiguous, leave data and stop. Retry uses the persisted inventory, never names alone.

## Contracts

Backing CSI API/protobuf/control socket unchanged. The Kubernetes contract comprises one
backing RWO PVC, one gateway Deployment+TLS Service, protected Secrets, owned NetworkPolicies,
node proxy service/config and one Retain static NFS PV with a UID-bound consumer PVC. Exact
image digests, config grammar, capabilities/seccomp rules and tunnel identity encoding belong
in a required `qualification.md` fixture before implementation ships; their absence blocks
exposure. No new metadata RPC or replica capability is granted. The later CSI RWX stage needs
its own controller, authorization, scheduling/accounting and deletion contracts.

## Files touched

- `deploy/gateway/`: templates/config and qualification fixture, S1–S6.
- `scripts/dev/`: reproducible pinned server/tunnel image builder, S2/S5/S6.
- `scripts/checks/gateway.sh`, `scripts/lab.sh`, Makefile help: S7–S8 command contracts.
- `scripts/tools/gateway-test.sh`: faked lifecycle/API/SSH tests, S1/S4–S7.
- `docs/lab.md`, README roadmap, `docs/followups.md`: prerequisites and actual delivery scope.
  Existing CSI mode validation and node failover behavior are not altered by this increment.

## Tests

| Rules | Planned evidence |
|---|---|
| S1/S7 | script fixtures for valid/hostile names, foreign UID, interrupted creation/deletion, occupied node, missing policy, retained backing PVC, no unrelated deletes |
| S2 | pinned server fixture with stable writes/COMMIT and fsync, abrupt SIGKILL, byte verification; refuse async configuration |
| S3 | open handle across restart, lock conflict then reclaim, same identity/address, persisted recovery; changed identity/config must refuse startup |
| S4 | restricted namespace/API tests plus raw socket/inline-NFS attempts from both allowed and foreign namespaces; PV theft/claimRef mismatch denied |
| S5 | correct/wrong node and gateway certs, expiry, credential rotation and old connection closure; capture verifies no cross-host plaintext |
| S6 | actual capabilities and seccomp fixture, forbidden server-private path traversal, root squash, no secret/API/control/disk access |
| S8 | two nodes alternate writes/renames and fsync; restart server; blocked bypasses; unchanged ordinary CSI checks |

Local `make lint` includes script tests; `make test race hygiene contracts` covers affected
Go/Rust baseline if any code is added. Server qualification requires real kernel NFS clients.
No lab or server execution was performed for this draft.

## Lab

Proposed `make gateway-test` provisions dedicated claims/test files only after explicit lab
authorization. It restarts the gateway process, not the node, and records COMMIT/fsync evidence,
reclaim timing and checksums. It deletes its test objects on cleanup; preserves backing PVC
unless operator separately authorizes destruction. Restore prior proxy/policy configuration
only for objects this run created, keeping isolation until all consumers are gone. Then run
existing `make csi-test csi-failover` with separate authorization for node power cycling.
No raw disk format is part of this check. #83 host profile and #84 durability remain prerequisites.
Settled at approval, 2026-11-13: `make gateway-test` as proposed (see decisions.md, Settled at
approval).

## Docs, risks and ADRs

Document client pause and NFS grace semantics, trusted administrator boundary, refusal paths,
retained data, prerequisites and later stages. No node-loss failover or automatic RWX claim.
ADR0001–0004 consistent with an external user-space service and unchanged Rust data path;
ADR0005's current lease rules remain. The new server/tunnel dependency needs the
[Proposed ADR](adr-proposal.md); no acceptance or TLS exception is inferred. Privilege profile,
server stable/recovery semantics, proxy lifecycle and namespace bypass evidence are release
gates. NFS server caching and client caches require separate node-loss fencing work.
Settled at approval, 2026-11-13: the ADR is accepted with the plan; no TLS exception (see
decisions.md, Settled at approval).

## Parallelizable?

No. Qualify server, privilege and isolation contracts first; split image/script/docs work
only after exact tested configuration is committed. No Rust/Go protocol split in this increment.
