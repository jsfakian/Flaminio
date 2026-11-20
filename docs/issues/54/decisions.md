# Issue #54 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review rounds 1 to 5 (2026-11-19) fixes, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The restore's state, writer and finalization protocol:** the proposed direction, a
  first-header gate plus a frozen all-ISR proof and a closed writer. The restore is claimed in
  the volume's first Creating header (Incomplete, Writing, Finalizing, Ready) and every ordinary
  attach path refuses it until Ready; each writer takes a fresh destination epoch with a
  purpose-bound restore ticket and writes through the target's normal durable replication;
  finalizing freezes membership, closes the writer, verifies each chunk on every ISR member with
  generation-bound proofs, and detaches the writer before the Ready CAS. [spec.md](spec.md), Open
  questions; [design.md](design.md), Admission and specialized write authority, and Frozen
  finalization and Ready; [adr-proposal.md](adr-proposal.md), Decisions proposed and Options.
- **The minimal origin identity:** as proposed, a home `cluster_origin_uuid` in the namespace of
  #52's `SnapshotRef.cluster_uuid`, set once by an administrator's conditional write and never
  regenerated; it is provenance only and grants no authority. It is agreed with #52 before
  coding. [spec.md](spec.md), Open questions; [design.md](design.md), Source validation, origin
  and claim; [contracts.md](contracts.md), Identity, operation and metadata.
- **The source and owner API seams:** as [contracts.md](contracts.md) proposes: the
  administrator RPCs (`InitializeClusterOrigin`, `BeginRestore`, `AcquireRestoreWriter` and the
  rest), the restore ticket, the control and status contracts and the `restore-volume` command;
  protobuf tags are coordinated with #50, #51, #52, #70 and #77 before coding.
  [spec.md](spec.md), Open questions; [design.md](design.md), Files and contracts, and Lab,
  docs, risks and ADRs.
- **Sparse layout:** as proposed, logical zeros are preserved by writing only nonzero 4 KiB
  extents into the fresh volume; physical allocation may differ by destination layout and no
  page-map or hole equivalence is claimed. This qualifies the intent's "unwritten" wording.
  [spec.md](spec.md), Open questions; [design.md](design.md), Data, sparse output and
  interruption; [adr-proposal.md](adr-proposal.md), Decisions proposed.

## Plan review round 1 (2026-11-19), changes made after approval

Reviewed commit 52df1de; findings R1-C1 to R1-C18 (code) and R1-S1 to R1-S10 (security), three
Nits (code) and one (security). Each change below was made after approval, in this round;
nothing in it is implemented, measured or approved anew. Where a finding offered options, the
simplest safe one was taken; where two reports overlap, one fix closes both.

