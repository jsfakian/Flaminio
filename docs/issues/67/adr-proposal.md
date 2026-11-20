# Proposal: One protected NFSv4.1 gateway per filesystem

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12

## Context

[#67](https://github.com/jsfakian/Flaminio/issues/67) needs a shared filesystem above one RWO
block target. Plain AUTH_SYS plus node reachability lets unauthorized pods bypass claim
ownership. The [intent](intent.md) also requires stable data and recoverable handles/locks.

## Proposed decision

Qualify a pinned NFS-Ganesha VFS server, with export identity and recovery on its backing PVC.
Use NFSv4.1 only, synchronous stable acknowledgements, and per-export mutually authenticated
node tunnels. Bind one static consumer claim by UID; protect all tenant namespaces against
inline NFS and host access and deny tenant PV creation. Expose no plaintext NFS to pods.
Gateway has no Flaminio/control identity. Runtime privilege and transport version choices need
qualification before enabling an export. No node-loss replacement or CSI RWX in this increment.
Settled at approval, 2026-11-13: this decision is accepted with the plan; versions and privileges
are fixed by qualification (see decisions.md, Settled at approval).

## Options

Kernel nfsd avoids a C userspace dependency but introduces host kernel server state and broader
host/pod privileges. Ganesha isolates service state but still needs handle syscall privileges;
it is not automatically safe as nonroot. Raw AUTH_SYS over WireGuard encrypts inter-host bytes
but cannot enforce namespace/claim ownership. Native NFS authenticated encryption could replace
tunnels only after kernel/server compatibility and identity tests at the supported baseline.

## Consequences

Adds server/tunnel dependencies, a node mount/proxy lifecycle and administrative isolation
prerequisites. Same-node process restart can be qualified separately from node-loss cache
fencing. A single claim shares Unix permission trust; it does not promise per-user NFS identity.
Privilege/certificate lifecycle configuration remains unaccepted; do not deploy by default.
Settled at approval, 2026-11-13: the measured minimal privilege profile and authenticated tunnels
as proposed, their exact configuration fixed by qualification (see decisions.md, Settled at
approval).

## Confirmation

The [design](design.md) maps stability, reclaim, credential and namespace bypass tests.
Ship exact pinned configuration and privilege evidence before exposure. Later node-loss,
automatic provisioning and performance work are still needed to close #67.


## Subsequent review correction — proposed, unapproved, 2026-11-20

The historical accepted decision above is preserved. The proposed additions are the
[qualification contract](qualification-contract.md) and [withdrawal contract](withdrawal-contract.md):
physical/VM and host-client preflight, sync/reply ordering evidence, ordinary finite resource
containment and an owned host mount-helper gate with durable withdrawal ownership. The helper
installation and potentially held teardown are new unapproved consequences; no acceptance of
these revisions is inferred from the 2026-11-13 approval. No new CSI or gateway API identity.


## R2 refinement — proposed, unapproved, 2026-11-20

The mount-admission source additionally carries the actual consumer PVC UID; permanent export,
filesystem and reclaim identity remain unchanged. The [withdrawal contract](withdrawal-contract.md)
orders UID acquisition/binding/publication and rejects stale owned sources after replacement.
This is a subsequent unapproved refinement, not historical acceptance of the changed mechanism.
