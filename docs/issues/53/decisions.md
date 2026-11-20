# Issue #53 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-18) fixes, made after approval: see below.
- Plan review round 2 (2026-11-19) fixes, made after approval: see below.
- Plan review round 3 (2026-11-19) fixes, made after approval: see below.
- Plan review round 4 (2026-11-19) fixes, made after approval: see below.
- Plan review round 5 (2026-11-19) fixes, made after approval: see below.
- Plan review round 6 (2026-11-19) fixes, made after approval: see below.
- Plan review round 7 (2026-11-19) fixes, made after approval: see below.
- Plan review round 8 (2026-11-19) fixes, made after approval: see below.
- Plan review round 9 (2026-11-19) fixes, made after approval: see below.
- Plan review round 10 (2026-11-19): clear; one wording Nit fixed after approval, see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The immutable profile and its IAM enforcement:** as proposed, immutable-v1 is fixed when a
  new repository is set up (versioning, COMPLIANCE mode) and older unlocked archives are never
  relabelled. The uploader may add, list, read and extend retention within bounds, and cannot
  delete an object or version, bypass or shorten a lock, or change the bucket's policy or
  settings; owner credentials stay off the cluster. Enforcement is shown by the owner's applied
  policy and the uploader's negative probes on canaries. [spec.md](spec.md), Open questions;
  [design.md](design.md), New immutable profile and credential checks;
  [adr-proposal.md](adr-proposal.md), Decisions proposed.
- **The requested period against the exact period and finalization margin:** as proposed, the
  requested period is a minimum counted from the qualifying root's store commit time, with a
  bounded extra (commit budget and timestamp uncertainty) disclosed up front and capped by the
  maximum effective period set at setup; a root that lands after its deadline is an incomplete
  candidate and is not listed. This clarifies the intent's period wording.
  [spec.md](spec.md), Open questions; [design.md](design.md), Store time, deadline and root
  qualification; [contracts.md](contracts.md), Immutable profile;
  [adr-proposal.md](adr-proposal.md), Decisions proposed and Options.
- **Qualified server and clock behaviour:** as proposed, retention is proved only by the
  owner-qualified store time (Date, LastModified and their precision), never the worker's clock,
  and native AWS and each S3-compatible backend is a separate profile, accepted only after the
  validation profile's tests. [spec.md](spec.md), Open questions; [design.md](design.md), Store
  time, deadline and root qualification, and CLI, outputs, policy and files;
  [contracts.md](contracts.md), Validation profile.
