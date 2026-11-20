# #53 plan review, round 1, design-security

Scope: docs/issues/53/ (intent, spec, design, contracts, adr-proposal, decisions) at HEAD, checked
against the code in /home/jsfakian/Documents/src/flaminio-53, #52's clear plan
(/home/jsfakian/Documents/src/flaminio-52/docs/issues/52/), #73 and #50 as clear, and the batch
hand-offs.

### R1-S1 — High
**Immutability is qualified only through the S3 API. The store's other paths, and the host it runs on, are outside every rule and test, and the first profile runs on a cluster host.**
The plan qualifies SeaweedFS first, "the lab store #52 pins" (decisions.md, Settled at the user's
instruction). #52 runs that store on fl-w (#52 design.md:334-346). fl-w is the third etcd member and
the k3s server, and it holds `admin.key` and `token.key` (scripts/lab.conf:48-57;
scripts/lab.sh:807, 929). The store's data is a file on fl-w's root disk, its clock is fl-w's
clock, and its configuration holds every S3 identity's secret.

SeaweedFS also serves master, volume and filer HTTP and gRPC APIs, and `weed shell`, below the S3
gateway. These act on stored files with no Object Lock check unless they are bound to loopback or
protected by JWT or mTLS. #52's own lab step 6 alters a locked block version "at the store's
backend by its administrator path" (#52 design.md:399-401). A compromised cluster, the plan's first
actor, can therefore delete or alter locked versions, or move the store's clock, without touching
the uploader's credential.

The validation profile tests only conditional, version, COMPLIANCE, clock and policy behaviour
through S3 (contracts.md:82-89; design.md:156-162). The design keeps only the *owner credential*
off-cluster (design.md:29-31). Nothing requires the store's host, its storage, its clock source,
its non-S3 interfaces or its configuration file to be out of the cluster's reach. Out of scope
(intent.md:120) covers "a store that breaks its own lock, its operator", not a store the cluster
can reach under the lock.

**Fix:** add a deployment prerequisite to S1/S5 and to the guide: the store's host, disks, clock
source, admin interfaces and identity configuration are outside every cluster identity's reach,
and the store is never on a Flaminio node or control-plane VM. Add to the validation profile, for
each backend: from a cluster host, every non-S3 interface of the store is unreachable or refuses
an unauthenticated delete of a locked version. Record in the profile that a lab store on fl-w
proves S3-API enforcement only, never resistance to a compromised cluster.

### R1-S2 — Medium
**A forged root poisons every later incremental backup, past the compromise.**
The plan admits that a holder of the uploader credential and the repository key can write
authenticated roots (intent Why 5; design.md:98-100). It protects only list, verify and restore
from such roots (spec S6, spec.md:62-67).

