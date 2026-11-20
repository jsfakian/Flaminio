# Issue #54 — report

## Review rounds

### Plan review round 1 — 52df1de

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer, design-code focus (54-r1-code.md); plan-reviewer, design-security focus
(54-r1-security.md).

- **R1-C1 — High:** restore-volume's identity, user and RPC callers were unspecified and
  contradicted #52's worker rules (never root, never an administrator pair); io was named as the
  reporter of progress and proofs it could not know or receive. Evidence: #52 contracts, Identity
  lifecycle and Where it runs; design, Source validation and Admission; contracts:40-41, :68-69;
  `flaminio-io/src/control.rs:9-10`.
- **R1-C2 — High:** reaching #52's mTLS store from another trust domain was unspecified, and S9's
  "no source certificate" contradicted #52's store profile. Evidence: #52 contracts:372-374,
  design:347-351; intent outcome 8; contracts:91-95.
- **R1-C3 — Medium:** #52's cluster-UUID hand-off was not taken; an administrator-chosen UUID
  could equal the source's and break S2. Evidence: #52 contracts:98, :112; contracts:8-10, :27.
- **R1-C4 — Medium:** `Volume` tags 40–41 collided with #75. Evidence: contracts:16;
  batch-2026-11-12.md:373.
- **R1-C5 — Medium:** integration with #76's create root, reconciler, op ids and GC was
  unspecified. Evidence: #76 spec S1–S7, design:24-56; design:37-50.
- **R1-C6 — Medium:** #79's use state was not set in the first header. Evidence: #79
  initialization-contract:18-21; design:37-38.
- **R1-C7 — Medium:** #17's maintenance attach could depose the restore writer. Evidence: #17
  spec Terms, S3; spec S4.
- **R1-C8 — Medium:** #73's roles, owner records and deletion rules were not applied; "Reader" is
  no role. Evidence: #73 spec S1, contracts:26-53, :319; `meta/service/auth.go:63-82`.
- **R1-C9 — Medium:** the phase graph left Incomplete restores undeletable and made abort depend
  on a live writer. Evidence: contracts:21-23; design:119-123; `meta/service/service.go:460-465`.
- **R1-C10 — Medium:** older metadata binaries erase the restore mark; the enabling step had no
  mechanism. Evidence: `meta/service/service.go:708, 722-724`; spec S10; #76 spec Q3.
- **R1-C11 — Medium:** restore-verify carried no Merkle path, the ticket had no encoding, and the
  records had no keys. Evidence: contracts:45-46, :57-61, :68; `pkg/captoken/captoken.go:9-40`.
- **R1-C12 — Medium:** restore-volume skipped #53's restore checks and output fields. Evidence:
  #53 spec S8, contracts:222-240; contracts:99-102.
- **R1-C13 — Medium:** the test plan named no tests and too few injected bugs. Evidence:
  design:157-165; AGENTS.md.
- **R1-C14 — Medium:** the lab plan was not runnable as written. Evidence: intent outcomes 8–9;
  design:145-146, :172-177.
- **R1-C15 — Low:** control-socket compatibility was incomplete (plain status, detach, delete).
  Evidence: contracts:72; `control/csi/dataplane.go:94`; `control/csi/node.go:140`; #15 S8.
- **R1-C16 — Low:** #80's worst-view budget did not count the new `Volume` fields. Evidence: #80
  spec Terms; contracts:16-20.
- **R1-C17 — Low:** "durable" restore writes needed `O_DIRECT` or a flush. Evidence: design:69,
  :85.
- **R1-C18 — Low:** terminal job records were kept with no bound. Evidence: design:47-49.
- **R1-S1 — High:** the untrusted-archive parser ran in a root process holding the destination's
  administrator pair, against #52's settled worker rules. Evidence: design:24, :64; #52
  design:31-33, contracts:77-84; `flaminio-io/src/control.rs:10,51`.
- **R1-S2 — High:** #73 was not taken: method rows, roles, owner, quota, default name, abort
  events and token issuance were unspecified. Evidence: #73 S1, S3–S5, contracts; contracts
  (RPCs); spec S1.
- **R1-S3 — Medium:** the cluster identity contradicted #52; the hand-off was not taken.
  Evidence: #52 contracts:98, :112; contracts (Identity); design:31-35.
- **R1-S4 — Medium:** object-store access across trust domains was unspecified (client
  certificate, CA bundles, store identity, credentials). Evidence: #52 contracts:356-375,
  design:347-351, :377-378; `pkg/pki/pki.go:244-283`.
