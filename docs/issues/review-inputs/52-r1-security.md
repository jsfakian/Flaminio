# #52 plan review, round 1, security focus

Scope: docs/issues/52/ (intent, spec, design, contracts, adr-proposal, decisions) at the commit below,
checked against `meta/service/auth.go`, `pkg/pki`, `pkg/captoken`, `flaminio-net`, and the clear plans
#13, #16, #73 (origin/main), #50 and #51 (their worktrees).

### R1-S1 — High
**Encrypted volumes: the plan backs up snapshots that #51 refuses to create, and keeps keys through a deletion that #50 performs.**
- #51 (clear) refuses RequestSnapshot for an encrypted volume (field 20, or the header mark), "until a later step pins its keys" (flaminio-51 `design.md:59-63`, `decisions.md:112-114,162-163`; `contracts.md`: "encrypted retention remains unavailable pending #50 policy").
- #50 (clear) admits "no encrypted snapshots, clones or backups ... until their catalog pins this descriptor". Its key record `volkeys/<id>/<first epoch>` is "deleted by the transaction that deletes its header" (flaminio-50 `contracts.md:61-68,167-169`; `design.md:333-336`).
- #52 still promises intent 7, S8, the design's "Encrypted input" paragraph (`design.md:112-116`) and an encrypted-volume lab check (intent 12). It promises "retained-key references ... survive ordinary volume deletion" (`design.md:99-105`; spec S8) but gives no etcd key, schema or bound for them, and no change to #50's DeleteVolume, which deletes `volkeys/` with the header. The release path is "future authorized repository GC", which does not exist, so the records only grow.
- As written, no encrypted SnapshotRef can exist. If one did, its key reference would have no defined record and could be deleted by #50's path.

**Fix:** AuthorizeSnapshotRead refuses an encrypted volume (`FailedPrecondition`), checked by field 20 and by #50's header mark as #51 checks them, with a test of each. S8's key-retention mechanism and the encrypted lab check move to the step that lifts #51's refusal; record that as the seam. Alternatively, specify the retained-key catalog record (key, schema, bound, writer, releaser) and the change to #50's DeleteVolume, and amend #51's refusal through that step.

### R1-S2 — High
**No admin step creates the grant, so the worker would need administrator authority.**
- `AuthorizeSnapshotRead` is "admin only" (`contracts.md:41-43`). The worker identity "has no mutation/general reader privileges" (`design.md:32-33`).
- The only commands are `flaminio-backup ... backup --volume V --snapshot S --operation-id UUID` with `--config FILE` (`contracts.md:161-163`). No grant id is passed in, and no RPC looks one up by operation.
- RegisterSnapshotSource needs the grant UUID and generation (`contracts.md:44-46`), and those come only from AuthorizeSnapshotRead's response. No `flaminio-meta` subcommand is planned (Files touched, `design.md:140-150`, omits `meta/cmd/flaminio-meta`).
- So the obvious implementation runs the backup command under an administrator certificate. Before #73 activates, that certificate also allows DeleteVolume, MoveReplica and DrainNode (`auth.go:64-65`). That breaks the ADR's core separation and S10's "offline repository access never becomes live-volume mutation authority".

**Fix:** add an administrator command (for example `flaminio-meta snapshot-read authorize|close`) that calls AuthorizeSnapshotRead and prints the grant. `backup` and `verify --compare-source` take that grant reference. The worker refuses to start with a non-`backup` SPIFFE identity. Tests: the worker identity's AuthorizeSnapshotRead is refused, and a worker config that holds an admin or `io` pair is refused at start.

### R1-S3 — Medium
**#73 (clear): no two-column role rows for the five new RPCs, and OverrideDelete bypasses the reader barrier.**
- #73 S1 says that once tenancy is active, every method other than the three open reads, Attach and the node/agent/target rules needs a role bound in the authorization record. It also says a non-administrator that no rule allows is refused before any store read (origin/main `docs/issues/73/spec.md:54-68`; `contracts.md:26-80`).
- #52 says only "admin only", "owning worker only", "owning worker or admin" and "node/io for itself" (`contracts.md:40-54`). It defines no "Before activation / Once active" column (which role for AuthorizeSnapshotRead and an admin CloseSnapshotRead: `cluster-admin`?), no new worker rule in #73's rule set, and no insecure-instance behaviour (#73 refuses every new method there).
- The barrier is wired only into DeleteVolume (`design.md:49-52`; `contracts.md:55-57`). #73's `OverrideDelete` (the `deletion-override` role) deletes a volume by another path. Unchanged, it reaches the deleting/NotFound state without closing grants or draining sources. That breaks the spec's invariant ("deletion's externally absent state cannot precede this barrier").

