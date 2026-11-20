# Issue #53: immutable repository extension contracts

Appendix to [design](design.md). Depends on #52/contracts.md; neither proposal is accepted.
Settled at approval, 2026-11-13: both proposals are accepted with their plans (see decisions.md, Settled at approval).
Changed after approval in plan review rounds 1 to 5 (decisions.md).

**Pending the coordinated byte review (round 1, R1-C11).** The descriptor and root fields below
extend #52's repository format v1 bytes, which #52 shares with #51 and #58. Until #58's
coordinated byte review, they are proposed, not final, and none is built before it (deferred, not
settled), as #51 and #52 mark their side. An immutable descriptor and root carry `schema: 2`;
pages keep `schema: 1`. #52's decoders refuse an unknown schema, so a #52-only reader refuses an
immutable repository with no change to #52.

## Immutable profile

New immutable-v1 repository only. The `repository` descriptor (schema 2) has #52's fields with
`immutable: true`, plus `immutable_profile_version: 1`, `min_period_seconds`,
`max_effective_period_seconds`, `commit_budget_seconds`, `store_time_uncertainty_seconds`,
`backend_profile_id`, `backend_version` and `owner_evidence_digest` (Owner evidence, below).
Its `immutable: true` names the repository's mode (every object PUT locked), not a guarantee: an
output reports one only by `immutable_guarantee` (Backend profiles, R3-S1).
#52 object/envelope crypto, exact version IDs, complete sparse manifest graph and repository UUID
stay unchanged. An unknown `immutable_profile_version` refuses (exit 3). Roots carry
`encryption_descriptor` absent; a root naming one refuses (S7).
Add root fields `{immutable_profile_version,requested_minimum_seconds,commit_deadline,
retention_floor}` (store-time seconds since the Unix epoch, u64 decimal strings as #52's JSON)
encrypted/authenticated under the same root envelope. Root identity remains #52's
(repository_uuid,backup_uuid,version_id) at `roots/<backup_uuid>`, never mutable key/current
version (R1-C3).

**Descriptor authority (R1-S8, R2-S3).** The authority is the oldest version of `repository` in
the store's version order. If it cannot be read or does not authenticate, every command refuses
(exit 3; exit 1 on a transport error) and never falls through to a later version. The guide states
that the owner's cleanup never deletes that version while the repository is in use. `repo init
--immutable` writes it with If-None-Match and
the initial lock below; every archive's graph includes it, so each backup covers it through E.
The worker's config records its exact version (#52) and refuses (exit 3) when the authority
differs. Later versions are reported (`descriptor_versions_ignored`) and never read for
configuration; a root whose `descriptor_version` is not the authority is not listed
(`descriptor_mismatch`).

**Initial lock (R1-C4).** Every object the worker PUTs in an immutable repository carries
COMPLIANCE mode and retain-until I = S + `min_period_seconds`, S being the Date of the latest
store response the worker received, rounded up to the second; a store refusing I as past is
retried once with a fresh S. The root alone is PUT with retain-until E instead. So an interrupted
backup's objects are locked until exactly their I unless a later floor reached them, and an orphan
holds space for at most `min_period_seconds` past its upload unless extended. The bound is the
descriptor's `min_period_seconds`; no field is added.

Durations (`D` in the CLI) are a decimal integer and one suffix `s`, `m`, `h` or `d`; no suffix,
zero, a sign or another unit refuses (exit 2). Require P>=min, P+commit_budget+uncertainty<=max,
with checked u64 arithmetic: an overflow in either sum, or in D+P, refuses (exit 2). Uncertainty
is at least the profile's qualified time precision (1 s for HTTP dates). Select D from
qualified store time plus commit budget and E=D+P. All root graph references must report COMPLIANCE
and retain-until>=E. Root store LastModified T must satisfy T+uncertainty<=D. List only if all
proofs validate; E>=T+P. No prediction of exact request-arrival time or client-clock proof.
Effective protection may exceed requested minimum by the disclosed commit budget/uncertainty;
max bounds cover that extra. This clarification requires approval against existing intent.
Settled at approval, 2026-11-13: the requested-minimum interpretation (see decisions.md, Settled at approval).

**One root per operation (R1-C3, R1-C12, R2-C1).** #52's checkpoint gains P, D, E and the
operation's `backup_uuid`, fsynced before the first extension. A rerun with another `--retain-for`
exits 2 naming the checkpoint's P; another P needs another grant. Before the root's PUT (resolved
first as #52 does, by listing `roots/<backup_uuid>`'s versions), every run, first or rerun, checks
that the latest store Date + uncertainty + `put_allowance` <= D, `put_allowance` being twice the
descriptor's `store_time_uncertainty_seconds`: the time left for the root's own PUT (R3-C1). If
not, the run picks a new D, the latest store Date plus the descriptor's `commit_budget_seconds`
(fixed at `repo init`), and E = D + P, checkpoints them and covers the graph again, once; if the
check then fails again, it exits 1 with the grant active and the checkpoint kept, and sends no root
PUT (a rerun starts that check again). So a root the worker knows is late is never PUT. The second
cover costs as much as the first (every version extended to the new E), so a graph whose cover
time exceeds the budget fails every run until the repository is recreated with a larger budget or
the graph shrinks; the guide sizes the budget to the graph's cover time (up to 3 requests a
version, `cover`) plus 3 × uncertainty. As #52, a different root for that operation is a conflict,
and CloseSnapshotRead(completed, root) takes only a qualifying root. If the root's
T+uncertainty>D (a PUT that took longer than its allowance), the root is an incomplete candidate:
the worker exits 1 naming it and `flaminio-meta snapshot-read close GRANT-FILE --abort`, keeping
#52's rule that once a root is published the worker sends only its completed close (a rerun
resolves the same root and exits 1 again). Another attempt is a new grant, operation and
`backup_uuid`; it verifies arrived content as #52 does for objects it did not record. Never
overwrite a protected root. Expired protection is not presented as currently retained; store time
determines that state.

**What each command checks (R1-C5).** `list`, per root version: #52's GET and authentication, one
GetObjectRetention on that exact version (COMPLIANCE, retain-until>=E), T+uncertainty<=D, schema 2
and the descriptor authority; 2 requests a root version plus the ListObjectVersions pages of
`roots/` and `repository` and the GET of the descriptor authority's version; it reports
`graph_checked: false` beside #52's `verified: false`, and so `immutable_guarantee: false`
(R5-S1).
`verify` and `restore` also read the retention of every page and block version and of the
descriptor's (COMPLIANCE, >=E) with #52's content checks: graph size + 2 retention reads beyond
#52's requests. The worker checked the whole graph before the root's PUT (S3).

**Bases (R1-S2, R2-S4).** In an immutable repository the worker keeps `completed-roots`, beside
its checkpoints (fsync and atomic rename, `flaminio-backup` 0600), listing each
`(repository_uuid,backup_uuid,version_id)` it closed `completed`. The base is #52's compatible
base chosen among those entries only, the newest by its root version's store LastModified (never
the manifest's `created_at`), so a forged root, listed or not, never displaces it. Only when no
entry is eligible is the backup full with reason `base_not_own`. A lost file costs full backups,
nothing else. A leaked repository key retires the repository (guide).

## Required object-client methods

- `bucket_profile(expected_owner)` -> versioning enabled, Object Lock enabled/compliance support,
  the default retention rule (setup refuses one), qualified server time/precision; no permissive
  unsupported defaults.
- `retention(key,version_id,expected_owner)` -> mode, retain-until, legal-hold state, store time;
  the profile's absence answers (Backend profiles) read as no retention or no hold (R3-B1);
  unknown/denied/error is not proof of coverage.
- `cover(key,version_id,E,expected_owner)` (R1-C2) -> `Covered` or `Extended`, or a failure: read
  the exact version's retention; COMPLIANCE and retain-until>=E is `Covered`, with no extension
  request (one retention read); otherwise PutObjectRetention COMPLIANCE until E on that version,
  never a bypass header, then read back the SAME version. A refused extension whose read-back
  shows COMPLIANCE>=E (a longer earlier floor, or a concurrent backup's) is `Covered`; anything
  else fails. A version costs 1 request, or 3 when an extension is sent (R2-C6). Counted:
  `retention_reads`, `extensions`, `refused_but_covered`, `covered_without_extension`. A refusal
  here is never a probe result.
- `put_if_absent_locked(key,body,retain_until)`: #52's `put_if_absent` with the lock headers.
- PUT/HEAD/GET/LIST-version retain #52 semantics. Retention applies to every immutable graph
  member: the `repository` descriptor's exact version, pages, blocks and root; no data GET/PUT for
  authenticated reused immutable versions. Unknown orphan content needs #52 integrity evidence
  first. `ObjectStore` has no delete, shortening, bypass, legal-hold or bucket-mutation call.
All API attempts use #52/#59 request hooks, including canary/retention/bucket metadata calls.
SDK ETag is not a content hash; conditional header and exact version are distinct concepts.

## Probes (R1-C6, R1-S4, R2-S2)

`CanaryProbe` is a type separate from `ObjectStore`, holding only the canary keys; it is the only
code that sends a destructive, bypass or bucket request, and only these. The owner's canaries are
under the repository prefix, which #52's listing ignores (it reads `repository`, `blocks/`,
`pages/`, `roots/` only):
- `canary/unlocked`, written by the owner with no retention. The bucket has no default retention
  rule (setup refuses one, so none can lock it), and the policy denies the uploader PutObject,
  PutObjectRetention and PutObjectLegalHold on it.
- `canary/locked`, written by the owner in COMPLIANCE until a date R, with no legal hold; the
  policy denies the uploader PutObject on it. The owner keeps it locked, R at least 1 h ahead of
  every probe (row 4), by writing a newer locked version before then (the guide); the probe uses
  the newest version, every version there being the owner's.
- The evidence's `outside_key`: an existing owner object outside the repository prefix.

Before `repo init`, `repo check` and each backup, in order:

| # | Request | If allowed | Required |
|---|---|---|---|
| 1 | GetObjectRetention and GetObjectLegalHold, `canary/unlocked` newest version U | read only | no retention, no hold or hold OFF |
| 2 | DeleteObject `canary/unlocked`, no version | a marker on the canary | AccessDenied |
| 3 | DeleteObjectVersion `canary/unlocked` U | the canary's version gone | AccessDenied |
| 4 | GetObjectRetention and GetObjectLegalHold, `canary/locked` newest version L | read only | COMPLIANCE, R >= store time + 1 h (proposed), no hold or hold OFF |
| 5 | PutObjectRetention L, COMPLIANCE, R - 1 s | the canary shortened | refused |
| 6 | PutObjectRetention L, GOVERNANCE, R | the canary's mode changed | refused |
| 7 | PutObjectRetention L, COMPLIANCE, R - 1 s, bypass header | the canary shortened | refused |
| 8 | PutObjectLegalHold L, OFF | no change (no hold) | AccessDenied |
| 9 | PutBucketVersioning Enabled | no change | AccessDenied |
| 10 | PutObjectLockConfiguration, the configuration just read back | no change | AccessDenied |
| 11 | PutObject `outside_key`, If-None-Match `*` | no write (412) | AccessDenied |
| 12 | DeleteObjects (multi-object `POST ?delete`) naming `canary/unlocked` U | the canary's version gone | AccessDenied, for the request or as U's entry in its Errors |

Only AccessDenied is a denial; for 5 to 7, also the profile's retention refusal (Backend
profiles, below). A canary failing row 1 (a retention, even an expired one, or a legal hold, set by
a bucket default, the uploader, another principal or a bug) or row 4 (stale or held) is uncertain,
as are a transport error, a timeout, 5xx, NotFound, NotImplemented and any other code: exit 1,
nothing published. Rows 1 and 4 take "no retention" and "no hold" only from a body saying so or
from the profile's exact absence answer, its code and status (Backend profiles; R3-B1); any other
404, `NoSuchKey` or `NoSuchVersion` among them (the canary itself gone), is uncertain, as is a row
12 response naming U in neither Deleted nor Errors. A held or retained U would make the delete rows
refused by the lock, not by the policy, so a delete-capable credential could pass them; row 1 rules
that out. An allowed request (2xx, or 412 on row 11, or U in row 12's Deleted) exits 3 naming the
row; it touched only its canary, which the owner restores. Row 11 relies on the profile's order of
authorization before the precondition. Not probed, no safe form: bucket policy put or delete,
lifecycle, ACL, CORS, bucket deletion and privilege-assuming calls; they rest on the owner evidence
alone.

## Uploader/owner policy boundaries

For the configured bucket/prefix, ship explicit Allow for ListBucket/ListBucketVersions,
GetBucketVersioning/GetBucketObjectLockConfiguration, GetObject/GetObjectVersion,
GetObjectRetention/GetObjectLegalHold, PutObject and PutObjectRetention. Read rights are needed
for base/dedup/verification; “write-only” means no destruction, not no read.
Explicit Deny DeleteObject/DeleteObjectVersion, BypassGovernanceRetention, PutObjectLegalHold
(R1-S3), bucket deletion/configuration/versioning/lock/policy changes and privilege-assuming
calls; Deny PutObject on `canary/*` and PutObjectRetention on `canary/unlocked`. A cap on retention
modes or periods by the pinned provider's condition keys is the owner's choice, documented by the
guide and checked by nothing (S5, R1-S3). Worker never sends bypass=true (only `CanaryProbe` row
7 does, to its canary) or receives owner/collector credentials. Other principal permissions remain
owner's responsibility; targeted Deny must win against broader inherited grants for this
repository.

Owner bootstrap prepares disposable probes and policy/settings evidence outside the cluster.
Finite probes prove those attempts only; owner deployment/enforcement of the full policy is an
explicit prerequisite, as is S1's deployment prerequisite for the store itself.
No uploader API can prove a future administrator will not change policy or delete the account.

## Owner evidence (R1-C7, R1-S5)

`--owner-evidence FILE` (`repo init`, `repo check`, `backup`; its path may be in the config): strict
JSON, at most 64 KiB, unknown fields refused: `{schema:1, bucket, prefix, expected_owner?,
backend_profile_id, backend_version, uploader_policy (the applied document, verbatim),
bucket_settings:{versioning:"Enabled", object_lock:"Enabled", default_retention:null},
outside_key, written_at}`. It holds no secret. The worker checks that bucket and prefix match its
config, that the backend profile is one it knows (Backend profiles, below), that the settings
match its read-back (GetBucketVersioning, GetObjectLockConfiguration), and runs the probes.
`owner_evidence_digest` is BLAKE3-256 of the file's bytes: recorded, not verified, since the
uploader cannot read the applied policy. It is the owner's attestation, not proof. Before each
backup a digest, backend profile or backend version other than the descriptor's exits 3
(`evidence_changed`): a policy revision or a backend upgrade needs a new repository (another
prefix); the old one stays readable by `list`, `verify` and `restore`, which do not take the
evidence. Renewing a canary changes nothing in the evidence.

**Backend profiles (R2-S1, R2-S8).** The worker ships a table of backend profiles, each with its
codes (rows 1 and 4's absence answers, the code and status for no retention and for no legal
hold, R3-B1; rows 5 to 7's retention refusal; row 11's order; the code and status of
GetObjectLockConfiguration on a bucket with no lock configuration, R4-C1) and a state, `qualified`
or `candidate`. No absence answer may be NotImplemented, `NoSuchKey`, `NoSuchVersion`,
AccessDenied or any 5xx (R4-B2). The no-lock answer exits 3 (the bucket's missing lock); any
other 404, `NoSuchBucket` among them, exits 1.
None is qualified in this increment (Validation profile). `seaweedfs-<pinned version>-candidate`
ships as a candidate, its codes proposed and checked by the lab's runs; a qualified profile is a
new id, so a repository set up under a candidate is never relabelled (another id exits 3,
`evidence_changed`). `repo init`, `repo check` and `backup` refuse a candidate (exit 3,
`profile_not_qualified`) unless given `--allow-unqualified-profile`, which the guide names a test
setting only; they refuse an unknown id with or without it. A code other than the profile's fails
closed (exit 1 or 3), never passes. Every JSON output, of `list`, `verify` and `restore` as well,
carries `backend_profile: {id, version, qualified, source: "evidence"}` and an
`immutable_guarantee`: top-level, and in `list` per archive (R4-S1; always `false` there,
R5-S1). **Under a candidate (R3-S1)**
they read `qualified: false` and `immutable_guarantee: false`, and `candidate_state` reads
`completed-unqualified` wherever it would read `completed`; exit codes are unchanged (0 on
success). `candidate_state: completed` appears only under a qualified profile, for a completed
archive: in `list`, an entry whose root passed `list`'s checks, with `immutable_guarantee: false`
(R6-C2). Only a qualified profile's outputs say `immutable_guarantee: true`, and only for a
completed, covered archive (R4-S1): `backup`'s root once it qualifies (exit 0),
and the archive `verify` checked or `restore` read (exit 0): an output whose own checks covered
the whole graph (R5-S1). It is `false` in every failing output (a late root's exit 1 among
them), in `repo init` and `repo check`, which cover no archive, and in `list`, top-level and for
every entry: `list` checks each root only (`graph_checked: false`), and a root retained past E
says nothing of its pages' and blocks' versions, so the guide points to `verify`.
`list`, `verify` and `restore` take no flag and never refuse for the profile (S8): under a
candidate, or an id this build does not know, they report `qualified: false` and
`immutable_guarantee: false`; under an unknown id they apply no profile codes, so an answer other
than a retention body counts as not covered, and `candidate_state` never reads `completed`. The
backend's identity and version are the owner's attestation: the worker detects an upgrade only
through the evidence (the owner's duty, in the guide), and where a profile defines a
store-reported version, it also compares that version with the evidence's and exits 3 on a
difference.

## Keys and output contracts

Every archive is plain (S7): the repository key, escrowed off-cluster by the administrator (#52),
is its only key. No GC/key release in this increment. The retained key-reference graph of
encrypted archives (KEK/DEK/source/version references, the floor, holds and every root version)
is deferred with #52's S8 to the plan that lifts #51's refusal (R1-C1, R1-S6); no record, RPC or
`auth.go` rule is added here.

CLI `repo init --immutable --min-period D --max-effective-period D --commit-budget D
--store-time-uncertainty D --owner-evidence FILE`, `repo check --owner-evidence FILE`,
`backup ... --retain-for D`; each of these three also takes `--allow-unqualified-profile` (Backend
profiles). Setup/backup reject missing/out-of-range/unsupported precision. JSON
includes immutable_profile/store_commit_at/retention_floor/requested_minimum_seconds/
extra_coverage_bound_seconds/candidate_state/uploader_check (each probe row's result)/
retention_request_counts (the four counts of `cover`)/graph_checked/descriptor_versions_ignored/
backend_profile/immutable_guarantee (R3-S1).
Store time and manifest self-time are separate. Exit and safe credential/output contracts remain
#52's: 1 also for an uncertain probe, 2 for a duration or a rerun's other P, 3 for an allowed
probe, a changed evidence, a candidate profile without the flag, a default retention rule, a
bucket without Object Lock (the profile's no-lock answer or a read-back showing it off, R4-C1), a
descriptor authority mismatch or an unauthentic oldest descriptor version.

## Validation profile

Serverless fake versions/policies/clock prove client logic. A real backend profile is accepted
only after conditional/version/COMPLIANCE/enforcement/clock/policy tests, the probes' answer codes
(rows 1 and 4's absence answers, rows 5 to 7, row 11's order, the no-lock answer), exact
retained-version restore, permitted expiry cleanup under the owner credential, and (R1-S1, R2-S5),
from a cluster host, every non-S3 interface of the store unreachable or refusing an unauthenticated
delete, overwrite, or retention or metadata change of a locked version. An absence answer is
accepted only if rows 1 and 4's requests answer differently on three versions (R4-B2): one with no
retention or hold (the absence answer), one with a retention or a hold respectively (a body) and a
deleted version. For SeaweedFS that means its master,
volume and filer HTTP and gRPC APIs and `weed shell`, which the review reports act below the S3
gateway's lock unless bound to loopback or protected by JWT or mTLS: to be verified when the
profile is qualified, not taken as fact. A lab store on fl-w proves S3-API enforcement only, never
resistance to a compromised cluster. This plan does not certify SeaweedFS, Versity or AWS from
documentation alone. No runtime/approval claim is made by drafting these contracts.
Settled 2026-11-13 (user's instruction): SeaweedFS, the lab store #52 pins, is qualified first, by the validation profile's tests on a dedicated disposable store, and accepted only if every test passes (see decisions.md).
Round 1 (R1-S1): the run that can qualify it needs a store off the cluster; which profile is
qualified first for a deployment stays open (deferred, not settled). Round 2 (R2-S1): until one
is, the lab runs SeaweedFS as a candidate (Backend profiles), its results labelled unqualified.