- **Which backend profile is qualified first:** no recommendation in the plan. Settled 2026-11-13
  (user's instruction): see below.
  [adr-proposal.md](adr-proposal.md), Consequences and open choices; [design.md](design.md), Lab
  and documentation.
- **Retaining deduplicated objects:** the proposed option: extend and check the exact referenced
  versions, with the requests counted, rather than reuploading payloads.
  [adr-proposal.md](adr-proposal.md), Options; [design.md](design.md), Deduplication, stable
  reading and retained keys.
- **The owner's key-reference interfaces:** as proposed, #52's retained key-reference graph gains
  the retention floor, hold status and every root version; ordinary volume or snapshot deletion
  removes no wrapped descriptor or KEK or repository-key reference; there is no GC or
  crypto-erasure; plaintext restore of encrypted volumes stays #50's and #54's and is never
  claimed without their evidence. [spec.md](spec.md), Open questions; [design.md](design.md),
  Deduplication, stable reading and retained keys; [contracts.md](contracts.md), Key/reference
  and output contracts. *Superseded in round 1 (R1-C1, R1-S6): deferred with #52's S8; see
  below.*
- **The immutable-profile fields in #52's descriptor and root:** the proposed extension that
  [contracts.md](contracts.md) fixes, agreed before #52's bytes freeze; old readers refuse an
  unknown immutable profile. [design.md](design.md), CLI, outputs, policy and files. *Changed in
  round 1 (R1-C11): pending #58's coordinated byte review, `schema: 2`; see below.*

## Settled at the user's instruction of 2026-11-13

- **Which backend profile is qualified first:** SeaweedFS, the lab store #52 pins and
  licence-checks, run through the validation profile's tests (contracts.md, Validation profile)
  on a dedicated disposable store; it is accepted only if every test passes, and if one fails it
  is not qualified and the next candidate is chosen then, under the same tests. Recommended by
  Claude, no recommendation in the plan: #52 already pins it for the lab, it needs no external
  account, its documentation names compliance mode, and acceptance stays gated by the tests, so
  the choice weakens nothing.

## Plan review round 1 (2026-11-18), changes made after approval

Reviewed commit 525f70c; findings R1-C1 to R1-C12 (code) and R1-S1 to R1-S9 (security), two Nits
(code) and one (security). Each change below was made after approval, in this round; nothing in
it is implemented, measured or approved anew. Where a finding offered options, the simplest safe
one was taken.

- **Keys: deferred, not settled** (R1-C1, R1-S6, one fix): #52 (clear) refuses encrypted volumes and
  defers its retained-key records with its S8; #51 has no single-snapshot deletion and #50 no
  rotation, so S7's key-reference graph had no mechanism. Every archive here is plain, its only key
  the repository key, escrowed off-cluster (#52). An immutable repository keeps #52's encrypted
  refusal; no GC, no crypto-erasure, no DEK. The graph (floor, holds, every root version), its
  records, RPCs, `auth.go` rules and tests, and the parent-snapshot and rotation cases move to the
  plan that lifts #51's refusal. "Wrapped-key metadata" and "key descriptors" leave S3's coverage
  list. **A rule narrowed, shown necessary by R1-C1 and R1-S6:** S7 (and the invariant's "retained
  key references") cannot hold while no encrypted archive can exist and no record carries the graph;
  it is deferred, not removed. S7's "verify independent repository-key escrow exists" is #52's
  administrator duty that no worker can check, now stated as such. This supersedes the Settled entry
  "The owner's key-reference interfaces" for this increment. [spec.md](spec.md), invariant, S3, S7,
  S8; [design.md](design.md); [contracts.md](contracts.md), Keys and output contracts.
- **Reused versions already covered** (R1-C2): `cover` reads the exact version's retention
  first; COMPLIANCE through E is covered with no request; otherwise it extends and reads back, and
  a refused extension whose read-back shows E is covered (a longer earlier floor, a concurrent
  backup). Four request counts. A production refusal is never a probe result.
- **One root per operation** (R1-C3, R1-C12): #52's rules kept: `backup_uuid` and
  `roots/<backup_uuid>`, a different root for one operation is a conflict, and once a root is
  published the worker sends only its completed close. `finalization_attempt_uuid` is cut. A late
  root ends its operation: the worker exits 1 naming the root and the administrator's abort close;
  another attempt is a new grant, operation and `backup_uuid`. Before the root's PUT a rerun past
  D picks a new D and E. The checkpoint holds P, D, E; a rerun with another P exits 2. No change to
  #52 is needed.
- **Initial lock** (R1-C4): every object carries COMPLIANCE until the latest store Date plus
  `min_period_seconds` (the root carries E); the descriptor is so locked at `repo init`, and each
  backup covers it; no new field.
- **What `list` checks** (R1-C5): `list` checks each root version (2 requests, `graph_checked:
  false`); `verify` and `restore` the whole graph's retention as well. Intent outcome 4 still
  holds: the worker covers and checks every version before the root's PUT.
- **Probes and canaries** (R1-C6, R1-S4, one fix): a `CanaryProbe` type separate from
  `ObjectStore`, which gains no destructive call; canaries `canary/unlocked`, `canary/locked` under
  the prefix (#52's listing ignores them) and an owner `outside_key`; eleven rows, each with its
  request, its safe form and its required answer; the unlocked canary's retention read before the
  delete probes (none or expired, else uncertain), only AccessDenied accepted there; the policy
  denies the uploader writes to the canaries and retention on the unlocked one; a bucket default
  retention rule refuses setup (the simplest way to keep the unlocked canary unlocked); the owner
  keeps `canary/locked` locked by writing a newer locked version; probes with no safe form (bucket
  policy, lifecycle, ACL, bucket deletion, privilege-assuming calls) are cut and rest on the
  evidence. Design and contracts agree: both deletes on the unlocked canary.
- **Owner evidence** (R1-C7, R1-S5, one fix): an owner-written file to `repo init`, `repo check`
  and `backup` (`--owner-evidence`), its fields, the worker's checks against its read-back and the
  probes; its BLAKE3 digest is recorded, not verified: attestation, not proof. A later digest,
  backend profile or version other than the descriptor's exits 3, so a policy revision or backend
  upgrade needs a new repository (the reviewer's simpler option, not an updatable record); old
  repositories stay readable. `--store-time-uncertainty` added.
- **The lab** (R1-C8, R1-S7, one fix): a second SeaweedFS instance on fl-w (the reviewer's first
  option: #52's store and its `down`, which deletes its volume file, stay unchanged and never hold
  a locked version); `make backup-lock-store-up|down`, `make backup-lock-check`, the guest and
  check scripts, buckets `flaminio-lock` and `flaminio-nolock`, the owner's and uploader's
  credentials made in the VM, the owner's never under the worker's paths; a lab-only client
  example and an owner client pair `backup-owner` (#16's `pki.sh` list gains it, by whichever of
  #16 and #53 lands second); minute periods; steps in outcome 10's order; ids validated and quoted;
  `down` refusing, with no override, while any version is locked past the store's Date; a script
  test. The owner's credential is kept on fl-w as the store's user rather than on the workstation
  (the reviewer's suggestion): the store's identity configuration on fl-w holds it anyway, so a
  copy elsewhere would protect nothing, and the lab proves S3-API enforcement only (R1-S1).
- **Tests** (R1-C9): named per rule, the missing rows added (initial lock, cover first, each
  probe's classification, the evidence, durations at overflow, the old reader's refusal).
- **Audit** (R1-C10, R1-S9, one fix): no metadata mutation in this increment, so no #73 event;
  `docs/followups.md` gains "Audit events for immutable repository setup, lock and probe
  outcomes" (issue to be filed); the intent's line now names #73.
- **Shared bytes** (R1-C11): the descriptor and root fields are pending #58's coordinated byte
  review, as #51 and #52 mark theirs; immutable descriptors and roots carry `schema: 2`, which
  #52's decoders already refuse (the simpler option: no reliance on an old reader checking
  `immutable`). The hand-off for #52's and #58's notes is in the round's report entry.
- **Store's reach** (R1-S1): S1's deployment prerequisite (host, disks, clock source, admin
  interfaces and identity configuration outside every cluster identity's reach; never on a
  Flaminio node or control-plane VM), a per-backend validation-profile check of non-S3 interfaces
  from a cluster host, and the lab on fl-w proving S3-API enforcement only. The review's account of
  SeaweedFS's master, volume and filer APIs and `weed shell` is to be verified when the profile is
  qualified, not taken as fact. The user's instruction of 2026-11-13 stands (SeaweedFS is the first
  candidate run), but the lab's run cannot pass the new check, so whether SeaweedFS qualifies, and
  which backend profile is qualified first for a deployment, stay open: deferred, not settled.
- **Forged bases** (R1-S2): a base must be in the worker's local `completed-roots` record (the
  reviewer's first option, needing no RPC), else the backup is full (`base_not_own`); a leaked
  repository key retires the repository.
- **The uploader's extension bound** (R1-S3): the second option, cut before adding: the worker
  extends only within its bounds, and a cap on the credential is the owner's policy, documented
  and checked by nothing, as the intent's Out of scope has it. **A rule narrowed, shown necessary
  by R1-S3:** S5's "extend compliance coverage only within configured bounds", read as a limit
  on the credential, has no mechanism the first backend is known to have; it now binds the worker,
  not the credential. `s3:PutObjectLegalHold` joins the Deny and the probes (row 8); the design's
  docs list updates ARCHITECTURE §14.1.
- **Descriptor authority** (R1-S8): the oldest authenticating version of `repository`; later
  versions reported and ignored; a root naming another is not listed.
- **Nits:** `store_time_uncertainty_seconds`; the spec's Open questions and the ADR's lines that
  contradicted the approval reworded; "descriptor" named the `repository` descriptor. None left.
- **Risks named, not accepted:** a stolen uploader credential can lock objects for as long as the
  store and the owner's policy allow, a quota denial (R1-S3); a store the cluster can reach below
  its S3 API voids the lock (R1-S1); a late root costs its grant and a new attempt that re-reads
  content it did not record (R1-C3); a policy revision or backend upgrade needs a new repository
  and full backups (R1-C7); a lost `completed-roots` file costs full backups (R1-S2); orphans hold
  space for `min_period_seconds` (R1-C4).

## Plan review round 2 (2026-11-19), changes made after approval

Reviewed commit 36b64f0; findings R2-C1 to R2-C7 (code) and R2-S1 to R2-S8 (security), two Nits
(code) and one (security). Each change below was made after approval, in this round; nothing in
it is implemented, measured or approved anew. Where a finding offered options, the simplest safe
one was taken; where the two reports overlap, the root cause is fixed once.

- **A root known to be late is never PUT** (R2-C1): before the root's PUT every run, not only a
  rerun, checks that the latest store Date plus uncertainty is at most D; past it, the run picks a
  new D and E, checkpoints them and covers again, once, and if still past exits 1 with the grant
  active, the checkpoint kept and no root PUT. The guide sizes the commit budget to the graph's
  cover time (up to 3 requests a version). Test `known_late_root_not_put`;
  `resume_new_deadline_before_root` becomes `new_deadline_before_root` (first run and rerun). A
  root slower than the check foresaw still ends its operation as in round 1.
- **The locked canary's lifetime** (R2-C2, R2-S1's second part, one fix): the reviewers' first
  option, cut before adding (no new profile value): the lab locks `canary/locked` until the check's
  start plus 2 h, so row 4's 1 h margin holds for every probe of a check that ends its probes
  within its first hour; a later probe fails as stale (exit 1), never passes. `cleanup-after`
  includes that date, so `down` waits for it. Row 4's margin is unchanged.
- **No qualified profile; the lab runs a candidate** (R2-S1's first part): the reviewer's fix. The
  worker's table of backend profiles has a state, `qualified` or `candidate`; none is qualified in
  this increment; `seaweedfs-<pinned version>-candidate` ships as a candidate with proposed codes.
  `repo init`, `repo check` and `backup` refuse a candidate (exit 3, `profile_not_qualified`)
  unless given `--allow-unqualified-profile`, a test setting only (the guide); with it every JSON
  output, `list`'s, `verify`'s and `restore`'s too, carries `backend_profile` with `qualified:
  false` and no immutable guarantee. A qualified profile is a new id, so a candidate's repository
  is never relabelled. Test `candidate_profile_needs_flag`. **S1 gains an exception, shown
  necessary by R2-S1:** as written, S1 refused every unqualified profile and S9's lab could not
  run at all. The exception claims nothing: the guarantee is never reported under it, and without
  the flag S1's refusal is unchanged. The lab check passes the flag to every worker command; its
  step 7 passes it too, so its exit 3 is the bucket's missing lock.
- **The lab store's restarts** (R2-C3): `backup-lock-store.sh` gains `stop|start` as #52's
  `backup-store.sh` has them (`make backup-lock-store-stop|start`); `start` remounts and restarts
  with the existing credentials; `down` names `start` when the store is down. The claim "no early
  wipe" now reads: the store's own commands have no early wipe; destroying fl-w would wipe it, as
  `docs/lab.md` states. Script-test cases added.
- **`backup-owner` in `pki.sh`** (R2-C4): the five items as #52 gives them for its two: directory
  `/etc/flaminio/backup-owner-pki` `flaminio-backup-store` 0700, first key `flaminio-backup-store`
  0600, `file ca.crt` into it only when it exists, `backup-owner-pki.[0-9a-z]*.new` in the sweep,
  and renewal by an `identities w` line `backup-owner CA ID --client` (the safer of the two
  options: `down` may run long after the check, past the pair's 30-day life), skipped by `pki
  issue`. #52's conditional restart after renewing `backup-store` also restarts the lock store
  when it is up. By whichever of #16 and #53 lands second; #16 and #52 are clear and unchanged.
  Script-test cases added.
- **Listeners** (R2-C5's ports, R2-S7, one fix): `lab.conf` names the S3 port (on fl-w's address)
  and the master, volume and filer ports and their gRPC ports, all six bound to 127.0.0.1, on
  values distinct from SeaweedFS's defaults, which #52's instance keeps (it sets no port); `up` and
  `start` refuse when any listener but S3 is on a non-loopback address (`ss -Hltn`). The port
  values and bind flags are to be checked against the pinned release.
- **The example and the files** (R2-C5's other parts): `backup-lock-store up` builds `lock_lab`
  on fl-w (`guest/build.sh -p flaminio-backup --example lock_lab`) and installs it 0755 as
  `/usr/local/bin/flaminio-lock-lab`; the lock repository's config, key file and evidence are
  `/var/lib/flaminio-backup/lock.conf`, `lock.key` and `lock.evidence.json`.
- **`cover`'s cost** (R2-C6): "no extension request (one retention read)"; a version costs 1
  request, or 3 when extended; the counter is `covered_without_extension`; the test asserts one
  GetObjectRetention and no PutObjectRetention; the lab's per-block count names both.
- **No destructive call** (R2-C7, the security Nit): the reviewer's runtime option, cut before
  adding a compile-fail harness: the serverless fake records every request the library's commands
  send through `ObjectStore`, and none is a delete, multi-delete, bypass, legal hold or bucket
  mutation; `CanaryProbe` and the lab example are outside it.
- **Holds and multi-delete on the canaries** (R2-S2): row 1 reads U's retention and legal hold and
  requires no retention and the hold OFF ("or expired" dropped); row 4 also requires L's hold OFF;
  a new row 12, DeleteObjects naming U, requires AccessDenied for the request or as U's entry in
  Errors (appended, so no row is renumbered). Tests: the held and expired cases,
  `multi_delete_allowed_refuses_setup`, a held L in `locked_canary_stale_fails`. The lab's step 4
  adds the multi-object delete.
- **Descriptor authority** (R2-S3): the oldest version, full stop; one that cannot be read or does
  not authenticate refuses (exit 3, or 1 on a transport error), never the next. The guide says the
  owner's cleanup keeps it while the repository is in use. Test
  `descriptor_oldest_unauthentic_refuses`. This matches S6's wording; the contracts' had drifted.
- **The base's order** (R2-S4): the newest compatible root among `completed-roots` entries, by
  its root version's store LastModified, never `created_at`; `base_not_own` only when none is
  eligible. `forged_root_never_base` extended.
- **Non-S3 interfaces** (R2-S5): the validation profile's check now requires each to refuse an
  unauthenticated delete, overwrite, or retention or metadata change of a locked version (S1,
  contracts). A stronger check, not a narrowing.
- **`down`** (R2-S6): it first removes the uploader's identity from the store's configuration and
  restarts the store, so no uploader request is accepted from then on; its listing includes legal
  holds, which the owner clears before the deletes; any refused or failed delete exits 1 before
  the store is stopped or anything else deleted. `down` is the end of the store's use: a refused
  `down` leaves the store serving the owner alone. Script-test cases added.
- **Backend identity** (R2-S8): the reviewer's fix: an upgrade is detected only through the
  owner's evidence (the owner's duty, in the guide); every output carries `backend_profile` with
  `source: "evidence"`; where a profile defines a store-reported version, the worker compares it
  with the evidence's and exits 3 on a difference.
- **Nits:** `list`'s bound names the descriptor authority's GET; row 7 names COMPLIANCE; the
  security Nit closed by R2-C7's fix. None left.
- **Risks named in round 2, not accepted:** a graph whose cover overruns the commit budget twice
  fails its run, which a rerun retries (R2-C1); a run under `--allow-unqualified-profile` claims no
  guarantee, and the flag used outside the lab leaves a repository no qualified profile can take
  over (R2-S1); a false owner evidence file makes the worker apply another profile's codes and
  claim it, the probes still running (R2-S8); destroying fl-w wipes the lab's lock store (R2-C3).
  Round 1's risks stand; its "a late root costs its grant" (R1-C3) now applies only to a root
  slower than the last check foresaw.

## Plan review round 3 (2026-11-19), changes made after approval

Reviewed commit 5b47237; one combined report (design-code and design-security): R3-B1 and R3-S1
(Medium), R3-C1, R3-B2, R3-B3, R3-C2 and R3-C3 (Low), two Nits. Each change below was made after
approval, in this round; nothing in it is implemented, measured or approved anew. Where a finding
offered options, the simplest safe one was taken.

- **Absence answers** (R3-B1): a backend profile's codes also name the exact answers, code and
  status, for no retention and for no legal hold (rows 1 and 4); `retention()` reads them as
  absent; any other 404, `NoSuchKey` or `NoSuchVersion` among them, stays uncertain (exit 1).
  The validation profile and the lab's step 3 record them for the candidate. Tests:
  `unlocked_canary_locked_is_uncertain` and `locked_canary_stale_fails` gain both cases.
- **What a candidate run reports** (R3-S1): every output carries `backend_profile` and a top-level
  `immutable_guarantee` beside `retention_floor`; under a candidate, `qualified: false`,
  `immutable_guarantee: false` and `candidate_state` `completed-unqualified` wherever it would read
  `completed`; exit codes unchanged (0 on success). The descriptor's `immutable: true` names the
  repository's mode, not a guarantee. `list`, `verify` and `restore` take no flag and never refuse
  for the profile (S8): under a candidate or an id the build does not know they report
  `qualified: false` and `immutable_guarantee: false`, and under an unknown id they apply no
  profile codes. `repo init`, `repo check` and `backup` still refuse an unknown id with or without
  the flag. Tests: `candidate_profile_needs_flag` asserts these fields for all six commands;
  `unknown_backend_profile_refuses` covers the three reading commands. S1 states it. A
  clarification, not a narrowing: the guarantee was already never reported under a candidate.
- **The deadline re-check** (R3-C1): before the root's PUT the check is Date + u + `put_allowance`
  <= D, `put_allowance` = 2u (the reviewer's first example; no new field), so time is left for
  the root's own PUT; the new D is the latest Date plus the descriptor's `commit_budget_seconds`,
  fixed at `repo init`. "Retry with explicit bounds" is cut: no such remedy exists. The guide sizes
  the budget to the cover time plus 3u. Round 2's risk now reads: a graph whose cover time
  exceeds the commit budget fails every run until the repository is recreated with a larger budget
  or the graph shrinks. `deadline_edges` tests the boundary. S4 says so.
- **The lock store's layout** (R3-B2): the reviewer's second option, which keeps a single
  process: one `weed server` with `-ip` and `-ip.bind` 127.0.0.1, so every listener of it,
  the S3 gRPC port (`BACKUP_LOCK_S3_GRPC_PORT`, 18334) among them, is on loopback, and a `socat`
  forwarder on fl-w's address at `BACKUP_LOCK_STORE_PORT` to the S3 gateway on 127.0.0.1 at the
  same port (TLS ends at the store; the signed Host names the gateway's port). Chosen over four
  `weed` processes because their S3 gRPC listener would share S3's non-loopback bind, and the
  check is simpler. The check is `sudo ss -Hltnp` filtered by the two recorded PIDs: any `weed`
  listener off loopback, on a named port or not, or a forwarder listener but S3's, refuses; other
  processes (#52's store) are not the check's. The flags and the forwarder are to be checked
  against the pinned release. Script tests: a stray listener on an unnamed port, another PID's
  listener ignored.
- **`start` and the identity configuration** (R3-B3): only `up` writes the store's identity
  configuration; `start` uses it as it stands and never regenerates it from the credential files,
  refusing when it is missing. So an identity `down` removed stays removed across `stop`, `start`,
  a reboot or #52's renewal restart. Script test added.
- **Two lab steps** (R3-C2): the flag goes only to `repo init`, `repo check` and `backup`; every
  recorded output must say `qualified: false` and `immutable_guarantee: false`. Step 7 has its own
  `nolock.conf`, `nolock.key` and `nolock.evidence.json`, the evidence written by the owner steps
  for `flaminio-nolock` from the same template, so the read-back of the missing lock refuses.
  This corrects round 2's "passes the flag to every worker command".
- **No destructive call** (R3-C3): `object_store_has_no_destructive_call` is cut (it could not
  fail: the fake implements `ObjectStore`, which has no such method). `make hygiene` gains the
  reviewer's check: a word grep over `flaminio-backup/src/`, `probe.rs` excepted, failing on
  `delete_object`, `delete_objects`, `put_object_legal_hold`, `put_bucket_*`,
  `put_object_lock_configuration` or `bypass_governance_retention`. Chosen over an HTTP S3 fake as
  the smaller change.
- **Nits:** `multi_delete_allowed_refuses_setup` gains a response naming U in neither Deleted nor
  Errors (uncertain, exit 1), and the contracts say so; `backup-lock-store up` puts `ca.crt` after
  issuing `backup-owner`, as #52's `up` does. None left.
- **Rules narrowed:** none.
- **Risks named in round 3, not accepted:** none new. Round 2's R2-C1 risk is reworded as above
  (R3-C1). Whether `weed server`'s flags and the forwarder behave as stated is to be checked
  against the pinned release (R3-B2), as round 2 left the bind flags.

## Plan review round 4 (2026-11-19), changes made after approval

Reviewed commit afb3fc5; one combined report (design-code and design-security): R4-B1, R4-B2,
R4-C1, R4-S1, R4-C2 and R4-B3 (Low), three Nits. Each change below was made after approval, in
this round; nothing in it is implemented, measured or approved anew. Each takes the reviewer's
recommended fix; where it offered options, the simpler one was taken.

- **The lock store's listeners must be present** (R4-B1): `up` and `start` start `weed` and the
  forwarder each by `exec` under `setpriv` as `flaminio-backup-store`, so the recorded PID is the
  listening process's, not sudo's (chosen over `runuser`, which can stay the parent for its PAM
  session, and over reading the PID from the port). They then wait up to 60 s (`BACKUP_LOCK_LISTEN_WAIT`) until
  every named port listens under those PIDs, `weed`'s eight on 127.0.0.1 and the forwarder's one
  on fl-w's address, and stop both and refuse if any is missing; the off-loopback rule follows.
  Script test: a recorded PID with no listeners, or only some, refuses; `setpriv` faked.
- **Absence answers told apart** (R4-B2): the validation profile accepts an absence answer only if
  rows 1 and 4's requests answer differently on three versions (no retention or hold, a retention
  or a hold, a deleted version). No shipped absence answer may be NotImplemented, `NoSuchKey`,
  `NoSuchVersion`, AccessDenied or a 5xx; test `shipped_absence_answers_distinct` over the table.
- **Step 7 exits 3** (R4-C1): the owner steps apply the uploader policy rendered for
  `flaminio-nolock` too, and `nolock.evidence.json`'s `uploader_policy` is that document. A
  profile's codes name GetObjectLockConfiguration's answer for no lock configuration, which exits
  3 (the bucket's missing lock, now in the exit-3 list); any other 404, `NoSuchBucket` among them,
  exits 1. `refuses_unlocked_bucket` asserts both. The check writes `nolock.conf` in step 7 from
  `lock.conf`, with the bucket, key file and evidence path changed (`flaminio-backup` 0600,
  refusing an existing one).
- **When `immutable_guarantee` is true** (R4-S1): only under a qualified profile and only for a
  completed, covered archive: `backup`'s qualifying root, the archive `verify` or `restore`
  checked (exit 0), and in `list` per archive (a `completed` entry whose root passed `list`'s
  checks; `graph_checked` says which). It is `false` in every failing output, for `list`'s other
  entries, and in `repo init` and `repo check`, which cover no archive. Test
  `guarantee_only_for_covered_archive` with a qualified profile the test injects (never in the
  shipped table); `unknown_backend_profile_refuses` also asserts `candidate_state` never
  `completed`. S1 states it. A clarification, not a narrowing: round 3 already said only a
  qualified profile's outputs may say `true`; no output reported `true` where this now forbids it.
- **The hygiene grep** (R4-C2): the word list gains `delete_bucket[a-z_]*`, `put_object_acl`,
  `[a-z_]*_public_access_block` and `object_lock_legal_hold_status` (chosen over an allowlist of
  aws-sdk-s3 operations, the smaller change). The Makefile's `hygiene` help and `docs/ci.md`'s
  `hygiene` entry join the documentation the design updates.
- **The store's identity configuration** (R4-B3): `/var/lib/flaminio-backup-lock-store/s3.json`,
  beside `owner.cred` and outside the volume, `flaminio-backup-store` 0600 (the file name `weed`'s
  S3 configuration flag takes, to be checked against the pinned release). `up` creates it
  exclusive, refusing an existing one; only `down` rewrites it (atomic rename), to remove the
  uploader, and `down` deletes it with the credentials. This replaces round 3's "only `up` writes
  that configuration", which contradicted `down`. Script tests: `up` refusing an existing file and
  its owner and mode; no file holding either secret left after a full `down`.
- **Nits:** contracts' "Unknown profile refuses" now names an unknown `immutable_profile_version`;
  "a top-level `immutable_guarantee` beside `retention_floor`" now reads top-level, per archive in
  `list` (with R4-S1); the `socat` forwarder forks at most 32 children (`max-children`). None left.
- **Rules narrowed:** none.
- **Risks named in round 4, not accepted:** none new. The 60 s listener wait and the 32-child bound
  are lab values, to be checked with the pinned release as the bind flags are (R3-B2).

## Plan review round 5 (2026-11-19), changes made after approval

Reviewed commit 386c947; one combined report (design-code and design-security): R5-B1, R5-S1,
R5-C1 and R5-S2 (Low), two Nits. Each change below was made after approval, in this round;
nothing in it is implemented, measured or approved anew. Each takes the reviewer's recommended
fix; where it offered options, the simpler one was taken.

- **The lock store runs as transient systemd units** (R5-B1): `up` and `start` start `weed` and
  the forwarder as the guest scripts start their daemons (`ublk.sh`, `replica.sh`, `node.sh` on
  origin/main): `sudo systemd-run --quiet --unit=flaminio-backup-lock-store` and
  `--unit=flaminio-backup-lock-fwd`, `--property=Restart=no --property=User=flaminio-backup-store`,
  after `systemctl reset-failed`. No PID is recorded: each listener check reads each unit's
  `systemctl show -p MainPID --value` afresh (`0` refuses). The store is up when both units are
  active and runs when either is; `stop` (and so `down` and the renewal's restart) is `systemctl
  stop`. Transient units do not survive a reboot, so after `make vm-restart VM=w` no stale PID can
  be hit and `start` brings the store back. This supersedes round 4's `exec` under `setpriv`
  (R4-B1), which ran under `sudo` and so recorded sudo's PID. Script test: fake `systemd-run` and
  `systemctl` replace `setpriv`; a MainPID with no or only some listeners, and `0`, refuse; the PID
  filtered is the one the fake returns at that check.
- **`list` never says `immutable_guarantee: true`** (R5-S1): `true` only for an archive whose whole
  graph that output checked: `backup`'s qualifying root, `verify` and `restore` (exit 0). `list`
  checks each root only (`graph_checked: false`), so its `immutable_guarantee`, top-level and per
  archive, is always `false`, and the guide points to `verify` (the reviewer's first option, which
  keeps the field and adds no `root_retained`). `guarantee_only_for_covered_archive` gains the
  forged root (authentic, retained past E, over a block version short of E: `false` in `list`,
  refused by `verify`); `list_checks_root_only` asserts `false`. **A rule narrowed, shown
  necessary by R5-S1:** S1's "per archive in `list`" could not hold as stated, since `list` never
  sees the coverage the rule requires; `list`'s per-archive guarantee is now always `false`. The
  guarantee itself is unchanged: `verify` and `restore` still report it.
- **`down` removes the check's worker-side files** (R5-C1): after its deletes succeed, `down` also
  removes, where present, each by name and named in its output, `lock.conf`, `lock.key`,
  `lock.evidence.json`, `nolock.conf`, `nolock.key` and `nolock.evidence.json` from
  `/var/lib/flaminio-backup` (the reviewer's first option; the two keys' removal is explicit, never
  by a pattern), so a second `up` and check do not refuse. A refused or failed `down` keeps them.
  The after-`down` script test asserts none remains, and the refused-delete test that all remain.
- **`down`'s rewrite names its temporary file** (R5-S2):
  `/var/lib/flaminio-backup-lock-store/s3.json.new`, created exclusive and no-follow,
  `flaminio-backup-store` 0600, then renamed over `s3.json`; a leftover one (an interrupted
  `down`'s) is removed first, `start` never reads it, and the final delete includes it. Script
  tests: a leftover removed before the exclusive create, and none after a full `down`.
- **Nits:** the volume's mount point is named, `/var/lib/flaminio-backup-lock-store/data`, and
  `s3.json` is outside it; the validation profile's second version has "a retention or a hold
  respectively". None left.
- **Rules narrowed:** S1's `list` per-archive guarantee (R5-S1), above.
- **Risks named in round 5, not accepted:** none new.

## Changes after approval — plan review round 6 (2026-11-19)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R6-B1 (Low): `down`'s final step is resumable.** When neither unit is active and the volume
  file is gone, `down` skips the owner steps and finishes the remaining deletes, each where
  present, then exits 0. Script test: a fake `rm` fails in the final step, and a rerun leaves none
  of the files.
- **R6-C1 (Low): one unit active.** With exactly one unit active, `start` and `down` refuse naming
  `make backup-lock-store-stop` first. Script test with only the forwarder's unit active.
- **R6-C2 (Low): `completed` in `list`.** contracts.md's sentence is split: `candidate_state:
  completed` appears only under a qualified profile, for a completed archive (in `list`, an entry
  whose root passed `list`'s checks, with `immutable_guarantee: false`); `immutable_guarantee:
  true` only in `backup`, `verify` and `restore`, as before.

## Changes after approval — plan review round 7 (2026-11-19)

Fixed by the orchestrator, taking the reviewer's fix. No rule narrowed; no risk named.

- **R7-C1 (Low): the final step's order.** After stopping both units, `down` unmounts the volume
  and deletes its volume file first, then the credentials, the identity configuration, any
  `s3.json.new` and the worker-side files. A failed `rm` of the volume file leaves every other file
  and the rerun names `start`; a failure after it leaves the resume state. Script tests fail each
  in turn.
- **Nit (easy, fixed):** the test of `down` naming `start` with the store down keeps the volume
  file present, so it does not take the resume path.

## Changes after approval — plan review round 8 (2026-11-19)

Fixed by the orchestrator, taking the reviewer's fix. No rule narrowed; no risk named.

- **R8-C1 (Low): the final step's failures.** The final step exits 1 at its first failed unmount
  or `rm`, naming it. A failed unmount or volume-file `rm` leaves every other file, so a rerun
  names `start`, and `start` then a full `down` finish; a later failure leaves the resume state.
  Script tests: the unmount, the volume-file `rm` and `owner.cred`'s `rm` each fail in turn, with
  the outcomes above.

## Changes after approval — plan review round 9 (2026-11-19)

Fixed by the orchestrator, taking the reviewer's fix. No rule narrowed; no risk named.

- **R9-C1 (Low): `start` after a failed unmount.** `start` mounts the volume unless it is already
  mounted there (`mountpoint -q`), so the `start` that `down` names after a failed unmount works;
  the script test's failed-unmount case asserts that `start` mounts nothing and the following
  `down` finishes.

## Changes after approval — plan review round 10 (2026-11-19)

Nothing of Low or worse; the plan clears. The Nit, fixed by the orchestrator: the script test's
list of fakes names `mount`, `umount`, `mountpoint` and `rm`, which its cases use.