- **R1-S5 — Medium:** a restore whose writer node is lost could not be deleted. Evidence:
  design:119-123; contracts (AbortRestore); intent outcome 4.
- **R1-S6 — Medium:** the lab destination helper had no disk mechanism, commands or id grammar.
  Evidence: `scripts/lab.conf:67`; design:172-177; AGENTS.md.
- **R1-S7 — Low:** restore-volume dropped #53's retention check and outputs. Evidence: #53 spec
  S8; contracts (JSON).
- **R1-S8 — Low:** the `Volume` tags collided with #75. Evidence: contracts (tags 40, 41);
  batch-2026-11-12.md:373.
- **R1-S9 — Low:** restore details bypassed GetRestore's role through the open reads. Evidence:
  contracts (RestoreInfoV1 in `Volume`); `meta/service/auth.go:79`.
- **R1-S10 — Low:** operation and proof records had no bound or collection. Evidence: design:48;
  contracts (proof batches).

Changes/resolution:
- R1-C1, R1-S1 (one fix): the command is split. `flaminio-backup restore-read` runs as #52's
  `flaminio-backup` user with no Flaminio pair and parses all bucket input; `flaminio-io
  restore-volume`, root with the destination's administrator pair, reads only its bounded frames
  (fuzz target `restore_frames`), calls metadata and drives the socket. io itself calls
  ReportRestoreChunkProof after `restore-verify` and ReportRestoreWriterClosed after
  `restore-detach`; ReportRestoreProgress is the helper's administrator RPC; `auth_test` rows for
  each. No change to #52 is needed.
- R1-C2, R1-S4 (one fix): the reader's config gains a separate `store_ca`, a required exact
  `store_identity` in any trust domain, and a store client pair from the store owner's CA whose
  identity no Flaminio service accepts; S9 now reads "no source-cluster identity". The lab issues
  the pair on fl-c, adds `backup-store.sh read-credential`, and moves the credential and key over
  ssh stdin, 0600. Tests `store_ca_separate`, `store_identity_required`,
  `store_identity_foreign_trust_domain`, and TLS tests that the destination refuses the store
  CA's certificates. Risk named, not accepted: in the lab the store owner's CA is the first
  cluster's.
- R1-C3, R1-S3 (one fix): `InitializeClusterOrigin` and caller-supplied UUIDs are cut; BeginRestore
  reads #52's `cluster` record or creates it with a CSPRNG value and a compare, and refuses an
  archive of the same cluster; `TestBeginRestoreClusterRecord`.
- R1-C4, R1-S8, R1-S9 (one fix): one `Volume` field, `RestoreState restore = 11` (phase and
  operation id only), the lowest unclaimed number; `home_cluster_uuid` cut; everything else
  served by GetRestore only.
- R1-C5: the four points mapped onto #76's root, admission, finalization and op ids, with
  `TestReconcilerFinalizesRestoreRoot`, `TestBeginRestoreAfterAbandonedCreateOp` and
  `TestCreateVolumeOverRestoreAlreadyExists`.
- R1-C6: #79's use is Unknown in the first header; AcquireRestoreWriter seals nothing;
  `TestRestoreUseUnknown`, `TestNodeStageReadyRestoreNeverFormats`.
- R1-C7: the writer's epoch is a third kind, `restore`; #17's maintenance attach and start refuse
  a non-Ready restore (cross-plan, whichever lands second); `TestMaintenanceAttachRefusesRestore`.