`backup` also reads roots. #52 picks "the newest complete compatible base" from the repository
listing (#52 spec S1; #52 design.md:124-128). #53 then reuses a base's block versions after only an
AEAD check and a retention check (design.md:89-94; spec S3). A forged newer root, with the same
source identity and pages that name forged block versions, becomes the base of the next legitimate
backup. Its "unchanged" blocks are copied into a new, locked, store-timed archive that looks
legitimate.

The forged root is itself locked, so it stays newest for its whole period, even after the
credential is rotated. That defeats intent Why 6's "the means to tell them apart".

**Fix:** in an immutable repository, accept as a base only a root whose exact
`(repository_uuid, backup_uuid, version_id)` the worker recorded durably as its own completed
outcome (its checkpoint, or the grant outcome #52 records). Otherwise do a full backup. State that
a leaked repository key retires the repository. Add a test: a forged newer root is never chosen
as a base.

### R1-S3 — Medium
**A stolen uploader credential can lock objects for any length of time. S5's bound has no mechanism the first backend is known to have, and no test.**
Spec S5 says the uploader extends coverage "only within configured bounds" (spec.md:55-60). The
design and contracts lean on provider condition keys (design.md:46; contracts.md:51-53). The
intent says the opposite: caps on periods in the bucket's policy are "the owner's ... nothing
checks them" (intent.md:122-124). It also notes extension "up to 100 years on AWS"
(intent.md:45).

No test exercises the bound:
- spec Tests S1/S5, spec.md:93-95, has no such case;
- the validation profile, contracts.md:82-89, has no such case;
- the probe list, design.md:37-40, has no over-bound PutObjectRetention or PutObject with lock
  headers.

So whether SeaweedFS fails qualification on it is undecided. An over-bound probe that the store
allows locks its canary for that period, irreversibly.

`PutObjectLegalHold` is also missing from the explicit Deny (contracts.md:50-52). An inherited
grant would let the uploader pin every version indefinitely.

ARCHITECTURE §14.1 still says retention runs under a separate identity
(docs/ARCHITECTURE.md:727-748), and the docs list does not update it (design.md:164-166).

**Fix:** pick one of two.
- Keep the S5 bound as a store-enforced rule. Add a qualification probe at max+1 day on a tiny
  canary under the uploader's credential, with the canary's cost stated. A backend that allows it
  fails.
- Or restate S5 as the intent has it (a guide-documented owner cap that nothing checks) and name
  the long-lock quota DoS as a risk.

Either way, add `s3:PutObjectLegalHold` to the Deny and the probe list, and update §14.1.

### R1-S4 — Medium
**The "unlocked/expired canary" probe can pass a credential that is able to delete.**
Its purpose is to stop a lock from hiding excess privilege (design.md:33-35; contracts.md:59-60).
But a "verified retention denial" counts as a safe denial (contracts.md:58), and nothing checks
that the canary is still unlocked when it is probed. Two cases lock it anyway:
- a bucket default retention, common on Object Lock buckets, locks every new canary version;
- the uploader holds PutObjectRetention over the namespace the canaries live in (contracts.md:46-48),
  so it, or a bug, can lock the canary.

Either way, the delete-version probe is refused by the lock and "passes". The per-backup recheck
with "renewable" canaries (design.md:40) does not say who renews them or how their state is read.

**Fix:** before each delete probe, read the canary's exact version retention by store time. It
must show no retention or an expired one, else the check is uncertain and fails. On that canary,
accept only AccessDenied. Keep the canaries under a prefix where the policy also denies the
uploader PutObject and PutObjectRetention. State how a bucket default retention is handled. Add
tests for both masked cases.

### R1-S5 — Medium
**The "owner-applied policy/config evidence" that S1 requires has no input, no format and no check.**
S1 makes setup depend on owner evidence plus probes (spec.md:30-34; design.md:41-42). The
descriptor stores an `owner_policy_digest` (contracts.md:10). Nothing says how evidence reaches the
worker:
- `repo init`'s flags are only `--immutable --min-period --max-effective-period --commit-budget`
  (design.md:114-116; contracts.md:74-75);
- the uploader has no GetBucketPolicy (contracts.md:46-49), so the digest cannot be compared with
  the applied policy.

As designed, the evidence is a value the possibly compromised worker reports about itself.

The "safe no-change" probes of bucket mutation (design.md:37-38) have no form for PutBucketPolicy
or lifecycle and ACL changes either, because the uploader cannot read their current value.

**Fix:** define the evidence: an owner-supplied file to `repo init` and `repo check` (policy
document, settings, backend version, signed or at least digested), what the worker checks of it,
and that it is attestation, not proof. List which bucket mutations are probed with a real no-change
request (versioning `Enabled`, the read-back lock configuration) and which rest on that evidence
alone. Add tests.

### R1-S6 — Medium
**S7 relies on a key-reference graph that #52 (clear) deferred, and names no records, RPC or `auth.go` rule.**
Spec S7 and the design use "#52's retained key-reference graph, extended with effective retention
floor/hold status and all referenced root versions". They say ordinary deletion and KEK re-wrap
"preserve" those references (spec.md:69-75; design.md:103-110; contracts.md:66-72), with "per-method
auth ... if fields change" (design.md:125-127).

#52 defers exactly those records with its S8 and refuses encrypted volumes (#52 spec.md:89-99;
#52 design.md:162-163, 170-176). #50 forbids shipping retained encrypted backups without that
catalog/GC contract (#50 design.md:333-335).

So the mechanism S7 and its tests (spec.md:101-103; design.md:145) depend on does not exist. If it
is built here, the design names no record, no writer, no RPC and no role.

**Fix:** defer S7's key-reference part with #52's S8, as #52 did. Keep #52's encrypted refusal for
immutable repositories, and keep only what this increment can do (no GC, no erasure, the
repository-key escrow statement). Otherwise specify the records, the writer identity, the
transaction and #73-style rows plus `auth.go` rules.

### R1-S7 — Medium
**The lab plan cannot be run as written, and its credentials and remote commands are unspecified.**
design.md:156-162 names no script, Makefile target, command or host, and says nothing about:
- **The owner credential:** where it is made, where it is kept, and how it stays out of the
  `flaminio-backup` user's and fl-w root's reach. #52's `backup-store up` generates one S3
  credential, the worker's, and creates the bucket with it (#52 design.md:341-345). This plan needs
  a separate owner identity and an uploader without bucket rights.
- **The bucket:** how it is created with Object Lock. SeaweedFS needs the lock at creation
  (intent.md:62), and #52 creates only a versioned bucket.
- **Canaries and the applied policy:** who creates them.
- **Ids:** validation and `printf %q` quoting of ids before ssh (AGENTS.md Principles; #52's
  backup.sh does this).
- **Clean-up:** #52's `backup-store down` deletes the volume file (#52 design.md:345-346), which
  this design forbids as "early disk wipe" cleanup (design.md:161-162), while intent outcome 10
  wants "a store that can be emptied".

**Fix:** name the check script and Makefile target, the dedicated store (host, port, directories,
users), how the owner and uploader credentials are generated and kept (owner material on the host
workstation or a non-cluster VM, never in `/var/lib/flaminio-backup`), the lock-enabled bucket
creation, and canary and policy setup. Validate and quote every id before ssh. Settle whether
`down` on a disposable store is allowed before the dates expire. Add script-test cases.

### R1-S8 — Low
**Which version of the repository descriptor is authoritative is unspecified.**
The descriptor (`repository`, #52 contracts.md:402-405) carries the immutable profile and its
bounds (contracts.md:7-10). The uploader can add versions under any name (intent Why 5), so it can
write a forged descriptor: `immutable:false`, other bounds, or an unknown profile. A bucket-only
reader (spec S8/S9) gets no rule for choosing one. "Unknown profile refuses" (contracts.md:11) lets
a forged unknown-profile version block reads, and a forged `immutable:false` version could switch a
reader to #52 behaviour (S9).

**Fix:** for immutable repositories, make the descriptor's first version, by store time and
version order, the authority. Report and ignore later versions, and refuse a root whose
`descriptor_version` is not that version. Add a test.

### R1-S9 — Low
**#73's audit hand-off is not taken.**
#73 (clear) hands the audit events, protection checks and quota of backup operations to
#50–#53 (#73 spec.md:132-133; #73 intent.md:48-49; batch notes line 222). #53 says the audit log
"does not exist yet" (intent.md:132). #73 defines it, and #53 records no follow-up.

**Fix:** state that this increment adds no audited metadata method (if S7 is deferred, per R1-S6),
or audit its key-reference writes through #73's event append. Add a `docs/followups.md` row for
auditing lock and refusal events, as #52 does for snapshot-read grants.

## Nits
- contracts.md:39-40: "descriptor" is ambiguous (repository descriptor or #50's encryption
  descriptor). Name it `repository` descriptor, exact version.

Reviewed commit: 525f70c3117f0780d06edf44e0da770213e14e4a
