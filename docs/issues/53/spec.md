# Issue #53: Immutable backups — specification

The existing [intent](intent.md)'s locked manual-backup/uploader increment only.
[Design](design.md), [contracts](contracts.md), [Proposed ADR](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review rounds 1 to 5
(decisions.md).
Policy retention, post-retention GC and the complete ransomware runbook remain open under #53.

## Terms

- **Archive:** #52's authenticated self-contained graph of exact object versions: the
  `repository` descriptor, root, pages and blocks; current key names are not its identity. Every
  archive here is plain: #52 refuses encrypted volumes (round 1, R1-C1, R1-S6).
- **Qualifying root:** an authenticated immutable root version whose store time T satisfies
  T + uncertainty <= its deadline D, and whose exact version the store reports in COMPLIANCE
  until at least its floor E (contracts, Immutable profile).
- **Owner evidence:** the owner-written file naming the applied policy, bucket settings and
  backend profile; the worker records its digest: attestation, not proof (R1-C7, R1-S5).
- **Immutable repository:** a new versioned/Object-Lock repository using compliance retention;
  its mode cannot be changed in place from a nonimmutable repository.
- **Uploader:** add/list/read/retention-extension identity with explicit deny of deletion,
  bypass/shortening and bucket/policy changes; “write-only” does not mean unable to read.
- **Commit time:** object-store-assigned time of the qualifying final archive root version.
- **Protection period:** requested minimum coverage after commit time, with the bounded
  finalization margin and maximum effective coverage disclosed at setup and in output.

## The invariant

A listed immutable archive's exact authenticated versions remain covered through its reported
retention floor (retained key references are deferred with S7: round 1, R1-C1, R1-S6). Uploader
actions cannot remove/replace those versions or make a later version shadow them. No lock
response, local clock, mutable key name or manifest timestamp alone establishes that guarantee.
Store owner failure, a store the cluster can reach below its S3 API (S1), and post-compromise new
archives remain outside the guarantee; old retained versions are independently identifiable.

## Behaviour

**S1. New mode and target qualification.** Initialize immutable only on a new repository with
versioning, Object Lock/compliance support and the required #52 durable/exact-version/TLS profile.
Setup requires the owner evidence file, the worker's read-back of the bucket's versioning and lock
configuration (enabled, no default retention rule), and negative enforcement probes, not a
boolean success response; the evidence's digest is recorded, not verified. Before each backup
recheck mode/profile, the evidence digest and the probes; unknown/unavailable evidence fails
before publication, and a changed digest or backend version refuses (a new repository follows a
policy change or backend upgrade; the backend's identity and version are the owner's attestation,
so the worker sees an upgrade only through the evidence, R2-S8). A backend profile not yet
qualified is refused unless the command is given `--allow-unqualified-profile`, and then every
output says `qualified: false` and reports no immutable guarantee (`immutable_guarantee: false`,
`completed-unqualified` for `completed`; round 2, R2-S1, and round 3, R3-S1: the lab runs so).
`list`, `verify` and `restore` take no such flag and never refuse for the profile (S8): under a
candidate or a profile id the worker does not know, they report it so (R3-S1). An output says
`immutable_guarantee: true` only under a qualified profile and only for a completed archive
whose whole graph that output checked (R4-S1, R5-S1): `list` checks each root only
(`graph_checked: false`), so its `immutable_guarantee`, top-level and per archive, is always
`false`, and the guide points to `verify`. Never upgrade old unlocked history in place.
**Deployment prerequisite (round 1, R1-S1):** the store's host, disks, clock source, admin
interfaces and identity configuration are outside every cluster identity's reach, and the store
is never on a Flaminio node or control-plane VM. The guide states it; the worker cannot check it;
each backend's validation profile checks, from a cluster host, that every non-S3 interface of the
store is unreachable or refuses an unauthenticated delete, overwrite, or retention or metadata
change of a locked version (R2-S5).

**S2. Period and clock.** Backup requires a period within configured requested/effective bounds.
Object-store time/LastModified and read-back retention define coverage; cluster wall time cannot
shorten it. Reject governance, missing dates, shortening or unknown clock/profile behavior. The
proposed finalization protocol may add a bounded margin, never secretly exceed configured maximum;
its explicit minimum-period interpretation needs ADR approval against the intent's period wording.
Settled at approval, 2026-11-13: the requested-minimum interpretation (see decisions.md, Settled at approval).

**S3. Cover the complete graph.** Before listing, every exact root/page/block/`repository`
descriptor version is in compliance mode until at least the archive's verified retention floor.
Reused blocks check, then extend if needed, the referenced version, never the newest one, without
rereading or reuploading unchanged data: a version already in COMPLIANCE through E is covered
with no extension request (its one retention read, R2-C6), and a refused extension counts only
if the read-back shows E (R1-C2). Unknown preexisting/orphan content still needs #52 integrity
evidence. No archive commits on partial retention, a failed extension or a lock on the wrong
version.

**S4. Publication and interruption.** Bound finalization against a store-time commit deadline,
verify final root LastModified and all retention proofs, and publish/list only qualifying roots.
Late/uncertain/incomplete candidates stay hidden as incomplete work; retry reconciles exact
operation/root/version first. One operation has one root, as #52 (R1-C3): a late root ends its
operation incomplete, and another attempt is a new grant and operation with its own
`backup_uuid`. The operation's P, D and E are checkpointed; a rerun with another P refuses
(R1-C12). A run whose store time, plus uncertainty and an allowance for the root's own PUT, is
past D before that PUT picks a new D and E within the commit budget fixed at setup and covers
again, once, and otherwise stops before the PUT: a root known to be late is never PUT (R2-C1,
R3-C1).
Earlier archives remain readable. Arrived objects stay locked until their initial date
(contracts) or a later floor, and can remain orphaned; no rollback deletion is attempted.

**S5. Uploader policy.** Ship exact policy/templates and owner setup procedure. Uploader can add,
list/read exact versions and extend compliance coverage; it cannot DeleteObject/
DeleteObjectVersion, add delete markers, bypass/shorten/remove retention, set a legal hold, alter
bucket settings/policies/versioning/lock, or assume a stronger identity. The worker itself extends
only within the configured bounds; a cap on how far the credential can extend is the owner's, in
the bucket's policy, documented by the guide and checked by nothing (narrowed in round 1, R1-S3,
as the intent's Out of scope has it). Cluster holds no collector/owner credential. Probe only the
named disposable canaries (contracts) and reject a credential found able to delete; finite
negative probes alone do not prove all current/future IAM policy behavior. S1's deployment
prerequisite applies.

**S6. Version-stable reading under attack.** List all relevant root versions with store-assigned
commit times/retention dates and authenticate each. A newer forged root/version cannot replace an
older archive in list/verify/restore. Overwrite or delete-marker attempts either fail by policy
or leave retained versions usable by exact identity. Data/key/name/hash substitutions fail before
wrong output. A writer with repository key can forge new archives, not rewrite old locked versions;
they are distinguished by trusted store time/version, not self-reported creation time. In an
immutable repository a backup's base is only a root the worker recorded as its own completed
outcome, the newest of them by store time, else the backup is full (R1-S2, R2-S4); a leaked
repository key retires the repository. The `repository` descriptor's authority is its oldest
version in the store's version order, and an oldest version that does not authenticate refuses,
never giving way to the next (R2-S3); later versions are reported and ignored, and a root naming
another descriptor version is refused (R1-S8).

**S7. Keys.** *Narrowed after approval (round 1, R1-C1, R1-S6):* every archive here is plain, so
its only key is the repository key, escrowed off-cluster by the administrator (#52; nothing here
checks the escrow). An immutable repository keeps #52's refusal of encrypted volumes. This
increment performs no data/key GC or crypto-erasure, and no worker obtains a DEK. The encrypted
key-reference graph (retention floor, hold status, every root version), its records, RPCs,
`auth.go` rules and tests, and the parent-snapshot and KEK-rotation cases, are deferred with #52's
S8 to the plan that lifts #51's refusal of encrypted snapshots (deferred, not settled).

**S8. Restore and observability.** Repository-only listing/verify/output restore use #52's safe
paths and exact versions even after live-volume deletion or newer object versions. `list` checks
each root version only and reports `graph_checked: false`; `verify` and `restore` check the whole
graph's retention as well as its content (R1-C5). Show uploader check status, store time,
requested minimum/effective date/margin, reused-version lock requests and incomplete candidates
without exposing credentials/keys. Encrypted restore is deferred with S7.

**S9. Scope and evidence.** Nonimmutable repositories retain #52 behavior. No scheduler/policy,
post-retention collector, legal-hold management, governance mode or clean-cluster ransomware
runbook is added in this increment. Future GC uses a separate off-cluster identity and reference/
hold/key graph; until then repository grows. Serverless tests prove worker decisions, not real
store enforcement. Only later explicitly authorized, retention-aware lab checks certify the
qualified store/policy, restore behavior and cleanup after expiration; the lab's store on fl-w
proves S3-API enforcement only, never resistance to a compromised cluster (S1, R1-S1), under an
unqualified candidate profile, so its results claim no qualification (R2-S1).

## Tests

Named per rule in the design (R1-C9).
- **S1, S5:** serverless capability/policy matrix plus real later probes: missing versioning/lock,
  a default retention rule, lying lock support, governance, wrong account/prefix, missing or
  changed evidence, each probe's request and classification, a canary locked by a bucket default
  or by the uploader or held, DeleteObject, DeleteObjectVersion and DeleteObjects on the unlocked
  canary, legal hold, rows 1 and 4's absence answers and another 404 in their place, a candidate
  profile with and without the flag and an unknown profile id, on every command, and the guarantee
  under an injected qualified profile (R3-B1, R3-S1, R4-S1). Uncertain checks never count as
  safe denial.
- **S2–S4:** duration parsing and checked bounds at overflow; scripted store clocks at
  deadline/rounding edges and the root PUT's allowance; delayed root after deadline hidden and its
  operation ended; the initial lock's date; every version lock/extension/check failure; crash/lost
  reply and resume, a rerun with another P refused; a root known to be late never PUT; no listed
  early archive.
- **S3, S6:** two deduplicated archives with different floors; a reused version already past E;
  concurrent extensions to different dates; overwrite current versions, add delete markers,
  corrupt data/root metadata; a forged newer root never a base, the worker's own newest root
  still one; a later descriptor version ignored, an unauthentic oldest one refused; old exact
  versions still list/verify/restore, no unchanged payload reread/reupload, count all retention
  requests; an old reader refuses an immutable descriptor or root.
- **S7, S8:** an immutable repository's roots carry no encryption descriptor and #52's encrypted
  refusal holds; bucket-only list (root checks only, its request bound) and verify/restore (the
  whole graph) after the live volume's deletion and newer versions.
- **S9:** unchanged unlocked profile; privileged collector/owner credentials absent from worker;
  opt-in lab records cleanup dates and deletes only expendable versions after retention expires.

## Open questions

Immutable profile/IAM enforcement, requested minimum versus exact period/finalization margin,
qualified server/clock behavior and owner key-reference interfaces were Proposed until the
approval of 2026-11-13 (below); no risk is accepted. The intent's outcomes are unchanged (one Out
of scope line reworded, R1-C10) and the full issue remains open. Deferred after approval (round
1), not settled: the encrypted key-reference graph (S7); whether SeaweedFS qualifies, and which
backend profile is qualified first for a deployment, until a run under S1's prerequisite (R1-S1).

Settled at approval, 2026-11-13: the immutable profile and IAM enforcement as proposed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the period is a minimum from store commit time, with a disclosed bounded extra (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: store time and per-backend qualification, as proposed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the key-reference graph as proposed; no GC (see decisions.md, Settled at approval).
Superseded in round 1 (R1-C1, R1-S6): the key-reference graph is deferred with #52's S8; no GC.
Settled 2026-11-13 (user's instruction): SeaweedFS, the lab store #52 pins, is qualified first, by the validation profile's tests on a dedicated disposable store, and accepted only if every test passes (see decisions.md).
Round 1 (R1-S1): SeaweedFS stays the first candidate run; the run that can qualify it needs a store
off the cluster for S1's interface check, and the lab's store on fl-w proves S3-API enforcement
only. Whether it qualifies, and which profile is qualified first for a deployment, stays open.
