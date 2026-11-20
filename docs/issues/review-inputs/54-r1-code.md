# #54 plan review, round 1, focus design-code

Scope: docs/issues/54/ (intent, spec, design, contracts, adr-proposal, decisions) at HEAD, checked
against the code in /home/jsfakian/Documents/src/flaminio-54 and against the clear plans #15, #17,
#52, #53, #73, #75, #76, #79 and #80. Hand-offs in batch-2026-11-12.md for #54: move off `Volume`
40–41 (not taken, R1-C4); read #52's cluster UUID record (not taken, R1-C3).

### R1-C1 — High
**restore-volume's identity, user and RPC callers are unspecified, and they contradict #52.**
#52 (clear) fixes the `flaminio-backup` worker: it runs as the system user `flaminio-backup`,
"never as root", and "refuses to start, exit 1, when its configured pair is not a `backup`
identity (an administrator, `io` or other pair)" (#52 contracts, Identity lifecycle and Where it
runs; test `refuses_non_backup_pair`). #54 "extend[s] Rust flaminio-backup with restore-volume"
(design, Source validation), but the command calls only administrator RPCs (`BeginRestore`,
`AcquireRestoreWriter`, `BeginRestoreFinalize`, `CompleteRestore`, `AbortRestore`; design "Admin
alone initializes origin/starts/acquires/finalizes/completes/cancels"). It also drives the
root-only control socket (mode 0600, `flaminio-io/src/control.rs:9-10`) and opens `/dev/ublkbN`
("local root worker", design, Admission). Under #52's rules the command cannot start with an
admin pair, and as `flaminio-backup` it cannot open the socket. The design is also inconsistent
about who reports what. `ReportRestoreProgress`, `ReportRestoreChunkProof` and
`ReportRestoreWriterClosed` are "current own destination node/io only" (contracts:40-41), but
progress is known only to the worker, and `restore-verify` returns its proof to the socket
caller, which is the worker, not io (contracts:68-69). Nothing says how a proof gets from io to
the metadata service.
**Fix:** state which identity and user restore-volume runs as on the destination node (for
example root with the destination's administrator pair, in a mode #52's backup-pair check does
not cover). Record the change to #52's refusal rule as a cross-plan change, made by whichever
lands second. State that io itself calls `ReportRestoreChunkProof` after `restore-verify` and
`ReportRestoreWriterClosed` after `restore-detach`. Make progress an administrator RPC, or drop
it from the io-only set. Add `auth_test` rows for each.

### R1-C2 — High
**Reaching the store from another trust domain is unspecified, and S9's "no source
certificate" contradicts #52's store profile.** #52's store profile needs TLS 1.3 mutual
authentication plus SigV4 (#52 contracts:372-374, design:194-195). The lab store's server pair is
issued by the first cluster's CA as `spiffe://$TRUST_DOMAIN/flaminio/node/$VM_W/backup-store`,
and the worker checks that identity against its own `ca.crt` (#52 design:347-351, test
`store_identity_required`). The destination has its own trust domain and CAs (intent outcome 8,
spec S11). So restore-volume on fl-c needs three things the plan never names: the source
cluster's CA to verify the store, a client credential the store accepts (today one under the
source CA), and a store-identity check that expects a foreign trust domain. Spec S9 says the
restore needs "no source metadata/certificate", and the contracts' `--destination-config` lists
only "repository read credentials and key file paths" (contracts:91-95). As written, the issue's
"Done when" path (a second cluster restoring from the bucket alone) cannot connect.
**Fix:** add to the config the store's CA bundle, the store's expected identity and trust domain,
and a client credential issued independently of either cluster's node PKI, or name an approved
exception. Say how the lab issues them on fl-c. Add a test that a store identity from the other
trust domain is accepted only as configured.

### R1-C3 — Medium
**The cluster-UUID hand-off is not taken, and an administrator-chosen UUID breaks S2.** #52
(clear) defines the record `/flaminio/v1/cluster` = `{cluster_uuid}`, "16 random bytes, hex",
written by "the first AuthorizeSnapshotRead, if absent", never deleted, and says "#54 reads it"
(#52 contracts:98, :112). #54 instead adds `InitializeClusterOrigin(uuid)`, where the
administrator supplies the value, and it refuses a missing record (contracts:8-10, :27). The
plan never names #52's key, and gives no rule for a record #52 has already created. An
administrator-supplied UUID equal to the source's makes the restored volume's later
`SnapshotRefV1` share the source's `cluster_uuid`. #52 picks bases on "cluster/FirstEpoch/
geometry/encryption" (#52 design:124), so two clusters' equal first epochs (intent Why 4) can then
produce a base match. That is exactly what S2 forbids, and nothing refuses it.
**Fix:** use #52's record. BeginRestore reads it, or creates it if absent with random bytes, in
its own transaction with a compare. Drop the UUID argument, or keep only an idempotent
initializer with no UUID input. Refuse BeginRestore when the archive's `source.cluster_uuid`
equals the home UUID. Add tests: the record exists from a backup, the record is absent, and
source equals home.

### R1-C4 — Medium
**`Volume` tags 40–41 collide with #75.** contracts:16 proposes "Volume restoration field
proposed tag40, home_cluster_uuid tag41". #75 (clear) holds `Volume` 40 (addendum, Contracts
already claimed), and the batch notes record that "#54 moves off 40–41 when it is reviewed"
(batch-2026-11-12.md:373). decisions.md says nothing about it.
**Fix:** move both fields to the lowest `Volume` numbers no plan claims (claimed: 9–10, 15–20,
30–31, 40) and record the choice in decisions.md.

### R1-C5 — Medium
**Integration with #76's creation protocol is unspecified.** #76 (clear) creates a volume through
an immutable create root and plan with a full create identity. Its admission writes the Creating
header at epoch 0, which stays "byte-for-byte at C until visibility". Reconciliation "resume[s]
admitted operations" and finalizes them visible, and an op ID with an ambiguous first CAS is
abandoned, never retried (#76 spec S1-S7, design:24-56). #54 only says "Reuse #76's conditional
creation batches" and "Final create clears Creating while leaving Incomplete" (design:37-40). It
leaves four things open:
- whether the restore mark is part of #76's immutable create identity and root;
- that #76's reconciler, not only BeginRestore, must finalize such a root into Incomplete rather
  than an ordinary volume;
- how the restore `operation_uuid` relates to #76's create op ID, which an ambiguous CAS forces to
  be abandoned while a BeginRestore retry reuses the restore UUID;
- how the separate restore job record (design:45-50) relates to #76's root and its GC.

**Fix:** state these four points as a mapping onto #76's records. Add tests: the reconciler
finalizes a restore root (still Incomplete), BeginRestore is retried after an abandoned create
op, and an identical CreateVolume for the name returns AlreadyExists.

### R1-C6 — Medium
**#79's use and initialization state is not set.** #79 (clear) persists
`{schema, profile, state, ...}` "in the initial hidden creating header transaction" and says
"Restore, clone, import and legacy migration MUST force Unknown/nonfresh regardless of their
desired mount mode" (#79 initialization-contract:18-21, `Volume.initialization` = 10). Formatting
is then granted only for FreshFs (#79 spec S3). #54's first-header write lists only the restore
fields (design:37-38, contracts:15-20). Intent Why 2's mkfs analysis still describes today's
blkid rule (`control/csi/node.go:107-114`).
**Fix:** BeginRestore writes #79's use state as Unknown in the same first Creating header, never
FreshFs. Add tests: a restored volume never gets FreshFs, and NodeStage after Ready never formats
it.

### R1-C7 — Medium
**#17's maintenance attach is a missed path.** #17 (clear) lets the drain make a "maintenance
attach" of any volume that is not held, taking a new epoch on a maintenance node, and "each epoch
has one kind" (#17 spec, Terms and the invariant). AcquireRestoreWriter is not a client attach,
so a restoring volume is not held. A drain can then depose the restore writer while it is
Writing, and while it is Finalizing it bumps the epoch under the proofs. S4's list ("Attach/
ControllerPublish/NodeStage/NodePublish, admin attach, snapshot/copy/move/growth and raw control")
and the design's freeze leave it out.
**Fix:** name the restore writer's epoch kind under #17. Either a maintenance attach refuses a
non-Ready restore, or AcquireRestoreWriter counts as held until restore-detach. Add a test.

### R1-C8 — Medium
**#73's roles, owner records and deletion rules are not applied to the new RPCs and volumes.**
Once tenancy is active, #73 (clear) requires a role from its method table for every method. It
refuses deletion of "a protected or unassigned volume that is not being created", counts any
header that #76's test does not call creating as published, and audits `DeleteVolume` (#73 spec
S1, contracts:26-53, :319). #54 has:
- no #73 rows for its ten RPCs;
- a "Reader" role (contracts:39, design:127) that neither `meta/service/auth.go:63-82` nor #73
  defines;
- no owner record for the restored volume.

So once active, an Incomplete restore (published under #73, unassigned) cannot be deleted by
AbortRestore's "existing deletion/tombstone cleanup" without the override.
**Fix:** add #73 rows (role before and after activation, insecure refusal, event) for each RPC.
Map GetRestore to existing roles. Give BeginRestore an owner (system, or a tenant through #73's
checks). State how AbortRestore's deletion meets #73's protection and unassigned rules. Add the
rows to `auth_test`.

### R1-C9 — Medium
**The phase graph leaves Incomplete restores undeletable and makes abort depend on a live
writer.** contracts:21 allows "Writing/Finalizing -> Aborting -> Deleted". It has no edge from
Incomplete, which is the state "failed Writing remains" in, and the state before any writer. The
Finalizing-to-Incomplete retry (contracts:22-23) is in prose only. Ordinary DeleteVolume
"refuses nonterminal restore jobs" (design:119), and abort needs the writer closed or
"independently fence[d]", with "unreachable authority can block safe deletion" (design:120-123).
So a writer node that is gone for good makes the volume undeletable, against intent outcome 4
("which can be deleted"). "Independently fence" has no mechanism. Today's deletion does not need
writer closure: a recreated name's first epoch is above every earlier epoch and token
(`meta/service/service.go:460-465`, docs/issues/30). The plan also does not say what a crash
after Aborting and before the deletion leaves, or which call finishes it.
**Fix:** give the full transition table (Incomplete/Writing/Finalizing → Aborting;
Finalizing → Incomplete through a new writer generation; Aborting → deletion, resumable by
AbortRestore or DeleteVolume). Let abort rely on the metadata invalidation plus #30's first-epoch
fencing instead of a closure proof, or state why that is not enough. Add tests for each edge and
for an abort crash.

### R1-C10 — Medium
**Old metadata binaries erase the restore mark; "roll out gates before enabling" has no
mechanism.** Go's `json.Unmarshal` ignores unknown fields, and `Attach` re-marshals the whole
`domain.Header` (`meta/service/service.go:708, 722-724`). So a metadata instance without this
change attaches an Incomplete restore and writes its header back without the `restore` field,
turning a partial image into an ordinary volume. Other header writers (the DeleteVolume mark,
creation) do the same. S10 says "Roll out metadata/node/CSI restore gates before enabling the
feature" but defines no enabling step. #76 settled on "a maintenance barrier excluding old
writers; no rolling mixed-version safety" (#76 spec Q3).
**Fix:** adopt #76's barrier explicitly and define the enabling step, for example a capability
record that only new binaries write and that BeginRestore requires, written after every instance
is upgraded. Document it in the upgrade notes. Add a test that BeginRestore refuses before the
step.

### R1-C11 — Medium
**The new contracts are incomplete or inconsistent.**
- `restore-verify VOLUME OP_UUID WRITER_GEN CHUNK EXPECTED_SHA256` (contracts:68) carries no
  Merkle inclusion path, yet `ReportRestoreChunkProof` must carry one (contracts:45-46). The node
  holds only the leaf, so it cannot build the proof.
- `RestoreTicketV1` gives only a magic and a list of bound fields (contracts:57-61). It has no
  byte layout, version, field order, length bounds or signing key, and no Go/Rust vectors.
  Compare captoken's exact encoding (`pkg/captoken/captoken.go:9-19`, `MaxLen` at :40).
- The job and proof records have no etcd keys or JSON schema (design:45-50, :108-111).

**Fix:** add the path, or the leaf index plus siblings, to the control line, or let metadata
store the expected leaves. Write the ticket encoding and its bounds, and add cross-language
vectors to `make contracts`. Give the keys under `/flaminio/v1/` with their values, writers and
deleters, as #52's Records table does.

### R1-C12 — Medium
**restore-volume skips #53's restore contract.** #53 (clear) requires `restore` to check "the
whole graph's retention as well as its content", and requires every JSON output of `list`,
`verify` and `restore` to carry `backend_profile: {id, version, qualified, source}` and
`immutable_guarantee`, with `candidate_state` under a candidate (#53 spec S8, contracts:90-92,
:222-240). Roots are `schema: 2` in an immutable repository. #54's restore-volume is a restore,
but its JSON schema (contracts:99-102) has none of these fields. The plan says only "Retained
roots use #53 exact-version/trust checks" (design:29), and S9's "not silently strengthened" is
not a mechanism.
**Fix:** restore-volume runs #53's restore checks before BeginRestore and reports #53's fields,
plus schema-2 roots. Add tests extending #53's `output_fields` and `verify_checks_graph_retention`
to restore-volume.

### R1-C13 — Medium
**The test plan names no concrete tests and too few injected bugs.** The design's Tests table
(design:157-165) and spec Tests are category phrases, with no test or simulator check names. The
rules add several safety properties:
- no Ready without a proof from every frozen ISR member;
- proofs bound to the finalization generation;
- membership frozen in Finalizing;
- abort invalidating the writer;
- writer closure before Ready.

Only two injected bugs are named ("early-Ready and stale-writer-after-completion"). AGENTS.md
requires a simulator check and an injected bug per new safety rule. These refusals have no test:
snapshot (#51), move, growth (#70), the CSI CreateVolume compatible-success refusal (design:49-50)
and the restore-only control commands on old nodes.
**Fix:** name each test by file and function. Name the simulator checks, and add injected bugs
for at least a proof from an old finalization generation, an ISR change committed in Finalizing,
a missing member proof, and abort without invalidation, each one the simulator must catch. Add
`meta-vectors` cases for the new transitions.

### R1-C14 — Medium
**The lab plan is not runnable as written.** Intent outcomes 8 and 9 require this change to
build the second cluster and the lab check if it comes before #58 ("Whichever of #58 and this
change comes first builds it"). #58 has no spec or design yet (batch manifest: missing spec.md
and design.md). The design gives only "Later explicit consent provisions #58's ... fl-c
destination" and a "shared isolated destination lab helper/check" (design:145-146, :172-177).
It names no script, Makefile target or check, no second CA or trust domain under #16's
`pki.sh`, no etcd ports, and no path from fl-c to fl-w's store.
**Fix:** name the commands (for example `lab.sh cluster2 up|down`, `make restore-check`), the
guest scripts, the fl-c resources and ports, the second PKI's directories, and the check's steps
and assertions per outcome 9. Mark them as shared with #58.

### R1-C15 — Low
**The control-socket compatibility is not fully specified.**
- "Plain status uses explicit `restoring VOLUME OP_UUID`" (contracts:72) changes the plain
  status. #15 (clear) keeps that format for older CSI plugins. Today's parser cuts at the first
  space (`control/csi/dataplane.go:94`), so an old plugin reads volume `restoring`. That is a
  valid id (`meta/domain/domain.go:87`), and for it `device()` returns `"VOLUME OP_UUID"` as the
  device.
- Plain `detach VOLUME`, which NodeUnstageVolume sends unconditionally
  (`control/csi/node.go:140`; `control.rs:192-213`), and `delete VOLUME` against a restore
  attachment are left unspecified.

**Fix:** report the restore only in #15's per-volume status, with plain status as `none` or the
other volume. Make plain `detach` and `delete` refuse a restore attachment. Add tests.

### R1-C16 — Low
**#80's worst-view budget does not count the new `Volume` fields.** #80 (clear) admits geometry
against "4,194,304 encoded protobuf bytes for any single create/get/attach reply", reserving for
the worst legal view (#80 spec, Terms). `RestoreInfoV1` and `home_cluster_uuid` add bytes to
every reply (contracts:16-20), but the plan gives no maximum size and no change to #80's constant.
**Fix:** bound each `RestoreInfoV1` field, give its maximum encoded size, and add it to #80's
worst-view fixtures.

### R1-C17 — Low
**"Durable" restore writes need O_DIRECT or a flush.** The worker writes through `/dev/ublkbN`
(design:69). Buffered writes complete in the host page cache, before the target sees them, so
"Wait for ordinary durable ISR completions" (design:85) is not met. Pages written back after
`restore-close` fail. The design does not say how the worker writes.
**Fix:** state O_DIRECT writes (or fsync of the device before `restore-close`), and test that
closing with dirty pages is refused or flushed.

### R1-C18 — Low
**Terminal job records are kept with no bound.** "Keep terminal job identity so deleting/
recreating a name cannot make an old job create or overwrite a later volume" (design:47-49) keeps
every restore's record forever. The plan gives no size bound, no collection and no named cost.
**Fix:** bound the record, and either collect it safely (for example #76-style birth records) or
name the growth as a risk.

## Nits
- contracts.md:13: `ArchiveRef {repository_uuid,root_uuid,version_id}`; #52 calls it
  `(repository_uuid,backup_uuid,version_id)`.
- design.md:19-20 and spec S1 call #18/#80 "proposals ... not approved", though both are clear and
  approved. design.md:184 says the ADR "leaves ... open", against its own "Accepted with the plan".
- contracts.md:94: the key path is in `--destination-config`, where #52's worker takes
  `--repository-key-file FILE`. Say which one restore-volume uses.

Reviewed commit: 52df1de4f36aa943353d9cfacfd271b1be8cc534
