# #54 plan review, round 1, security focus (design-security)

### R1-S1 — High
**restore-volume puts the untrusted-archive parser in a root process that holds the destination's administrator pair. This contradicts #52's settled worker rules.**
The design extends `flaminio-backup` with `restore-volume` on the destination node (design.md:24). Its config carries "destination metadata/admin TLS paths, selected local node/control socket" (contracts.md:281-284), and it runs as a "local root worker" with the private ublk device (design.md:64). The control socket is root-only, mode 0600 (`flaminio-io/src/control.rs:10,51`). #52 (clear) settles that the worker "never holds an administrator pair and refuses to start with anything but a `backup` identity (R1-S2)" (#52 design.md:31-33, contracts.md:77). It also settles that the worker runs as `flaminio-backup`, "never root" (#52 contracts.md:79-84, design.md:377). The separation exists because the worker parses bucket input: S3 listings and envelope headers before any authentication, and roots, pages and decompression after it.
Under #53's ransomware runbook, which is this restore (intent Why 7), the source cluster's repository key may be the attacker's. "Authenticated" archives can then be crafted. A decoder bug becomes root, plus administrator rights, on the clean cluster.
**Fix:**
- Split the command. An unprivileged reader (`flaminio-backup`, no admin pair, no socket access) authenticates and decodes the archive. It streams verified `{offset, len, bytes}` records, with fixed bounded framing, to a minimal root helper. Only the helper holds the admin pair, calls metadata and drives `restore-start/close/verify/detach`.
- Alternatively, state an explicit, reasoned exception to #52's R1-S2/R2-S4.
- Tests: the reader runs without root and cannot read the admin key; a fuzz target for the helper's framing.