**Fix:** add #73-style rows for AuthorizeSnapshotRead, RegisterSnapshotSource, ValidateSnapshotRead, CloseSnapshotRead and AckSnapshotReadClosed, with the backup identity's rule beside the node rules. Name OverrideDelete (and any other path to the deleting mark) as running the same barrier. Tests in #73's S1 matrix: every new method by every role, unbound identity, inactive and active; OverrideDelete blocked by an unacked source.

### R1-S4 — Medium
**Audit: the #73 hand-off is not taken.**
- The ledger (`batch-2026-11-12.md:222`) asks #51–#53 to audit their operations through #73's event append, or to record that as a follow-up. #73 S7 assigns these to the issues that add them (`73/spec.md:131-133`).
- AuthorizeSnapshotRead gives a non-tenant identity read access to a whole volume, tenant data included. It writes no event, and no follow-up is recorded: the design only lists "audit" among things that "remain open" (`design.md:191`).
- The intent's out-of-scope line says "The audit log ... does not exist yet" (`intent.md:151`). #73 defines it.

**Fix:** either have AuthorizeSnapshotRead and CloseSnapshotRead (and the closing intent that DeleteVolume/OverrideDelete write) append #73 events in their own transactions, within the 120-operation limit, with tests; or record "audit events for snapshot-read grants" as to be filed, as #51 did. Correct `intent.md:151`.

