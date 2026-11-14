# Issue #14: Tombstones on nodes outside Kubernetes — intent

[Issue #14](https://github.com/jsfakian/Flaminio/issues/14), follow-ups §14. This change is the
issue's step 2, and what the lab needs from it. It shipped (pull request #31); issue #30 merged
after it. What is left of it: the corrections the design lists under "The next step". Step 1 is
issue #13's: issue #14 stays open after this change.

## Why

As the lab and the code stood before step 2. Issue #30 changed what items 5 and 6 say: each
says how.

Deleting a volume leaves a tombstone, `/flaminio/v1/tombstones/<id>`. It names the nodes that
the volume's chunk records named, as replicas or in a move (`replicaNodes`, `DeleteVolume`,
`meta/service/service.go`). Each of them deletes its replicas and confirms (`ConfirmDeletion`).
The last confirmation removes the tombstone. Until then the id is taken: `CreateVolume` answers
`AlreadyExists ... still has a deletion tombstone`, so that a sweep still running cannot wipe a
new volume with the same id.

1. **Only the CSI node plugin sweeps** (`control/csi/tombstones.go`). Every 5 s it lists its
   node's tombstones, deletes the node's replicas through the data plane's control socket, and
   then confirms. A data node without one never confirms. In the lab that is fl-c: `k8s up`
   starts k3s agents on fl-a and fl-b only (`cmd_k8s`, over `vms_of all`).
2. **The lab has two such tombstones** (etcd, 2026-10-30): `/flaminio/v1/tombstones/wbench` and
   `/flaminio/v1/tombstones/volb`, each
   `{"nodes":["fl-a","fl-b","fl-c"],"confirmed":{"fl-a":true,"fl-b":true}}`. fl-a's and fl-b's
   plugins confirmed; fl-c never will. Once a tombstone names a node outside Kubernetes, nothing
   removes it, and the id is taken for good.
   - **Why all three nodes.** A tombstone does not name every node: only those the chunk
     records named when the deletion listed them. volb's chunks were on fl-b+fl-a and fl-b+fl-c
     ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md)). An RF 1 wbench, created with
     fl-b preferred (`write_volume`, `domain.Place`), names fl-b alone. So the wbench deleted
     last had replicas, or moves, on all three nodes; which run created it is not recorded.
3. **The write campaign cannot run.** `make bench-writes` deletes and creates wbench again
   before every run (`cmd_bench_writes` → `write_reset` → `delete_wbench`, then
   `write_volume`). `metactl create wbench` now fails, and the campaign with it. Issue #10's
   measurement waits for it, and #10's design then creates volb again.
4. **No command confirms.** `ConfirmDeletion` accepts an administrator for any node, and a
   node's agent for itself (`Authorize`, `meta/service/auth.go`). But `flaminio-meta`, which the
   lab runs as `metactl`, has no command for tombstones.
5. **The harness deleted the replicas already, but could not tell that it did.** `delete_wbench`
   deleted wbench's replicas on fl-a, fl-b and fl-c through each node's control socket
   (`node.sh delete`), because a new wbench started at epoch 1 again and was deposed by old
   replicas fenced at a later epoch. Since #30 a new wbench starts above every epoch of its name
   and drops those replicas at its first session; they are deleted to free their space
   (`delete_wbench`'s comment, `scripts/lab.sh`). `node.sh delete` succeeded without deleting
   anything when the node was not running (it returned at once, logging nothing), and the
   harness discarded its output. Nobody then confirmed for fl-c.
6. **A wrong confirmation is worse than a blocked id.** A node's store names a volume by a hash
   of its name alone (`volume_uuid`, `flaminio-node/src/lib.rs`), and every delete of a name
   through a node's control socket (the CSI sweep's, `node.sh delete`'s) deletes all its chunks
   there, whatever their fence (`Store::delete_volume(uuid, None)`, `replica/mod.rs`). A
   confirmation that frees the id while a delete has yet to run on the node lets that delete
   wipe the replicas of a volume created again with the name there: with RF 1, its data. That is
   what the tombstone guards (`createVolume`). Before #30, leftovers on a confirmed node also
   became the new volume's chunks, or deposed it. Since #30 a volume created again starts above
   every epoch of its name and drops them at its first session on each node. Until then they
   hold space, and the old volume's tokens still open sessions on them; a volume created by an
   old metadata service (a rolling upgrade, a downgrade) is still deposed by them (#30, S8 and
   S9).

## What we want

1. `flaminio-meta` commands to list tombstones and to confirm a deletion for a node. Confirming
   needs the node's own agent and an explicit acknowledgement that its replicas are deleted,
   and says what it is about to confirm.
2. The lab harness deletes a volume's replicas on every data node and waits for its tombstone
   to go. It never confirms by itself. A tombstone left pending for fl-c stops it at once, with
   the procedure to confirm by hand.
3. A lab command, run by an operator, that confirms for fl-c only after checking on fl-c that no
   sweep runs there and that its replicas are deleted, as fl-c's own agent. Exactly one confirmer
   per node: the CSI sweep on fl-a and fl-b, an operator on fl-c.
4. The lab's stale tombstones pending for fl-c cleared once, with the new commands, after fl-c's
   replicas are deleted: wbench, volb, and those of the one-off volumes.

## Out of scope

- **Step 1: the sweep on every data node** (follow-ups §14). Issue #13 takes it in: the sweep
  runs inside the node daemon, `flaminio-io node`. Then no data node is without a sweep, and
  `vol-confirm`, `confirm.sh` and `flaminio-meta confirm` go in the same change (#13, S13).
- **The metadata service's rules:** `ListTombstones`, `ConfirmDeletion` and their caller rules
  stay as they are. The one change is a log line, with its revision, for each new confirmation
  (spec, S4).
- **A tombstone's incarnation.** Tombstones are keyed by the volume's name alone, so a
  confirmation meant for one deletion can land on the next one of the same name. A revision or
  incarnation field, given by the listing and required by the confirmation, would close that. It
  changes the API; decisions and follow-ups §14 record it for later.
- **The CSI sweep** stays as it is.
- **Confirming for a node gone for good.** Only a node's own agent confirms in this step: an
  administrator could confirm for any stopped node, which still holds its replicas, and a
  liveness lease cannot tell the two apart. Decisions and follow-ups §14 record it.
- **Replicas no record names,** such as a completed move's source: issue #13.
- **The other paths that delete volumes with replicas on fl-c:** `meta-smoke` (`smoke-*`),
  `scripts/checks/resync.sh` (`resync-*`), and the CSI controller's deletions of PVCs. Their ids
  are not reused, so they block nothing, but they keep leaving tombstones pending for fl-c, and
  replicas there. Decisions and follow-ups §14 record them; step 1 ends them.
- **A lab-wide lock.** Two harness runs at once (two operators, or a workstation and the lab
  runner by hand) can delete each other's replicas, as today. Decisions and follow-ups §14
  record a lock for later.
- **What fl-c holds beyond its agent identity.** `cmd_pki issue c` also puts the token key and a
  metadata service etcd client certificate on fl-c, which is not a metadata member. Root on fl-c
  can write etcd or mint tokens: confirming as fl-c's agent guards against mistakes, not against
  a compromised fl-c. Decisions and follow-ups §14 record restricting those to the metadata
  members.
