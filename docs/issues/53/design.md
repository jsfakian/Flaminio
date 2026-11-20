# Issue #53: Immutable backups — design

[Spec](spec.md), S1–S9; exact [retention contracts](contracts.md); [Proposed ADR](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review rounds 1 to 5
(decisions.md).
Manual locked-backup/uploader increment only; full #53 remains open.

The facts it starts from:
- There is no backup/Object Lock code in this checkout. #52's new draft contracts define a
  Rust worker/ObjectStore adapter and exact-version encrypted self-contained archive graph;
  they are dependencies, not implemented APIs. Its immutable flag is setup-fixed.
- [S3 Object Lock](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html)
  protects a particular version, not its key/current-version selection. New versions/delete
  markers can still exist. Retention must cover reused data/page/root/descriptor versions.
- [Retention inspection](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock-managing.html)
  requires explicit rights; governance bypass is not compliance. A reported flag does not prove
  real enforcement. Object-store identity/policy is distinct from Flaminio metadata mTLS.
- Audit Proposed ADR0009 requires retained keys/references, including after live PVC deletion;
  ciphertext-only readability is not evidence of encrypted plaintext recovery. #50/#54 own that
  runtime; this scope does not implement a KMS, key rotation or post-retention collection. #52
  (clear) refuses encrypted volumes and defers its retained-key records with its S8, so every
  archive here is plain and S7's key-reference graph waits with it (round 1, R1-C1, R1-S6).
- The store's non-S3 paths, host and clock are outside what its S3 API enforces: the lock holds
  against the cluster only if the cluster cannot reach them (S1's deployment prerequisite, R1-S1).

## New immutable profile and credential checks (S1, S5)

Extend #52 repository initialization with immutable-v1. New descriptor fixes versioning,
COMPLIANCE, requested-period minimum, maximum effective coverage, commit budget/profile and
qualified store/policy evidence. Reject existing nonimmutable repositories, suspended versioning,
missing lock APIs, governance mode, wrong expected account/bucket/prefix, unknown enforcement or
incompatible #52 durable/version/TLS requirements. Do not rewrite old history into “immutable”.

Owner creates/configures the bucket and applies the shipped policy; no bucket/identity creation
is done by worker. One-off owner bootstrap writes the canaries (`canary/unlocked`,
`canary/locked`, an `outside_key`; contracts, Probes) and the owner evidence file (contracts,
Owner evidence), which `repo init`, `repo check` and `backup` take; the worker records its digest
and checks the bucket's versioning and lock configuration (no default retention rule) against it.
Owner credential stays off-cluster, and the store itself meets S1's deployment prerequisite
(R1-S1): its host, disks, clock source, admin interfaces and identity configuration out of every
cluster identity's reach, never on a Flaminio node or control-plane VM. Uploader checks only that
target and its capabilities, never discovers/administers other buckets. The ordinary, version
and multi-object deletes probe the unlocked canary, whose retention and legal hold are read first
and must both be absent: a lock or hold would refuse the delete in the policy's place, and testing a
locked version alone cannot prove lack of delete permission (R1-S4, R2-S2).

`CanaryProbe`, separate from `ObjectStore`, sends the contracts' twelve probe requests (the
multi-object DeleteObjects among them, R2-S2), each in a form that changes nothing but its canary
if allowed; probes with no such form (bucket policy, lifecycle, ACL, bucket deletion,
privilege-assuming calls) rest on the evidence alone (R1-C6, R1-S5). Only AccessDenied (or the
profile's retention refusal on the shortening rows) is a denial; transport/unsupported/not-found
and a canary in the wrong state are uncertain and fail, not “secure”. No backend profile is
qualified yet: a candidate profile (SeaweedFS's) is refused unless `--allow-unqualified-profile`
is given, and then every output says `qualified: false` and `immutable_guarantee: false`; `list`,
`verify` and `restore` never refuse for the profile; the backend's identity is the owner's
attestation (contracts, Backend profiles; R2-S1, R2-S8, R3-S1). Recheck profile, evidence digest
and every probe before each backup. Finite probes do not prove arbitrary
IAM/future-policy behavior: the owner's evidence is attestation, not proof, and the plan states
that boundary. No production history is used as a permission probe.

Ship explicit Allow for list/get exact versions/put/get retention/put retention within namespace,
plus policy Deny for object/version deletion, bypass, legal holds, bucket/version/lock/policy
changes and privilege escalation, and for writes to the canaries. A cap on how far the uploader can
extend is the owner's, in the bucket's policy, documented and checked by nothing (S5, R1-S3);
never infer a policy from client intent. Policy/config examples and action mapping are in
contracts.md. No collector/owner/assume-role credential is mounted by the worker. Object Lock is
not set/checked by metadata Authorize; SDK requests retain #52 TLS peer verification/SigV4 and
universal per-attempt hook.

## Store time, deadline and root qualification (S2–S4)

Exact period-from-last-network-check is not an atomic S3 operation. Proposed interpretation is a
requested MINIMUM protection after the qualifying root's store commit time, with bounded extra
coverage disclosed up front. This is an explicit open contract against the intent's period wording.
Use owner-qualified store Date/LastModified precision, never worker wall clock as retention
proof. Setup defines MinPeriod, MaxEffectivePeriod, CommitBudget and timestamp uncertainty.
Accept P only when P>=MinPeriod and P+CommitBudget+uncertainty<=MaxEffectivePeriod.
Settled at approval, 2026-11-13: the requested-minimum interpretation (see decisions.md, Settled at approval).

After all data arrive, choose a store-time commit deadline D; set retention floor E=D+P, and
checkpoint P, D and E with the operation (R1-C12). Cover every referenced exact version in
COMPLIANCE through E (`cover`: read first, extend only if needed). Before the root's PUT, check
that the latest store Date plus uncertainty plus the PUT's allowance (twice the uncertainty) is at
most D; if not, pick a new D (that Date plus the descriptor's commit budget) and E, checkpoint
them and cover again, once, and if the check fails again, exit 1 with the grant active and no root
PUT (R2-C1, R3-C1), so a root known to be late is never PUT. Then upload the immutable root with
that same floor, deadline, requested period and completion profile. Inspect root's exact version:
store commit timestamp T plus uncertainty must be <=D, and all exact versions/root must report
retain-until>=E. Thus E>=T+P without predicting network completion time. Root accepted late is an
incomplete candidate and cannot list as a completed immutable archive.

Finalize only after these checks; list/verify/restore repeat the qualification from exact
root/profile/version metadata, not a mutable completion pointer: `list` checks the root version
only (2 requests each, `graph_checked: false`, so `immutable_guarantee: false`, R5-S1), `verify`
and `restore` the whole graph's retention too (R1-C5, contracts). Resolve uncertain root PUT by
exact operation/root before any retry. One
operation has one root under its `backup_uuid`, as #52 (R1-C3): a late root ends the operation
incomplete (exit 1, the administrator's abort close), and another attempt is a new grant and
operation. Every run, first or rerun, makes that check before the root's PUT; a rerun with
another P exits 2. Offline listing treats each authenticated exact root version as its own identity.

Blocks, pages and the `repository` descriptor carry an initial compliance lock when uploaded:
store Date plus `min_period_seconds` (R1-C4, contracts); the root carries E; finalization extends
as needed. Existing archived versions are never weakened. A failure leaves locked/orphaned
objects and pins as #52 requires; no cleanup DELETE is attempted.
No generic “ignore retention check” or local-time fallback exists, nor an unreported extension.
A graph whose cover time exceeds the commit budget fixed at `repo init` fails every run until the
repository is recreated with a larger budget or the graph shrinks. The guide sizes the commit
budget to the graph's cover time, up to 3 requests a version, plus 3 × uncertainty (R2-C1, R3-C1).

## Deduplication, stable reading and retained keys (S3, S6–S8)

Extend ObjectStore with typed bucket_profile, retention(version), cover(version,date),
put_if_absent_locked and trusted store-time/LastModified metadata (contracts); it gains no
delete, shortening, bypass or bucket call. Every call names version ID and expected account.
Batch at most the bounded #52 request concurrency; count checks/extensions and expose O(number of
referenced versions) work. #59 hook applies to these control calls too.
Never HEAD/extend the newest name and assume it is the version a manifest references.

For an unchanged block/page already authenticated in a complete immutable base, exact-version
immutability plus successful retained-coverage check permits reuse without payload GET/PUT.
Unknown/orphan objects still need #52's content proof; a durable upload checksum checkpoint can
prove the exact retained version only with the qualified server's checksum semantics. Do not
silently use HEAD/ETag as BLAKE3/AEAD validation. Cover first (R1-C2): a version already in
COMPLIANCE through E (reused from a longer earlier floor, or covered by a concurrent backup) needs
no extension request, only its one retention read, and a refused extension whose read-back shows
E counts as covered (1 request a version, or 3 when extended, R2-C6); failed/missing/
deleted versions cannot enter a new manifest. New/changed data still follow #52 authenticated
upload/readback.

Catalog lists ALL root versions, carries trusted store commit time and retain-until, and returns
#52's (repo UUID, backup UUID, version ID). Newer forged versions are distinct entries and never
replace older locked history. Manifest AEAD/hash validates content under #52; store time, not
created_at, separates post-compromise writes. A holder of uploader credentials plus repository key
can create later valid-looking archives and consume quota; this does not authenticate their
history. Such a root never becomes a base: in an immutable repository the base is the newest,
by store LastModified, of the compatible roots in the worker's local `completed-roots` record of
its own completed outcomes, else the backup is full (R1-S2, R2-S4); a leaked repository key
retires the repository. Configuration comes from the `repository` descriptor's oldest version
only, and a command refuses when that version does not authenticate, never taking the next
(R1-S8, R2-S3). Old archives remain exact-version
addressable after new versions or delete markers.

Keys (S7, narrowed in round 1, R1-C1, R1-S6): every archive is plain, its only key the repository
key, escrowed off-cluster by the administrator (#52). An immutable repository keeps #52's refusal
of encrypted volumes. This increment performs no object/key GC/crypto-erasure, does not manage
legal holds, and adds no metadata record, RPC or `auth.go` rule. The encrypted key-reference graph
(floor, holds, every root version), its records and tests, and the parent-snapshot and rotation
cases are deferred with #52's S8 to the plan that lifts #51's refusal (deferred, not settled).

## CLI, outputs, policy and files

Extend `flaminio-backup repo init --immutable --min-period D --max-effective-period D
--commit-budget D --store-time-uncertainty D --owner-evidence FILE`, `repo check --owner-evidence
FILE`, and `backup ... --retain-for D`, the three also taking `--allow-unqualified-profile`
(R2-S1); existing #52 config/key files, operation/identity safety and nonimmutable commands
remain unchanged. Durations take a suffix and are validated, checked and stored as seconds
(contracts); backend policy granularity must support the requested bounds. Add JSON
immutable_profile, store_commit_at, retention_floor, requested_minimum_seconds,
extra_coverage_bound_seconds, candidate_state, uploader_check, retention_request_counts,
graph_checked, descriptor_versions_ignored, backend_profile and immutable_guarantee (R2-S1,
R2-S8, R3-S1). Never label a client timestamp as store time or print credentials/keys/owner policy
contents.

Files: #52's Rust backup/repository/client/CLI modules/tests; new `retention.rs` (`cover`, the
deadline, the initial lock), `probe.rs` (`CanaryProbe`), `evidence.rs` (the owner evidence's
strict decoder) and typed policy/qualification fixtures; immutable setup/uploader IAM templates
under deploy/backup; exact-profile integration tests, fuzz cases, scripts/checks, Makefile and
docs. No data-plane write/fencing/replication token/on-disk format change. No metadata record,
RPC or `auth.go` rule is added (S7 deferred), so no etcd transaction either. Do not add
crypto-erasure or key-release rights to backup workers.

#52 v1 descriptor/root immutable-profile fields are an explicitly proposed extension, pending
#58's coordinated byte review as #51 and #52 mark theirs (R1-C11): the immutable descriptor and
root carry `schema: 2`, which #52's decoders already refuse, so an old reader refuses an immutable
repository rather than ignore its profile.
The appendix fixes version-scoped retention APIs, deadline proof, policy actions and output.
SDK request bodies/condition keys and server clock precision must be qualified for the pinned
backend version; native AWS and each S3-compatible backend are distinct supported profiles.
Settled at approval, 2026-11-13: the proposed extension of #52's fields (see decisions.md, Settled at approval).

## Tests

Named per rule, as #52's (R1-C9); each can fail without its change. Files are under
`dataplane/crates/flaminio-backup/tests/` unless named otherwise.

| Rules | Tests |
|---|---|
| S1, S5 | `immutable_setup.rs`: `refuses_unlocked_bucket` (no versioning, suspended, no Object Lock, a default retention rule, governance; the profile's answer for no lock configuration exits 3 and `NoSuchBucket` exits 1; R4-C1), `refuses_existing_repository`, `evidence_required` (missing, malformed, over 64 KiB, unknown field, bucket or prefix not the config's), `evidence_digest_recorded`, `evidence_changed_refuses` (another digest, backend profile or version exits 3), `unknown_backend_profile_refuses` (`repo init`, `repo check` and `backup`, with and without the flag; `list`, `verify` and `restore` of a repository whose id this build does not know exit 0, not refused, with `qualified: false`, `immutable_guarantee: false` and `candidate_state` never `completed`; R3-S1, R4-S1), `candidate_profile_needs_flag` (a candidate refused without `--allow-unqualified-profile`, exit 3; with it, the output of each of `repo init`, `repo check`, `backup`, `list`, `verify` and `restore` carries `backend_profile` with `qualified: false` and `source: "evidence"`, `immutable_guarantee: false` and, where it has one, `candidate_state` `completed-unqualified`, never `completed`, exit 0; a store-reported version other than the evidence's exits 3; R2-S1, R2-S8, R3-S1), `guarantee_only_for_covered_archive` (with a qualified profile the test injects, never in the shipped table: `immutable_guarantee: true` for `backup`'s qualifying root, `verify` and `restore` of a covered archive; `false` for `repo init` and `repo check`, for a late root's exit 1, for a `verify` of a block version short of E, and in `list` top-level and for every entry, a `completed` entry whose root passed among them; a forged root, authentic and retained past E over a block version short of E, `false` in `list` and refused by `verify`; R4-S1, R5-S1), `uncertainty_below_precision_refuses`. `probes.rs`: `probe_requests_exact` (each row's request, key, version and headers), `shipped_absence_answers_distinct` (over the shipped profile table: no absence answer is NotImplemented, `NoSuchKey`, `NoSuchVersion`, AccessDenied or a 5xx; R4-B2), `delete_marker_allowed_refuses_setup`, `delete_version_allowed_refuses_setup`, `uncertain_probe_fails` (transport error, timeout, 5xx, NotFound, NotImplemented, another code), `unlocked_canary_locked_is_uncertain` (locked by a bucket default and by the uploader, an expired retention, a legal hold set by another principal; R2-S2; the profile's absence answers for no retention and no hold pass, and `NoSuchKey` or `NoSuchVersion` in their place is uncertain, exit 1; R3-B1), `locked_canary_stale_fails` (stale, or held; the profile's absence answer for no hold passes, `NoSuchKey` or `NoSuchVersion` in its place is uncertain; R3-B1), `multi_delete_allowed_refuses_setup` (U among DeleteObjects' Deleted exits 3; AccessDenied for the request or as U's entry in Errors accepted; a response naming U in neither is uncertain, exit 1), `shorten_mode_bypass_allowed_refuses`, `legal_hold_allowed_refuses`, `wrong_prefix_412_refuses`, `bucket_noop_allowed_refuses`. `make hygiene` gains the adapter check (R2-C7, R3-C3; it replaces a runtime test through `ObjectStore`, which could not fail): a word grep over `dataplane/crates/flaminio-backup/src/`, `probe.rs` excepted (`examples/` is outside it), that fails on any of `delete_object`, `delete_objects`, `delete_bucket[a-z_]*`, `put_object_legal_hold`, `object_lock_legal_hold_status` (a legal hold sent on PutObject), `put_object_acl`, `put_bucket_[a-z_]*`, `[a-z_]*_public_access_block`, `put_object_lock_configuration` or `bypass_governance_retention` (R4-C2). The Makefile's `hygiene` help and the `hygiene` entry of `docs/ci.md` name the check. |
| S2 | `period.rs`: `duration_parse` (each suffix; no suffix, zero, sign, other unit refused), `period_bounds` (P below min, P+budget+uncertainty above max, each sum and D+P at u64 overflow, exit 2). |
| S2, S4 | `retention.rs`: `deadline_edges` (T+u equal to D accepted, one second later refused, precision rounding; before the root's PUT, Date+u+2u equal to D sends the PUT and one second later covers again with a new D; R3-C1), `late_root_hidden`, `late_root_ends_operation` (exit 1 naming the abort close; a rerun resolves the same root), `second_root_same_operation_conflict`, `local_clock_ignored`, `initial_lock_date` (an interrupted run's objects carry S + min exactly; a stale S retried once), `descriptor_locked_at_init`, `new_deadline_before_root` (a first run and a rerun past D each pick a new D and E and cover again), `known_late_root_not_put` (the store's Date still past D after that cover: exit 1, the grant active, the checkpoint kept, no root PUT; R2-C1), `resume_other_period_exit2`. |
| S3 | `retention.rs`: `reused_version_already_covered` (exactly one GetObjectRetention and no PutObjectRetention, R2-C6), `extends_exact_not_current`, `concurrent_extensions_differ` (two backups extending one version to different dates both complete), `refused_extension_readback_covered`, `refused_extension_short_fails`, `no_unchanged_payload_io`, `request_counts` (each of the four counts; 1 request a covered version, 3 an extended one). |
| S4 | `failure_matrix.rs` (#52's) gains `immutable_fault_each_step` (a fault before and after each cover, the root PUT and its check: no early listed backup, old archives usable, resume reuses safe versions, an unknown root resolved before any retry). |
| S5, S6 | `history.rs`: `uploader_attempts_keep_history` (delete, marker, version delete, overwrite, shorten, mode, bypass, legal hold: denied or exact history intact), `forged_root_separate_entry`, `forged_root_never_base` (a forged root with a later `created_at` and a later LastModified: the next backup stays incremental from the worker's own newest root, by LastModified; `base_not_own` only when no own root is eligible; R2-S4), `descriptor_first_version_authority` (a later forged descriptor version, `immutable:false` or unknown profile, ignored and reported; a root naming it not listed), `descriptor_oldest_unauthentic_refuses` (an oldest version corrupt or under another key exits 3, a transport error exits 1, and a later authenticating version is never taken; R2-S3). `repository_golden.rs` (#52's) gains `old_reader_refuses_schema2` (descriptor and root). Fuzz: `backup_root` gains schema-2 roots; new `backup_evidence` target with `regressions/backup_evidence/`. |
| S7 | `immutable_setup.rs`: `immutable_roots_plain` (a root carrying an encryption descriptor refused); #52's `TestAuthorizeSnapshotRead` encrypted refusal unchanged. |
| S8 | `restore.rs` (#52's) gains `bucket_only_after_new_versions` (list/verify/output restore of old versions after new versions and delete markers, the live volume deleted), `list_checks_root_only` (2 requests a root version, plus the listings and the descriptor authority's GET, `graph_checked: false`, `immutable_guarantee: false`), `verify_checks_graph_retention` (a block version short of E fails verify), `output_fields` (store time labelled, no credential or policy). |
| S9 | `immutable_setup.rs`: `nonimmutable_unchanged` (#52's goldens and outputs byte-identical), `no_owner_credential_in_worker`. `scripts/tools/backup-lock-scripts-test.sh` (Lab, below). |

Future `make lint test race hygiene contracts`, Rust retention/repository fuzz regressions and
serverless tests. No new replication safety rule is added; #52 pin/revocation model tests remain
required. The suite counts every lock request and decoder limit, not only successful outputs.
No runtime tests or real store enforcement checks ran during drafting.

## Lab and documentation

Later explicit consent only, on a dedicated disposable S3 test store. Record its exact
version/policy/clock/profile; select minute periods only if that profile enforces their bounds.
The lab's store is on fl-w, a cluster VM, so it proves S3-API enforcement only, never resistance
to a compromised cluster (R1-S1); every result says so. No bypass, manual clock jump or early disk
wipe masquerades as S3 cleanup; immutable objects cannot be reclaimed merely because the lab check
ended.

**Commands (R1-C8, R1-S7, R2-C3).** `make backup-lock-store-up`, `-down`, `-stop` and `-start`
(`lab.sh backup-lock-store up|down|stop|start`) and `make backup-lock-check` (`lab.sh
backup-lock-check`), each in the Makefile's help and `docs/lab.md`. They need #52's `make
backup-store-up` first (the worker's directory, its `backup` pair, the store's `backup-store`
pair, the pinned SeaweedFS).
- **The store.** A second SeaweedFS instance on fl-w, separate from #52's, so #52's store and its
  `down` (which deletes its volume file) stay as they are and never hold a locked version.
  `scripts/guest/backup-lock-store.sh up|down|stop|start` runs on fl-w: `up` uses #52's pinned
  release (checked again), a 1 GiB file-backed volume under `/var/lib/flaminio-backup-lock-store` on
  the root disk (never the data disk), loop-mounted at `/var/lib/flaminio-backup-lock-store/data`,
  run as `flaminio-backup-store` with #52's `backup-store` server pair, as one `weed server` process
  and a TCP forwarder, each a transient systemd unit (Its listeners, below), refusing if #52's pairs
  or worker directory are missing. It generates in the VM, into files created exclusive and
  no-follow, mode 0600, the owner's S3 credential (`/var/lib/flaminio-backup-lock-store/owner.cred`,
  the store's user) and the uploader's (`/var/lib/flaminio-backup/lock-uploader.cred`,
  `flaminio-backup`); the owner's is never under the worker's paths nor readable by
  `flaminio-backup`. The store's identity configuration,
  `/var/lib/flaminio-backup-lock-store/s3.json` (the file `weed`'s S3 configuration flag names, to
  be checked against the pinned release), beside `owner.cred` and outside the volume's mount point
  `…/data`, owner `flaminio-backup-store`, mode 0600, holds both secrets, one more reason the lab
  proves S3-API enforcement only. Only `up` creates it (exclusive, refusing an existing one) and
  only `down` rewrites it, through `s3.json.new` (below), to remove the uploader, then deletes it
  (R4-B3, R5-S2). The store is up when both its units are active (`systemctl is-active`) and runs
  when either is (R5-B1). `stop` stops both units (`sudo systemctl stop`) and unmounts its volume,
  keeping every file; `start` mounts it unless it is already mounted there (`mountpoint -q`, so a
  `down` whose unmount failed can be followed by `start`, R9-C1) and starts both again with the
  identity configuration as it stands, never regenerating it from the credential files (so an
  identity `down` removed stays removed, R3-B3), refusing when that configuration is missing or the
  store runs; with exactly one unit active, `start` and `down` refuse naming `make
  backup-lock-store-stop` first (R6-C1). Transient units do not survive a reboot, so after fl-w
  restarts (`make vm-restart VM=w`) the store is down and `start` brings it back (R2-C3).
- **Its listeners (R2-C5, R2-S7, R3-B2).** `weed server` shares one bind address among its
  components, so the store runs as one `weed server` process with `-ip` and `-ip.bind` 127.0.0.1:
  every listener of it is on loopback. A `socat` forwarder (the distribution's package, installed by
  `up` when missing), run as `flaminio-backup-store`, listens on fl-w's address (which the pair's
  `--ip` names) at `BACKUP_LOCK_STORE_PORT`, forks at most 32 children at once (`max-children`), and
  forwards bytes to the S3 gateway on 127.0.0.1 at the same port, so TLS still ends at the store and
  the Host the client signs names the port the gateway serves. `lab.conf` names each port:
  `BACKUP_LOCK_STORE_PORT` (S3, 8334) and `BACKUP_LOCK_S3_GRPC_PORT` (18334),
  `BACKUP_LOCK_MASTER_PORT` (9334), `BACKUP_LOCK_VOLUME_PORT` (8081), `BACKUP_LOCK_FILER_PORT`
  (8889) and their `BACKUP_LOCK_{MASTER,VOLUME,FILER}_GRPC_PORT` (19334, 18081, 18889), all on
  127.0.0.1. #52's instance sets no port, so it keeps SeaweedFS's defaults (8333, 9333, 8080, 8888
  and each plus 10000), which these avoid; the defaults, the S3 gRPC flag and the bind flags are to
  be checked against the pinned release. `up` and `start` start each process as the guest scripts
  start their daemons (`ublk.sh`, `replica.sh`, `node.sh`): `sudo systemctl reset-failed`, then
  `sudo systemd-run --quiet --unit=flaminio-backup-lock-store` (the forwarder's unit
  `flaminio-backup-lock-fwd`) `--property=Restart=no --property=User=flaminio-backup-store`, so no
  sudo, PAM or pty process stands between a unit and its process and nothing stays on the ssh
  session (R5-B1; round 4's `setpriv` ran under `sudo` and so recorded sudo's PID). No PID is
  recorded: each check reads each unit's process afresh, `systemctl show -p MainPID --value` (the
  exec'd process; `0`, a unit not running, refuses). They then wait, up to 60 s
  (`BACKUP_LOCK_LISTEN_WAIT` in `lab.conf`), until `sudo ss -Hltnp` shows every named port listening
  under those PIDs: the `weed` process's eight on 127.0.0.1 and the forwarder's one on fl-w's
  address at `BACKUP_LOCK_STORE_PORT`; any still missing stops both and refuses. Then, filtered by
  the PIDs read again, any listener of the `weed` process off loopback, on a named port or not, or
  any listener of the forwarder but that one, stops both and refuses. Other processes' listeners
  (#52's store among them) are not the check's. So fl-a and fl-b cannot reach the store below S3
  during the check: the lab's part of R1-S1's named risk is closed; fl-w itself still can.
- **The S3 client** of the owner steps and the attempts is a lab-only example of the crate,
  `flaminio-backup/examples/lock_lab.rs` (owner and attempt subcommands on aws-sdk-s3 with #52's
  TLS and store-identity checks; never shipped). `lab.sh backup-lock-store up` builds it on fl-w
  (`guest/build.sh -p flaminio-backup --example lock_lab`; `lab.sh deploy`'s build makes no
  example) and installs it `install -m 0755` as `/usr/local/bin/flaminio-lock-lab`, which both
  users run (R2-C5). It presents a client pair as #52's profile requires: the attempts the
  worker's `backup` pair, the owner a new client-only pair `backup-owner`
  (`spiffe://$TRUST_DOMAIN/flaminio/node/$VM_W/backup-owner`, `--client`, never `--server`),
  issued by `lab.sh backup-lock-store up` from the host as #52's `up` issues `backup-store`, then
  `put_file` of `ca.crt`, as #52's `up` does after issuing `backup-store`, and read again at each
  run.
- **`backup-owner` in `pki.sh` (R2-C4).** #16's `pki.sh` list gains, by whichever of #16 and #53
  lands second (#16 is clear and unchanged here; #53 alone, on today's `pki.sh`, adds the same):
  `backup-owner`: `/etc/flaminio/backup-owner-pki`, `flaminio-backup-store` 0700, a first key
  `flaminio-backup-store` 0600, its user made idempotently as #52 makes its two; `file ca.crt`
  also to that directory, only when it exists (only `csr backup-owner`, on fl-w, makes it); and
  `backup-owner-pki.[0-9a-z]*.new` in the sweep of old temporary directories. **Its renewal:**
  #16's `identities VM` gains the fl-w line `backup-owner CA ID --client` with that ID, added the
  same way; `pki issue` skips it, as it skips `backup-store` (`backup-lock-store up` issues it),
  and the renewal renews it like any other name. Where #52's renewal restarts its store after
  renewing `backup-store`, it also restarts the lock store when that is up
  (`backup-lock-store.sh stop`, then `start`), added the same way, so the lock store never keeps
  an old certificate.
- **Files (R2-C5).** The lock repository's, separate from #52's: its worker config
  `/var/lib/flaminio-backup/lock.conf`, its repository key file `/var/lib/flaminio-backup/lock.key`
  (made by `repo init`, which refuses an existing one) and the owner evidence
  `/var/lib/flaminio-backup/lock.evidence.json`; step 7's own, for `flaminio-nolock`, in the same
  directory: `nolock.conf`, `nolock.key` and `nolock.evidence.json` (R3-C2). The check writes
  `nolock.conf` in step 7, as `flaminio-backup`, mode 0600, refusing an existing one: `lock.conf`
  with the bucket, key file and evidence path changed (R4-C1).
- **Owner steps** run on fl-w as `flaminio-backup-store` through
  `scripts/guest/backup-lock-owner.sh`. They create the bucket `flaminio-lock` with Object Lock at
  creation (versioning on, no default retention) and the bucket `flaminio-nolock` (versioned, no
  lock), apply the shipped uploader policy rendered for each of `flaminio-lock` and
  `flaminio-nolock` (R4-C1), write the canaries (`canary/locked` until the check's start plus 2 h,
  with no legal hold: row 4 needs R at least 1 h ahead of each probe, and the check's probes end
  within its first hour; a later probe fails as stale, exit 1, never passes; R2-C2, R2-S1) and the
  `outside_key`, and write the owner evidence, naming `seaweedfs-<pinned version>-candidate`, and
  `nolock.evidence.json` for `flaminio-nolock` from the same template, its `uploader_policy` the
  document applied to that bucket (it states the lock the bucket lacks, so the worker's read-back is
  what refuses; R3-C2, R4-C1), each installed for the worker (`sudo install -o flaminio-backup -g
  flaminio-backup -m 0600`, refusing an existing destination; it holds no secret). The uploader's
  attempts run as `flaminio-backup` through `scripts/guest/backup-lock-attempts.sh` with the
  uploader's credential file. No secret is ever on an ssh command line or leaves the VM.
- **Periods:** min 2 min, max effective 30 min, budget 5 min, uncertainty 2 s; the first backup
  `--retain-for 5m`, the second `--retain-for 10m`.
- `scripts/checks/backup-lock.sh` runs from the host, as #52's `backup.sh`. It validates every
  volume, snapshot, operation and bucket id before any ssh (#52's grammars) and passes them quoted
  (`printf %q`). `repo init`, `repo check` and `backup` take `--allow-unqualified-profile`; `list`,
  `verify` and `restore` take no such flag (R3-C2). Every output it records must say `qualified:
  false` and `immutable_guarantee: false`, and the check fails otherwise (R2-S1, R3-S1). Its steps,
  in intent outcome 10's order: (1) refuse unless the lock store is up; create its volume `bkl-<UTC
  stamp>` (refusing one that exists), RF 2, attached on fl-a, under `write-load.sh` writing a known
  marker; (2) the owner steps above; (3) a locked backup: `repo init --immutable` on `flaminio-lock`
  (the probes pass; the answers rows 1 and 4 got for no retention and no hold are recorded against
  the candidate's absence answers, R3-B1), a snapshot, a grant (#52's install of the grant file) and
  `backup --retain-for 5m`; its JSON's store commit time, floor and counts recorded; (4) every
  request of outcome 6 by the uploader through the attempts script, on that archive's root and one
  block version: delete, version delete, multi-object delete, overwrite, shorten, mode change,
  bypass, legal hold; each refused or the exact versions intact; then `list`, `verify` and a restore
  still use them; (5) a restore to a new file equal to the snapshot, as #52's step 7; (6) a second
  snapshot and `backup --retain-for 10m` reusing blocks, its lock requests per kept block counted
  from the JSON (retention reads and extensions: 1 or 3 a version, R2-C6); after the first floor
  passes, the owner's version delete of a reused block version refused (still locked to the second
  floor); (7) `repo init --immutable` on `flaminio-nolock`, with `nolock.conf`, `nolock.key` and
  `nolock.evidence.json`, exits 3, its JSON naming the bucket's missing lock (the profile's no-lock
  answer, R4-C1; the flag given, so the profile is not the reason; its own files, so neither
  `lock.key` nor the evidence's bucket is); (8) once the first archive's floor has passed by the
  store's Date, the owner deletes a version only that archive held (its root version), which
  succeeds; (9) cleanup: the load stopped, grants closed, only its own volumes deleted; the owner
  lists every version of both buckets with its retention, and the latest retain-until (the canary's
  R among them) is recorded on the host in `.lab/backup-lock/cleanup-after` and printed.
- **`down`** may run in a later session, and ends the store's use (R2-C3, R2-S6). It refuses unless
  the store is up (both units active), naming `make backup-lock-store-start` when it is down. It
  first removes the uploader's identity from the store's identity configuration: it removes a
  leftover `/var/lib/flaminio-backup-lock-store/s3.json.new` (an interrupted `down`'s), writes the
  new configuration to that file, created exclusive and no-follow, `flaminio-backup-store` 0600, and
  renames it over `s3.json`; `start` never reads `s3.json.new` (R5-S2). It then restarts the store
  (`stop`, `start`, which keeps the configuration as it stands, R3-B3), so no uploader request, a
  worker's or the attempts', is accepted from then on; a refused `down` leaves the store serving the
  owner alone. The owner then lists every version of both buckets with its retention and legal hold,
  and `down` refuses while any version is locked past the store's Date. The owner clears every legal
  hold (none is expected; one would be an attempt's success, which failed the check), then deletes
  every version; on any refused or failed delete `down` exits 1 before stopping the store or
  deleting anything else. Only then is the store stopped (`systemctl stop` of both units), its
  volume unmounted and its volume file deleted first, then both credentials, the identity
  configuration and any `s3.json.new` deleted (R4-B3, R5-S2, R7-C1), and the check's worker-side
  files removed where present, each by name and named in `down`'s output: `lock.conf`, `lock.key`,
  `lock.evidence.json`, `nolock.conf`, `nolock.key` and `nolock.evidence.json` in
  `/var/lib/flaminio-backup` (the two keys are repository keys), so a later `up` and check start
  clean (R5-C1). A `down` refused or failed before the store is stopped keeps them. The final step
  exits 1 at its first failed unmount or `rm`, naming it (R8-C1); a failed unmount or a failed `rm`
  of the volume file leaves every other file, so a rerun refuses naming `start`, and after `start` a
  full `down` runs again. The final step is resumable (R6-B1): when neither unit is active and the
  volume file is gone, `down` skips the owner steps and finishes the remaining deletes, each where
  present, then exits 0, so a `down` interrupted during its final deletes is completed by a rerun.
  The store's own commands have no early wipe and no override; destroying fl-w (`make vm-destroy
  VM=w`) would wipe it, as `docs/lab.md` states.
- `scripts/tools/backup-lock-scripts-test.sh` joins `make lint`'s script tests, with fake `sudo`,
  `id`, `ss`, `systemd-run`, `systemctl`, `weed`, `socat`, `useradd`, `mount`, `umount`,
  `mountpoint`, `rm` and `lock_lab`: each id refused before any ssh and passed quoted; `up` refusing
  without #52's pairs or worker directory; each credential's owner and mode, the owner's never under
  `/var/lib/flaminio-backup`; every listener's flag and `lab.conf` port, `weed` on 127.0.0.1 only
  and the forwarder on fl-w's address; `up` and `start` refusing a fake non-loopback listener of the
  `weed` PID on a named port and on an unnamed one, and a forwarder listener on another port, and
  ignoring another PID's non-loopback listener (R3-B2); `up` and `start` refusing, the bound
  reached, a unit's MainPID with no listeners and one with only some of its named ports, and a
  MainPID of `0`; the PID filtered being the one the faked `systemctl show -p MainPID --value`
  returns at that check, read again at each (R4-B1, R5-B1); each unit started by `systemd-run` with
  its unit name and `User=flaminio-backup-store`; `up` refusing an existing identity configuration
  and writing it `flaminio-backup-store` 0600 (R4-B3); `stop` keeping every file, `start` using the
  identity configuration as it stands, never rewriting it, refusing with it missing or either unit
  active; `stop` by `systemctl stop` of both units; after `down` removes the uploader, a `stop` and
  `start` leaving it removed (R3-B3); the example built and installed 0755; `pki.sh csr
  backup-owner` accepted, its directory's owner and mode, its first key's, `file ca.crt` into it
  only when it exists, the sweep's pattern, and, with `identities` and `cmd_pki` extracted as #52's
  test does, `backup-owner` listed for fl-w only and not issued by `pki issue`; the evidence
  installed `flaminio-backup` 0600, an existing destination refused; `down` naming `start` with the
  store down and the volume file present; `down` and `start` with only the forwarder's unit active
  each refusing and naming `stop` (R6-C1); a fake `rm` of the volume file failing, and a fake
  unmount failing, each exiting 1 with every other file kept and a rerun naming `start`, after which
  `start` and `down` leave none of the files (after the failed unmount, `start` mounting nothing,
  the volume still mounted, R9-C1); and a fake `rm` of `owner.cred` failing after it, exiting 1, a
  rerun taking the resume path and leaving none of the files (R6-B1, R7-C1, R8-C1); `down` removing
  the uploader before its listing, refusing while a fake listing shows a version locked past the
  store's Date, clearing a fake hold before the deletes, and, after a fake refused delete, leaving
  the volume file, both credentials and the six worker-side files in place; a leftover `s3.json.new`
  removed before `down`'s exclusive create; and after a full `down` no file holding a secret left
  (both credentials, the identity configuration, a leftover `s3.json.new` placed before it ran,
  `lock.key` and `nolock.key`) and none of the six worker-side files (R4-B3, R5-C1, R5-S2).

Update README, docs/lab (the lock store's `start` after fl-w restarts, and that destroying fl-w
wipes it, R2-C3), repository format/commands/policy guide (S1's deployment prerequisite, the
owner evidence, the canaries and their renewal, the owner's optional cap on extension; round 2:
`--allow-unqualified-profile` a test setting only, the owner's duty to make a new repository on a
backend upgrade, which the worker sees only through the evidence, the owner's cleanup keeping the
descriptor's oldest version while the repository is in use, and the commit budget sized to the
graph's cover time; R2-S1, R2-S8, R2-S3, R2-C1; `list`'s `immutable_guarantee: false`, pointing
to `verify`, R5-S1), the
ransomware limitations, Makefile (the `hygiene` help, R4-C2), `docs/ci.md`'s `hygiene` entry and
ARCHITECTURE §14.1, whose "retention and garbage collection
run under a separate identity" becomes: the uploader sets and extends retention, never shortens
it; collection will run under a separate identity (R1-S3). `docs/followups.md` gains "Audit events
for immutable repository setup, lock and probe outcomes" (issue to be filed; R1-C10, R1-S9): this
increment adds no metadata mutation, so #73's events have nothing to record, and store-side
locks are not audited. Full clean-cluster runbook and post-retention GC are later owned steps;
encrypted archives wait with #52's S8.
No measured request-rate/overhead/enforcement claim comes from this draft.

## Risks, ADRs and parallel work

[Proposed ADR](adr-proposal.md) records the profile/IAM/minimum-period/deadline/version/key
choices. Owner/store behavior, policy drift, key escrow loss, quota abuse and blocked/long
finalization remain explicit; none is an accepted risk. Risks named in round 1, not accepted: a
stolen uploader credential can lock objects for as long as the store and the owner's policy allow,
a quota denial nothing here bounds (R1-S3); a store the cluster can reach below its S3 API
voids the lock (R1-S1); a late root costs its grant and a new attempt, which re-reads the content
it did not record (R1-C3); a policy revision or backend upgrade needs a new repository with full
backups (R1-C7); a lost `completed-roots` file costs full backups (R1-S2); orphans hold space for
`min_period_seconds` (R1-C4). Risks named in round 2, not accepted: a graph whose cover time
exceeds the commit budget fails every run until the repository is recreated with a larger budget
or the graph shrinks (R2-C1, worded so in round 3, R3-C1); a run under
`--allow-unqualified-profile` claims no guarantee, and the flag used outside the lab leaves a
repository no qualified profile can take over (R2-S1); a false owner evidence file makes the
worker apply another profile's codes and claim it, the probes still running (R2-S8); destroying
fl-w wipes the lab's lock store (R2-C3). Exact duration/extra margin is a proposed
contract adjustment, not hidden compliance with impossible cross-object atomic publication.
Accepted language/metadata/fencing ADRs are unchanged; audit0009/0011 remain Proposed.
Worker retention logic and policy/server qualification can be developed separately against the
appendix, then joined by actual pinned-backend enforcement and exact-version restore tests.
Settled at approval, 2026-11-13: the proposed profile, IAM, period, deadline, version and key choices; the ADR is accepted with the plan (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): SeaweedFS, the lab store #52 pins, is qualified first, by the validation profile's tests on a dedicated disposable store, and accepted only if every test passes (see decisions.md).
Round 1 (R1-S1): SeaweedFS stays the first candidate run, but the lab's store on fl-w proves S3-API
enforcement only; the run that can qualify a profile needs a store off the cluster, and which
profile is qualified first for a deployment stays open (deferred, not settled). Round 2 (R2-S1):
until one is, the lab runs SeaweedFS as a candidate profile, its results labelled unqualified.
