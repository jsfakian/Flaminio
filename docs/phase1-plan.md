# Phase 1 plan: replicated volumes with failover, managed from Kubernetes

The MVP ([mvp-plan.md](mvp-plan.md)) proved the data path: a replicated volume served at 93–97%
of the raw disk's read speed. Its control was static: fixed roles, an epoch on the command line,
no failover. Phase 1 ([README roadmap](../README.md#roadmap)) replaces that with the real system
of record and Kubernetes integration:

- metadata service (etcd-backed);
- chunk placement;
- in-sync replica sets (ISR), epoch fencing and replica-confirmed leases;
- failover;
- CSI driver;
- device discovery and drain.

## Order and reasoning

The most dangerous part of phase 1 is the **consistency logic**: ISR changes, fencing, leases,
failover and the reconciliation of in-flight writes. A bug there loses acknowledged data or
serves stale reads, and it only shows up under rare interleavings that no lab test reliably
reproduces. So it comes **first**, as a pure state machine (no I/O) that a seeded simulator
drives through crashes, partitions, message delays and clock drift. The architecture already
calls for this (§19, deterministic simulation testing). Everything after it plugs that state
machine into real I/O.

| # | Step | Runs on | Done when |
|---|---|---|---|
| 1 ✅ | **Replication protocol core + deterministic simulator**: crate `flaminio-repl`. Target, replica and metadata state machines without I/O, covering: per-chunk ISR; epochs and fencing; replica-confirmed leases; shrinking the ISR before acknowledging; resync and rejoin; reconciling in-flight writes after failover. A seeded simulator injects crashes, partitions, delays, lost metadata and clock drift, and a linearizability checker judges every run. Protocol spec: [replication-protocol.md](replication-protocol.md) | Any machine | Thousands of seeds pass; each safety rule, deliberately broken, is caught by the simulator |
| 2 | **Metadata service** (Go, `meta/`): the domain service over a dedicated etcd. It handles nodes, disks, volumes, placement, epoch issuance, conditional ISR transitions and node liveness, over gRPC. Its ISR and epoch rules are checked against the same transition table as step 1 | Any machine, then a 3-member etcd in the lab | Unit tests against embedded etcd; lab etcd on fl-a, fl-b and a small third VM (fl-w, a witness) |
| 3 | **Identity and mTLS**: a development CA; mTLS on replication and on control gRPC; capability tokens signed by the metadata service and checked by replicas | Lab | The replica refuses unauthenticated peers and stale tokens. Read results unchanged (reads stay local) |
| 4 | **Data plane on the protocol core**: `flaminio-io` hosts many volumes and chunk replicas; the target uses placement and ISR from the metadata service and the step-1 state machine; one heartbeat per node pair | Lab | The MVP's fault test passes with the ISR shrink and expand going through the metadata service |
| 5 | **Failover in the lab**: attach on the other VM with an epoch bump, fencing and reconciliation; partition tests (the old target must neither write nor read stale data) | Lab | Scripted failover and partition tests pass, and the data is verified |
| 6 | **Node agent**: device discovery, the claim policy (never claim a device with a signature), format, drain by moving chunk replicas | Lab | Adding and draining a disk while fio runs, with the data verified |
| 7 | **Kubernetes**: k3s on the lab VMs, the CSI driver (provision, attach, stage, publish; block and filesystem), placement controller, failover controller | Lab | A pod with a PVC runs fio. Killing its node moves it to the other node with its data intact |
| 8 | **Phase-1 benchmark and chaos run** | Lab | Read results for a pod against the raw disk; failover time measured; chaos run passes |

## Step 1 result

The protocol core is in `dataplane/crates/flaminio-repl`. The checks:
- 320 000 random simulation seeds pass;
- two scripted split-brain scenarios pass;
- every one of ten deliberately broken rules is caught.

The simulator found three flaws in the first draft, all fixed. The worst: a replica could
rejoin the ISR without a write it had missed. Its findings also simplified one rule. The details
are in [replication-protocol.md §10](replication-protocol.md#10-what-the-simulator-found). Run it
with `make sim`.

## Lab changes this phase needs

- **A third, small VM (fl-w)** for the third etcd member and the Kubernetes control plane. It is
  the 2-node + witness topology of the edge profile (ARCHITECTURE §1.2). With only two
  physical servers, fl-w shares a server with one of the data VMs, so it shows quorum behavior,
  not real failure independence.
- **Go toolchain** on the Mac and in the VMs (steps 2 and 7).

## What stays out of phase 1

Encryption at rest, snapshots, backups (phase 3); erasure coding, RDMA (phase 6); the operator's
automatic etcd member replacement (after step 2 works by hand).
