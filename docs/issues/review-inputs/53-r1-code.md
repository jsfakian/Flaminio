# #53 plan review, round 1, design-code focus

Scope: docs/issues/53/ (intent, spec, design, contracts, adr-proposal, decisions) against the
worktree, origin/main scripts, and clear plans #52 (fixed point: repository format, worker,
lab store), #51, #50, #73.

### R1-C1 — High
**S7's key-reference graph rests on something #52 deferred, and nothing in this increment can carry it.**
Spec S7, design "Deduplication, stable reading and retained keys" (design.md:103-110),
contracts "Key/reference and output contracts" and decisions "The owner's key-reference
interfaces" all say "#52's retained key-reference graph gains the retention floor, hold status
and every root version". #52, which is clear, defers that graph. Its spec S8 (52/spec.md:89-92)
says encrypted volumes are refused and "its retained-key records and its tests wait for the
plan that lifts #51's refusal". Its design (52/design.md:162-163, 170-176) says "Retained-key
references, and their records outliving the volume, are deferred with S8". Its contracts
(52/contracts.md:402-408, 517-521) put no key-descriptor object kind in repository v1
(repository/block/page/root only) and set `encryption_descriptor` absent.
- #51 has no single-snapshot deletion (51/spec.md S6: a snapshot goes only with its volume).
- #50 has no rotation (50/spec.md:29 "rotation is step 4"; 50/design.md:333-335 "do not ship
  retained encrypted ... backups without that catalog/GC contract").

So the following have no mechanism and cannot run: S7's "KEK re-wrap/rotation preserve ...
references", the design's "Key-reference persistence updates use #50/#52 metadata conditional
transactions, with per-method auth ... if fields change", and the test rows "key references
survive KEK version change/hold" and "Delete live volume/parent snapshot". The second names no
record, no RPC and no `auth.go` rule. Retention of "wrapped-key metadata"/"durable key
descriptors" (S3, contracts "Required object-client methods") names objects that do not exist.

