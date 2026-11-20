# Issue #67: RWX through an NFS gateway — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Review corrections of 2026-11-20 are proposed and unapproved (decisions.md).
The existing intent's first step: one manually provisioned lab gateway and statically
bound claims. Node-loss failover, automatic CSI RWX provisioning and performance remain later
steps. The issue stays open. Server and encryption choices require the Proposed ADR.
Settled at approval, 2026-11-13: Ganesha VFS and node-local authenticated tunnels, the ADR
accepted with the plan (see decisions.md, Settled at approval).

## Terms

- **Backing volume:** one ext4 Flaminio RWO volume, mounted only by its gateway.
- **Export:** one stable NFSv4.1 filesystem identity and path, for that backing volume alone.
- **Consumer claim:** a static namespace-bound claim allowing pods to mount the export.
- **Trusted operator:** cluster administrator provisioning the gateway, static PV and credentials.
- **Tenant:** an unprivileged pod creator without host access, PV/admin rights or access to the
  gateway namespace, node proxy credentials or policy configuration.

## The invariant

A successful stable NFS write or COMMIT survives gateway process death. Only workloads using
the explicitly bound consumer claim can reach the exported files. Network reachability alone
does not grant access. Gateway restart preserves export identity and durable recovery state.

## Behaviour

**S1. One owner.** One gateway serves exactly one backing volume, in a protected namespace.
The ordinary CSI single-target and RWO restrictions remain unchanged. A retry adopts only the
same UID-bound resources and refuses unrelated resources at the requested names.

**S2. Stable data.** Stable writes and COMMIT are answered only after the backing filesystem
has made them durable. Unstable writes use correct reboot/write-verifier semantics. No async
export option or hidden volatile acknowledgement can bypass this rule.

**S3. Restart.** After abrupt gateway process death on the same node, clients reconnect at the
same address without manual remount. Stable data, file handles, opens and locks recover under
NFS reclaim/grace semantics; all necessary persistent server state is on the backing volume.
Node-loss recovery is not claimed by this step.

**S4. Consumer isolation.** Only the selected claim in the selected namespace permits a mount.
A different namespace cannot use direct NFS, a forged static volume or proxy access to bypass
it. Tenant pods cannot reach an NFS plaintext endpoint or obtain its transport credentials.
Policy prerequisites are enforced before the export becomes reachable, including against
previously admitted workloads. Missing or unverifiable prerequisites stop deployment.

**S5. Protected transport.** Cross-host NFS traffic is encrypted and both endpoints authenticate
the intended gateway/node identities. Wrong identity, wrong export, revoked/untrusted or
expired credentials fail closed. Encryption is not inferred from the lab's WireGuard link.

**S6. Least authority.** The gateway has no metadata/data-plane identity, control socket, host
namespace, raw disk or API token. Its documented privileges serve only the chosen NFS server.
Export clients cannot read its recovery records or secrets. Root on the client is squashed;
consumer mounts use nosuid/nodev and hard NFSv4.1 behavior.

**S7. Repeatable lifecycle.** An explicit lab command installs, reports and removes the owned
gateway resources, with validated/quoted names. Teardown stops consumers before server and
proxy removal, preserving the backing volume unless a separate explicit data deletion is
requested. Host NFS clients and finite server/tunnel/proxy containment are prerequisites.
Withdrawal prevents new and delayed mounts before proving consumer quiescence. A replacement
consumer has a fresh API UID carried in its immutable mount source; cached old sources cannot
enter the new Serving lifetime, including after name/port reuse. Failed setup is diagnosable and resumable without touching unrelated resources.

**S8. Evidence.** A lab check uses consumers on both workload nodes, verifies cross-node file
visibility and stable-write survival across ungraceful gateway process restart, exercises
handles/open locks, and attempts namespace/direct-network bypasses. Existing CSI tests pass.
The check records versions, policies, privileges and results; no result is invented here.
Before #83, VM evidence names its older baseline; after #83, excluded VMs refuse and success
requires two admitted physical workload hosts. Stable-reply evidence includes delayed/failed
filesystem sync and a negative ordering fixture, beyond surviving kernel page-cache contents.

## Tests

- S1, S4–S7: manifest/config and fake-Kubernetes script tests; hostile names; wrong owner UID;
  missing policy; partial setup/teardown; delayed/new mounts during durable withdrawal;
  interrupted helper ownership and endpoint reuse; secret permissions; forbidden network paths.
- S2–S3: real NFS clients with fsync, stable-write/COMMIT evidence, killed gateway, persistent
  recovery directory and reclaim tests; block durability depends on #84 conformance evidence.
- S4–S6: lab tenant attempts inline NFS, foreign PVC/PV, hostNetwork/hostPath, direct port,
  wrong certificate and export identity; all refused while allowed claims work on both nodes.
- S8: authorized gateway check and ordinary csi-test/csi-failover regression results.

## Open questions

Approve a server/version, capability/seccomp profile and authenticated transport. The
[proposal](adr-proposal.md) recommends Ganesha VFS with node-local authenticated tunnels; its
configuration and durable recovery behavior require qualification before enabling a gateway.
NFS uid/gid controls do not provide per-user tenant authentication. One export is one trust
boundary; sharing it authorizes its users under the documented Unix permission model.

Settled at approval, 2026-11-13: Ganesha VFS, NFSv4.1 only, release pinned by qualification; a
measured minimal capability and seccomp profile; node-local mutually authenticated TLS tunnels;
one export as one trust boundary (see decisions.md, Settled at approval).