### R1-S2 — High
**#73 (clear) is not taken: new methods, the owner, quota, audit and token issuance are all unspecified.**
None of the plan files mentions #73. The intent says the audit log "does not exist yet" (intent.md:144); #73 defines it. The conflicts:
- **Method table:** #73 S1 needs a row for every method once tenancy is active. `InitializeClusterOrigin`, `BeginRestore`, `AcquireRestoreWriter`, `BeginRestoreFinalize`, `CompleteRestore`, `AbortRestore`, `GetRestore` and the three `Report*` methods have none (contracts.md:216-233).
- **Roles:** the "#69 Reader" (contracts.md:42, design.md:127) is no #73 role, and #69 is not clear.
- **Owner and quota:** `BeginRestore` creates a volume with no owner record and no quota charge in its claim (#73 S3, S4).
- **Default name:** the name defaults to the source's (spec S1). For a CSI volume that is `pvc-<UID>`, a tenant-form id, which #73 S3 says a system creation "cannot take". Restoring it ownerless breaks #73's invariant that "a tenant volume exists only with an owner the CSI controller verified".
- **Abort:** `AbortRestore` deletes without #73's `delete.admitted`/`delete.committed` events or S5's protected/unassigned check.
- **Token issuance:** `AcquireRestoreWriter` hands an FLCT target token to an administrator (contracts.md:221-222). Once tenancy is active, #73 lets only the CSI controller's ID (or an agent restarting itself) obtain a target token through Attach (#73 contracts, "Attach is decided by the certificate").

**Fix:**
- Add a #73 row (before and once active) for each new method.
- `BeginRestore` writes a `system` owner record and quota charge in its claim, under `system-provisioner`, and refuses a tenant-form name (require `--name`) until a tenant owner path exists.
- `AbortRestore` follows #73's deletion events and protection.
- Name the role that may acquire a writer token, bound by `security-admin`.
- Drop "Reader" for #73's roles. Add these cases to #73's matrix tests.

### R1-S3 — Medium
**The cluster identity contradicts #52, and the hand-off "#54 can read the cluster UUID record" is not taken.**
#52 (clear) defines `cluster = {cluster_uuid}`: 16 random bytes, written once, conditionally, by the first AuthorizeSnapshotRead, never deleted; "#54 reads it" (#52 contracts.md:98,112; decisions.md:138). #54 instead adds `InitializeClusterOrigin(uuid)`: an administrator-supplied value, "only explicit admin enrollment", "Do not generate a new UUID" (contracts.md:198-201,217; design.md:31-35). It also adds a per-Volume `home_cluster_uuid` (contracts.md:206).
- A caller-chosen UUID can equal the source's, which is exactly what a disaster-recovery operator "keeping the identity" would type. Two clusters' first epochs can be equal (intent Why 4), so that voids S2's only distinction for incremental bases and FLRT cluster binding.
- "Missing previously enrolled identity refuses" has no mechanism on a fresh etcd.

**Fix:**
- Read #52's `cluster` record, created if absent by the same conditional put with a service-generated CSPRNG value. Accept no caller-supplied UUID.
- `BeginRestore` refuses an archive whose source `cluster_uuid` equals the home one (same-cluster restore is #52's step 5).
- Test both.

### R1-S4 — Medium
**Object-store access from the destination is unspecified across trust domains.**
- **Client certificate:** #52's store profile requires mTLS plus SigV4, with a pinned `store_identity` (#52 contracts.md:356-375). In the lab, the store's pair and its client CA are the first cluster's (`spiffe://$TRUST_DOMAIN/.../fl-w/backup-store`, #52 design.md:347-351). The client certificate the store accepts is therefore a source-cluster credential, yet S9 says "no source metadata/certificate".
- **CA bundles:** no rule keeps the store's CA out of the destination's bundles. `pki.Files.load` builds one pool (pkg/pki/pki.go:267-283) and `PeerIdentity` checks only the trust domain (pki.go:244-259), so any CA added there can mint destination administrator IDs (#58 intent Why 2). fl-c already holds the first cluster's `ca.crt`.
- **Store identity:** the restore config names no `store_identity` (contracts.md:283-284).
- **Credentials:** nothing says who creates S9's read/list-version-only credential at the store. Nothing says how it and the repository key reach fl-c, given #52's lab rule that "no secret ... leaves the VM" (#52 design.md:377-378).

**Fix:**
- A store client identity issued by the store owner's CA, outside both clusters.
- A separate `store_ca` file, used only by the S3 client, and a required `store_identity`.
- A lab step that creates the read-only credential and installs the key and the credential on fl-c owner-only (0600), over stdin, never in argv.
- Tests: destination meta and node refuse a certificate from the store's CA; the restore refuses a store certificate lacking the pinned identity.

### R1-S5 — Medium
**A restore whose writer node is lost cannot be deleted: the volume, its name and its placement stay held for good.**
Ordinary DeleteVolume refuses nonterminal jobs and names AbortRestore. AbortRestore must "close/drain or independently fence the restore writer", and "unreachable authority can block safe deletion" (design.md:119-123, contracts.md:227-228). "Independently fence" has no mechanism, so a node lost mid-restore blocks AbortRestore forever. That contradicts intent outcome 4 ("which can be deleted"). Today's deletion already handles dead nodes through epochs, tombstones and forced confirmation.
**Fix:**
- AbortRestore CASes Aborting and raises the header epoch and writer generation, a metadata fence above any writer token.
- It then runs ordinary tombstoned deletion; a dead node goes through the existing forgotten/forced-confirmation path.
- Test with the writer node killed.

### R1-S6 — Medium
**The lab destination helper has no mechanism behind "never overwrite first-cluster disks", and its remote commands are unspecified.**
This change may build the helper (intent outcome 8), but nothing defines it:
- **Disk:** lab.conf gives fl-c one data disk (`VM_C_DATA_GIB`, lab.conf:67), the first cluster's store. No second disk is defined, and no rule says how the "separately inventoried" disk is chosen and checked (design.md:172-177).
- **Commands:** no script, make target or ssh command is named.
- **Ids:** no grammar or quoting is given for the volume, operation and archive ids that reach ssh (AGENTS.md: "Ids reaching shell commands ... are validated and quoted").

**Fix:**
- A `lab.conf` variable naming the destination disk by serial.
- A refusal of any device holding a Flaminio superblock, or mounted, or the first cluster's disk.
- Named scripts and Makefile targets, an id grammar with `printf %q`, and `scripts/tools` tests.

### R1-S7 — Low
**restore-volume drops #53's retention check and outputs.**
#53 (clear) S8: `restore` checks the whole graph's retention and reports `immutable_guarantee`, `qualified`, `graph_checked` and `backend_profile` (#53 spec.md:47-55,124-128; design.md:155-161). #54's JSON lists none of them (contracts.md:289-292). Yet this is #53's ransomware restore, where the guarantee matters most.
**Fix:** run #53's restore check and carry its fields; test under a candidate profile.

### R1-S8 — Low
**The `Volume` tags collide with #75.**
contracts.md:206 still proposes tags 40 and 41. 40 is claimed by #75 (clear), and the ledger hand-off says "#54 moves off 40–41 when it is reviewed" (batch-2026-11-12.md:373).
**Fix:** take the lowest unclaimed tags.

### R1-S9 — Low
**Restore details bypass `GetRestore`'s role through the open reads.**
`RestoreInfoV1` sits in `Volume` (contracts.md:206-210), which GetVolume and ListVolumes return to any identity (auth.go:79; #73's "any identity" row). It carries source cluster, volume and snapshot names and archive refs, which #52 keeps encrypted in the bucket. Meanwhile `GetRestore` is role-gated (contracts.md:229).
**Fix:** keep only the phase and operation id in `Volume`; serve the rest through `GetRestore`.

### R1-S10 — Low
**Operation and proof records have no bound or collection.**
Terminal job identity is kept forever (design.md:48), and per-chunk proof batches have no stated lifetime (contracts.md:235-240). #52 caps comparable records (64 grant records per volume).
**Fix:** state a cap and a collection rule for terminal jobs and proofs (proofs deleted at Ready or Abort), with a test.

## Nits
- spec.md:17 lists the phases as Incomplete/Finalizing/Ready, but contracts.md:211 adds Writing, Aborting and Deleted.

Reviewed commit: 52df1de4f36aa943353d9cfacfd271b1be8cc534
