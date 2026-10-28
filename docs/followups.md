# Open items and how to close them

What phase 1 left unfinished, with a plan for each: what to build, how to know it works, and in
which order. Each item names the step of [phase1-plan.md](phase1-plan.md) or the later phase it
belongs to.

The open work is tracked as
[issues labelled follow-up](https://github.com/jsfakian/Flaminio/issues?q=label%3Afollow-up),
in [milestones](https://github.com/jsfakian/Flaminio/milestones) ordered by what they depend on.

| # | Item | Why it matters | Order | Issues |
|---|---|---|---|---|
| 1 | Read performance, and a write benchmark that can be trusted: [performance plan](performance-plan.md) | Reads meet 90% of raw at QD32 (95%) and 1M (≈ 100%); 4K QD1 reads (84%) and 1M writes (about 70%) do not | Next | [#9](https://github.com/jsfakian/Flaminio/issues/9), [#10](https://github.com/jsfakian/Flaminio/issues/10) |
| 2 | Reconciliation after a replica restart | Done: the first read after both replicas of a 16 GiB volume restarted waits 0.2 s (was 160 s) | Before the chaos run (step 8) | [#11](https://github.com/jsfakian/Flaminio/issues/11) |
| 3 | Copying unwritten ranges | Done: a resync of 4 GiB written moves 4.3 GiB and stores 4.0 GiB (was 16 GiB) | With item 2: same tools | – |
| 4 | Failure domains in placement | Done: zone and rack placement, from `metactl` and the StorageClass; the lab cannot show physical independence | Before the edge profile | [#7](https://github.com/jsfakian/Flaminio/issues/7), [#18](https://github.com/jsfakian/Flaminio/issues/18) |
| 5 | More than one volume per node | A node serves one attached volume at a time | With item 1: same redesign | [#15](https://github.com/jsfakian/Flaminio/issues/15) |
| 6 | Fuzzing actually running | `make fuzz` runs every target; the 10-minute run each is open (weekly in CI) | Now: small | [#4](https://github.com/jsfakian/Flaminio/issues/4) |
| 7 | Short-lived certificates, rotated | Lab certificates last 30 days and are issued by hand | With the operator (phase 1 step 7+) | [#16](https://github.com/jsfakian/Flaminio/issues/16) |
| 8 | Deleting a volume's replicas | Done: 100 PVC create/delete cycles leave the stores' used space where it started | With the node agent (step 6) | [#8](https://github.com/jsfakian/Flaminio/issues/8) |
| 9 | The data plane in Kubernetes | `flaminio-io node` runs as a systemd unit beside k3s; the CSI node plugin drives it through its control socket | With the node agent (step 6) | [#12](https://github.com/jsfakian/Flaminio/issues/12) |
| 10 | A faster first attach after a restart | Attach limits sized for what an attach does now (docs/6); partition scanning goes off once libublk accepts the flag (ublk-org/libublk-rs#72) | With item 2 | [#6](https://github.com/jsfakian/Flaminio/issues/6) |
| 11 | Draining volumes that are not attached | A drain moves only attached volumes: their target carries the moves out | With the operator | [#17](https://github.com/jsfakian/Flaminio/issues/17) |
| 12 | Zero-copy local reads after an attach | With `--zero-copy`, local reads that waited for reconciliation fail with EINVAL, and the target drops the local replica for 3 s each time | Before the chaos run (step 8) | [#5](https://github.com/jsfakian/Flaminio/issues/5) |
| 13 | Replicas left behind by moves | A move leaves the source's replica on its disk: fl-a kept all 64 moved replicas (4 GiB) after the item 3 check | With the node agent (step 6) | [#13](https://github.com/jsfakian/Flaminio/issues/13) |
| 14 | Tombstones on nodes outside Kubernetes | Only the CSI node plugin sweeps tombstones; a data node without one (fl-c) keeps the replicas, and the volume id stays taken | With the node agent (step 6) | [#14](https://github.com/jsfakian/Flaminio/issues/14) |

## 1. Read performance

Superseded by the [performance plan](performance-plan.md), which adds a profile, measurement
rules and a plan for writes. Kept below for history.

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
1. **Digest-first reconciliation.** ✅ (protocol, simulator, store, data plane)
   - A new request, `Digest{chunk, ext}`, returns per-block CRC-32C values taken from the
     store's metadata, so no data is read. They are digests of what a read returns: an
     unwritten block gets the zero block's CRC, so replicas compare by content whatever their
     layout. One request covers up to a default chunk (65 536 blocks).
   - The target repairs only the blocks whose digests differ between ISR members.
   - In the simulator, a replica's digest is its block values. A new mutation, trusting
     digests from a single member, must be caught (`catches_trust_single_digest`).
   - *Lab, 2026-10-28:* after restarting both replicas of `vol1` (16 GiB), the first read
     waited 0.21–0.25 s, and one block of every chunk was read within 0.9–1.0 s (was 152–160
     s). Without zero copy; with it, see item 12.
   - The lab found two bugs on the way: unwritten blocks without a digest counted as differing
     from written zeros (a restart rewrote about 12 GiB of zeros onto fl-c), and repair-sized
     digest requests (256 blocks) kept the first read at 5–8 s.
   - Left: reconciliation queues a volume's chunks last to first, so chunk 0 is reconciled
     last. At about 1 s for 16 GiB this matters only for much larger volumes; fixing it
     reorders the simulator's runs, so the mutation seeds need searching again.
2. **Persisting the uncertain set at checkpoints** (a format v1 compat feature), so a restart
   reports only what was really uncertain. Optional once item 1 is in.

## 3. Copying unwritten ranges

**Steps:**
1. **A store query for mapped extents:** `mapped(key, range) -> extents`. ✅
2. **Resync and repair copy only mapped extents.** ✅ Protocol `Mapped` / `Discard`; the target
   probes the source before `BeginResync`, copies only mapped blocks, and discards holes on the
   joiner (with `force_copy` so writes during the probe are not lost).
3. **Digests for resync too:** ✅ after Mapped, the target digests source vs joiner and copies
   only disagreeing blocks (same `Digest` request as item 2).
4. *Lab, 2026-10-28:* a 16 GiB RF1 volume with 64 MiB written at the start of each chunk (4
   GiB) moved, chunk by chunk, from fl-a to fl-b, where it had no replica. fl-b received
   4 356 MiB over the overlay, WireGuard overhead included, and stored 65 536 clusters
   (4.0 GiB); every written range read back, and unwritten ranges read zeros. Resyncing `vol1`
   back onto fl-a, which already held its data, moved 46 MiB for 16 GiB.
   - The lab found that a whole-chunk `Mapped` probe was longer than the frame decoder allowed
     for a request: the replica closed the session on every attempt. Probes now have their own
     limit (65 536 blocks), and a larger chunk is probed in pieces.

## 4. Failure domains in placement

**Steps:**
1. **Labels:** ✅ nodes register `topology.kubernetes.io/zone` and `flaminio.io/rack`. The CSI
   node plugin (the node agent for now) copies them from the Kubernetes Node object; lab
   registration via `metactl` / `lab.sh` sets them on fl-a and fl-b directly.
2. **Placement:** ✅ `Place` takes a failure-domain level per volume (`node` | `rack` | `zone`)
   and puts no two replicas of a chunk in one domain. `strict` refuses if the topology is short;
   `bestEffort` falls back a level (ARCHITECTURE §8.2, §9). Threaded through Spec,
   CreateVolume and CSI StorageClass `failureDomain` / `failureDomainPolicy`.
3. **Tests:** ✅ Go tests in `meta/domain` with synthetic topologies (no two replicas share a
   domain; load spreads; strict refusal; bestEffort fallback).
4. **Lab:** label fl-a and fl-b as different zones (scripted in `lab.sh` meta smoke; for CSI,
   `kubectl label node fl-a topology.kubernetes.io/zone=lab-a flaminio.io/rack=rack-a` and the
   same for fl-b with `lab-b` / `rack-b`). The lab cannot show real independence (one switch,
   and fl-w shares a server with fl-a); that needs a third physical server.
   - *Lab, 2026-10-28, through `metactl`:* zones lab-a and lab-b put every chunk on fl-a and
     fl-b; with both nodes in one zone, zone/strict was refused ("1 distinct zone domains, 2
     replicas") and zone/bestEffort fell back to racks; re-registering a node's zone kept its
     rack.
   - *Through Kubernetes:* the CSI node plugins copied `zone-a` and `zone-b` from the Nodes,
     and a PVC on a zone-strict StorageClass got a replica in each. With both Nodes in one
     zone, the PVC stayed Pending: `ProvisioningFailed ... 1 distinct zone domains, 2
     replicas`.
   - Left: a volume does not record its failure domain, so moves and drains ignore it.

## 5. More than one volume per node

Today the engine lives on the attached device's queue thread, for the zero-copy local read
path.

**Blocked on item 1** (left for later): the per-queue admission and several ublk queues of
item 1.2.

**Steps** (come with item 1.2):
1. **The engine moves to a thread of its own.**
2. **Each device's queue threads** do their local reads with the per-queue admission check from
   item 1.2.
3. **The control socket** accepts any number of attaches.
4. *Done when:* two volumes are attached on one node, under fio on both, with the fault test
   passing for each.

## 6. Fuzzing

**Steps:**
1. **`make fuzz`:** ✅ `scripts/dev/fuzz.sh` installs the nightly toolchain and cargo-fuzz if
   missing, then runs every target for `FUZZ_SECONDS` each (default 60).
2. **Coverage:** ✅ on-disk format (`superblock`, `metadata_pages`, `journal`) and network
   (`repl_frame`, `capability_token`).
3. *Done when:* every target runs 10 minutes clean (`FUZZ_SECONDS=600 make fuzz`), and a crash
   found is a test case in the repository. Short smoke (`FUZZ_SECONDS=5`) is clean.

## 7. Certificates

**Blocked on the operator** (phase 1 step 7+): CSR issuance after TokenReview and 24-hour
certificates. Until that lands, `make pki-issue` remains the interim path for lab certificates.

**Steps:**
1. **The operator** (phase 1 step 7+) holds the CA and issues 24-hour certificates on CSR, after
   a TokenReview of the node's service account token.
2. **Rotation:** etcd and the metadata service rotate their certificates without a restart.
   Rustls and Go both support reloading.
3. Until then, `make pki-issue` reissues the lab's certificates.

## 8. Deleting a volume's replicas

**Steps:**
1. **The metadata service keeps a tombstone** for each deleted volume (its id, and the nodes
   that held its chunks) until every one of those nodes confirms the deletion. ✅
   `DeleteVolume` drops the volume header and chunk metadata, writes `/flaminio/v1/tombstones/<id>`,
   and exposes `ListTombstones` / `ConfirmDeletion`. The volume id cannot be reused while the
   tombstone remains (so an agent sweep cannot wipe a recreated volume).
2. **Each node's agent** lists its tombstones, deletes the chunks (`Store::delete_chunk`,
   under a fencing epoch higher than any target's), and confirms. ✅ The CSI node plugin sweeps
   via the data plane's control-socket `delete` (which calls `ReplicaService::delete_volume`).
3. *Lab, 2026-10-28:* 100 cycles of a PVC (RF 2, 1 GiB) and a pod writing 16 MiB through
   its filesystem, alternating fl-a and fl-b, then both deleted: about 20 s a cycle. Afterwards
   fl-a and fl-b had the same chunk replicas and clusters in use as before (168 / 431 167 and
   144 / 393 216), and no tombstone was left.
   - Left: data nodes outside Kubernetes (item 14), and `DeleteVolume` reads the chunks
     outside the transaction that writes the tombstone. A replica moved in between would be
     missed; today a deleted volume is never attached, and only attached volumes move.

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
wait for it.
- **Item 2 fixed the cause:** reconciling 16 GiB after both replicas restarted takes about 1 s.
- **The attach no longer waits for reads** (docs/6, measured on Linux 7.0). The control socket
  answers in under 0.1 s, even when the first read waits 5.8 s for a takeover: the kernel's
  partition scan runs after the device has started.
- **Done: limits sized for what an attach does now.**
  - The data plane answers a control command within 50 s: 30 s for the metadata service, 10 s
    to add the device, and 10 s to start it.
  - The CSI node plugin waits 60 s, unless kubelet's deadline comes first.
  - An attach that times out is dropped in the engine, so a late metadata answer cannot start
    a target without a device. It is also withdrawn on the control socket if it has not begun.
- **Pending: partition scanning off** (`UBLK_F_NO_AUTO_PART_SCAN`, which Linux 7.0 has). It
  keeps a partition table inside a volume off the host. libublk 0.4.8 refuses the flag; it goes
  in once a release includes ublk-org/libublk-rs#72.

## 11. Draining volumes that are not attached

Moves are carried out by the volume's target, so a volume without one keeps its replicas on a
draining node until it is next attached. The drain lists such volumes.

**Steps:**
1. **A maintenance target.** The drain attaches the volume to a node that can serve it but has
   no device for it: a target without a ublk device, which resyncs and moves, then detaches.
   That needs targets without devices, which come with item 5 (several volumes per node).
2. *Done when:* draining a node that holds replicas of unattached volumes completes, and
   attaching those volumes afterwards finds their data on the new nodes.

## 12. Zero-copy local reads after an attach

Found in the item 2 lab check. With `--zero-copy`, a local read that waited for reconciliation
right after an attach fails: io_uring's `ReadFixed` into the request's auto-registered buffer
returns EINVAL. A failed local read gets no reply, so the target drops the local replica from
the ISR after its request timeout (3 s), then resyncs it. On `vol1` this happened 4–5 times per
attach: the first read still came in 0.2 s, but every chunk was readable only after 7.2 s
instead of 1 s.

**Steps:**
1. **Find why the buffer is not valid:** the device and the kernel's partition scan read first
   and wait; the frontend now logs which step of a zero-copy read failed.
2. **A failed local read should fail over to another ISR member** for that request, not drop
   the local replica.
3. *Done when:* restarting both replicas of `vol1` with zero copy logs no failed local read,
   and every chunk is readable within the time measured without zero copy.

## 13. Replicas left behind by moves

A move ends with the source dropped from the chunk's replicas in the metadata service, but the
source keeps its copy on disk. After the item 3 check moved all 64 chunks of a volume from fl-a
to fl-b, fl-a still held them (4 GiB) until they were deleted by hand through the control
socket (`node.sh delete`).

**Steps:**
1. **The node agent deletes replicas the metadata service no longer places on it**, under a
   fencing epoch higher than any target's, as for tombstones (item 8). A replica of a chunk in
   a move is kept until the move ends.
2. *Done when:* draining a node leaves no chunk replicas of the drained volumes on its disk.

## 14. Tombstones on nodes outside Kubernetes

Only the CSI node plugin sweeps tombstones (item 8). A data node without one, such as fl-c in
the lab, never confirms: its replicas of deleted volumes stay, and the volume ids stay taken.

**Steps:**
1. **The sweep moves into the node agent** (step 6), which runs on every data node.
2. Until then, an administrator can confirm a deletion for a node that is gone for good
   (`ConfirmDeletion` accepts administrators); `metactl` needs a command for it.
