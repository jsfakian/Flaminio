# Issue #54: Restore to another cluster — intent

[Issue #54](https://github.com/jsfakian/Flaminio/issues/54), README roadmap phase 3,
ARCHITECTURE §11.5, §14.1 and §14.3. This change is the issue's step 1 for unencrypted volumes: a
backup restored, from the bucket and the repository key alone, as a new volume of a cluster that
never held it, with the second lab cluster that checks it. Keys for encrypted volumes, metadata
disaster recovery, Velero and the runbooks build on it: issue #54 stays open after this change.
It assumes the first steps of [#52](https://github.com/jsfakian/Flaminio/issues/52) (backups) and
[#50](https://github.com/jsfakian/Flaminio/issues/50) (encryption) as their intents plan them.

## Why

A backup is worth what a restore is worth after the cluster that made it is gone. #52's first
step gets close: its repository is listed and read with the bucket and the key alone, each
backup whole, and a backup is written to a device or file the administrator names. The rest is
missing.

1. **A restore into another cluster is five steps by hand, and nothing ties them.** List the
   repository; `flaminio-meta create` with the size typed from the listing
   (`meta/cmd/flaminio-meta/main.go:380`); attach; restore onto `/dev/ublkbN`; detach. The
   volume's record knows none of it: `Header` has a mark for a volume being created and one
   for a volume being deleted (`meta/domain/domain.go:50`, `:53`), none for one being filled.
   Restore as a volume is left out of #52's first step, and the CSI controller refuses a
   content source (`control/csi/controller.go:213`).
2. **A volume half restored is a volume like any other.**
   - Any attach by id works. `ControllerPublishVolume` attaches whatever id a PV names
     (`controller.go:281`). From another node the epoch moves (`meta/service/service.go:723`)
     and the restore's target is deposed. On the restore's node the pod gets the device the
     restore is writing (`control/csi/node.go:64`).
   - `NodeStageVolume` formats a device on which `blkid` finds no signature
     (`node.go:107`–`:114`). Before the block holding the filesystem's signature arrives, a
     pod's mount runs `mkfs` under the restore.
   - A PV brought back by Velero names the source's volume id. Restored under that name, the
     volume is one such PV away from both.
   - A restore that stopped (the worker killed, the store away, no space) leaves the same
     volume for good, and nothing says it is not whole.
3. **Nothing of the source's metadata holds in the destination.** Placement names the source's
   nodes (`Chunk.Replicas`, `domain.go:21`); ISRs, epochs and tokens are the source's.
   - The destination has other nodes and failure domains, maybe fewer: `Place` refuses RF 2 on
     one node (`ErrPlacement`, `domain.go:361`).
   - A restored volume is a new volume. `createVolume` starts its epochs at the destination's
     revision (`service.go:465`), which a node needs to tell it from an earlier volume of the
     name (docs/issues/30): nodes know a volume by a hash of its name
     (`flaminio-node/src/lib.rs:21`). Its snapshot generations start again (#51).
   - The name may be taken there, by a volume or by a tombstone (`service.go:378`).
4. **Nothing names a cluster.** The superblock's `cluster_uuid` is nil at format
   (`flaminio-io/src/cli.rs:83`) and nothing checks it; etcd has no cluster record
   (`service.go:4`–`:13`); the trust domain is the only name (`pkg/pki/pki.go:5`). Two
   clusters' first epochs are unrelated revisions and can be equal. So the name and first
   epoch, which tell a volume from an earlier one of its name (#52, Why 7), do not tell a
   restored volume from its source when both back up into one repository.
5. **A restore through the device takes the node's one attach.** A node serves one attached
   volume at a time (`flaminio-io/src/control.rs:7`, follow-ups §5, #15). What it writes is
   replicated as any write: durable on every in-sync replica before it is acknowledged, with
   no copy afterwards.
6. **An encrypted volume's backup cannot take that path:** the target would encrypt its
   ciphertext again (#52, Why 9). It needs a write below the target, a record that carries the
   source's wrapped key, and a key source in the destination that unwraps it: #50's ADR and
   step 2. Under #50's first key source that is a key file carried between clusters.
7. **A restore only reads the bucket.** What must outlive the source is the bucket and the
   repository key (#52, outcomes 5 and 8). #53's ransomware runbook is this restore, into a
   clean cluster, of backups the store kept locked.
8. **The cluster's own metadata is another problem** (steps 3 and 5), and fails differently.
   - **Nothing snapshots etcd.** The lab installs `etcdutl` (`scripts/guest/etcd.sh:30`) and
     never runs it. The ISRs and epochs exist nowhere else (protocol §1).
   - **A snapshot is older than the disks.** Restored, it names in sync a replica removed
     since: serving it loses acknowledged writes. Its epochs are below the fences set since.
   - **The disks do not say who is in sync.** A chunk header has a volume UUID (a hash: the
     name is not there), an index, the sizes, `fenced_epoch` and `last_write_seq` (format
     §5.3). No replica count, minimum ISR, first epoch or ISR; the volume descriptor is
     reserved (page type 7). A `Hello` fences every replica it reaches, in sync or not
     (protocol §3). `seq` restarts at 0 in each epoch and the header keeps the highest
     (`flaminio-store/src/state.rs:477`); resync copies carry none. So §11.5's "highest epoch
     and sequence number" does not pick the in-sync replica. A replica under resync is
     `active` on disk, and a move's source stays there (#13).
   - **A rebuilt store is what #30 forbids.** An etcd wiped while nodes keep their data is
     "not supported" (docs/issues/30, S2), and a record's first epoch decides what a node
     deletes at the next `Hello`.
9. **The lab is one cluster,** one trust domain (`scripts/lab.conf:103`), without an object
   store. #58 plans a second cluster of one node on fl-c; #52 plans the store.

## What we want

1. **One command restores a backup as a new volume,** in a cluster that holds no record of the
   source, from the bucket and the repository key alone. The backup says what the volume needs:
   its size, and whether it is encrypted.
2. **The volume is placed anew,** by the destination's placement over its own nodes. The
   administrator names the replica count, minimum ISR and failure domain; the source's are not
   assumed to fit. Its name is the source's unless the administrator gives another (once #73's
   tenancy is active, a source name of the tenant form `pvc-<UID>` needs another). A name
   taken, by a volume or a tombstone, is refused: a restore never writes into an existing
   volume.
3. **A restored volume is a new volume.** Its first epoch is the destination's, it has no
   snapshots, and it carries none of the source's placement, ISRs, epochs or tokens. Its
   record says which backup it came from. A later backup of it is never a diff against the
   source's backups, whatever the two clusters' epochs.
4. **It is not usable until it is whole.** Until the restore has completed, nothing else can
   attach the volume: no pod, no administrator. A restore that stopped leaves a volume marked
   incomplete, which running the restore again completes, and which can be deleted. It never
   leaves a volume that looks whole.
5. **The volume holds exactly the backup,** on every in-sync replica, when the restore reports
   success: written ranges equal, ranges never written still unwritten. A block missing or
   altered in the store fails the restore, by name; it never completes with other data.
6. **An encrypted volume's backup is refused,** by name, before any volume is created. Its
   ciphertext never goes through a target.
7. **The restore needs no right to write or delete in the bucket,** and no call to the source
   cluster. The repository key reaches the destination as #52's ADR provides, and is in the
   clear in no record, command line or log there.
8. **A second lab cluster,** as #58 plans it: one node on fl-c, with its own trust domain, CAs,
   token key, etcd member, `flaminio-meta` and data plane on its own disk. `docs/lab.md`
   describes it. Whichever of #58 and this change comes first builds it.
9. **A lab check.** A volume written with a known pattern under load is snapshotted and backed
   up in the first cluster, which is then stopped. The second cluster lists the repository and
   restores the volume; its content equals the snapshot's. Also checked: a restore killed
   midway cannot be attached and completes when run again; a name taken is refused; an
   encrypted volume's backup is refused; an altered object fails the restore by name. The
   restore's rate is recorded, not judged.
10. **ARCHITECTURE §14.1 describes restore as built.**

## Out of scope

- **Step 2: keys.** An encrypted volume restored as its ciphertext, below the target, with the
  source's wrapped key in its record; the same KMS key, or an exported escrow, in the
  destination. It needs #50's step 2. Until then such a backup restores as ciphertext to a
  file (#52).
- **Step 3: metadata disaster recovery.** Encrypted metadata snapshots on the backup target,
  and `recover meta` from the disks (Why 8). Which replica is in sync after either is a safety
  rule: it needs the simulator, probably the format, and S2 of #30 revisited.
- **Step 4: Velero and Kubernetes objects.** The CSI snapshot data-mover path needs
  `VolumeSnapshot` (#51 step 4). Outcomes 2 and 4 are its seam: a PV brought back finds its
  volume by name, and only once it is whole.
- **Step 5: the runbooks of §14.3.** Whole-cluster loss needs steps 2 and 4; metadata quorum
  loss needs step 3; ransomware is #53's.
- **A PVC from a backup,** and restore into the cluster that still holds the volume: #52's
  step 5, on the CSI content source.
- **#53's check of a locked backup.** The restore reads the manifests it is given; #53 says
  which to trust.
- **Several restores at once on a node** (#15), **instant restore** (#68: the volume usable
  before its data is local), and a restore faster than the write path.
- **Backing the restored volume up,** schedules and retention in the destination: #52's
  later steps. Outcome 3 is the seam.
- **A cluster identity** on disks and in the metadata service, beyond what outcome 3 needs.
- **Room for the volume.** Placement counts chunks, not free space (§11.6 is not built): a
  restore without room stops, as in outcome 4.
- **The audit log** of restores (§15.3): #73 plans it. A restore's deletion writes #73's
  deletion events; events for the rest of a restore are a follow-up (to be filed).
