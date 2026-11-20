# Issue #52: Incremental backups to S3 — intent

[Issue #52](https://github.com/jsfakian/Flaminio/issues/52), README roadmap phase 3,
ARCHITECTURE §14.1, §15.1 and §1.1. This change is the issue's steps 1, 2, 3 and 6 for one
snapshot at a time, at an administrator's command, with the part of step 5 that proves a
backup: reading it back, verified, onto a device. Schedules and retention (step 4) and restore
as a volume (the rest of step 5) build on it: issue #52 stays open after this change. It assumes
the first steps of [#51](https://github.com/jsfakian/Flaminio/issues/51) (snapshots) and
[#50](https://github.com/jsfakian/Flaminio/issues/50) (encryption) as their intents plan them.

## Why

ARCHITECTURE §14.1 describes what is wanted. The code has none of it.

1. **Nothing backs up.** No object-store client, BLAKE3, zstd or XChaCha20-Poly1305 in `go.mod`
   or in the Cargo workspaces (`ring`, the data plane's one crypto library, has
   ChaCha20-Poly1305 with a 96-bit nonce only). No backup record or RPC (`meta.proto`), no
   `flaminio-meta` command. The CSI controller refuses the README's `backupPolicy` as an unknown
   parameter (`parseParams`, `control/csi/controller.go:157`).
2. **What a backup can read is #51's.** Its first step gives, per replica, a snapshot read,
   digested and mapped at a generation, and layers keyed by generation: those from *a* up to
   *b* are what changed between two snapshots. Listing those changes is not among its outcomes.
   - The unit of change is the cluster (64 KiB by default): the first small write after a
     snapshot copies its whole cluster (format §8). The unit of upload is §14.1's 4 MiB block,
     64 clusters: one 4 KiB write costs a 4 MiB read and upload, before compression.
3. **No node need hold a whole volume, and a store has one reader.** Placement is per chunk
   (`ChunkMeta.replicas`, `meta.proto:104`). A device is opened `O_EXCL`
   (`flaminio-uring/src/uring.rs:131`): a second process cannot read a running node's store
   (`scripts/guest/digest.sh` wants the services stopped). So a backup reads through the running
   data plane of several nodes.
4. **Only a target can read a replica, and its session takes the volume over.**
   - A session is accepted from a node's data plane (`.../node/<node>/io`), with a token that
     names the `Hello`'s volume and epoch and the peer's node (`flaminio-net/src/session.rs:116`,
     `:143`). Tokens are issued by `Attach` alone, to the target (`meta/service/service.go:744`).
   - `Hello` fences the volume at its epoch and becomes its current session; requests of any
     other session are dropped (`flaminio-repl/src/replica.rs:173`–`190`, protocol §3). A reader
     with the target's token would cut the target's sessions, or be dropped itself.
   - A token never expires: it is worth nothing once its epoch is fenced off
     (`pkg/captoken/captoken.go:24`). A snapshot does not change with the epoch, so nothing
     would end a right to read one.
5. **Which replica holds a snapshot changes under the reader.** #51 promises it of in-sync
   replicas. The ISR is in the metadata service, readable by any identity
   (`meta/service/auth.go:79`), and moves with every resync and move (`isr_ver`). A replica
   being resynced, or created again by a move, holds a snapshot in part.
6. **Replica reads share the application's thread.** A `Read` is at most 1 MiB
   (`flaminio-net/src/frame.rs:14`), served with a buffer of its own on the engine's thread
   (`flaminio-node/src/replica/mod.rs:391`), which owns the store and every target: today the
   attached device's queue thread (follow-ups §5). §16.1 names fair queuing between foreground
   and backup work; the engine has none.
7. **An incremental needs a base both sides still hold, of the same volume.**
   - #51's first step never deletes a snapshot, so the base stays. But a chunk replica has at
     most 64 layers (format §8): a volume can be snapshotted, and so backed up at a new point,
     fewer than 64 times in its life. Schedules mean nothing before snapshots can be deleted.
   - A volume created again under its name starts its generations again: the store names a
     volume by a hash of its name (`flaminio-node/src/lib.rs:21`), and only the first epoch
     tells the two apart (`meta/domain/domain.go:47`, docs/issues/30). A diff against the earlier
     volume's backup would be wrong, and nothing would say so.
8. **Encrypted volumes are ciphertext below the target** (#50), so a replica-level reader needs
   no volume key: §14.1's "backup workers never see plaintext".
   - XTS under a key per volume, tweaked by block number: equal plaintext is not equal
     ciphertext, and ciphertext does not compress. Deduplication and zstd gain nothing there.
   - Restoring needs the volume's key. If its only copy was in the cluster that is gone, the
     backup is unreadable. The same holds for the repository's own key.
9. **Restore has no path.** The CSI controller refuses a content source
   (`control/csi/controller.go:213`); #51's first step has no clone and no snapshot as a device.
   What exists is a new volume's block device, written as any application writes. That path
   encrypts what it is given: it cannot take an encrypted volume's ciphertext.
10. **The worker's home is not decided.** ADR 0001: Go for what faces Kubernetes, Rust for
    `dataplane/`, the two talking gRPC only. §5.5 lists backup among the Go controllers; §14.1
    says "backup workers" and no more. Replica reads are `flaminio-net` frames, in Rust; the data
    plane serves no gRPC. A bucket's content is untrusted input (§15.1).
11. **The format is expensive to undo.** Every later restore reads every earlier backup. §14.1
    names the primitives, not the layout, the versions, the key derivation or the manifest.
12. **Neither the lab nor `make test` has an object store.** MinIO's server is AGPL. fl-w, the
    one VM without volume data, has 2 vCPUs, 4 GiB, a 40 GiB root disk and an 8 GiB data disk
    (`scripts/lab.conf`); the lab's volumes are 16 GiB.

## What we want

1. An ADR, accepted before the design, for what this step fixes for the later ones: where the
   backup worker runs, in which language and under which identity; the object-store client; the
   repository format and its version; the repository key, who holds it, and how it survives
   the cluster.
2. A right to read a snapshot that is not a target's. The metadata service grants it per
   volume and snapshot, to the worker's identity alone. A replica serves it reads of that
   snapshot and nothing else: no write, no fence, no session taken from the target. It does not
   outlive the volume, nor pass to a later volume of the name.
3. A replica says which ranges of a chunk changed between two snapshots, from its layers: the
   same answer on every in-sync replica. #58 ships the same ranges.
4. An administrator backs up one snapshot of a volume into a repository in an S3-compatible
   store. The backup holds exactly the snapshot: each chunk is read from a replica that holds
   it in full, and a replica that left the ISR, moved or was created again meanwhile is
   noticed, not trusted. It works attached or not, and through a failover.
5. The repository is §14.1's: 4 MiB blocks under keyed BLAKE3 addresses, zstd,
   XChaCha20-Poly1305, an encrypted manifest per backup. The store sees nothing in the clear.
   With the bucket and the repository key alone, and no metadata service, its backups can be
   listed and read. Each backup is whole: reading it needs no other backup's manifest. Ranges
   never written take no space, and read back unwritten.
6. A later backup of the volume reads from the replicas, and uploads, only the blocks holding
   ranges that changed since the newest snapshot the repository holds and the replicas still
   have. Without such a base (a first backup, another volume of the name, a base gone from the
   replicas) the backup is a full one, and says so. It is never a diff against the wrong base.
7. An encrypted volume is backed up as the ciphertext its replicas hold. The worker never asks
   for its key. The backup records what a restore needs to find that key (#50's ADR).
8. The repository key is in the clear in no bucket, record, command line or log, and can be
   had again after the cluster is lost.
9. The object store away, or the worker killed, at any point: every earlier backup stays
   readable, the interrupted one is not listed, and running it again completes it without
   uploading again the blocks that arrived. No application I/O waits for the object store.
10. A backup can be proven and used. Verifying one reads it back in full and checks every
    block, and, while the snapshot exists, compares it with the snapshot on the replicas.
    Restoring one writes it to a block device or file the administrator names: for an
    unencrypted volume, a new volume's device. A block missing or altered in the store fails
    both, by name; neither returns wrong data.
11. A backup under application load fails no application I/O, and its replica reads are bounded.
12. Tests and the lab. `make test` runs the worker against a store that needs no server, and
    the decoders of what a bucket returns are fuzzed. The lab gets an S3-compatible store on a
    VM, by a lab command, under a licence the project accepts. A lab check, on a volume under
    write load: a backup, then a second one that uploads only what changed (counted); the store
    stopped during a backup, which then resumes; a failover during a backup; an object altered,
    which verification names; a restore onto a new volume, equal to the snapshot; an encrypted
    volume's backup, equal to its replicas' ciphertext; no known plaintext in the bucket.
    Measured, by the performance plan's rules: the backup's rate, and the load's with and
    without it.

*Deferred after approval (plan review round 1, R1-C6, R1-S1), not settled:* outcome 7 and
outcome 12's encrypted-volume check wait for the plan that lifts #51's refusal of snapshots of
encrypted volumes (#50 and #51, both clear, admit none until the keys are pinned). This step
refuses an encrypted volume. The outcomes' text is unchanged.

## Out of scope

- **Step 4: `BackupTarget`, `BackupPolicy`, schedules and retention,** and the Go controller
  that runs them. They need snapshots deleted (#51 step 3; Why 7). Until then the worker's
  settings are its command's.
- **Deleting backups and collecting their blocks.** The repository only grows. The worker only
  adds objects: it needs no right to delete or overwrite, which #53 builds on.
- **Holding the base.** Once #51 can delete a snapshot, the newest one backed up must stay
  until the next backup, or that one is full. The metadata service's record of what is backed
  up comes with it.
- **The rest of step 5: restore as a volume.** A PVC from a backup needs the CSI content source
  (#51 step 4). An encrypted volume's ciphertext needs a write path below the target, and its
  key (#50): until then its backup can be verified and restored as ciphertext, not read.
- **Taking the snapshot.** The administrator cuts it with #51's command, then backs it up.
  Application consistency and groups of volumes are #51's steps 5 and 6.
- **CSI `VolumeSnapshot` and Velero's data-mover path:** #51 step 4, #54.
- **#53, immutable backups:** Object Lock, write-only credentials, the retention identity.
- **#54, restore to another cluster:** key escrow, metadata snapshots on the backup target
  (§11.5), the runbooks. Outcomes 5 and 8 are its seam.
- **#59, WAN tolerance:** bandwidth caps, time windows, long outages, fleets. Outcome 9 is its
  seam.
- **#58, asynchronous replication,** which shares outcomes 2 and 3; **#68, instant restore.**
- **Deduplication across repositories,** and content-defined block boundaries.
- **Backup throughput:** measured, not tuned.
- **Checking a repository periodically.** Verification runs on request.
- **Audit events** of backups and restores (§15.3): #73 defines the audit log and leaves these
  events to the issues that add the operations (#73 S7); a follow-up, to be filed (round 1,
  R1-C11, R1-S4).
- **Rolling upgrades.** The replica protocol gains requests; nodes are upgraded together.
