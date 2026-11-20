# #54 plan review, round 2 (incremental), design-security focus

Scope: `git diff 52df1de..05fba15 -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. I read other code and plans only where the changed lines rely on them: `pkg/pki/pki.go`, `meta/service/{auth,service}.go`, `control/csi/{tombstones,dataplane}.go`, `flaminio-io/src/control.rs`, `pkg/captoken`, and the #52 and #73 plans. Nothing was run.

## Round-1 findings

- **R1-S1:** fixed. The reader and helper split is in place (design.md:29-40, contracts.md:198-205). The residuals are R2-S3 and R2-S5.
- **R1-S2:** fixed except token issuance. The role rows, the `system` owner, the tenant-name refusal and #73's deletion events are all present. The writer token is still a full ordinary-attach capability: see R2-S1.
- **R1-S3:** fixed. It uses #52's `cluster` record with a CSPRNG value and a compare, accepts no caller UUID, and refuses an archive from the same cluster (contracts.md:9-20).
- **R1-S4:** fixed. `store_ca`, `store_identity` and a store client pair outside `flaminio/` are specified. I checked that `ParseID` refuses that URI (pki.go:233-239).
- **R1-S5:** partly fixed. The metadata fence (mark, `aborted`, generation + 1) is fixed. The node-side end of the writer is now broken: see R2-S2.
- **R1-S6:** fixed. The disk by serial and its refusals, the named targets, the id grammar with `printf %q` and the script test are all there. One residual: R2-S4.
- **R1-S7:** fixed (contracts.md:240-246).
- **R1-S8:** fixed. The field is now 11; `Volume` today uses tags 1-8 (api/proto meta.proto:117-127).
- **R1-S9:** fixed. `Volume` carries only the phase and operation; the rest is served through GetRestore.
- **R1-S10:** fixed for proofs (none are kept). The growth of terminal jobs is named but not accepted, which is the user's call; I am not raising it again.
- **Security Nit (phases):** fixed.

## New findings

### R2-S1 (Medium): the restore's target token is an ordinary attach capability, and "ordinary attach refuses even with a token" has no mechanism
- **Claim:**
  - AcquireRestoreWriter returns "normal replica-target token plus a distinct signed FLRT" (design.md:124-128, contracts.md:97-98). That token is a plain FLCT bound to the writer node and epoch (pkg/captoken: volume, epoch, node, first epoch; no purpose field).
  - The control socket's ordinary `attach VOLUME TOKEN-BASE64` takes a token from the caller (control.rs:5,84). The node consults no metadata on that path.
  - "Normal engine/control Attach refuses a Restoring volume even with a target token" (design.md:127-128) and "the engine needs BOTH plus a matching non-Ready job" (contracts.md:150-151) name no way for the engine to know the phase.
  - Two windows follow:
    - **Finalizing, after restore-detach:** an ordinary attach with the restore FLCT opens a writable device after the proofs and before Ready.
    - **Ready, before the first user attach:** CompleteRestore clears the target, but the replicas' fence stays at the writer epoch. The same token still attaches.
  - Either window defeats design.md:185-187 ("prevents a late restore write between Ready and the first user attach"). It also gives `system-provisioner` an attach that #73 denies once tenancy is active. #73 contracts.md:59-62 says "Attach is decided by the certificate": only the CSI controller's ID and the agent's own restart.
- **Fix:**
  - Make the restore's target token unusable for ordinary attach. One way is a purpose byte (FLCT version 3, or carrying the target authority inside FLRT) that ordinary Attach refuses and AttachRestore requires; replicas accept it as any target token.
  - Alternatively, have ordinary control attach check GetVolume's phase (open read, off the I/O path) and refuse any non-Ready restore.
  - State the exception to #73's "Attach by certificate" in the cross-plan list.
  - Tests: `ordinary_attach_refuses_restore_token`, in Finalizing and after Ready. Add a simulator path for it, so that `restore_no_write_after_ready` can catch it.

### R2-S2 (Medium): after an abort, or a writer moved to another node, the old node's restore attachment cannot be ended, so deletion stalls
- **Claim:**
  - Abort "needs no writer closure" (design.md:191-197, contracts.md:108-111) and raises the writer generation, so "no later ... closure ... commits" (design.md:193-194).
  - The writer node's replicas are removed by the tombstone sweep, which sends plain `delete VOLUME` over the control socket (control/csi/tombstones.go:40-47, dataplane.go:125-126). For a restore attachment, that line answers an error and changes nothing; "only restore-detach ends it" (contracts.md:174-175).
  - restore-detach answers `ok` only after "io's own ReportRestoreWriterClosed has committed" (contracts.md:167-168). The abort forbids that commit.
  - Abort is used mainly when the helper has died with its attachment still live. In that case nothing in AbortRestore, `restore-volume cancel` or the sweep ends the attachment. The node never confirms, the tombstone and the name stay held, and the device stays allocated. Forced confirmation is only for a forgotten node, not a live one.
  - The same holds for a resume on another node (design.md:131-132 covers only the same node). Node A keeps a stale-generation attachment, and with "only one restore/node" it can host no other restore.
  - The test `plain_detach_delete_refuse_restore` (design.md:254) locks this behaviour in.
- **Fix:**
  - Let the sweep's `delete VOLUME` end a restore attachment of a tombstoned volume: drain, end the device and target, delete the replicas.
  - Define restore-detach to end the attachment, and answer a distinct stale/aborted code, when the closure report is refused for a stale generation or an aborted job. `cancel` should run restore-close/detach on the writer node when it is local.
  - Tests: `abort_with_live_writer_attachment_completes_deletion` and `moved_writer_old_attachment_ended`.

### R2-S3 (Medium): the "unprivileged" reader shares a uid and a directory with #52's backup worker, which holds a Flaminio `backup` pair
- **Claim:**
  - The reader runs as #52's `flaminio-backup` user, and its config, key and credential go in `/var/lib/flaminio-backup` (design.md:32, 299-301; contracts.md:201).
  - That is #52's worker's user and directory. The worker's `backup` pair (AuthorizeSnapshotRead over the cluster's volumes) is in `/etc/flaminio/backup-pki`, owned by `flaminio-backup` (#52 contracts.md:79-82; design.md:338-339).
  - On a destination node that also runs the destination's own backups, a decoder bug reached through a crafted archive (the named risk) therefore gains the destination's `backup` identity, its repository key and its store write credential. That contradicts S9's "holds no destination authority" and `refuses_flaminio_pair`, which checks only the config.
  - In the lab, adding the user and its directories to fl-c also contradicts #52's "no other VM gains either" (#52 design.md:367). This is not in the cross-plan list.
- **Fix:**
  - Give the reader a system user of its own (e.g. `flaminio-restore-read`) and a directory of its own (`/var/lib/flaminio-restore`), with no access to `backup-pki` or the worker's directory.
  - Name the change to #52's lab (or the new user) in the cross-plan list.
  - Test: `runs_reader_unprivileged` also asserts that the `backup` key and the worker's directory are unreadable.

### R2-S4 (Low): the lab installs secrets as root into a directory an unprivileged user owns
- **Claim:** design.md:305-307 runs `sudo install -o flaminio-backup ... /dev/stdin DST` into `/var/lib/flaminio-backup`, which `flaminio-backup` owns, after a separate "refusing an existing DST" check.
  - GNU `install` opens an existing destination and follows a symlink. A `flaminio-backup` process could plant a link between the check and the install, for example a reader compromised in an earlier run.
  - Root would then overwrite the link's target and chown it to `flaminio-backup`.
- **Fix:** create the file as the target user, exclusive and no-follow (`sudo -u flaminio-backup` with `O_EXCL|O_NOFOLLOW`, umask 077), as #52's store script does. Add a `scripts/tools` test for a planted symlink.

### R2-S5 (Low): the reader process has no resource or lifetime bounds on a storage node
- **Claim:** the spawn rules give no groups, an empty environment and only the pipes (contracts.md:201-203). They set no memory, CPU or file limits, no `NO_NEW_PRIVS`, and no parent-death signal.
  - A decoder bug or a compromised reader can exhaust memory on the node that runs `flaminio-io`. The OOM killer may then take the data plane, and with it every volume on that node.
  - If the helper dies, the reader keeps running with the credential.
- **Fix:** start the reader under a memory-capped cgroup or rlimits, with `PR_SET_NO_NEW_PRIVS`, `PR_SET_PDEATHSIG` and its own session. Test that it exits when the helper is killed and that it is bounded under an allocation bomb.

### R2-S6 (Low): the io authorization rule refers to a request field the RPCs do not have
- **Claim:** "the request's `node` must equal it" (contracts.md:122-124). But ReportRestoreChunkProof and ReportRestoreWriterClosed carry no `node` field (contracts.md:102-105). Today's node rules read the node from the request (auth.go:59-61).
- **Fix:** either add `node` to both requests, or state that the rule compares the certificate's node with the job's `writer_node`, in the method's transaction. Add an `auth_test` row for another node's `io`.

### R2-S7 (Low): reader-controlled error text has no validated path into the job
- **Claim:** the job bounds `error_code` (≤ 64 bytes of `[a-z0-9_]`) and `error_object` (≤ 256 printable bytes) (contracts.md:69-70). The reader's Error frame is up to 4 KiB of JSON it controls (contracts.md:234). No RPC writes either job field (ReportRestoreProgress, contracts.md:99, has none).
- **Fix:** name the RPC that carries them. Require the helper to validate the code against the grammar, and to truncate or escape the object text, before any metadata call. Extend `frames_rejected` to cover this.

## Nits

- The named risk "a compromised reader can choose the restored bytes" (design.md:331-332) should also cover #53's outputs (`immutable_guarantee`, `qualified`) and the provenance it reports. All of these come from the reader's Header.
- The helper should require the Header's `archive_ref` to equal `--archive REF` (contracts.md:228-230).
- The destination's PKI is described as "CA, token key" (design.md:291-292), but `etcd.sh` also needs an etcd CA (scripts/guest/etcd.sh:49). Name it so the destination's etcd keeps client-certificate authentication.
- `--archive REF`'s text form is never defined, but restore-check validates archive ids with `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$` (design.md:310-311). Define the encoding so the two agree.