- R1-C8, R1-S2 (one fix): a #73 row per method (`system-provisioner` for the restore's calls and
  the writer token, `cluster-admin` for EnableRestore, both for GetRestore, the writer node's
  `io` for its reports, insecure refused); "Reader" dropped; a `system` owner record in the claim
  once active (no quota charge, as #73's system owners); a tenant-form default name refused once
  active, naming `--name` (a narrowing, shown necessary by R1-S2 through #73 S3); AbortRestore is
  DeleteVolume's deletion with #73's events and protection; audit events for the rest of a
  restore a follow-up, to be filed.
- R1-C9, R1-S5 (one fix): three phases (Incomplete, Finalizing, Ready) with a full transition
  table; abort is DeleteVolume's mark plus the job's invalidation, resumable by AbortRestore or
  DeleteVolume, needing no writer closure (#30's first-epoch fencing, the tombstone, forced
  confirmation); `TestAbortRestoreDeadWriter`, `TestAbortRestoreCrashAfterMark`.
- R1-C10: #76's maintenance barrier and an enabling step, EnableRestore writing
  `features/restore`, which BeginRestore requires; `TestBeginRestoreNotEnabled`.
- R1-C11: `restore-verify` carries the finalization generation and the Merkle path (≤ 16 × 32
  bytes); RestoreTicketV1's byte layout, `MaxLen` 417 and shared Go/Rust vectors in
  `make contracts`; the records' keys, values, writers and deleters; proofs advance a cursor in the
  job, so no proof record exists.
- R1-C12, R1-S7 (one fix): the reader runs #53's restore checks before its Header, and
  restore-volume reports #53's fields; #53's `output_fields` and `verify_checks_graph_retention`
  extended to restore-read.
- R1-C13: every test named by file and function; simulator checks `restore_ready`,
  `restore_no_write_after_ready`, `restore_frozen`, `restore_abort`; injected bugs caught by
  `catches_early_ready`, `catches_stale_writer_after_completion`,
  `catches_old_finalization_proof`, `catches_isr_change_in_finalizing`,
  `catches_missing_member_proof`, `catches_abort_without_invalidation`; meta-vectors for the
  transitions.
- R1-C14, R1-S6 (one fix): `make restore-dest-up|down` and `make restore-check`, the `lab.conf`
  variables, the destination disk by serial with its refusals (mounted, a Flaminio superblock,
  the first cluster's disk), the second PKI, units, ports and socket, the id grammar with
  `printf %q` and `scripts/tools/restore-check-test.sh`, the check's nine steps. Shared with #58;
  what #58's shipping needs of the destination is deferred to #58, not settled.
- R1-C15: plain `status` skips a restore attachment, `status VOLUME` answers `restoring`, plain
  `detach` and `delete` refuse one; tests named.
- R1-C16: `restore` is at most 38 encoded bytes, added to #80's worst-view fixtures by whichever
  lands second.
- R1-C17: `O_DIRECT|O_EXCL` writes, `fdatasync` and close before `restore-close`, which refuses
  while the device is open; `o_direct_and_flush`, `close_refused_while_open`.
- R1-C18, R1-S10 (one fix): a job is at most 2048 bytes, one per restore; no proof records; the
  terminal job is kept so S3 holds as written, its growth named, not accepted, and a collector a
  follow-up, to be filed; `TestRestoreJobBounds`.
- Nits: ArchiveRef uses #52's `backup_uuid`; #18 and #80 named as clear and approved and the
  ADR no longer said to leave choices open; `restore-read` takes `--repository-key-file`; the
  security Nit (phases) closed by R1-C9's cut.

Nits left: none.

### Plan review round 2 — 05fba15 (incremental, `52df1de..05fba15`)

Reviewers: plan-reviewer, design-code focus (54-r2-code.md); plan-reviewer, design-security
focus (54-r2-security.md). Every round-1 finding fixed or carried into a round-2 finding.
Findings: 3 Medium and 10 Low after merging two shared ones (R2-C1 = R2-S6, R2-C2 = R2-S2), and
6 Nits. Fix: `55c05e7`.

- R2-C1, R2-S6 (Medium, one fix): the io proof and closure RPCs named no `node`; they now carry
  it, checked against the certificate and the job's `writer_node`; `auth_test` rows.
- R2-C2, R2-S2 (Medium, one fix): a restore attachment could not be ended after a helper crash,
  an abort or a moved writer, so deletion stalled. restore-close is idempotent and the nonce stays
  in io; restore-detach works in any phase (`ok superseded`); the helper ends old attachments
  before restore-start and in cancel; plain `delete` ends a deleted or superseded restore's
  attachment; simulator check `restore_deletion_ends`, bug `catches_stuck_restore_deletion`.
- R2-S1 (Medium): the restore's target token attached ordinarily in Finalizing and after Ready.
  AcquireRestoreWriter returns only the FLRT ticket, which replicas accept as a target token and
  ordinary attach refuses; restore-start checks the phase; two injected bugs; #73 hand-off.
- R2-S3 (Medium): the reader shared #52's `flaminio-backup` user and directory. It has its own
  `flaminio-restore-read` user and `/var/lib/flaminio-restore`.
- R2-C3 (Low): the ticket's cluster UUID had no checker; cut.
- R2-C4 (Low): restore-close fsyncs and holds its `O_EXCL` fd; the helper retries a busy device.
- R2-C5 (Low): field 11 sat in #75's note of #70's 11–14; 22 proposed, final number deferred to
  the tag coordination with #70 and #75.
- R2-C6 (Low): AbortRestore of a Creating job runs #76's deletion; `TestAbortRestoreCreating`.
- R2-C7 (Low): the lab proves one ISR member; a named gap, a follow-up to be filed.
- R2-C8 (Low): a metadata rollback after EnableRestore needs no open restore; operator docs.
- R2-S4 (Low): lab secrets are written by the target user, exclusive, no link followed.
- R2-S5 (Low): the reader runs with rlimits, no-new-privs and a parent-death signal.
- R2-S7 (Low): ReportRestoreProgress carries the error fields, sanitized and bounded.
- Nits: tag sources cited, DeleteVolume's job key, risk wording, `archive_ref` match, the etcd
  CA, REF's text form.

Risks named, not accepted: a node the writer left keeps a fenced restore attachment until the
helper or an io restart ends it; an io restart loses a kept closure (a full replay); rollback
safety is the operator's check; a reader over 2 GiB of address space fails; one ISR member in
the lab; a compromised reader also chooses #53's outputs and the provenance.

Nits left: none.

### Plan review round 3 — 55c05e7 (incremental, `05fba15..55c05e7`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r3-both.md). Every round-2
finding fixed or carried into a round-3 finding. Findings: 3 Medium, 6 Low, 3 Nits. Fix:
`ca41362`.

- R3-C1 (Medium): round 2's `status VOLUME` and `list` forms broke #15's and #48's grammars.
  Round 1's forms restored; the helper reads its own `restore-list` line; parser tests.
- R3-C2 (Medium): `superseded` was decided after the header and `writer_node` checks, so cancel
  and a moved writer got errors. The job is read first; `TestWriterClosedSuperseded`.
- R3-C3 (Medium): a closure recorded in Incomplete survived finalization, so Ready could come
  with a live writer. A closure is reported only for an attachment that committed a proof in
  Finalizing, with its finalization generation; simulator check `restore_closed_at_ready`, bug
  `catches_closure_before_finalize`.
- R3-C4 (Low): `O_EXCL` sees only exclusive holders; restated, admission closure and the proofs
  named as the guarantees; round 2's fsync and held fd cut.
- R3-C5 (Low): ordinary attach ends a superseded restore attachment; the risk reworded.
- R3-C6 (Low): an identical closure is an idempotent success.
- R3-S1 (Low): dash's noclobber followed links to devices and FIFOs; `mktemp` and `ln -T`.
- R3-S2 (Low): limits and the parent-death signal set in `pre_exec` after the uid switch; the
  reader spawned from the main thread.
- R3-S3 (Low): the reader bounds its threads and malloc arenas; the risk names reservations.
- Nits: `restore_no_write_after_ready`'s scope, cancel's `no restore attachment`, restore-close
  after the end.

Risks named, not accepted: a plain opener's stray write fails the proofs rather than being
refused; any moved-from node keeps a fenced attachment and the restore slot until the helper, an
ordinary attach, the sweep or an io restart ends it; a same-user process could swap the lab's
temporary secret file; a reader whose reservations pass 2 GiB fails. Cross-plan: #15's STATE set
and #48's `list` parser gain `restoring`.

Nits left: none.

### Plan review round 4 — ca41362 (incremental, `55c05e7..ca41362`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r4-both.md). Every round-3
finding fixed; R3-C5's fix carried into R4-C1. Findings: 2 Medium, 1 Low, 5 Nits. Fix: `fcce15b`.

- R4-C1 (Medium): round 3's ordinary attach that ends a superseded restore attachment could end
  a live one in a race, and its call path could not end a device. Cut: #15's `reserve` refuses an
  ordinary attach of a volume with a restore attachment; every `resume` ends old attachments.
- R4-C2 (Medium): under #48 an io restart would keep a restore device and could reissue an old
  generation's writes after Ready. Restore devices are added without the recovery flags and with
  no #48 block; `restore_device_not_kept_across_restart`; #48's simulator scenario deferred to #48.
- R4-C3 (Low): the restore attachment's count under `--max-volumes` was unspecified; it counts,
  and `list` stays at 16 entries, 2,236 bytes.
- Nits: the Transactions table's reads, a generation above the job's, the chaos phase's
  CompleteRestore, an `O_RDONLY` probe, step 1's answer to any `io`.

Risks named, not accepted: a moved-from node's fenced attachment refuses an ordinary attach of
that volume there until the helper, the sweep or an io restart ends it, and where #15 forces the
limit to 1 it refuses every ordinary attach on that node. Cross-plan: #15's `reserve` counts the
restore entry and its longest STATE is 9 bytes; #48 adds no recovery flags or block for it.

Nits left: none.

### Plan review round 5 — fcce15b (incremental, `ca41362..fcce15b`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r5-both.md). R4-C1 and R4-C3
fixed; R4-C2 fixed in direct mode only, carried into R5-C2. Findings: 1 High, 1 Medium, 1 Low,
4 Nits. Fix: `4328ca1`.

- R5-C1 (High): CSI asks `device-info` before attaching, and a restore entry would have answered
  `running` with its device, so CSI could mount the fenced restore device on a moved-from node.
  `device-info` answers `ok none`, `detach-device` `ok absent`; CSI and #79 map
  `is restoring here` to `FailedPrecondition`. Every #15 and #48 line now has a stated answer for a
  restore entry; the walk also fixed a deposed entry, a late start, a kept entry and #48's stamper.
- R5-C2 (Medium): under `--privsep` #74's helper adds every device recoverable, so R4-C2's hazard
  remained. Restores run in direct mode only; under `--privsep` `restore-list` and `restore-start`
  refuse before anything is created. Restore under `--privsep` deferred to #74 and #48.
- R5-C3 (Low): the helper did not retry `limit:` or `restore slot taken` after detaching another
  volume's attachment; it retries them for up to 30 s after its own restore-detach.
- Nits: list size, both resume answers, the entry's states, "ends or leaves, never resumes".

Rule narrowed (plan scope, not intent): restores need direct mode. Risks named, not accepted: a
cluster whose nodes all run under #74's helper cannot restore in this increment; on a moved-from
node a stage of the Ready volume answers `FailedPrecondition` until the fenced attachment ends.
To be filed: restore under `--privsep` (#74, #48).

Nits left: none.

### Plan review round 6 — 4328ca1 (incremental, `fcce15b..4328ca1`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r6-both.md). Every round-5
finding fixed. Findings: 1 Medium, 2 Low, 4 Nits. Fix: `8a485ad`.

- R6-C1 (Medium): round 5's late start, deposed entry and restart scan had no tests, and a
  resume meeting a starting restore entry exited on an unnamed answer. `restore-list` shows a
  starting entry; restore-detach answers `is attaching`, retried for up to 30 s; three tests.
- R6-C2 (Low): restore-start was not refused after SIGTERM, and #12's `status --none` reads a node
  mid-restore as idle. restore-start refuses after SIGTERM; the preStop result is a named risk.
- R6-C3 (Low): "#48 is not review-clear" was stale; corrected to "review-clear, not landed".
- Nits: the ADR's flag wording, #48's `is resuming`, a well-formed tuple, both `restore-list`
  reads and the late `restore needs direct mode` exit code.

Risk named, not accepted: on a node run as #12's DaemonSet, preStop and `pod_nodes_idle` read a
node mid-restore as idle; the process exits when the entry ends or is killed at the 3600 s grace,
and `resume` replays. Cross-plan: #15 states `should_exit` waits for a restore entry.

Nits left: none.

### Plan review round 7 — 8a485ad (incremental, `4328ca1..8a485ad`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r7-both.md). Every round-6
finding fixed. Findings: 2 Low, 5 Nits. Fix: `3fd4cef` (by the orchestrator).

- R7-C1 (Low): restore-start's SIGTERM refusal and its test rested on #12 without being gated on
  it; gated, with #12's `begin_attach` hand-off.
- R7-C2 (Low): `cleanup_leaves_restore_record`'s `first_id` assertion failed on correct code after
  a deleted record; asserted only where the id is still recorded.
- Nits: the retry summary, the 30 s wording, a running restore entry under #48, the sweep case's
  precondition, the ADR's `--privsep` cause.

No rule narrowed, nothing deferred, no risk named.

Nits left: none.

### Plan review round 8 — 3fd4cef (incremental, `8a485ad..3fd4cef`)

Reviewer: plan-reviewer, design-code and design-security focus (54-r8-both.md). Every round-7
finding and Nit fixed. Findings: none. The plan is review-clear at round 8.

Status: clear. The plan was approved on 2026-11-13; the fixes of rounds 1–7 were made after
approval and are not themselves approved.
