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
| 5 ✅ | **Failover in the lab**: attach on the other VM with an epoch bump, fencing and reconciliation; partition tests (the old target must neither write nor read stale data) | Lab | Scripted failover and partition tests pass, and the data is verified |
| 6 | **Node agent**: device discovery, the claim policy (never claim a device with a signature), format, drain by moving chunk replicas | Lab | Adding and draining a disk while fio runs, with the data verified |
| 7 (mostly ✅) | **Kubernetes**: k3s on the lab VMs, the CSI driver (provision, attach, stage, publish; block and filesystem), placement controller, failover controller | Lab | A pod with a PVC runs fio. Killing its node moves it to the other node with its data intact |
| 8 | **Phase-1 benchmark and chaos run** | Lab | Read results for a pod against the raw disk; failover time measured; chaos run passes |

## Step 4 breakdown

Step 4 puts the real data plane on the step-1 core, so that production runs the code the
simulator checked. It comes in five parts, each committed on its own:

| Part | What | Done when |
|---|---|---|
| 4a ✅ | **Ranges and payloads in the core.** Operations, locks, uncertain sets, dirty sets and resync copies work on extents (runs of blocks) instead of single blocks. Data is an opaque payload type: per-block values in the simulator, buffer handles in the data plane. A replica's uncertain set becomes volatile: after a restart it reports the whole chunk | The simulator passes and every mutation is still caught, with multi-block operations |
| 4b ✅ | **Transport.** Framed messages over TLS 1.3 with the step-3 identities. The capability token is presented in `Hello`, and the replica checks it against the peer certificate | Loopback tests: wrong CA, wrong node, stale epoch and bad token are all refused |
| 4c ✅ | **Replica service on the real store.** The core's replica rules (fencing, sessions, uncertain sets, resync filters) in front of `flaminio-store`, with group commit per session | Loopback tests against a file-backed store |
| 4d (i–iii ✅) | **Target runtime.** `flaminio-io serve` attaches with a token, drives the core from ublk, sends to local and remote replicas, and makes ISR changes through the metadata service (gRPC client on its own thread) | The local end-to-end test passes |
| 4e (correctness ✅, reads pending) | **Lab.** An RF=2 volume across fl-a and fl-b, placed by the metadata service | The MVP's fault test passes, with the ISR shrink and expand visible in the metadata service; reads still at the MVP's level |

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
- **(iii) ublk on the engine:** `flaminio-io node` runs a node.
  - **Every node** serves its replicas over mutual TLS on the overlay, registers itself, and
    sends liveness heartbeats.
  - **With `--attach VOLUME --token FILE`,** it is also the volume's target and serves
    `/dev/ublkbN`. The engine lives on the ublk queue thread, woken by an eventfd and a 10 ms
    tick.
- **(iv) Asynchronous local reads** ✅. A client read whose source is the local replica
  works like this:
  1. The target state machine still decides: lock, lease, source.
  2. The engine admits the read through the replica rules, and the store plans it.
  3. The ublk task reads the device straight into the ublk buffer, on its own ring, and
     verifies the checksums.
  4. The target gets a "placed" reply and checks the lease before completing.

  Ticks run on the 10 ms timer only, not on every I/O event.

## Step 7 result so far (lab, 2026-10-09)

**Done:**
- k3s on the lab VMs;
- the CSI driver: provision, attach, stage and publish, block and filesystem;
- the failover controller.

Placement stays in the metadata service's `CreateVolume`; a separate placement controller
comes with failure domains ([follow-ups](followups.md) item 4).

**Left for step 8:** the pod running fio. The tests below write and verify 256 MiB with
checksums instead.

**Kubernetes (`make k8s-up`, `make k8s-smoke`):** k3s v1.36.5. The server is on the witness
fl-w (tainted, so no workloads run there) and agents are on fl-a and fl-b. Every node address
is an overlay address, and flannel's VXLAN runs over WireGuard. Ready in about a minute.

**CSI driver** (`control/csi`, one binary `flaminio-csi` in three roles):
- **Controller** (on fl-w): volumes are created, deleted and published in the metadata
  service. Publishing makes the node the target, and is a no-op if it already is.
- **Node plugin** (on fl-a and fl-b): it takes a new epoch for its own node, hands the token
  to the data plane through its control socket, formats ext4 only a device without any
  signature, and mounts.
