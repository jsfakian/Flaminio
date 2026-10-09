# Open items and how to close them

What phase 1 left unfinished, with a plan for each: what to build, how to know it works, and in
which order. Each item names the step of [phase1-plan.md](phase1-plan.md) or the later phase it
belongs to.

| # | Item | Why it matters | Order |
|---|---|---|---|
| 1 | Read performance: multiple queues, less work per I/O, zero-copy | Reads are judged first; we are at 72–84% of raw for 4K and 55% for 1M | Now, alongside Kubernetes |
| 2 | Reconciliation after a replica restart | 160 s before the first read for 16 GiB after both replicas restarted | Before the chaos run (step 8) |
| 3 | Copying unwritten ranges | Resync and repair turn thin volumes thick (16 GiB used for 4 GiB written) and waste the network | With item 2: same tools |
| 4 | Failure domains in placement | Today placement only spreads chunks over nodes | Before the edge profile; tested now with node labels |
| 5 | More than one volume per node | A node serves one attached volume at a time | With item 1: same redesign |
| 6 | Fuzzing actually running | The fuzz targets build but have never run (no nightly toolchain) | Now: small |
| 7 | Short-lived certificates, rotated | Lab certificates last 30 days and are issued by hand | With the operator (phase 1 step 7+) |
| 8 | Deleting a volume's replicas | `DeleteVolume` removes the metadata only; the chunk replicas stay on the nodes' disks | With the node agent (step 6) |
| 9 | The data plane in Kubernetes | `flaminio-io node` runs as a systemd unit beside k3s; the CSI node plugin drives it through its control socket | With the node agent (step 6) |
| 10 | A faster first attach after a restart | An attach answers once the device exists, but the kernel's partition scan and udev's probe read the device and wait for the whole-volume reconciliation (item 2) | With item 2 |
| 11 | Draining volumes that are not attached | A drain moves only attached volumes: their target carries the moves out | With the operator |

## 1. Read performance

**Where the time goes.** The ublk queue thread is CPU-bound (`perf`, 2026-10-09). Per I/O it
pays for:
- the engine's bookkeeping (B-tree maps, small allocations);
- the target state machine;
- a copy for remote pieces;
- checksum verification;
- one queue for the whole volume.

**Steps:**
1. **Profile-driven trimming of the per-I/O path.** Pre-allocated op slots instead of maps keyed
   by op id, and no `Vec` building in the hot path. *Done when:* 4K depth 32 reaches ≥ 85% of raw
   on fl-b; checked with `make bench-node`.
2. **Several ublk queues.** One reactor thread per queue; each owns its share of the tags.
   - The engine stays single-threaded, but becomes cheap to call. Reads that the local replica
     serves go through a lock-free admission check per queue, and only ISR, lease and session
     changes need the engine.
   - *Done when:* 1M sequential reaches ≥ 85% of raw (it is at 55%, limited by one thread).
3. **Zero-copy for writes** (ublk user-copy or auto buffer registration on kernel ≥ 6.15), so a
   write is not copied into a `Vec` before it is sent and committed.
4. **Benchmark target** `make bench-node`: the fio read jobs on the attached volume and on the raw
   disk, three runs each, the same day, results in `docs/benchmarks/`.

## 2. Reconciliation after a replica restart

A restarted replica reports whole chunks as uncertain (replication-protocol.md §6), and the next
new target copies them all.

**Steps:**
1. **Digest-first reconciliation.**
   - A new request, `Digest{chunk, ext}`, returns per-block CRC-32C values taken from the
     store's metadata. Unwritten blocks have none, so no data is read.
   - The target repairs only the blocks whose digests differ between ISR members.
   - In the simulator, a replica's digest is its block values. A new mutation, trusting
     digests from a single member, must be caught.
   - *Done when:* after restarting both replicas of a 16 GiB volume, the first read waits under
     5 s.
2. **Persisting the uncertain set at checkpoints** (a format v1 compat feature), so a restart
   reports only what was really uncertain. Optional once item 1 is in.

## 3. Copying unwritten ranges

**Steps:**
1. **A store query for mapped extents:** `mapped(key, range) -> extents`.
2. **Resync and repair copy only mapped extents.** On the receiving side, an extent that is
   unmapped on the source becomes a discard, so stale data does not survive.
