# Issue #53: Immutable backups — intent

[Issue #53](https://github.com/jsfakian/Flaminio/issues/53), README roadmap phase 3,
ARCHITECTURE §14.1 (ransomware resistance), §14.3 and §15.1. This change is the issue's step 3,
step 1 with the period taken from the backup command instead of a `BackupPolicy`, and the
uploader of step 2: a backup locked for a period, by an identity that cannot destroy it, and
read back whatever was written since. Collection after retention (the rest of step 2), the
policy's retention and the runbook (step 4) build on it: issue #53 stays open after this
change. It assumes the first step of [#52](https://github.com/jsfakian/Flaminio/issues/52)
(incremental backups) as its intent plans it.

## Why

A backup that the cluster's own credentials can delete does not survive a compromised cluster.
§14.1 names the answer in two sentences. The code has none of it, and no backup at all (#52's
intent, Why 1).

1. **#52's worker does not delete, but nothing stops its credential from deleting.** Its first
   step only adds objects, and "needs no right to delete or overwrite". That is a need, not a
   limit: no lock is asked for, and the rights of the store's credential are not looked at.
2. **The lock is the store's: on one version of an object, until a date.** What follows is S3
   Object Lock as AWS's guide describes it. Other stores are compatible in part (Why 8).
   - It needs a versioned bucket. Writing over a locked name adds a version, and succeeds.
     Deleting without naming a version adds a delete marker, and succeeds for an identity that
     may delete. Only deleting the locked version by its id is refused. The issue's "overwriting
     a backup is refused" is not what a store does: the locked version stays, under a newer one.
   - In compliance mode nobody removes the version, shortens its date or changes its mode,
     the account's owner included. Whoever may set a retention may extend it. In governance
     mode an identity with the bypass right undoes all of it.
   - A lock asked for with an upload overrides the bucket's default one, shorter or longer.
   - The date is compared with the store's clock. The cluster's clock decides nothing.
3. **Deduplication works against the lock.** A later backup reuses blocks an earlier one
   uploaded, locked until the earlier one's date. They must stay until the later one's.
   - Either the lock is extended, or the block is uploaded again (a new version, 4 MiB each).
   - A request that names no version acts on the newest. After someone wrote over a block,
     that extends the wrong version.
   - Extending is a request per block. The store's listing does not give lock dates. A 1 TiB
     volume is 262 144 blocks: asked one by one, a backup that changed nothing costs that many
     requests, against #52's outcome 6.
4. **"Write-only" cannot be literal.** #52's worker lists objects (deduplication, resuming),
   reads the base backup's manifest (every manifest is complete), and reads everything back to
   verify. §14.1's own gloss is "(no delete)".
5. **An identity that only adds can still do harm.**
   - It adds objects and versions under any name, locked: nobody can remove them before their
     date. It extends any lock, up to 100 years on AWS unless the bucket's policy caps it.
   - The worker also holds the repository key (#52). With both, an attacker writes well-formed
     manifests: backups that never were, and versions over genuine manifests.
   - So the newest version of a name proves nothing. A block is named by the keyed hash of its
     content: a version matches its name or does not. How a manifest is named is #52's ADR's.
6. **A compromised cluster can also wait.** Backups made after the compromise are the
   attacker's. What can be kept is every backup made before it, and the means to tell them
   apart: the time the store took an object in, which the cluster cannot set.
7. **The store's identities are not the cluster's.** Flaminio's are X.509 certificates
   (`pkg/pki/pki.go:6`); `meta/service/auth.go` authorizes metadata RPCs only. A store's keys
   and policies are its owner's. The cluster can neither grant nor limit them, and learns what a
   credential may do only by trying.
8. **Stores differ, and a lock nobody checked is a claim.** A store may accept a lock request
   and not enforce it. From the candidates' documentation only, none run:
   - versitygw: Object Lock and versioning, each behind a gateway flag on its POSIX backend;
     bucket policies with separate actions for delete, delete of a version and retention. The
     pages read do not name its modes.
   - SeaweedFS: compliance mode, on a bucket created with the lock; policies with `Deny`.
   - `rclone serve s3`: no versioning, no lock, no policies. It cannot check this issue.
9. **There is no policy to take a period from, and nothing deletes.** #52's first step leaves
   out `BackupPolicy`, pruning and collection (they need #51's snapshot deletion). Collection
   under a second identity can be neither built nor tested yet.
10. **A mistake is as lasting as an attack.** A period mistyped in compliance mode cannot be
    taken back. In the lab a locked object cannot be removed through the store, and fl-w's
    data disk is 8 GiB (`scripts/lab.conf`).

## What we want

1. An ADR, accepted before the design (its own, or an amendment of #52's): the identities of a
   backup target and what each may do; how a backup's objects are named and locked so that a
   reader finds the ones the backup wrote; where a period comes from, and its bounds.
2. A repository is immutable or not, fixed when it is set up. Setup refuses a target that does
   not lock: no versioning, no Object Lock, no compliance mode, or a lock that the store
   reports and does not enforce, found by trying. Every backup checks the target again before
   it uploads.
3. Each backup into an immutable repository is locked in compliance mode for a period. Until a
   `BackupPolicy` exists the period is the backup command's, within a least and a most fixed at
   setup. It is counted by the store's clock, from the end of the backup.
4. A backup is listed only once the store itself reports its manifest and every block it names
   locked until at least its date. That holds for blocks an earlier backup uploaded: the
   version covered is one that holds the block, and it is neither read nor uploaded again.
5. The uploader: one identity of the store, the only one the cluster holds. It adds objects,
   lists, reads and sets locks. It cannot delete an object or a version, bypass or shorten a
   lock, or change the bucket's settings. The product ships the rights to give it, and setup
   refuses a credential found able to delete.
6. Nothing done with the uploader's credential inside a backup's period removes the backup or
   changes what is read from it. Deleting, deleting a version, writing over a manifest or a
   block, shortening a lock, changing its mode: each is refused by the store, or leaves the
   locked versions in place, and listing, verifying and restoring still use them.
7. A backup is listed with the time the store took it in and the date the store holds it to,
   not with what its manifest says of itself. A manifest added later is a later backup: it
   never stands in for an earlier one.
8. An interrupted backup is not listed. The blocks that arrived stay locked until their date
   and take space until then. Running it again reuses and covers them (#52's outcome 9).
9. A repository that is not immutable behaves as #52 left it.
10. Tests and the lab. `make test` runs the worker against a test store with versions, locks
    and identities' rights: it proves the worker, not a store. The lab's store proves the
    enforcement, with periods of minutes: a locked backup; every request of outcome 6, with
    the uploader's credential; a restore equal to the snapshot afterwards; a second backup
    reusing blocks, which stay locked past the first one's date; a bucket without the lock,
    refused; a version deleted by another identity once its date has passed. Counted: the lock
    requests of a backup, per block kept. The check leaves a store that can be emptied.

## Out of scope

- **Collection after retention, under the second identity** (the rest of step 2, and the
  issue's "Done when" for it). It needs #52's pruning, which needs #51's snapshot deletion.
  This step gives that identity no work, and the cluster never holds it. Until then an
  immutable repository only grows.
- **Retention from a `BackupPolicy`** (the rest of step 1): #52's step 4. It will supply
  outcome 3's period.
- **The ransomware runbook** (step 4): it needs a restore into a clean cluster, #54. This step
  documents setting a target up and the rights of its identities.
- **Backups made after the cluster is compromised.** Nothing here protects them (Why 6).
  Noticing a compromise, and alerts on refused requests, are not here either.
- **A store that breaks its own lock,** its operator, or the account's deletion. Client-side
  encryption and keyed addresses (#52) show what was altered or removed; nothing brings it back.
- **Creating the bucket, its identities and their policies:** the owner's. So are quotas, caps
  on periods in the bucket's policy, and lifecycle rules: the guide names them, nothing checks
  them.
- **A separate identity for reading.** Verify and restore use the uploader's rights, or a
  credential the administrator gives the command.
- **Governance mode, legal holds, variable retention.**
- **Locking a repository that already holds unlocked backups.** An immutable repository is new.
- **Where the uploader's credential is kept in Kubernetes, and its rotation:** `BackupTarget`,
  #52's step 4.
- **Metadata snapshots on the backup target** (§11.5), and their lock: #54.
- **The audit log** of what was locked, and of refusals (§15.3): #73's events cover metadata
  mutations only, and this step makes none; store-side locks are not audited. A follow-up, to be
  filed (round 1, R1-C10, R1-S9).