- **Failover controller** (`control/failover`, on fl-w): it taints a node
  `node.kubernetes.io/out-of-service` when Kubernetes has seen it not Ready for 40 s and its
  data plane has stopped heartbeating to the metadata service.

| Test | Result |
|---|---|
| `make csi-test`: a PVC provisioned, formatted, written (256 MiB, checksum) | pod Ready in 8 s |
| …the pod moved to the other node (cordon, delete), data verified | Ready again in 8 s |
| `make csi-failover`: the pod's VM powered off without warning (`virsh destroy`) | NotReady at 46 s; tainted at 86 s; **pod Ready on the other node, data verified, at 126 s** |
| …the VM started again | taint removed within 5 s of its data plane starting; its replicas back in every ISR 44 s later |

Of the 126 s, Kubernetes' own detection takes 46 s and the failover controller's grace period
40 s. Both are settings.

## Step 5 result (lab, 2026-10-09)

`make partition-test` passes: a target cut off by a partition stops reading when its lease
runs out, never acknowledges a write after the cut, and the new target has every write the old
one acknowledged.

**Setup.**
- `vol1` (16 GiB, RF 2) has its target on fl-a and full ISRs.
- On fl-a's `/dev/ublkb1`, two probes do one O_DIRECT 4 KiB operation every 50 ms, each
  logged with its start and end time:
  - a reader at 1 GiB, which holds `gen-1`;
  - a writer of `writer-1`, `writer-2`, … at 2 GiB.
- Then fl-a is cut off the overlay in both directions (iptables on `wg0`). It keeps its local
  etcd member and metadata service, which have no quorum alone.
- From fl-w, `vol1` is attached to fl-b. After 30 s, the overlay heals.

| Check | Result |
|---|---|
| Reads on fl-a started after the cut, within the lease (5 s) | 97 served |
| Reads on fl-a started after the lease | **0 served**; 79 failed |
| Writes on fl-a acknowledged after the cut | **0** |
| Last write fl-a acknowledged, and what fl-b reads at 2 GiB | `writer-98` and `writer-98` |
| fl-b's first read, after the cut | 12.8 s, including the 2 s before the attach and the 5.5 s takeover wait L(1+ρ) |
| fl-a told it was deposed, after the heal | at once |
| fl-a back in every ISR as a replica | 151 s (a whole-chunk resync; [follow-ups](followups.md) item 3) |

Each check compares times from one VM's clock only.

**What the test found on the way:**
1. **The data plane's metadata client did not fail over on timeouts.** tonic reports its own
   timeouts as `Cancelled`, which the client did not treat as "try the next instance". A node
   whose first listed instance was cut off could not attach.
2. **One unanswering instance could stall every metadata call.** An instance that accepts
   connections and never answers the TLS handshake held a call forever: the channel's
   timeouts do not cover the handshake. Every call on every instance now has a deadline, and
   the interop test puts a silent instance and a refusing one before the live one.
3. **The metadata service waited on a cut-off etcd member.** A partition drops packets, so the
   connection to that member still looks open, and requests sent on it waited for the caller's
   deadline. Each etcd request now has 3 s per attempt, tried again (on another member) while
   the caller's deadline lasts; client keepalives take the dead connection out of rotation.
4. **A deposed target kept its device.** I/O failed, but the node stayed busy until a detach.
   A deposed target now stops its device, and an attach while another volume is attached is
   refused at once instead of waiting.
5. **Same-epoch restarts are unsafe** (found while designing the CSI node plugin, from the
   protocol rather than the lab). A restarted target numbers its sessions from 1 again. So
   every target start now takes a new epoch, and a node's agent may ask for one for a volume
   it already holds (`Attach` with `only_if_target`; [protocol](replication-protocol.md) §3).

## Step 4e results (lab, 2026-10-09)

**Setup.** `vol1` is 16 GiB in 64 chunks of 256 MiB, RF 2 with `minInSyncReplicas` 1. The
metadata service placed every chunk on fl-a and fl-b, none on the witness. Both nodes
registered themselves and stay alive through their heartbeats.

**`make node-fault`:**
1. **Writes:** fio wrote 4 GiB of random 64 KiB blocks with checksums on fl-a's
   `/dev/ublkb1`, while fl-b's node was stopped and restarted 20 s later.