**Fix:** narrow S7 as #52 narrowed S8: every archive here is plain, so the only key is the
repository key (escrow off-cluster, already #52's). Defer the encrypted key-reference graph,
its metadata records, RPCs and tests, and the parent-snapshot and rotation cases, to the plan
that lifts #51's refusal (record it in decisions.md as deferred, not settled). Drop
"wrapped-key metadata"/"key descriptors" from the S3 coverage list until #52 has such an object.

### R1-C2 — Medium
**"Extend first" fails on a reused version that is already locked past E.**
Design :93 ("Extend first; failed ... versions cannot enter a new manifest") and contracts
`extend_compliance(key,version_id,E,...)` ("never ... shorten"), with S3 ("No archive commits
on ... a failed extension"). In compliance mode a PutObjectRetention to an earlier date is
refused, and the uploader's policy denies shortening too. A realistic case: blocks reused
from an earlier backup with a longer period, such as a 7-day backup after a 30-day one. The
same happens when two backups (of two volumes, or of one) share a block version and race their
extensions. The extension is refused, so the backup can never complete. The plan also counts
that refusal against the shortening probe's classification.

**Fix:** define the step as: read the exact version's retention; if it is COMPLIANCE and at
least E, it is covered and no request is sent; otherwise extend to E and read it back. A
refused extension whose read-back shows at least E counts as covered. Count each request kind.
Add tests: a reused version already past E, and two concurrent extensions to different dates.

### R1-C3 — Medium
**A new finalization attempt under the same operation contradicts #52's root rules, and the plan does not amend them.**
Design :70-73 and contracts "Immutable profile" ("A confirmed late/invalid candidate may use a
new persisted finalization attempt/root UUID with the same snapshot/op"). #52's contracts
(52/contracts.md:434-435) say "Lost root reply resolves operation UUID/root version; a different
root for that operation is conflict". #52 also lists every authenticated root (52/design.md:165).
Its resume (52/design.md:185-188) resolves "the" root of the operation. Under #53, a late
candidate and a qualifying root share one `operation_uuid`. #52's rule turns that into a
conflict, and its resume rule has no way to choose between them. The plan also renames #52's
`backup_uuid` to "root UUID" (contracts :13, design :71). The root key is
`roots/<backup_uuid>` (52/contracts.md:386-387), so a new attempt needs a new `backup_uuid`, and
the plan does not say so.

**Fix:** state the amendment to #52's contract: in an immutable repository one operation may
have several roots, one per `finalization_attempt_uuid`, each under its own `backup_uuid`.
Resume and CloseSnapshotRead(completed) take only the one qualifying root. A second qualifying
root for the same operation is the conflict. Use #52's name `backup_uuid`. Add a resume test
with a late root plus a qualifying root for the same operation.

### R1-C4 — Medium
**The initial lock's date is unspecified.**
Design :75-76: "All blocks/pages/descriptors ... use temporary initial compliance locks when
uploaded; finalization extends as needed". Intent outcome 8 says the blocks of an interrupted
backup "stay locked until their date". Neither the design nor the contracts say what that date
is (upload store time plus MinPeriod? a fixed bound in the descriptor?) or how it relates to
MaxEffectivePeriod. In compliance mode it cannot be undone, and it sets how long orphans hold
space on fl-w. The plan also does not say whether the descriptor's own version is locked at
`repo init`, and until when.

**Fix:** fix the initial retain-until (for example, the store's Date at the PUT plus
`min_period_seconds`) in contracts.md, store its bound in the descriptor, and say how the
`repository` object is locked at init. Add a test that an interrupted backup's objects carry
exactly that date.

### R1-C5 — Medium
**What `list` checks, and what it costs, is left open.**
Design :68-69: "list/verify/restore repeat the immutable qualification from exact
root/profile/version metadata". Intent outcome 4 says an archive is listed only if the store
reports its root and every block locked until at least its date. Read literally, every `list`
does a GET of every page and a GetObjectRetention on every block version of every archive:
O(archives x blocks) requests, unbounded, against #52's `list`, which only authenticates roots
(52/contracts.md:512-514, `verified: false`). If `list` checks less, the plan does not say what.

**Fix:** split the check. `list` checks the root version (T + uncertainty <= D, the root's
retention at least E, COMPLIANCE, the descriptor's profile) and reports `graph_checked: false`.
`verify` and `restore` check the whole graph's retention as well as its content. State the
request bound of each and test it.

### R1-C6 — Medium
**The probes have no client methods, no canary names, and no safe request form per action.**
Design :33-42 and contracts "Uploader/owner policy boundaries" require probes of DeleteObject,
DeleteObjectVersion, shortening, mode change, bypass, bucket/policy changes and wrong-prefix
actions, using "safe no-change requests", before every backup.
- The client surface (contracts "Required object-client methods"; #52's `ObjectStore::{list_versions,head_version,get_version,put_if_absent}`)
  has no delete, shorten, bypass or bucket-mutation call. `extend_compliance` forbids shortening
  by contract.
- The canaries' keys are not named. #52's key space is `repository`, `blocks/`, `pages/`,
  `roots/` only. Their renewal ("renewable disposable canary fixtures") has no mechanism: once
  the locked canary's lock expires, the shortening and bypass probes on it no longer test
  anything.
- Some bucket mutations have no no-change form. A DeleteBucketPolicy that succeeds removes the
  owner's Deny statements. A wrong-prefix PutObject that succeeds writes an object, which the
  bucket's default retention may then lock.
- Design :33-35 puts the ordinary delete on the unlocked/expired canary; contracts :59-60 put it
  on the locked canary.

**Fix:** add a probe-only client type, separate from `ObjectStore`, that holds the canary keys
(name them, for example `canary/locked` and `canary/unlocked` under the prefix, which #52's
listing ignores). List each probe's exact request and its safe form: for instance,
If-None-Match onto an existing owner canary outside the prefix (412 means allowed, 403 means
denied), and PutBucketVersioning with Enabled. Drop any probe with no safe form, such as
DeleteBucketPolicy, and leave it to the owner's evidence. Say who keeps the locked canary
locked, and until when. Make design and contracts agree on the canary for DeleteObject.

### R1-C7 — Medium
**The owner's policy evidence cannot be checked by the uploader, and a locked descriptor cannot follow a policy or backend change.**
The descriptor (contracts :8-11) records `backend_profile_id/version` and `owner_policy_digest`.
S1 requires "owner-applied policy/config evidence" and a recheck before every backup.
- The uploader's Allow list (contracts :46-48) has no GetBucketPolicy, so the worker can neither
  compute the digest nor compare it with what is deployed. Nothing says what the evidence is,
  how the worker receives it, or what is compared before each backup.
- The descriptor is written once, conditionally, and locked. When the owner revises the policy
  or upgrades the backend to a newly qualified version, the plan neither updates the record
  (that would break If-None-Match) nor says the repository stays usable or is refused.
- `repo init` takes no flag for `store_time_uncertainty` or for the evidence (design :114-115,
  contracts :74-76).

**Fix:** define the evidence: an owner-written file passed to `repo init` and
`repo check` (path flag), its fields, and what the worker checks against the store (bucket
versioning, lock configuration, probes). Say that the digest is recorded, not verified. Define
what a later backup does on a different backend version or policy digest: refuse with exit 3,
or a new repository. Add the uncertainty flag. Test each case.

### R1-C8 — Medium
**The lab plan cannot be run as written, and it collides with #52's lab store.**
Design "Lab and documentation" names no script, Make target, `lab.conf` value, bucket,
credential or step order. Intent outcome 10's checks (Done when: a delete or overwrite inside
retention refused, a locked backup restoring) have no runnable form. #52's lab store
(52/design.md:330-345) is one SeaweedFS on a 2 GiB file-backed volume. Its `up` creates one plain
versioned bucket with one generated credential. Its `backup-store down` "deletes the volume file
and the credentials", which is the early wipe #53 forbids (design :161-162) once a bucket holds
locked versions. #53 needs an Object Lock bucket, a bucket without the lock (for the refusal),
an uploader credential and an owner credential with its policy, minute periods, and a cleanup
that waits for the recorded date, possibly across sessions.

**Fix:** name the commands (for example `make backup-lock-check`, `lab.sh backup-lock-check`,
`scripts/checks/backup-lock.sh`) and their steps in intent outcome 10's order. Name the store:
a separate SeaweedFS instance, or #52's with `down` refusing while any locked version's date is
in the future. Name the buckets, both credentials and where each is kept (the owner's never on
fl-w's worker paths), the periods, and the recorded cleanup date. Add script tests for the
refusals.

### R1-C9 — Medium
**The test plan names no tests, and several rules have none.**
Design "Tests" and spec "Tests" describe classes of tests ("Serverless identity/profile tests",
"Scripted store clock/deadline boundaries") with no file or test names, unlike #52
(52/design.md:309-316). These rules have no row:
- the initial lock's date (R1-C4);
- reading retention before extending (R1-C2);
- each probe's classification (AccessDenied, against transport errors, NotFound or
  unsupported);
- the descriptor's evidence recheck (R1-C7);
- duration parsing with its suffixes and checked arithmetic, including
  `P + budget + uncertainty <= max` at overflow;
- the old reader's refusal of an immutable descriptor or root.

**Fix:** name the tests per rule as #52 does, for example `flaminio-backup/tests/retention.rs`:
`late_root_hidden`, `reused_version_already_covered`, `extends_exact_not_current`, and
`tests/probes.rs`: `uncertain_probe_fails`, `delete_marker_allowed_refuses_setup`. Add the
missing rows.

### R1-C10 — Low
**#73's hand-off is not taken, and the intent's "it does not exist yet" is now drift.**
batch-2026-11-12.md:222 asks #51–#53 to either audit their operations through #73's event
append or record that as a follow-up. #73 (clear, origin/main docs/issues/73/spec.md:28-33,
S7) defines hash-linked audit events. #53's intent, Out of scope, says the audit log "does not
exist yet". Neither the design's docs list nor decisions.md records a follow-up. #52 records one
for grants (52/design.md:424-425).

**Fix:** record a follow-up in docs/followups.md, "Audit events for immutable repository setup,
lock and probe outcomes", noting that this increment adds no metadata mutation. Reword the
intent's line to "#73's events cover metadata mutations only; store-side locks are not
audited".

### R1-C11 — Low
**The descriptor and root fields extend shared bytes without the #58 hand-off.**
Design :129-134 and contracts :8-14 add fields to #52's descriptor (`immutable:false` today,
52/contracts.md:402-405) and root (52/contracts.md:406-408) "agreed before freezing its bytes".
#52's contract does not list them, and #52's shared bytes wait for #58's coordinated review
(batch :1104; 52/design.md:3-4). #53 neither names #58 nor says whether the schema stays 1.

**Fix:** name #58's coordinated byte review as the place these fields are agreed. Say whether
`schema` stays 1 with `immutable:true` gating the new fields, or becomes 2. Queue the hand-off
for #52/#58 in the batch notes.

### R1-C12 — Low
**Resuming with a different `--retain-for` is unspecified.**
Contracts "Immutable profile" persists operation, attempt and root UUID before the PUT, and the
root carries `requested_minimum_seconds`, `commit_deadline` and `retention_floor`. A rerun with
the same grant file and checkpoint (52/contracts.md:461-466) may pass another `--retain-for`.
The plan does not say whether the rerun refuses, or starts a new attempt after resolving the
old one.

**Fix:** checkpoint P, D and E with the attempt. On resume, refuse a different P (exit 2)
until the earlier attempt is resolved. Test it.

## Nits
- contracts.md:9: `store_time_uncertainty` lacks the `_seconds` unit that the other duration
  fields carry.
- spec.md "Open questions" ("No approval or accepted risk is recorded") and
  adr-proposal.md:12-13, 61-63 ("supplied for batch review without acceptance"; "No ... human
  approval exists") contradict the recorded approval.

Reviewed commit: 525f70c3117f0780d06edf44e0da770213e14e4a
