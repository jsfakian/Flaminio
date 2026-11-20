# Issue #53 — report

## Review rounds

### Plan review round 1 — 525f70c

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: design-code focus (53-r1-code.md), design-security focus (53-r1-security.md).

- **R1-C1 — High:** S7's key-reference graph rests on records #52 (clear) deferred with its S8;
  nothing in this increment can carry it, and the S3 coverage list names key objects that do not
  exist. Evidence: 52/spec.md S8, 52/design.md (retained-key references deferred), 52/contracts.md
  (no key-descriptor kind, `encryption_descriptor` absent); 51/spec.md S6; 50/spec.md, 50/design.md.
- **R1-C2 — Medium:** "extend first" fails on a reused version already locked past E, and on two
  backups extending one version to different dates. Evidence: design (Deduplication), contracts
  `extend_compliance`, S3; compliance refuses shortening.
- **R1-C3 — Medium:** a new finalization attempt under the same operation contradicts #52's "a
  different root for that operation is conflict" and its resume; the plan also renames
  `backup_uuid`. Evidence: design (Store time), contracts (Immutable profile); 52/contracts.md.
- **R1-C4 — Medium:** the initial lock's date, and the descriptor's lock at `repo init`, are
  unspecified. Evidence: design (Store time); intent outcome 8.
- **R1-C5 — Medium:** what `list` checks, and its cost, is open; read literally it is
  O(archives x blocks). Evidence: design (Store time); intent outcome 4; 52/contracts.md (`list`,
  `verified: false`).
- **R1-C6 — Medium:** the probes have no client methods, no canary names and no safe request
  form per action; design and contracts disagree on the canary of DeleteObject. Evidence: design
  (credential checks), contracts (policy boundaries, object-client methods); #52's `ObjectStore`.
- **R1-C7 — Medium:** the owner's policy evidence cannot be checked by the uploader, a locked
  descriptor cannot follow a policy or backend change, and `repo init` lacks the uncertainty and
  evidence flags. Evidence: contracts (Immutable profile, policy boundaries); S1; design (CLI).
- **R1-C8 — Medium:** the lab plan names no command, store, bucket or credential, and #52's
  `backup-store down` wipes the volume file, the early wipe #53 forbids. Evidence: design (Lab);
  intent outcome 10; 52/design.md (Lab).
- **R1-C9 — Medium:** the test plan names no tests, and several rules have none. Evidence: design
  and spec Tests; 52/design.md Tests.
