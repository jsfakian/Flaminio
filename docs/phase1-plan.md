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
| 2 ✅ | **Metadata service** (Go, `meta/`): the domain service over a dedicated etcd. It handles nodes, disks, volumes, placement, epoch issuance, conditional ISR transitions and node liveness, over gRPC. Its ISR and epoch rules are checked against the same transition table as step 1 | Any machine, then a 3-member etcd in the lab | Unit tests against embedded etcd; lab etcd on fl-a, fl-b and a small third VM (fl-w, a witness) |
| 3 ✅ | **Identity and mTLS for the control plane**: a development CA; mTLS on etcd and the metadata service, with per-method authorization; capability tokens signed by the metadata service. Replication TLS and token checks in the replica move to step 4, which replaces the MVP's replication transport | Lab | Unauthenticated and unauthorized callers are refused; tokens verify |
| 4 | **Data plane on the protocol core**:<br>• `flaminio-io` hosts many volumes and chunk replicas;<br>• the target uses placement and ISR from the metadata service, and the step-1 state machine;<br>• one heartbeat per node pair;<br>• replication over mutual TLS (step-3 identities), with the replica checking capability tokens | Lab | • The MVP's fault test passes, with ISR shrink and expand going through the metadata service.<br>• The replica refuses unauthenticated peers and stale tokens.<br>• Read results unchanged (reads stay local) |
| 5 | **Failover in the lab**: attach on the other VM with an epoch bump, fencing and reconciliation; partition tests (the old target must neither write nor read stale data) | Lab | Scripted failover and partition tests pass, and the data is verified |
| 6 | **Node agent**: device discovery, the claim policy (never claim a device with a signature), format, drain by moving chunk replicas | Lab | Adding and draining a disk while fio runs, with the data verified |
| 7 | **Kubernetes**: k3s on the lab VMs, the CSI driver (provision, attach, stage, publish; block and filesystem), placement controller, failover controller | Lab | A pod with a PVC runs fio. Killing its node moves it to the other node with its data intact |
| 8 | **Phase-1 benchmark and chaos run** | Lab | Read results for a pod against the raw disk; failover time measured; chaos run passes |

## Step 4 breakdown

Step 4 puts the real data plane on the step-1 core, so that production runs the code the
simulator checked. It comes in five parts, each committed on its own:

| Part | What | Done when |
|---|---|---|
| 4a ✅ | **Ranges and payloads in the core.** Operations, locks, uncertain sets, dirty sets and resync copies work on extents (runs of blocks) instead of single blocks. Data is an opaque payload type: per-block values in the simulator, buffer handles in the data plane. A replica's uncertain set becomes volatile: after a restart it reports the whole chunk | The simulator passes and every mutation is still caught, with multi-block operations |
| 4b ✅ | **Transport.** Framed messages over TLS 1.3 with the step-3 identities. The capability token is presented in `Hello`, and the replica checks it against the peer certificate | Loopback tests: wrong CA, wrong node, stale epoch and bad token are all refused |
| 4c ✅ | **Replica service on the real store.** The core's replica rules (fencing, sessions, uncertain sets, resync filters) in front of `flaminio-store`, with group commit per session | Loopback tests against a file-backed store |
| 4d | **Target runtime.** `flaminio-io serve` attaches with a token, drives the core from ublk, sends to local and remote replicas, and makes ISR changes through the metadata service (gRPC client on its own thread) | The local end-to-end test passes |
| 4e | **Lab.** An RF=2 volume across fl-a and fl-b, placed by the metadata service | The MVP's fault test passes, with the ISR shrink and expand visible in the metadata service; reads still at the MVP's level |

## Step 4d progress

- **(i) Metadata client** (`flaminio-metaclient`): gRPC over mutual TLS, authenticated as the
  node's data plane, on its own thread. `make meta-interop` checks it against the Go service.
- **(ii) Engine** (`flaminio-node::engine`): one thread owns the replica service and the
  volume targets.
  - **Targets** run the verified state machine. The engine routes their messages: to the
    local replica in process, to other nodes over the network thread, to the metadata service.
  - **Client I/O** is split at chunk boundaries.
  - **Local requests** of one turn share a group commit.
  - **A data reply** of the wrong length is dropped before it reaches a target, so a faulty
    replica can only cause a timeout.

  **The two-node test:** each node a full engine with a TLS network thread, and a model of
  the metadata service.
  - Writes across a chunk boundary read back from the target and from the other node's
    replica.
  - With the replica deaf, a write completes after 0.5 s: one request timeout, then the ISR
    shrinks.
  - After the replica recovers, it is resynced and rejoins within 3.6 s, holding the write it
    missed.
- **(iii) Next:** the ublk frontend on the engine, with local reads kept on the MVP's
  asynchronous, zero-copy path; then the lab (4e).

## Step 4c result

The crate `flaminio-node` holds two parts:

- **`ReplicaService`** carries out the replica core's decisions on the store:
  - fences are durable before the `Hello` reply;
  - writes and copies from all sessions share one group commit, and reads see the writes
    before them;
  - a store failure closes the session.
- **`server`** puts it on the network: a network thread for TLS sessions, and an engine loop
  that batches everything that has arrived.

**`Hello` now carries the volume's geometry and the chunks placed on the node.** The replica
creates those chunks at `Hello`, so a fence is durable even before any write, and it refuses
requests for chunks not placed on it. The token's holder controls the volume, so trusting its
geometry grants nothing more.