- **Two processes** (R1-C1, R1-S1, one fix): the reviewers' split. `flaminio-backup
  restore-read` runs as #52's `flaminio-backup` user with no Flaminio pair (one in its config is
  refused) and parses all bucket input; `flaminio-io restore-volume`, root with the destination's
  administrator pair, reads only its bounded frames (fuzzed), calls metadata and drives the
  socket. #52's worker rules (never root, never an administrator pair) thus hold for every process
  that parses bucket input, and no change to #52 is needed. io itself reports chunk proofs after
  `restore-verify` and the closure after `restore-detach`; ReportRestoreProgress is the helper's,
  an administrator RPC. Every method has its `auth_test` rows. This changes the Settled entry "The
  source and owner API seams" (the command's home and the RPC callers).
- **The store from another trust domain** (R1-C2, R1-S4): the reader's config gains `store_ca` (a
  file of its own, S3 client only, never in a Flaminio bundle), a required `store_identity`
  compared exactly in any trust domain, and a store client pair from the store owner's CA whose
  identity no Flaminio service accepts. S9's "no source certificate" now reads "no source-cluster
  identity". The lab issues the pair on fl-c, creates a read-only credential with a new
  `backup-store.sh read-credential`, and moves it and the key over ssh stdin, 0600. Risk named,
  not accepted: in the lab the store owner's CA is the first cluster's CA.
- **#52's cluster record** (R1-C3, R1-S3): `InitializeClusterOrigin` and its caller-supplied UUID
  are cut. BeginRestore reads `/flaminio/v1/cluster`, or creates it with a CSPRNG value and a
  compare, and refuses an archive whose source cluster equals it. This takes #52's hand-off and
  supersedes the Settled entry "The minimal origin identity" (its administrator-set UUID); the
  record is still provenance only.
- **`Volume` field 11** (R1-C4, R1-S8, R1-S9): one field, `RestoreState restore = 11` (phase and
  operation id only), the lowest number no plan claims (9–10, 15–21, 30–31 and 40 are claimed;
  #50 holds 20–21). `home_cluster_uuid` is cut: the cluster record is the home UUID. Refs,
  progress and errors move to GetRestore, so the open reads expose none of them.
- **#76's creation** (R1-C5): the root's create identity carries the restore binding; admission
  writes the restore mark, #79's Unknown use and the job; every finalization, the reconciler's
  included, keeps the mark and writes the job's first epoch; the restore UUID is the caller's and
  #76's create op the service's, an abandoned create op leaving no job. An equal CreateVolume is
  AlreadyExists.
- **#79** (R1-C6): Unknown use in the first header; AcquireRestoreWriter seals nothing; after
  Ready the first ordinary attach seals Raw or ReadyFs and never formats. #79's token-issuing
  list gains AcquireRestoreWriter, by whichever lands second.
- **#17** (R1-C7): the restore writer's epoch is a third kind, `restore`; #17's maintenance attach
  and start refuse a non-Ready restore, by whichever of #17 and #54 lands second.
- **#73** (R1-C8, R1-S2, one fix): a role row per method (`system-provisioner` for the restore's
  calls and the writer token, `cluster-admin` for EnableRestore, both for GetRestore, the writer
  node's `io` for its reports, insecure refused); "Reader" dropped; a `system` owner record in the
  claim once active (charging nothing, as #73's system owners); AbortRestore is DeleteVolume's
  deletion with #73's events and protection. **A rule narrowed, shown necessary by R1-S2:** once
  tenancy is active, the default name of outcome 2 and S1 cannot be a tenant-form `pvc-<UID>`,
  since #73 S3 refuses that id to a system creation and #73's invariant needs a CSI-verified
  owner; such a restore needs `--name`. Before activation, and for any other name, the default
  holds. A tenant-owned restore is #52's step 5 (deferred, not settled). Audit events for the
  rest of a restore: a follow-up, to be filed.
- **Phases and abort** (R1-C9, R1-S5, one fix): the phases are Incomplete, Finalizing and Ready,
  as the spec's Terms (Writing, Aborting and Deleted are cut: the security Nit); contracts give the
  full table. Abort is DeleteVolume's mark plus the job's invalidation, resumable by AbortRestore or
  DeleteVolume, and needs no writer closure: #30's first epoch fences a lost writer and the
  tombstone removes its replicas. A plain DeleteVolume of a non-Ready restore does the same.
- **Rollout** (R1-C10): #76's maintenance barrier, and an enabling step: `EnableRestore` writes
  `features/restore`, which BeginRestore requires.
- **Contracts** (R1-C11): `restore-verify` carries the finalization generation and the Merkle
  path (at most 16 × 32 bytes, N ≤ 65,536); RestoreTicketV1 has a byte layout, bounds and shared
  vectors; the records have keys, values, writers and deleters. Proofs advance a cursor in the job,
  so no proof record exists.
- **#53's restore** (R1-C12, R1-S7): the reader runs #53's restore checks before its Header and
  restore-volume reports #53's fields; schema-2 roots accepted.
- **Tests** (R1-C13): named per file and function; simulator checks `restore_ready`,
  `restore_no_write_after_ready`, `restore_frozen` and `restore_abort`, with six injected bugs;
  meta-vectors for the transitions.
- **Lab** (R1-C14, R1-S6, one fix): `make restore-dest-up|down`, `make restore-check`, the
  `lab.conf` variables, the disk by serial with its refusals, the second PKI and units, the id
  grammar with `printf %q` and a script test. Shared with #58: what #58's shipping needs of the
  destination (bundle exchange, peer ports) is #58's, deferred, not settled.
- **Control socket** (R1-C15): plain `status` skips a restore attachment; `status VOLUME` answers
  `restoring`; plain `detach` and `delete` refuse one.
- **#80** (R1-C16): `restore` is at most 38 encoded bytes, added to #80's worst-view fixtures by
  whichever lands second.
- **Durable writes** (R1-C17): `O_DIRECT|O_EXCL`, `fdatasync` and close before `restore-close`,
  which refuses while the device is open.
- **Terminal records** (R1-C18, R1-S10, one fix): a job is at most 2048 bytes, one per restore;
  no proof records. Kept, so S3 holds as written; the growth is named, not accepted, and a
  collector is a follow-up, to be filed.
- **Nits:** ArchiveRef is #52's `{repository_uuid,backup_uuid,version_id}`; #18 and #80 are named
  as clear and approved, and the design no longer says the ADR leaves choices open;
  `restore-read` takes #52's `--repository-key-file`. The security Nit is closed by the phase cut.

## Plan review round 2 (2026-11-19), changes made after approval

Reviewed commit 05fba15; findings R2-C1 to R2-C8 (code), R2-S1 to R2-S7 (security), two Nits
(code) and four (security). Each change below was made after approval, in this round; nothing in
it is implemented, measured or approved anew. Where two reports share a cause, one fix closes
both. Where a mechanism could be cut instead of guarded, it was cut.

- **The io reports' `node`** (R2-C1, R2-S6, one fix): ReportRestoreChunkProof and
  ReportRestoreWriterClosed gain `node`. `auth.go`'s node rule takes it (the certificate's node,
  component `io`); the transaction compares it with the job's `writer_node`
  (`FailedPrecondition`). `auth_test` rows for another node's `io`, the writer's agent and a
  non-writer `io`.
- **Ending a restore attachment** (R2-C2, R2-S2, one fix): restore-close is idempotent per
  generation and the closure nonce is cut from the control lines (io keeps and reports it
  itself, so no helper crash can lose it); restore-detach works in any phase, ends the
  attachment first and answers `ok superseded` when ReportRestoreWriterClosed answers
  `superseded` (aborted or completed job, later generation); the helper ends its node's own and
  stale restore attachments before restore-start and in `cancel`; the abort's tombstone also
  names the writer node; the sweep's plain `delete VOLUME` ends a restore attachment once
  GetVolume shows the volume gone, Deleting or of no or another restore. This replaces round 1's
  refusal of plain `delete` (R1-C15) and its test `plain_detach_delete_refuse_restore`; plain
  `detach` still refuses. **Risks named, not accepted:** a node a writer moved away from that
  holds no replica keeps a fenced attachment until the helper next runs there or io restarts;
  an io restart loses a kept closure, forcing a new writer and a replay.
- **The restore target's capability** (R2-S1): AcquireRestoreWriter returns only the FLRT, no
  FLCT; replicas accept the FLRT as a target token; ordinary attach refuses its magic;
  restore-start reserves the node's slot, then requires GetVolume to show the operation
  Incomplete at the ticket's epoch with this node as target. The token thus attaches nothing in
  Finalizing or after Ready. Cutting the second token was chosen over a purpose byte in FLCT
  (no change to FLCT's format). Simulator: the chaos phase tries both attach paths;
  `restore_no_write_after_ready` covers them; bugs `catches_restore_ticket_ordinary_attach` and
  `catches_restore_start_without_phase_check`. Cross-plan: #73's "Attach is decided by the
  certificate" gains this one exception. The ADR proposal's ticket decision says so.
- **The ticket's cluster UUID** (R2-C3): cut, since no node checks it and the destination's own
  key binds the ticket to the cluster; `MaxLen` 401.
- **restore-close and a buffered writer** (R2-C4): io `fsync`s its `O_EXCL` descriptor before
  closing admission and holds it until the device ends; the helper retries `device is open` 10
  times, 500 ms apart.
- **`Volume` field number** (R2-C5, code Nit): 22 proposed, outside every claim including #75's
  11–14 for #70, each cited from #13's registry or its owner's design. **Deferred, not settled:**
  the final number, to the tag coordination with #70 and #75 before `make proto`.
- **Abort while Creating** (R2-C6): AbortRestore on a job with `first_epoch` 0 compares
  `create_op` and runs #76's deletion of the incomplete operation, whose revocation writes the
  job `aborted` in the same transaction (a hand-off to #76, by whichever lands second);
  `TestAbortRestoreCreating`.
- **One ISR member in the lab** (R2-C7): a named gap, not accepted; per-member proofs and the
  return to Incomplete are proved in the simulator only. A two-node destination is a follow-up,
  to be filed.
- **Rollback after EnableRestore** (R2-C8): the operator documentation requires every restore
  aborted or completed before a metadata rollback (ListVolumes shows none Incomplete or
  Finalizing). **Risk named, not accepted:** the check is the operator's. The optional automatic
  check is not taken.
- **The reader's own user** (R2-S3): `flaminio-restore-read`, directory
  `/var/lib/flaminio-restore`, store pair owned by it; the helper refuses to run the reader as
  uid 0 or `flaminio-backup`. This changes round 1's "no change to #52 is needed": #52's lab puts
  only the `flaminio-backup` binary on fl-c, never its user, `backup-pki` or worker directory.
- **Secrets installed as the user** (R2-S4): `sudo -u flaminio-restore-read` with dash's
  noclobber (`O_CREAT|O_EXCL`), umask 077; `scripts/tools/restore-secret-test.sh` plants a
  symlink and a file.
- **The reader's bounds** (R2-S5): no-new-privileges, SIGKILL on parent death, its own session,
  `RLIMIT_AS` 2 GiB, `RLIMIT_NOFILE` 256, `RLIMIT_CORE` 0; the helper kills it on error. A
  memory cgroup was not chosen: the address-space limit fails the reader's allocation before the
  node's OOM killer acts. **Risk named, not accepted:** a reader needing more than 2 GiB fails the
  restore.
- **The reader's error text** (R2-S7): ReportRestoreProgress carries `error_code` and
  `error_object`, which the helper sanitizes and the service bounds.
- **Security Nits:** the compromised-reader risk names #53's outputs and the provenance; the
  helper refuses a Header of another `archive_ref`; the destination PKI names its etcd CA; REF's
  text form is `REPOSITORY_UUID/BACKUP_UUID/VERSION_ID` (a hand-off to #52, which owns REF), and
  the lab's narrower version-id grammar is a lab limit. **Code Nit:** DeleteVolume reads the job
  key from `header.restore.operation`.

## Plan review round 3 (2026-11-19), changes made after approval

Reviewed commit 55c05e7; findings R3-C1 to R3-C6 and R3-S1 to R3-S3 (one report, code and
security), three Nits. Each change below was made after approval, in this round; nothing in it
is implemented, measured or approved anew. Where a mechanism could be cut instead of guarded, it
was cut.

- **#15's and #48's line grammars** (R3-C1): round 1's forms are restored, `ok VOLUME restoring`
  for `status VOLUME` and the pair `VOLUME restoring` in `list`, so `restoring` is one more STATE
  and neither grammar's arity changes. The helper gets its own line, `restore-list -> ok none |
  ok VOLUME OP_UUID WRITER_GEN` (one entry at most, the node's restore slot). Tests
  `TestListAndStatusVolumeWithRestore`, `TestKeptStateParsingRestoring`, `restore_list_line`,
  `status_volume_and_list_forms`. **Cross-plan, by whichever lands second:** #48's parser takes
  `restoring` as a known STATE its loop skips (it refuses an unknown one), and #15's STATE set
  gains it.
- **The order of `superseded`** (R3-C2): ReportRestoreWriterClosed decides `superseded` from the
  job record alone, first (terminal, or a later writer generation), and only then compares the
  header, `writer_node` and the finalization generation for the current generation. The auth
  paragraph's `writer_node` compare is ordered after that step. Tests `TestWriterClosedSuperseded`
  (aborted with the header gone, completed, a later generation from another node). The "aborted
  while Creating" case is not tested: no writer exists before #76's finalization.
- **A closure only in Finalizing** (R3-C3, a safety rule): the closure in Incomplete is cut, as
  the review proposed first, rather than having BeginRestoreFinalize clear it. restore-detach
  reports a closure only for an attachment through which a proof committed, carrying that
  finalization generation, which the service compares in Finalizing; any other detach answers
  `ok not-recorded` and sends nothing. Such an attachment held the node's only restore slot from
  a start read in Incomplete to an end after a proof in Finalizing, so no later attachment of the
  generation can start (restore-start reads Finalizing). This also closes a race the review did
  not name: a closure RPC sent in Incomplete that commits after BeginRestoreFinalize. Simulator:
  the chaos phase replays restore-start in Incomplete after a restore-detach; check
  `restore_closed_at_ready`; bug `catches_closure_before_finalize` (round 2's rule). Tests
  `TestWriterClosedOnlyInFinalizing`, `restore_detach_not_recorded_without_proof`. The ADR
  proposal's finalization decision says which attachment's end is the closure.
- **`O_EXCL` at restore-close** (R3-C4): restated as a check for an exclusive holder (a mount,
  another `O_EXCL`, a dm or md claim); admission closure and the proofs are named as the
  guarantees. Round 2's `fsync` and held descriptor guard nothing those two do not, so they are
  cut, with `close_holds_fd_across_drain`; `write_after_close_refused` and
  `stray_write_fails_proof` replace it, and `close_refused_while_open` holds the device `O_EXCL`
  or mounted. **Risk named, not accepted:** a plain opener's stray write fails the restore's
  proofs rather than being refused.
- **A moved-from node's attachment** (R3-C5): the cheaper fix is taken. An ordinary attach of
  the volume on that node reads GetVolume as today and ends a restore attachment that is of a
  Ready restore, at another epoch or of another target, before attaching. Test
  `ordinary_attach_ends_superseded_restore_attachment`. The risk is reworded: any moved-from
  node, with or without a replica, keeps a fenced attachment and the restore slot until the
  helper, an ordinary attach, the deletion's sweep or an io restart ends it (named, not
  accepted).
- **A resent closure** (R3-C6): an identical closure for the current generation is an idempotent
  success that writes nothing; another closure is refused. Tests `TestWriterClosedIdempotent`,
  `restore_detach_resends_closure`.
- **The lab's secret install** (R3-S1): dash's noclobber is cut. The secret goes into a fresh
  `mktemp` file in the 0700 directory, then `ln -T` (which neither follows nor overwrites) to
  DST, and the temporary name is removed. `restore-secret-test.sh` adds a dangling link, a link
  to `/dev/null` and one to a FIFO. **Risk named, not accepted:** a process of the same user
  running during the install could swap the temporary file (no escalation; none runs then).
- **The parent-death signal** (R3-S2): set in a `pre_exec` closure, which Rust's `Command` runs
  after its uid and gid switch; the helper spawns the reader from its main thread. Tests
  `reader_dies_with_helper` (fails if set before the switch), `reader_survives_thread_exit`.
- **`RLIMIT_AS`** (R3-S3): the reader calls `mallopt(M_ARENA_MAX, 2)` and runs 2 worker threads
  and at most 4 blocking threads; `reader_address_space_bounded`, and the lab check's restore
  runs at fl-c's core count. The risk is reworded to name reservations (named, not accepted).
- **Nits:** `restore_no_write_after_ready` is scoped to the writer epoch of the generation that
  reached Ready, after its BeginRestoreFinalize; `cancel` and the helper's loop count `no
  restore attachment` as ended; a restore-close repeat after the attachment ended answers `no
  restore attachment`.

## Plan review round 4 (2026-11-19), changes made after approval

Reviewed commit ca41362; findings R4-C1 to R4-C3 (one report, code and security) and five Nits.
Each change below was made after approval, in this round; nothing in it is implemented, measured
or approved anew. Where a mechanism could be cut instead of guarded, it was cut.

- **An ordinary attach and a restore attachment** (R4-C1): round 3's rule, an ordinary attach
  ending a superseded restore attachment (R3-C5's cheaper fix), is **cut**, not narrowed. The
  narrowed rule (Ready, gone, Deleting or another operation; in io's socket and registry layer;
  only the entry captured before the read; a race test) is safe but adds a GetVolume read and an
  end step ahead of #15's `reserve`, a change to #15, to serve a liveness gap only; the cut adds
  nothing to the attach path. An ordinary attach of a volume with a restore attachment on the
  node is now refused by #15's `reserve`, `error: VOLUME is restoring here`, in any phase. The
  operator's remedy is the helper's existing step, which now also runs in every `resume`, a
  completed job's included: it ends its node's restore attachment of a terminal job (`ok
  superseded`). Tests: `ordinary_attach_ends_superseded_restore_attachment` is replaced by
  `ordinary_attach_refused_by_restore_attachment` and `resume_of_completed_job_ends_attachment`.
  **Risk named, not accepted** (R3-C5's wording, kept): a moved-from node keeps a fenced restore
  attachment until the helper runs there, the deletion's sweep or an io restart, and meanwhile
  refuses an ordinary attach of that volume there.
- **Restore devices are unrecoverable** (R4-C2, a safety rule): restore-start adds its device
  with neither `UBLK_F_USER_RECOVERY` nor `UBLK_F_USER_RECOVERY_REISSUE`, and its #15 record has
  no #48 block, so an io restart ends it by #15's rule and #48 never keeps or resumes it: no held
  restore write is issued again after Ready. This makes true the plan's premise that an io
  restart ends the attachment and loses a kept closure (contracts, restore-detach; Risks). Test
  `restore_device_not_kept_across_restart` (a ublk host test). The ADR proposal's finalization
  decision says so. **Cross-plan, by whichever of #48 and #54 lands second:** #48 adds neither
  flag nor block for a restore device and does not read that add as its fallback (it keeps
  asking, and `recovery` stays `on`); #48's simulator `RestartKept` gains a restore scenario,
  check `restore_not_kept` and bug `catches_kept_restore_device`. **Deferred, not settled:** the
  simulator part, to #48's landing, since #48 owns `RestartKept` and its plan, approved and
  review-clear, is not landed (corrected in round 6, R6-C3; round 4 said "not review-clear").
- **`--max-volumes`** (R4-C3): a restore attachment is a #15 entry that `reserve` counts under
  the limit, beside the node's restore slot (`error: VOLUME: restore slot taken`). `list` stays at
  16 entries, at most 2,236 bytes with a 9-byte state. Where #15 forces the limit to 1
  (`--privsep`, before Linux 6.9), a restore takes the node's only slot. The helper retries
  restore-start's `is stopping here` after its own restore-detach, as it retries `device is open`.
  Tests `restore_counts_under_limit`, `start_after_detach_retries_stopping`. **Cross-plan, by
  whichever of #15 and #54 lands second:** #15's `reserve` counts a restore entry; its longest
  STATE becomes `restoring`, 9 bytes, and its longest `list` 2,236 bytes (its S1 bound and its
  16-entry `list` test). **Risk named, not accepted:** at limit 1, a moved-from node's fenced
  restore attachment refuses every ordinary attach there until it ends (R4-C1's remedies).
- **Nits:** the transactions table says a `superseded` answer reads the job only and an
  idempotent closure answer reads header and job; a request's writer generation above the job's
  is `FailedPrecondition` `restore OP has no writer generation G` (tested in
  `TestWriterClosedOnlyInFinalizing`); the chaos phase calls CompleteRestore while the replayed
  attachment is live, which `catches_closure_before_finalize` needs; restore-close's probe opens
  `O_RDONLY|O_EXCL` (`restore_close_probe_read_only`); step 1's `superseded` to any admitted
  node's `io` is stated as harmless, an operation's terminal state and a later generation not
  being secret.

## Plan review round 5 (2026-11-19), changes made after approval

Reviewed commit fcce15b; findings R5-C1 to R5-C3 (one report, code and security) and four Nits.
Each change below was made after approval, in this round; nothing in it is implemented, measured
or approved anew. Where a mechanism could be cut instead of guarded, it was cut.

- **What CSI's device check sees** (R5-C1): `device-info VOLUME` answers a restore entry `ok
  none` and `detach-device` answers it `ok absent`, changing nothing; the contract's revision 5
  grammar (`95a1dba`, which #79 pins) is unchanged. The plugin's `device()` then calls `Attach`,
  which `reserve` refuses `error: VOLUME is restoring here`, mapped to `FailedPrecondition`, as
  stopping; kubelet retries with the same publish context. Tests `device_info_hides_restore`,
  `detach_device_absent_for_restore`, `TestDeviceRestoringHereFailedPrecondition`.
  **Cross-plan, by whichever lands second:** #15's contract and S9 mapping; #79's plugin maps
  the same answer for `attach-init` and `attach-operation`, and its `initialization-status` and
  `attachment-status` answer a restore entry as another operation's (#79 owns that form).
- **Every line, walked once** (the recurring pattern of rounds 3 to 5): contracts' "A restore
  entry's lines" gives one answer per #15, #48 and #79 line for a restore entry: plain and
  volume `status`, `list`, `device-info`, `detach-device`, `detach`, `delete`, every ordinary
  attach path, #79's status lines, restore-start's own `reserve` answers, a deposition, a late
  start, the restart scan and `cleanup`, and #48's stamper, `should_exit`, loop and fallback.
  Found by the walk, beyond the review: a deposed restore entry is marked stopped and keeps its
  count and the slot until it is ended (as the moved-from risk already says); a late-started
  restore device is ended by the helper's `resume`; a restore-start meeting a #48 kept entry of
  the volume answers `is kept here` and resumes nothing (`restore_start_refuses_kept`); #48's
  stamper skips a restore entry and its `should_exit` counts one unrecoverable, so #12's S4
  holds on SIGTERM (`restore_entry_not_stamped`, `sigterm_waits_for_restore_entry`, by
  whichever of #48 and #54 lands second).
- **Restores in direct mode only** (R5-C2, a safety rule): option (b) is taken. The intent does
  not ask for restores under `--privsep` (it names neither #74 nor the helper), so the smaller,
  safer choice is a refusal: under `--privsep` `restore-list` answers `error: restore needs
  direct mode` and `restore-start` `error: VOLUME: restore needs direct mode`, before any
  `reserve` or helper request; the helper sends `restore-list` before its first metadata call
  and exits 3, having created nothing. Option (a), an unrecoverable form of #74's `add`, would
  change #74's fixed request grammar and #48's exit rules (#48 is approved and review-clear but
  not landed; corrected in round 6, R6-C3) for a mode the intent does not need. The hazard's closure is stated for every mode (contracts, Direct mode
  only): without `UBLK_F_USER_RECOVERY` the kernel stops the device after its server exits, so
  no mode can resume it. The lab's destination node runs in direct mode. Tests
  `restore_refused_under_privsep`, `privsep_node_exits_3`; `restore_device_not_kept_across_restart`
  is direct mode, where `recovery` answers `on`. This narrows the plan's earlier scope (round 4
  named restores under `--privsep`), not the intent. **Deferred, not settled:** restore under
  `--privsep`, to #74 and #48 (a follow-up, to be filed). **Risk named, not accepted:** a
  cluster whose nodes all run under the helper cannot restore until then.
- **When the slot is freed, and the helper's retry** (R5-C3): the restore slot is freed with the
  entry's `--max-volumes` count when the #15 entry ends. After its own restore-detach in that
  run, the helper retries restore-start's `is stopping here`, `limit:` and `restore slot taken`
  every 500 ms for up to 30 s, #15's own detach wait (round 4's 10 × 500 ms is replaced), then
  exits 1; without its own detach each exits 1 at once. `start_after_detach_retries_stopping`
  gains another volume at `--max-volumes 1` and at the default limit.
- **Nits:** `restore_counts_under_limit` expects 2,221 bytes, the bound with one `restoring`
  (the single slot makes 2,236 unreachable; the round-4 hand-off to #15 is corrected);
  `resume_of_completed_job_ends_attachment` tests both answers, `ok not-recorded` (detached in
  Incomplete, the usual case, no RPC) and `ok superseded` (a proof committed through it), and
  round 4's "(`ok superseded`)" above is read with that correction; the restore entry's states
  are named, #15's starting, running and stopping, all shown `restoring`, with restore-start
  alone answering a stopping entry `is stopping here` (`restore_entry_states`); "deletes it by
  #15's rule" becomes "ends or leaves; never resumes".
- **Not applicable:** none. **Risk named, not accepted** (R5-C1): on a moved-from node the plugin
  answers a stage of the Ready volume `FailedPrecondition`, kubelet retrying, until the fenced
  restore attachment ends (round 4's remedies).

## Plan review round 6 (2026-11-19), changes made after approval

Reviewed commit 4328ca1; findings R6-C1 to R6-C3 (one report, code and security) and four Nits.
Each change below was made after approval, in this round; nothing in it is implemented, measured
or approved anew.

- **A starting restore entry, and the round-5 cases' tests** (R6-C1): `restore-list` shows the
  restore entry in each of #15's states, starting included. restore-detach of a starting entry
  answers #15's `detach` wording, `error: VOLUME is attaching`, and changes nothing; the sweep's
  `delete` gets the same answer and tries again at its next pass. The helper retries `is
  attaching` every 500 ms for up to 30 s, the same 30 s as #15's detach wait, then exits 1, the restore left
  Incomplete (a cancelled one aborted, the sweep ending the entry). restore-start's `is restoring
  here` and `no answer in time` exit 1 at once, never retried. Tests
  `late_restore_start_ended_by_resume` (a helper test), `deposed_restore_entry_holds_slot` (a
  control test), `cleanup_leaves_restore_record` (a ublk host test, without and with #48,
  `cleanup --expired` included); `restore_entry_states` gains the `restore-list` and starting
  answers.
- **SIGTERM** (R6-C2): restore-start refuses once a SIGTERM was requested, `error: VOLUME: the
  node is shutting down`, after the `--privsep` refusal and in the same acquisition of the
  registry's lock as `reserve`, as #12's `begin_attach`; the helper exits 1 at once. The other
  restore lines are still served, so the restore in flight can end its entry.
  `sigterm_waits_for_restore_entry` leaves the "with #48" group: it runs over #15's registry
  without #48, and with #48's `should_exit` by whichever lands second, and gains a restore-start
  after the signal, refused before any slot, limit or state answer. **Choice:** the preStop
  result is named as a risk rather than changing `status --none` to read `list`. The change
  would cross #12's agent, its preStop hook, `pod_nodes_idle` and the upgrade's checks, and #15's
  lines, in two plans not landed, and an older agent would still read plain `status`; the
  result is not a safety issue, and the refusal above bounds the wait to the restore in flight.
  **Cross-plan, by whichever of #12, #15 and #54 lands second:** #15 states that #12's
  `should_exit`, read over its registry, is true only once no entry is left, a restore entry
  included (#48 already reads it over the registry). **Risk named, not accepted:** on a
  destination node run as #12's DaemonSet, `status --none` reads a node mid-restore as idle; the
  process then exits when the restore entry ends or at the 3600 s grace's SIGKILL, which fails
  the helper's I/O (exit 1; `resume` replays). The lab's destination node is a systemd unit on
  fl-c, which runs no k3s.
- **#48's status, corrected** (R6-C3): #48's plan is approved and review-clear (its round 5,
  2026-11-17) but not landed. The grounds are corrected in rounds 4 and 5 above and in the
  design's Deferred list. The deferrals stand: the `RestartKept` scenario waits for #48, which
  owns that action; restore under `--privsep` stands on the intent not needing it and on #74 and
  #48 owning the request and the keep path, neither implemented.
- **Nits:** the ADR proposal says #74's `add` takes no flag from the child, and that under
  `--privsep` with #48 it asks for both recovery flags; the refusal stays unconditional under
  `--privsep`. restore-start answers a resuming entry with #48's own `error: VOLUME is resuming`
  (`restore_start_refuses_kept` gains it). `detach-device` answers `ok absent` for any
  well-formed tuple; a malformed one fails the contract's grammar first. The helper reads
  `restore-list` twice, before its first metadata call and immediately before restore-start;
  in a restore or a `resume`, `restore needs direct mode` after the first metadata call (the
  node restarted under `--privsep` between the reads) exits 1, not 3, the restore left
  Incomplete (`privsep_node_exits_3` gains it); `cancel` still counts it as no attachment.
- **Not applicable:** none. **Deferred, not settled:** none new.

## Plan review round 7 (2026-11-19), changes made after approval

Fixed by the orchestrator (54-r7-both.md: 2 Low, 5 Nits).

- **R7-C1:** main has no SIGTERM handling; #12's S4 adds it. restore-start's SIGTERM refusal and
  `sigterm_waits_for_restore_entry` are marked "with #12, by whichever of #12 and #54 lands
  second". Cross-plan: if #12 lands after #54, its `begin_attach` predicate also guards
  restore-start's `reserve` and `no_attach_after_sigterm` gains a restore-start. The three-plan
  `should_exit` hand-off reads "lands last".
- **R7-C2:** `cleanup_leaves_restore_record` asserts `first_id` above the id only for a record
  left and for a start after the kill with no prior `cleanup`; after `cleanup` deletes a dead
  device and its record, the id may be reused, so that branch asserts only that both are gone.
- **Nits:** the design's retry summary names restore-detach's `is attaching`; "#15's own detach
  wait" reads "the same 30 s as #15's detach wait"; under #48 only a running restore entry holds
  the process (a starting one's device is ended by the scan, consistent with R4-C2); the
  `restore_entry_states` sweep case names its GetVolume precondition; the ADR's `--privsep`
  sentence no longer gives #48's cause for the case without #48.

No rule narrowed, nothing deferred, no risk named. Not applicable: none.