- **R1-C10 — Low:** #73's audit hand-off is not taken, and the intent's "it does not exist yet" is
  drift. Evidence: batch-2026-11-12.md (#73's round-1 cross-plan items); 73/spec.md; intent.
- **R1-C11 — Low:** the descriptor and root fields extend #52's shared bytes without naming #58's
  coordinated review or the schema. Evidence: design (CLI), contracts; 52/contracts.md, 52/design.md.
- **R1-C12 — Low:** resuming with a different `--retain-for` is unspecified. Evidence: contracts
  (Immutable profile); 52/contracts.md (checkpoint).
- **R1-S1 — High:** immutability is qualified only through the S3 API; the store's other
  interfaces, host, disks, clock and identity configuration are outside every rule, and the first
  profile runs on fl-w, a cluster VM holding `admin.key` and `token.key`. Evidence: decisions
  (user's instruction); 52/design.md (Lab); scripts/lab.conf, scripts/lab.sh; contracts
  (Validation profile); design.
- **R1-S2 — Medium:** a forged root becomes the base of later incremental backups, past the
  compromise. Evidence: intent Why 5; design; spec S6; 52/spec.md S1, 52/design.md (base).
- **R1-S3 — Medium:** S5's bound on how far the uploader extends has no mechanism the first
  backend is known to have and no test; `PutObjectLegalHold` is not denied; ARCHITECTURE §14.1 is
  not updated. Evidence: spec S5; intent (Out of scope, Why 5); contracts; docs/ARCHITECTURE.md.
- **R1-S4 — Medium:** the unlocked canary probe can pass a credential able to delete, if a bucket
  default retention or the uploader locks the canary. Evidence: design, contracts (policy
  boundaries).
- **R1-S5 — Medium:** S1's owner evidence has no input, no format and no check, and bucket
  mutation probes have no no-change form. Evidence: spec S1; design; contracts.
- **R1-S6 — Medium:** S7 relies on #52's deferred key-reference graph and names no record, RPC or
  `auth.go` rule. Evidence: spec S7; design; contracts; 52/spec.md S8; 50/design.md.
- **R1-S7 — Medium:** the lab's credentials, bucket creation, canaries, id quoting and clean-up are
  unspecified, and #52's `down` wipes the volume. Evidence: design (Lab); 52/design.md (Lab);
  AGENTS.md.
- **R1-S8 — Low:** which version of the `repository` descriptor is authoritative is unspecified.
  Evidence: 52/contracts.md (descriptor); contracts; intent Why 5.
- **R1-S9 — Low:** #73's audit hand-off is not taken. Evidence: 73/spec.md, 73/intent.md; batch
  notes; intent.
- **Nits (code):** `store_time_uncertainty` lacks `_seconds`; the spec's Open questions and the
  ADR contradict the recorded approval. **Nit (security):** "descriptor" is ambiguous in contracts.

Changes/resolution:
- R1-C1, R1-S6 (one fix): every archive is plain, its only key the repository key (escrowed
  off-cluster, #52's); #52's encrypted refusal kept; the encrypted key-reference graph, its
  records, RPCs, `auth.go` rules and tests, and the parent-snapshot and rotation cases deferred
  with #52's S8 to the plan that lifts #51's refusal (deferred, not settled); key objects cut from
  S3's list. S7 and the invariant narrowed, shown necessary by these findings (decisions.md).
- R1-C2: `cover` reads the exact version first (COMPLIANCE >= E is covered with no request),
  extends otherwise and reads back; a refused extension whose read-back shows E is covered; four
  counts; tests for a reused version past E and concurrent extensions.
- R1-C3: #52's rules kept: `backup_uuid`, one root per operation, a different root a conflict,
  only the completed close after a root is published. `finalization_attempt_uuid` cut. A late root
  ends its operation (exit 1, the administrator's abort close); another attempt is a new grant,
  operation and `backup_uuid`; tests for both.
- R1-C4: initial lock = latest store Date + `min_period_seconds` on every object (the root
  carries E); the descriptor so locked at `repo init`; a test of the exact date.
- R1-C5: `list` checks each root version only (2 requests, `graph_checked: false`); `verify` and
  `restore` the whole graph's retention; bounds tested.
- R1-C6, R1-S4 (one fix): `CanaryProbe` separate from `ObjectStore`; named canaries; an eleven-row
  probe table with each request, safe form and required answer; the unlocked canary's retention
  read first, only AccessDenied accepted; policy denies writes to the canaries; a bucket default
  retention rule refuses setup; the owner renews the locked canary; probes with no safe form cut to
  the evidence; design and contracts agree.
- R1-C7, R1-S5 (one fix): an owner evidence file (`--owner-evidence`) with its fields and the
  worker's checks; its digest recorded, not verified (attestation); a changed digest or backend
  version exits 3 and needs a new repository; `--store-time-uncertainty` added; tests.
- R1-C8, R1-S7 (one fix): a second SeaweedFS instance on fl-w, leaving #52's store and `down`
  unchanged; `make backup-lock-store-up|down`, `make backup-lock-check`, guest and check scripts;
  buckets, both credentials and where kept; a lab-only client example and a `backup-owner` client
  pair; periods; steps in outcome 10's order; ids validated and quoted; `down` refusing while any
  version is locked, no override; a script test.
- R1-C9: tests named per rule, missing rows added.
- R1-C10, R1-S9 (one fix): no metadata mutation, so no #73 event; a `docs/followups.md` row
  "Audit events for immutable repository setup, lock and probe outcomes" (issue to be filed); the
  intent's line reworded.
- R1-C11: fields pending #58's coordinated byte review; immutable descriptor and root
  `schema: 2`, which #52's decoders already refuse.
- R1-C12: P, D, E checkpointed; a rerun with another P exits 2; a test.
- R1-S1: S1's deployment prerequisite; a per-backend validation-profile check of non-S3
  interfaces from a cluster host (SeaweedFS's, as the review describes them, to be verified at
  qualification, not taken as fact); the lab on fl-w proves S3-API enforcement only. Which backend
  profile is qualified first for a deployment stays open (deferred, not settled); SeaweedFS stays
  the first candidate run, per the user's instruction.
- R1-S2: a base must be in the worker's local `completed-roots` record, else a full backup; a
  leaked repository key retires the repository; a test.
- R1-S3: the second option: the worker extends within its bounds; a cap on the credential is the
  owner's, unchecked, as the intent has it (S5 narrowed, shown necessary by this finding); the
  long-lock quota denial named as a risk, not accepted; `PutObjectLegalHold` denied and probed;
  ARCHITECTURE §14.1 in the docs list.
- R1-S8: the oldest authenticating `repository` version is the authority; later ones reported
  and ignored; a root naming another not listed; a test.
- Nits: `store_time_uncertainty_seconds`; the Open questions and ADR lines reworded; `repository`
  descriptor named.

For #52 and #58 (not edited): the immutable descriptor and root fields (`schema: 2`) join #58's
coordinated byte review with #51 and #52.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — 36b64f0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: design-code (`53-r2-code.md`), design-security (`53-r2-security.md`).

- **R2-C1 — Medium:** a run whose cover overruns D still PUTs a root it knows is late, which
  ends the operation and burns its grant; only a rerun re-picked D. Evidence: contracts, One root
  per operation; design.md:74, 85-88 (D chosen after all data arrive, then a cover of nearly
  every version).
- **R2-C2 — Medium:** the lab's `canary/locked` (start + 40 min) fails probe row 4 (R >= store
  time + 1 h), so step 3 cannot pass. Evidence: contracts, Probes row 4; design.md:223.
- **R2-C3 — Medium:** the lock store has only `up|down`, so after fl-w restarts nothing brings it
  back and `down` refuses forever; "no early wipe" is false of `make vm-destroy VM=w`. Evidence:
  design.md:201, 205-207, 250-253; #52 design `backup-store.sh up|down|stop|start`;
  `Makefile:164-171`.
- **R2-C4 — Low:** the `backup-owner` pair lacks what #52 gives its two names under #16's
  `pki.sh` (first key's owner and mode, `file ca.crt`, the sweep pattern, renewal, tests), and
  #52's renewal restarts only #52's store. Evidence: design.md:212-218; #16 design 120-132,
  186-196; #52 design 353-374.
- **R2-C5 — Low:** the second SeaweedFS's non-S3 ports, the example's build and install, and the
  lock repository's file paths are unnamed. Evidence: design.md:204; `scripts/guest/build.sh:3-6`,
  `scripts/lab.sh:219-231`; #52 design 315.
- **R2-C6 — Low:** "no request sent" for `Covered` contradicts the retention read `cover` always
  sends. Evidence: contracts, `cover`; design.md:112-113; `reused_version_already_covered`.
- **R2-C7 — Low:** `object_store_has_no_destructive_call` cannot be a compile-time check: the
  adapter holds an aws-sdk-s3 `Client`. Evidence: design.md:171; #52 contracts, Object client.
- **R2-S1 — Medium:** no backend profile is qualified, so the lab's `repo init --immutable` exits 3
  unless SeaweedFS is claimed qualified; and the locked canary expires too early. Evidence:
  contracts.md:126, 135-140, 170-171; spec.md:159-161; design.md:223, 235-236.
- **R2-S2 — Medium:** a legal hold (or an expired retention) on `canary/unlocked` lets a
  delete-capable credential pass the delete rows; multi-object delete is not probed. Evidence:
  contracts.md:88, 112-113, 123; design.md:171.
- **R2-S3 — Low:** the contracts' descriptor authority falls through to a later authenticating
  version when the oldest does not authenticate; S6 says the oldest. Evidence: contracts.md:29-30;
  spec.md:95.
- **R2-S4 — Low:** one forged far-future root can force full backups for its lock's length, since
  "newest" base is not ordered. Evidence: contracts.md:79-80; #52 design.md:124.
- **R2-S5 — Low:** the non-S3 interface check covers delete only, not overwrite or retention or
  metadata change. Evidence: spec.md:49-50; contracts.md:203-204.
- **R2-S6 — Low:** `down` lists and deletes with the uploader live, ignores legal holds, and does
  not say a refused delete stops it before the wipe. Evidence: design.md:240, 250-253.
- **R2-S7 — Low:** the second instance's master, volume and filer HTTP and gRPC listeners are
  unspecified: they collide with #52's and, on fl-w's address, let fl-a and fl-b act below S3.
  Evidence: design.md:204.
- **R2-S8 — Low:** the backend identity is self-declared in the evidence; an upgrade is detected
  only if the owner edits it. Evidence: contracts.md:167-177.
- **Nit (code):** `list`'s request bound leaves out the descriptor authority's GET. Evidence:
  contracts.md:69-72.
- **Nit (code):** row 7 does not name its retention mode. Evidence: contracts.md:129.
- **Nit (security):** `object_store_has_no_destructive_call` is false for the crate's
  `examples/lock_lab.rs`. Evidence: design.md:171.

Changes/resolution:
- R2-C1: before the root's PUT every run checks the latest store Date plus uncertainty against D;
  past it, a new D and E, checkpointed, and a second cover, once; still past, exit 1 with the grant
  active, the checkpoint kept and no root PUT. The guide sizes the commit budget to the cover time
  (up to 3 requests a version). Test `known_late_root_not_put`; `new_deadline_before_root`
  (spec S4, contracts, design).
- R2-C2 and R2-S1's canary part (one fix): the lab locks `canary/locked` until the check's start
  plus 2 h; row 4's margin unchanged; a later probe fails stale, never passes; `cleanup-after`
  includes the date.
- R2-S1's profile part: a backend-profile table with `qualified` and `candidate` states, none
  qualified; `seaweedfs-<pinned version>-candidate`; a candidate refused (exit 3,
  `profile_not_qualified`) without `--allow-unqualified-profile`; with it every output carries
  `backend_profile` with `qualified: false` and no guarantee; a qualified profile is a new id. S1
  gains this exception, shown necessary by R2-S1; the lab passes the flag (step 7 too). Test
  `candidate_profile_needs_flag`; ADR proposal's consequences note it.
- R2-C3: `backup-lock-store.sh stop|start` (and make targets); `start` reuses the credentials;
  `down` names `start`; the claim reworded (destroying fl-w wipes it, in `docs/lab.md`); script
  tests.
- R2-C4: `backup-owner`'s directory, first key `flaminio-backup-store` 0600, `file ca.crt` only
  into an existing directory, the `.new` sweep pattern, an `identities w` line `backup-owner CA ID
  --client` skipped by `pki issue`; #52's renewal restart also restarts the lock store when up;
  by whichever of #16 and #53 lands second; script tests.
- R2-C5 and R2-S7 (one fix for the ports): `lab.conf` names the S3 port and the six internal
  ports, the internal ones bound to 127.0.0.1, distinct from the defaults #52's instance keeps;
  `up` and `start` refuse a non-loopback internal listener (`ss -Hltn`); script tests. R2-C5's
  rest: `lock_lab` built on fl-w and installed 0755 as `/usr/local/bin/flaminio-lock-lab`; files
  `/var/lib/flaminio-backup/lock.conf`, `lock.key`, `lock.evidence.json`.
- R2-C6: "no extension request (one retention read)"; 1 request a version, 3 when extended;
  counter `covered_without_extension`; the test asserts one GetObjectRetention and no
  PutObjectRetention (spec S3, contracts, design, lab step 6).
- R2-C7 and the security Nit (one fix): a runtime check: the fake records every request the
  library's commands send through `ObjectStore`; none destructive; `CanaryProbe` and the example
  outside it.
- R2-S2: row 1 reads U's retention and hold, requires neither ("or expired" dropped); row 4 also
  requires L's hold OFF; row 12, DeleteObjects naming U, AccessDenied required; tests for the
  held, expired and multi-delete cases; the lab's attempts add the multi-object delete.
- R2-S3: the authority is the oldest version; an unreadable or unauthentic one refuses (exit 3, or
  1 on transport), never the next; the guide keeps it under the owner's cleanup; test
  `descriptor_oldest_unauthentic_refuses` (spec S6, contracts, design).
- R2-S4: the base is the newest compatible `completed-roots` entry by store LastModified;
  `base_not_own` only when none is eligible; `forged_root_never_base` extended.
- R2-S5: the non-S3 interface check requires refusing an unauthenticated delete, overwrite, or
  retention or metadata change of a locked version (spec S1, contracts).
- R2-S6: `down` first removes the uploader's identity and restarts the store; its listing
  includes legal holds, cleared by the owner before the deletes; any refused delete exits 1
  before the store is stopped or anything deleted; script tests.
- R2-S8: an upgrade is detected only through the evidence (the owner's duty, in the guide);
  `backend_profile.source: "evidence"`; a profile's store-reported version, where defined,
  compared (exit 3 on a difference).
- Nits (code): `list`'s bound names the descriptor GET; row 7 names COMPLIANCE.
- Risks named, not accepted: a cover overrunning the budget twice fails its run (R2-C1); a run
  under the flag claims no guarantee, and the flag outside the lab leaves a repository no
  qualified profile can take over (R2-S1); a false evidence file makes the worker apply another
  profile's codes (R2-S8); destroying fl-w wipes the lab's lock store (R2-C3).

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — 5b47237

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security focuses together).

- **R3-B1 — Medium:** Rows 1 and 4 could not pass: on S3 "no retention" (and possibly "no legal
  hold") comes back as an error code, which the classification called uncertain; and "any 404"
  read as absent would also accept `NoSuchKey`/`NoSuchVersion`. Evidence: contracts.md, Probes
  (NotFound and any other code uncertain) and Backend profiles (codes for rows 5 to 7 and 11 only).
- **R3-S1 — Medium:** "Never reports an immutable guarantee" under a candidate was neither defined
  nor tested, and `list`, `verify` and `restore` of a repository whose profile id the build does
  not know were unspecified. Evidence: contracts.md:17, 204-206; spec.md:46-48; design.md:184
  (`candidate_profile_needs_flag` asserts only `qualified` and `source`).
- **R3-C1 — Low:** The pre-PUT deadline re-check's retry is deterministic, the stated remedies
  ("a rerun retries", "retry with explicit bounds") do not exist since the commit budget is fixed
  at `repo init`, and Date+u = D leaves no time for the root's PUT. Evidence: contracts.md:17-18,
  101-106; design.md:104; decisions.md:258.
- **R3-B2 — Low:** `weed server` shares one bind address, so S3 on fl-w's address with the other
  listeners on loopback is not buildable as written; the S3 gRPC port was unnamed; `ss -Hltn`
  cannot tell the store's listeners apart. Evidence: design.md:227-235.
- **R3-B3 — Low:** `down` removes the uploader then restarts with `start`, whose "existing
  credentials" could bring it back; the same for a reboot or #52's renewal restart. Evidence:
  design.md:224-226, 300-304.
- **R3-C2 — Low:** The lab check passed `--allow-unqualified-profile` to `list`, `verify` and
  `restore`, which do not take it (exit 2); step 7 would fail on `lock.key` or the evidence's
  bucket, not the missing lock. Evidence: design.md:279, 292-294, 259-262; contracts.md:186-188,
  219-221.
- **R3-C3 — Low:** `object_store_has_no_destructive_call` could not fail: the fake implements
  `ObjectStore`, which has no destructive method, and the production adapter was never exercised.
  Evidence: design.md:184; contracts.md:113; #52 contracts.md:418-419.

Changes/resolution:
- R3-B1: a profile's codes name the exact absence answers (code and status) for no retention and
  for no legal hold; `retention()` reads them as absent; any other 404 (`NoSuchKey`,
  `NoSuchVersion`) stays uncertain; validation profile and lab step 3 record them; both canary
  tests gain both cases (contracts, design, spec tests).
- R3-S1: outputs carry a top-level `immutable_guarantee`; under a candidate `qualified: false`,
  `immutable_guarantee: false`, `candidate_state` `completed-unqualified` for `completed`, exit
  codes unchanged; the descriptor's `immutable: true` names the mode, not a guarantee; `list`,
  `verify`, `restore` never refuse for the profile and report a candidate or unknown id so; the
  setup commands still refuse an unknown id; `candidate_profile_needs_flag` and
  `unknown_backend_profile_refuses` assert this for all six commands (contracts, design, spec S1).
- R3-C1: the check is Date + u + `put_allowance` <= D with `put_allowance` = 2u; the new D is the
  Date plus the descriptor's commit budget; "retry with explicit bounds" cut; the risk reads
  "fails every run until the repository is recreated with a larger budget or the graph shrinks";
  the guide sizes the budget to cover time plus 3u; `deadline_edges` tests the boundary (contracts,
  design, spec S4).
- R3-B2: one `weed server` bound to 127.0.0.1 (S3 gRPC port named, 18334) and a `socat` forwarder
  on fl-w's address at the S3 port; the check is `sudo ss -Hltnp` filtered by the two recorded
  PIDs; script tests for an unnamed stray port and another PID's listener (design, Lab).
- R3-B3: only `up` writes the identity configuration; `start` uses it as it stands, never
  regenerating it; script test that a removed uploader stays removed across `stop` and `start`.
- R3-C2: the flag only for `repo init`, `repo check`, `backup`; every recorded output asserts
  `qualified: false` and `immutable_guarantee: false`; step 7 uses `nolock.conf`, `nolock.key`,
  `nolock.evidence.json`, the evidence written by the owner steps.
- R3-C3: the runtime test is cut; `make hygiene` gains a word grep over `flaminio-backup/src/`
  (`probe.rs` excepted) for the destructive SDK calls.
- Nits: `multi_delete_allowed_refuses_setup` gains the response naming U in neither list
  (uncertain), in the contracts too; `backup-lock-store up` puts `ca.crt` after issuing
  `backup-owner`. None left.

No rule narrowed. No new risk named; round 2's R2-C1 risk reworded (R3-C1).

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — afb3fc5

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together (incremental,
over `git diff 5b47237..b0f608e -- docs/issues/53/`, against origin/main's Makefile and
docs/ci.md and #52's clear plan).

- **R4-B1 — Low:** the lock store's PID-filtered listener check refuses only a bad listener and
  never requires the expected ones, so it passes with nothing checked when the recorded PID is a
  wrapper's (`sudo -u … weed server &` records sudo's PID) or when it runs before `weed` opens its
  later listeners; the script test with a fake `ss` proves only the filter. Evidence:
  design.md:246-250, :338-341.
- **R4-B2 — Low:** nothing shows a profile's absence answer means absence: the validation profile
  lists the answers but no test tells them apart, so a code the store also returns for
  "unsupported", a held version or a missing one would make row 1 always pass, masking a
  delete-capable uploader. Evidence: design.md:303-306; contracts.md:160-163, :254-256.
- **R4-C1 — Low:** step 7 can exit 1 rather than 3: the uploader policy is applied for
  `flaminio-lock` only (and `nolock.evidence.json` names a policy never applied to its bucket), no
  profile code names GetObjectLockConfiguration's no-lock answer, the exit-3 list and
  `refuses_unlocked_bucket` name no exit for a missing lock, and nothing writes `nolock.conf`.
  Evidence: design.md:188, :278-279, :283-284; contracts.md:196, :217-218, :249-251.
- **R4-S1 — Low:** when `immutable_guarantee` is true is undefined (a qualified `backup` ending in
  a late root, a `list` with incomplete or uncovered archives), and the unknown-id test does not
  assert `candidate_state`. Evidence: contracts.md:223; design.md:188.
- **R4-C2 — Low:** the hygiene grep misses `delete_bucket*`, `put_object_acl`, the public access
  block calls and PutObject's `object_lock_legal_hold_status`, against the rule that `ObjectStore`
  has no such call and only `CanaryProbe` sends one; the Makefile's `hygiene` help and
  docs/ci.md's entry are not updated. Evidence: contracts.md:121, :128; design.md:188;
  Makefile:111; docs/ci.md:23.
- **R4-B3 — Low:** the store's identity configuration holds the owner's secret but has no path,
  owner or mode, `down` does not delete it, and "only `up` writes that configuration" contradicts
  `down`'s edit. Evidence: design.md:227-229, :323-325, :331-332.
- **Nits:** contracts.md:22 "Unknown profile refuses" reads against list/verify/restore never
  refusing an unknown backend id; contracts.md:219-220 "top-level … beside `retention_floor`"
  (`repo init`/`repo check` have no floor, `list`'s is per archive); design.md:236-240 the
  `socat` forwarder has no `max-children` bound.

Changes/resolution:
- R4-B1: `up` and `start` start `weed` and the forwarder each by `exec` under `setpriv` as
  `flaminio-backup-store`, so the recorded PID is the listener's; they wait up to 60 s
  (`BACKUP_LOCK_LISTEN_WAIT` in `lab.conf`) for every named port under those PIDs (`weed`'s
  eight on 127.0.0.1, the forwarder's one on fl-w's address), stop both and refuse if any is
  missing, then apply the off-loopback rule. Script test: a PID with no listeners or only some
  refuses; the PID `setpriv` execs is recorded; `setpriv` faked.
- R4-B2: the validation profile accepts an absence answer only if rows 1 and 4's requests answer
  differently on three versions (none, a retention or hold, a deleted version); no shipped absence
  answer may be NotImplemented, `NoSuchKey`, `NoSuchVersion`, AccessDenied or a 5xx, tested by
  `shipped_absence_answers_distinct` over the profile table.
- R4-C1: the owner steps also apply the uploader policy rendered for `flaminio-nolock`, and
  `nolock.evidence.json` carries that document; a profile's codes name the no-lock answer (exit 3,
  added to the exit-3 list; any other 404, `NoSuchBucket` among them, exit 1), and the validation
  profile checks it; `refuses_unlocked_bucket` asserts both; the check writes `nolock.conf` in step
  7 from `lock.conf` with the bucket, key file and evidence path changed.
- R4-S1: `immutable_guarantee: true` only under a qualified profile and only for a completed,
  covered archive (`backup`'s qualifying root, `verify`'s and `restore`'s archive, per archive in
  `list`); `false` in every failing output, for `list`'s other entries and in `repo init` and
  `repo check`. New test `guarantee_only_for_covered_archive` with an injected qualified profile;
  the unknown-id case asserts `candidate_state` never `completed`; S1 states it.
- R4-C2: the word list gains `delete_bucket[a-z_]*`, `put_object_acl`,
  `[a-z_]*_public_access_block` and `object_lock_legal_hold_status` (the smaller change than an
  allowlist); the Makefile's `hygiene` help and docs/ci.md's `hygiene` entry join the docs the
  design updates.
- R4-B3: the identity configuration is `/var/lib/flaminio-backup-lock-store/s3.json`, beside
  `owner.cred`, outside the volume, `flaminio-backup-store` 0600, created exclusive by `up`; only
  `down` rewrites it (atomic rename) to remove the uploader, and deletes it with the credentials.
  Script tests: `up` refusing an existing one, its owner and mode, no file holding either secret
  after a full `down`.
- Nits: all three fixed (an unknown `immutable_profile_version`; "top-level, and in `list` per
  archive"; `max-children` 32). Nits left: none.
- Rules narrowed: none. Risks named, not accepted: none new.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — 386c947

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (one fresh reviewer, design-code and design-security focuses together).

- **R5-B1 — Low:** `exec` under `setpriv` still recorded a wrapper's PID, and recorded PIDs went
  stale across a restart. Evidence: the guest scripts get root through `sudo`, so `$!` is sudo's
  PID (sudo forks for PAM and `use_pty`), the listener wait then always refuses, and the script
  test fakes both `sudo` and `setpriv`; after `make vm-restart VM=w`, `stop`, `down` and the
  renewal's restart would act on PIDs that may be unrelated processes'; the guest scripts' own
  idiom is `sudo systemd-run --unit` (`node.sh:65`, `ublk.sh:24`, `replica.sh:18`, `probe.sh:68`).
- **R5-S1 — Low:** `list` reported `immutable_guarantee: true` for an archive whose coverage it
  never checked. Evidence: spec.md:51-53 and contracts.md:226-229 allow `true` per archive in
  `list`, while `list` checks each root's retention only and always reports `graph_checked:
  false` (contracts.md:84-88); a holder of the uploader credential and the repository key can PUT
  an authentic root retained past E over page or block versions with short or no retention.
- **R5-C1 — Low:** the worker-side lab files outlived `down`, so a second lab cycle refused.
  Evidence: `down` deleted only the volume file, both credentials and the identity configuration
  (design.md:340-341); `lock.conf`, `lock.key`, `lock.evidence.json`, `nolock.conf`, `nolock.key`
  and `nolock.evidence.json` stayed, and the evidence install, `repo init` and step 7's
  `nolock.conf` write each refuse an existing file (:283-300); the after-`down` test did not
  cover `lock.key`.
- **R5-S2 — Low:** `down`'s atomic rewrite of `s3.json` named no temporary file, so a crash could
  leave a copy of the owner's secret. Evidence: design.md:333-334 ("by atomic rename") and the
  final delete (:340-341) named only `s3.json` and the credentials, against the post-condition
  "no file holding either secret left" (:371-373); a rerun's behaviour over a leftover was
  unspecified.
- **Nit:** design.md:229, "outside the volume" named no mount point.
- **Nit:** contracts.md:91-92, "one with a retention or a hold" lacked "respectively".

Changes/resolution:
- R5-B1: `up` and `start` start `weed` and the forwarder with `sudo systemd-run --quiet
  --unit=flaminio-backup-lock-store` / `--unit=flaminio-backup-lock-fwd`
  `--property=Restart=no --property=User=flaminio-backup-store`, after `systemctl reset-failed`.
  No PID is recorded: each listener check reads each unit's `systemctl show -p MainPID --value`
  afresh (`0` refuses). The store is up when both units are active and runs when either is; `stop`
  (and so `down` and the renewal's restart) is `systemctl stop`; transient units do not survive a
  reboot, so `start` brings the store back after fl-w restarts. Round 4's `setpriv` is gone. The
  script test fakes `systemd-run` and `systemctl` instead of `setpriv`, and checks a MainPID with
  no or only some listeners, a MainPID of `0`, the PID read again at each check, each unit's name
  and user, `start` refusing with either unit active, and `stop` by `systemctl stop`.
- R5-S1: `immutable_guarantee: true` only for an archive whose whole graph that output checked
  (`backup`'s qualifying root, `verify`, `restore`); `list`'s, top-level and per archive, is always
  `false` (`graph_checked: false`), the guide pointing to `verify` (spec S1, contracts' What each
  command checks and Backend profiles, design). `guarantee_only_for_covered_archive` gains the
  forged root (`false` in `list`, refused by `verify`); `list_checks_root_only` asserts `false`.
  S1's "per archive in `list`" is narrowed, shown necessary by this finding (decisions.md).
- R5-C1: after its deletes succeed, `down` also removes, where present, each by name and named in
  its output, the six worker-side files, `lock.key` and `nolock.key` among them; a refused or
  failed `down` keeps them. Script tests: all six kept after a refused delete, none after a full
  `down`.
- R5-S2: `down` writes `/var/lib/flaminio-backup-lock-store/s3.json.new` (exclusive, no-follow,
  `flaminio-backup-store` 0600) and renames it over `s3.json`, removing a leftover one first;
  `start` never reads it and the final delete includes it. Script tests: a leftover removed before
  the exclusive create, none after a full `down`.
- Nits: the mount point `/var/lib/flaminio-backup-lock-store/data` is named and `s3.json` is
  outside it; "a retention or a hold respectively". None left.
- Rules narrowed: S1's `list` per-archive guarantee (R5-S1). Risks named, not accepted: none new.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 6 — 5567116

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R5-B1, R5-S1, R5-C1, R5-S2 and both Nits fixed.

- **R6-B1 — Low:** a `down` failing during its final deletes could not be finished: a rerun
  refused (store down) and the `start` it named refused too, leaving secrets and the files that
  block the next cycle. Evidence: design.md:342, :353-358, :239.
- **R6-C1 — Low:** with only one unit active, `down` named `start`, which refuses because the
  store runs; nothing named `stop`, and no test covered the state. Evidence: design.md:235-239,
  :289, :342.
- **R6-C2 — Low:** contracts.md:228-230 limited `completed`, not only the guarantee, to `backup`,
  `verify` and `restore`, against design.md:90, its test and intent outcome 4.

Changes/resolution (fixed by the orchestrator, the reviewer's fixes): R6-B1, `down`'s final step
is resumable (neither unit active and the volume file gone: skip the owner steps, finish the
deletes, exit 0), with a script test; R6-C1, with one unit active `start` and `down` refuse naming
`stop`, with a script test; R6-C2, the contracts sentence is split. No rule narrowed; no risk
named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 7 — 908cf93

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R6-C1 and R6-C2 fixed; R6-B1 fixed in part (R7-C1).

- **R7-C1 — Low:** the resume step assumed the final step deleted the volume file first, but no
  order or unmount was given; an earlier delete followed by a failed `rm` of the volume file left
  cleanup stuck again. Evidence: design.md:355-358, :386.
- **Nit:** the test of `down` naming `start` with the store down must keep the volume file
  present, or it takes the resume path.

Changes/resolution (fixed by the orchestrator, the reviewer's fix): the final step unmounts and
deletes the volume file first; script tests fail that `rm` and a later one in turn; the Nit's test
keeps the volume file. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 8 — 3c5a930

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R7-C1 fixed in the rule; the Nit fixed.

- **R8-C1 — Low:** the design never said the final step exits 1 at its first failed unmount or
  `rm`; the test claimed a rerun after a failed volume-file `rm` leaves none of the files, though
  that rerun refuses and names `start`; no test covered a failed unmount. Evidence: design.md:356-363,
  :387-389.

Changes/resolution (fixed by the orchestrator, the reviewer's fix): the final step exits 1 at its
first failed unmount or `rm`; a failed unmount or volume-file `rm` leaves the `start` path, a later
failure the resume path; script tests fail each in turn. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 9 — cc8ef2b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Earlier findings: R8-C1 fixed in part (R9-C1).

- **R9-C1 — Low:** after a failed unmount `down` named `start`, but `start` "remounts" a volume
  still mounted, and nothing said that works; the new test could not catch it. Evidence:
  design.md:237, :390-391.

Changes/resolution (fixed by the orchestrator, the reviewer's fix): `start` mounts the volume
unless it is already mounted (`mountpoint -q`); the failed-unmount test asserts `start` mounts
nothing and the following `down` finishes. No rule narrowed; no risk named.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 10 — ae66112

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Reviewed commit 14236dc (diff cc8ef2b..ae66112). R9-C1 fixed. Nothing of Low or worse. One Nit:
the script test's list of fakes left out `mountpoint`, `mount`, `umount` and `rm`
(design.md:370-371); fixed in a461a6b.

Status: clear. Approval remains separate; no approval is inferred.