3. **Digests for resync too:** a returning replica copies only blocks whose digests differ
   (same `Digest` request as item 2).
4. *Done when:* a resync of the 16 GiB lab volume with 4 GiB written moves ≤ 4.5 GiB, and the
   replica's store uses about 4 GiB.

## 4. Failure domains in placement

**Steps:**
1. **Labels:** nodes register `topology.kubernetes.io/zone` and `flaminio.io/rack` (the node
   agent reads them from the Kubernetes Node object).
2. **Placement:** `Place` takes a failure-domain level per volume (`node` | `rack` | `zone`) and
   puts no two replicas of a chunk in one domain. `strict` refuses if the topology is short;
   `bestEffort` falls back a level (ARCHITECTURE §8.2, §9).
3. **Tests** in Go, with synthetic topologies (property test: no two replicas share a domain;
   load spreads).
4. **Lab:** label fl-a and fl-b as different zones. The lab cannot show real independence (one
   switch, and fl-w shares a server with fl-a); that needs a third physical server.

## 5. More than one volume per node

Today the engine lives on the attached device's queue thread, for the zero-copy local read
path.

**Steps** (come with item 1.2):
1. **The engine moves to a thread of its own.**
2. **Each device's queue threads** do their local reads with the per-queue admission check from
   item 1.2.
3. **The control socket** accepts any number of attaches.
4. *Done when:* two volumes are attached on one node, under fio on both, with the fault test
   passing for each.

## 6. Fuzzing

**Steps:**
1. **`make fuzz`:** installs the nightly toolchain and cargo-fuzz if missing, then runs every
   target for `FUZZ_SECONDS` each.
2. **Coverage:** the on-disk format targets and the network targets (`repl_frame`,
   `capability_token`).
3. *Done when:* every target runs 10 minutes clean, and a crash found is a test case in the
   repository.

## 7. Certificates

**Steps:**
1. **The operator** (phase 1 step 7+) holds the CA and issues 24-hour certificates on CSR, after
   a TokenReview of the node's service account token.
2. **Rotation:** etcd and the metadata service rotate their certificates without a restart.
   Rustls and Go both support reloading.
3. Until then, `make pki-issue` reissues the lab's certificates.

## 8. Deleting a volume's replicas

**Steps:**
1. **The metadata service keeps a tombstone** for each deleted volume (its id, and the nodes
   that held its chunks) until every one of those nodes confirms the deletion.
2. **Each node's agent** lists its tombstones, deletes the chunks (`Store::delete_chunk`,
   under a fencing epoch higher than any target's), and confirms.
3. *Done when:* creating and deleting 100 PVCs in a loop leaves the stores' used space where
   it started.

## 9. The data plane in Kubernetes

**Steps:**
1. **The node agent** (step 6) runs as a DaemonSet with the data plane in the same pod,
   privileged, with the data disk and `/dev/ublk*` from the host. The control socket moves to
   a directory shared by the agent and the CSI node plugin.
2. **Upgrades:** a node's data plane restarts only after its targets have moved away (drain);
   replicas rejoin by resync.
3. *Done when:* `make node-up` is no longer needed in a Kubernetes lab, and a rolling upgrade of
   the DaemonSet keeps a pod with a PVC running.

## 10. A faster first attach after a restart

When an attach completes, the new target must still reconcile. Reads of chunks reported uncertain
wait for it, including the kernel's partition scan and udev's probe of the new device.
- **Item 2 fixes the cause.**
- **Meanwhile:**
  - the device is created with partition scanning off (`UBLK_F_NO_AUTO_PART_SCAN` where the kernel has it; to be checked on 7.0);
  - the CSI node plugin's timeouts allow for a whole-volume reconciliation (150 s on the 1 GbE lab
    for 16 GiB).

## 11. Draining volumes that are not attached

Moves are carried out by the volume's target, so a volume without one keeps its replicas on a
draining node until it is next attached. The drain lists such volumes.

**Steps:**
1. **A maintenance target.** The drain attaches the volume to a node that can serve it but has
   no device for it: a target without a ublk device, which resyncs and moves, then detaches.
   That needs targets without devices, which come with item 5 (several volumes per node).
2. *Done when:* draining a node that holds replicas of unattached volumes completes, and
   attaching those volumes afterwards finds their data on the new nodes.