**Checks:**
- **Unit tests** on an in-memory store:
  - placed chunks are created fenced;
  - 32 writes in one batch share a flush;
  - after a restart, the fence holds and whole chunks are reported uncertain, including a
    fence made before any write;
  - old sessions are ignored;
  - copies skip live-written blocks;
  - geometry is kept;
  - a second `Hello` is refused.
- **End to end** over real TLS with a file-backed store:
  - 64 pipelined writes are read back;
  - a newer epoch fences the old target off (`Stale`);
  - an older epoch cannot open a session.

## Step 4b result

The crate `flaminio-net` holds:
- the wire format, in which every decoder treats its input as untrusted;
- TLS 1.3 configurations (rustls with ring) that require client certificates both ways;
- SPIFFE identities read from certificates;
- capability-token verification, which agrees with the Go vectors.

**One connection is one session.** The replica side refuses a session unless:
- the peer is a node's data plane;
- its first frame is a `Hello` whose token verifies;
- the token names this volume, this epoch and the peer's own node.

The target side checks that it reached the node it meant to reach.

**Checks:**
- **Loopback tests** over real TLS cover a valid session, and refusals for:
  - a foreign CA;
  - the wrong component;
  - a token for another node, volume or epoch;
  - a forged token;
  - the wrong server;
  - a second `Hello`.
- **Frame tests** refuse every truncation and a catalogue of malformed frames; one of them
  found an empty data frame accepted, now fixed.
- **Fuzz targets:** `repl_frame` and `capability_token` (they need a nightly toolchain to
  run).

## Step 4a result

**What changed in the core:**
- operations, locks, uncertain sets, dirty sets, resync copies and filters work on extents;
- the target and replica are generic over the payload;
- the replica is split into a sans-I/O core, which returns actions to carry out, and the
  simulator's in-memory storage;
- uncertain sets are volatile, as specified in
  [replication-protocol.md §6](replication-protocol.md#6-failover-fencing-and-reconciliation).

**Checks:** 120 000 seeds with multi-block operations pass, and every mutation is still caught.
Reconciliation pruning got rarer, so its check now searches up to 60 000 seeds.

## Step 1 result

The protocol core is in `dataplane/crates/flaminio-repl`. The checks:
- 320 000 random simulation seeds pass;
- two scripted split-brain scenarios pass;
- every one of ten deliberately broken rules is caught.

The simulator found three flaws in the first draft, all fixed. The worst: a replica could
rejoin the ISR without a write it had missed. Its findings also simplified one rule. The details
are in [replication-protocol.md §10](replication-protocol.md#10-what-the-simulator-found). Run it
with `make sim`.

## Step 2 result

The metadata service is in `meta/`, with its API in `api/proto/flaminio/meta/v1`. It is built
in three layers:
- a pure domain package with the same transitions as the Rust model;
- a narrow key-value interface (ADR 0002), implemented on etcd and in memory;
- a gRPC service.

The checks:
- **Rust vectors:** the Go domain replays 4 463 transitions recorded from the Rust model (2 902
  applied, 1 561 rejected), and agrees on every one (`make meta-vectors` regenerates them).
- **Service tests**, on the in-memory store and on a real embedded etcd:
  - the volume lifecycle;
  - deposed targets;
  - liveness expiry;
  - witness exclusion;
  - a race of 8 goroutines attaching and changing ISRs, after which every applied change and
    every attach is accounted for exactly once.

  That race test found a double epoch bump in the first `Attach`, now fixed.
- **Lab:** the metadata service runs on a 3-member etcd cluster across fl-a, fl-b and the new
  witness fl-w, joined by a WireGuard overlay. It survives the loss of one member, refuses
  without quorum, and recovers intact (`make meta-smoke`; see [lab.md](lab.md#phase-1-witness-overlay-and-metadata)).

Not yet: TLS and authentication (step 3), and a watch API for targets (not needed while each
target is the only writer of its volume's ISRs).

## Step 3 result

- **`pkg/pki`:** SPIFFE-style workload identities, a CA that signs certificate requests (keys
  never travel), and TLS 1.3 configurations that require client certificates.
- **`pkg/captoken`:** Ed25519 capability tokens `{volume, epoch, node}`, with vectors in
  `pkg/captoken/testdata` for the Rust verifier.
- **`control/cmd/flaminio-ca`:** the lab's CA tool.
- **Metadata service:**
  - mutual TLS, with authorization per method:
    - administrators create, delete and attach;
    - a node registers and heartbeats only itself;
    - only the current target's data plane changes its volume's ISR;
  - tokens issued on attach.
- **Checks:**
  - an authorization test over real mTLS covers every rule, plus missing certificates and
    certificates from another CA;
  - a token test refuses every single-bit change and every truncation;
  - `make meta-smoke` in the lab, with etcd also on TLS 1.3 with client certificates, from a
    separate CA.

## Lab changes this phase needs

- **A third, small VM (fl-w)** for the third etcd member and the Kubernetes control plane. It is
  the 2-node + witness topology of the edge profile (ARCHITECTURE §1.2). With only two
  physical servers, fl-w shares a server with one of the data VMs, so it shows quorum behavior,
  not real failure independence.
- **Go toolchain** on the Mac and in the VMs (steps 2 and 7).

## What stays out of phase 1

Encryption at rest, snapshots, backups (phase 3); erasure coding, RDMA (phase 6); the operator's
automatic etcd member replacement (after step 2 works by hand).