### R1-S5 — Medium
**The revocation barrier's two ends are unspecified: how a source learns "closing", what an exclusion proof is, and what stops a new grant between the barrier and the deleting mark.**
- **How a source learns of closing.** Sources "notify/poll ... to close admission" (`design.md:42-44`). No RPC, heartbeat field or poll loop carries "closing" to a node; AckSnapshotReadClosed is node to metadata only. Without a trigger, every close and every DeleteVolume waits forever, even with all nodes up.
- **What an exclusion proof is.** Deletion may proceed on "a separately established exclusion proof accepted by the authority" (spec S3; `contracts.md:54`), but no such proof is defined. Undefined, it is either never available (a dead node's volume can never be deleted, and its quota and charge stay held) or an unspecified bypass. #13 S19's ForgetNode (a node retired for good, origin/main `13/spec.md:140-147`) is the existing candidate.
- **New grants after the barrier.** The closing intent is written before the barrier and the deleting mark after it, in separate transactions (`design.md:49-51`). AuthorizeSnapshotRead refuses only "deleting/unknown/uncommitted" volumes (`design.md:31`). Nothing says the closing intent refuses AuthorizeSnapshotRead and RegisterSnapshotSource, or that the mark's transaction compares the grant set. A grant issued in that window survives the deletion barrier.

**Fix:** name the channel. For example, the source re-runs ValidateSnapshotRead at each read batch and on a bounded poll of NodeHeartbeat's answer carrying closing grant ids. Define the exclusion proof: the node is forgotten under #13 S19, and ValidateSnapshotRead refuses a forgotten node. State that the closing intent refuses AuthorizeSnapshotRead and RegisterSnapshotSource, and that the mark compares the intent and the grant records. Add simulator mutants: a grant admitted under closing, and a close with no notification.

### R1-S6 — Medium
**The worker does not check that the 7412 server is the source node's data plane.**
- The contracts require mutual TLS 1.3 (`contracts.md:62`). They say what the replica checks about the worker, but not what the worker checks about the server.
- #16 S15 (clear) makes every metadata client accept only a `meta` server identity. The reader client has no equivalent check that the server's single URI SAN is `spiffe://<td>/flaminio/node/<SourceProof.node_id>/io`.
- Without it, any certificate from the cluster CA at that address (another node's `io`, an admin, another worker) can serve snapshot bytes. CRC is not authentication (`contracts.md:78`). Forged bytes would be sealed into an archive that then verifies as authentic.

**Fix:** the reader client accepts only the source node's `io` identity, and the source is the node that the SourceProof and the registration name. Tests: a server with another node's `io`, a `meta`, an admin and a backup certificate is refused.

### R1-S7 — Low
**The worker identity's lifecycle under #16 is unspecified.**
- The new `flaminio/backup/<worker_id>` identity (`contracts.md:35-37`) needs `pkg/pki`'s `ParseID` (`pki.go:222-240`) and the Rust `Identity` enum (`identity.rs:12-22`) to change. The plan names those files.
- It does not say who issues the pair (`pki issue`, the CA tool), which VM holds it, or whether #16 S10's lab renewal renews it.
- It does not say whether a long backup picks up renewed pairs (#16 S1) or is a run-and-exit command that fails at its next handshake past a short life.
- It does not say that the reader client and listener disable TLS session resumption (#16 S6, both sides, in Rust).

**Fix:** state issuance (lab: `pki issue` for fl-w), renewal under S10, the worker's pick-up behaviour, and no resumption on 7412, each with a test.

### R1-S8 — Low
**The claim that the worker has "no general reader privileges" is contradicted by the code.**
- The plan says the worker has no general reader privileges (`design.md:33`) and is "neither metadata Admin/Reader" (`contracts.md:36`).
- `auth.go:79-80` allows `GetVolume`, `ListNodes` and `ListVolumes` to any authenticated identity, and #73 keeps them "any identity" in both columns (`73/contracts.md:34`). The backup identity will read every volume's record, including #50's sealed envelope (flaminio-50 `contracts.md:90-120`). The worker also needs these reads to revalidate ISR around batches (`design.md:55-57`).

**Fix:** say that the worker uses the open reads, and what they expose to it, or justify excluding it (which would amend #73's row).

### R1-S9 — Low
**The worker URI is taken from a frame field.**
- `ReaderHello` "carries worker URI" (`contracts.md:63-64`). The FLBR token binds a worker URI, but the plan does not state that the node compares the token's URI with the verified peer certificate rather than with the frame's field.
- The target path (`session.rs:114-147`) derives the peer from the certificate.

**Fix:** the node takes the worker identity from the peer certificate and refuses a Hello whose URI or token differs. Test a valid token presented by another backup identity.

### R1-S10 — Low
**Reader resources have no node-wide or grant-wide bound.**
- The limits are per source: ≤2 reads, 32 MiB/s (`contracts.md:156-159`). Nothing caps concurrent 7412 connections or sessions per node, active grants per node or volume, or source registrations per grant (re-registration after invalidation, idempotence per (chunk, node)).
- Each new connection costs an online ValidateSnapshotRead call to the metadata service (`contracts.md:47-49`).
- Operation, grant and source records, and retained-key records (R1-S1), are never collected.
- A worker identity, or several admin grants, can therefore multiply engine reads and metadata calls, and grow etcd and the closure fan-in.

**Fix:** cap reader sessions per node and per grant. Make registration idempotent per (grant, chunk, node), bounded by chunks × RF. Check the token and certificate before the metadata call. Bound or collect completed operation records. Add tests at each cap.

### R1-S11 — Low
**Lab commands and secrets delivery are unspecified.**
- Intent 12 and S11 add a lab S3 store and a lab check (`design.md:181-187`), and Files touched names "test/lab/image scripts". No `lab.sh` subcommand or script is named.
- Nothing says how the S3 credential file, the repository key, the worker's key pair and the store's TLS server certificate are made and reach fl-w. Writing them through an `ssh` command line would put secrets in a remote argv.
- Nothing says how volume, snapshot, operation and bucket ids reaching remote commands are validated and quoted (AGENTS.md Principles).

**Fix:** name the subcommand and scripts. Generate secrets in the VM, or pipe them over stdin into 0600 files created with no-follow. Validate ids against the `pkg/pki` segment set before any ssh. Add script tests in `make lint`.

### R1-S12 — Low
**Creating and checking the repository key is unspecified.**
- K is "32 random bytes, independently escrowed" (`contracts.md:99`), but nothing says who generates it (`repo init`?) or how its file is created (exclusive, 0600, no-follow, fsync).
- "Opened no-follow, checked" (`contracts.md:91-92`) does not say what is checked: owner, mode with no group or other bits, regular file, exact length 32.

**Fix:** specify generation and creation in `repo init`, and the checks applied to every secret file. Test each refusal.

## Nits
- `contracts.md:115`: the AEAD's "exact object-key bytes" does not say whether the bucket prefix is included. If it is, moving or copying a repository to another prefix (#54) breaks every object. Say "the key relative to the repository prefix".

Reviewed commit: c6efcd778d462d865277c99094a692c1bc7ee76c