2. **No failed writes:** fio verified all 4096 MiB with 0 errors. During the outage, all 64
   ISRs shrank to fl-a alone (visible in the metadata service).
3. **Resync during the load:** every ISR was full again by the time the load ended.
4. **Failover:** fl-a's node stopped, and `vol1` was attached to fl-b at epoch 2. fio verified
   all 4096 MiB again on fl-b's `/dev/ublkb1`, served from fl-b's replica alone.
   - A control run over a never-written range fails verification ("bad magic header 0"), so
     verify-only does not rewrite the data.

**The first run livelocked** (resync copies flooding the replica's session); see
[replication-protocol.md §10](replication-protocol.md#10-what-the-simulator-found). Bulk work
is now bounded, and the run above is the second one.

**Reads on fl-b through the engine** (NVMe; the raw disk did about 12 800, 245 000 and 4 980
in the MVP's measurements):

| Job | Result | Share of raw |
|---|---:|---:|
| 4K random, depth 1 | 12 427 IOPS, p50 77 µs | ~97% |
| 4K random, depth 32 | 13 459 IOPS | ~5% |
| 1M sequential, depth 8 | 1 154 MiB/s | ~23% |

At depth 1 the engine adds little. At depth 32 it gets no further than at depth 1: local reads
are served synchronously, one batch at a time, on the engine's thread.

**After 4d (iv)** (same disk, same jobs):

| Job | Result | Share of raw | The MVP got |
|---|---:|---:|---:|
| 4K random, depth 1 | 12 303 IOPS | ~96% | 93% |
| 4K random, depth 32 | 127 135 IOPS | ~52% | 86% |
| 1M sequential, depth 8 | 3 030 MiB/s | ~61% | 62% |

Depth 1 and the sequential job are at the MVP's level; the sequential job is limited by the
single queue thread, as it was then. Depth 32 still trailed.

**After profiling** (`perf` on fl-b): the queue thread was CPU-bound, and the target's
`progress` was the largest cost. It walked all 64 chunks on every I/O event. Client events now
advance only what they can change: a submission or a read reply starts queued work; a write
acknowledgement advances its own chunk. 20 000 seeds pass, and every mutation is still
caught.

Three runs each, against the raw disk measured the same day:

| Job | Result | Raw | Share |
|---|---:|---:|---:|
| 4K random, depth 1 | 12 740 IOPS, p50 75 µs | 15 136, p50 62 µs | 84% |
| 4K random, depth 32 | 184 000 IOPS | 254 508 | 72% |
| 1M sequential, depth 8 | 3 130 MiB/s | 5 662 | 55% |

The sequential job, and much of depth 32, are limited by the single queue thread: copying,
checksums, and the engine. Multiple queues and zero-copy are the next performance steps.

**Found while measuring: reconciling after a replica restart is expensive.**
- A replica that restarts reports whole chunks as uncertain. The next new target then
  reconciles all of them.
- Here that was the entire 16 GiB volume over 1 GbE: the first read after an attach waited
  152 s.
- The lab made it worse: `vol attach` restarts the node, because `--attach` exists only at
  startup.

Both are now done:
1. **Attach and detach while the node runs**, through the control socket
   `/run/flaminio/node.sock`, which the CSI node plugin will use.
   - The engine moves to the device's queue thread on attach, and back on detach, with its
     state intact.
   - One volume per node at a time in this phase.
2. **`Settle` after reconciliation** (replication-protocol.md §6). Reconciliation also left its
   repairs as uncertain entries, so every failover reconciled the whole volume again.

Moving `vol1` between the running nodes now serves the first read in 15–17 ms, both ways,
where it waited 152–260 s. Only a restart of the replicas themselves still costs a whole
reconciliation, about 160 s for 16 GiB over 1 GbE. The remaining follow-up:
3. **Reconcile by comparing digests first** (for restarts and power loss). The store keeps a CRC-32C per 4 KiB block in its
   metadata, so replicas can compare their blocks without reading data, and repair only what
   differs. That keeps genuine whole-volume cases, such as a power loss, cheap.

The same applies to resyncs and repairs copying unwritten ranges as data: `vol1` occupies 16
GiB on fl-b for 4 GiB written.

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
