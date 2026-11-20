# Accepted with the plan, 2026-11-13: immutable archives retain exact object versions and required keys

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#53](https://github.com/jsfakian/Flaminio/issues/53)

## Context

The existing [intent](intent.md) scopes manual lock periods, target/uploader checks and protected
exact-version restore. It explicitly defers collector/policy retention and the clean-cluster
ransomware runbook. The intent asks for this ADR before the design; it was accepted with the plan
and its [design](design.md) on 2026-11-13, and changed after approval in plan review round 1
(decisions.md).
Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).

S3 compliance retention is per version; a new version/delete marker does not erase a retained
version. #52's proposed complete exact-version archive graph is the shared format. Deduplicated
versions need later coverage without data retransmission. Store clocks/IAM and key providers are
separate authorities. Proposed audit ADR0009/0011 require retained key/reference/commit evidence.

## Decisions proposed

- Immutable-v1 is fixed for a new repository at setup; never relabel unlocked older archives.
  Qualify the actual server's version/compliance/clock/conditional behavior and owner's deployed
  deny policy plus uploader negative probes. A lock flag or one failed locked-version deletion
  is not sufficient evidence of least privilege/enforcement.
- Uploader may add/list/read and extend coverage (the worker within its configured bounds; a cap
  on the credential is the owner's, unchecked: round 1, R1-S3); cannot delete object/version,
  bypass/shorten retention, set a legal hold, change bucket policy/settings or assume another
  identity. Owner/collector credentials stay outside the cluster, and the store's host, disks,
  clock, admin interfaces and identity configuration outside the cluster's reach (R1-S1). No GC
  ships in this increment.
- Cover every exact data/page/root/`repository` descriptor version. Reuse authenticated retained
  versions without data reread/reupload, checking the referenced version first and extending it
  only when needed (R1-C2).
- Interpret requested period as a minimum after qualifying root's store commit time, with bounded
  disclosed extra and maximum effective duration. A commit deadline/floor plus trusted store
  LastModified/read-back proves coverage; late/incomplete candidates are not listed. This is an
  explicit contract clarification to approve against the existing intent's period wording.
  Settled at approval, 2026-11-13: this interpretation (see decisions.md, Settled at approval).
- List/read all authenticated root versions by exact identity and store time. A later forged
  root cannot shadow old retained archives, but archives made after compromise are untrusted.
- Every archive is plain; its only key is the repository key, escrowed off-cluster. Retained
  data/KEK references of encrypted archives are deferred with #52's S8 (round 1, R1-C1, R1-S6;
  deferred, not settled). No object/key GC or crypto-erasure occurs. Actual encrypted plaintext
  restore/rotation/escrow remains #50/#54-owned; unknown key availability is never a healthy claim.

## Options

- **Lock names/current versions:** cheaper lookup but can lock/read the wrong version after an
  overwrite; incompatible with the existing intent and rejected as the proposed direction.
- **Reupload deduplicated payloads for each backup:** straightforward retention but defeats
  the intended unchanged-data reuse and cost boundary; not proposed.
- **Extend/check exact referenced versions:** O(graph-size) metadata requests, keeps payload
  reuse and stable historical reads; proposed with request counts/limits visible.
  Settled at approval, 2026-11-13: this option (see decisions.md, Settled at approval).
- **Exact period from final local return:** no atomic cross-object S3 primitive supplies that
  timestamp/boundary. Explicit store-commit minimum/deadline semantics avoid a false guarantee.

## Consequences and open choices

Store-owner/account compromise and policy drift remain external trust boundaries. Uploader can
consume quota and, with repository key, create false new archives. Long finalization/short periods
can fail rather than exceed configured coverage silently. Objects/orphans may remain locked after
an interrupted test; cleanup waits actual store expiry and uses only owner's disposable objects.

Mode/policy/server qualification and minimum/effective period semantics/precision were approved
with the plan on 2026-11-13; the encrypted key-reference/provider interfaces are deferred (round
1), and which backend profile is qualified first for a deployment stays open until a run under the
deployment prerequisite (R1-S1). Until one is, the lab runs SeaweedFS as a candidate profile,
behind an explicit flag, and its outputs claim no guarantee (round 2, R2-S1). No risk is accepted
and no runtime enforcement evidence exists.
Audit ADR0009/0011 remain Proposed; accepted metadata/fencing/ISR rules are not changed.
Settled at approval, 2026-11-13: mode, policy, qualification, period semantics and key references as proposed (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): SeaweedFS, the lab store #52 pins, is qualified first, by the validation profile's tests on a dedicated disposable store, and accepted only if every test passes (see decisions.md).

## Confirmation

[Contracts](contracts.md) and [design](design.md) map S1–S9 to capability/probe/evidence/deadline/
crash/reused-version/history tests plus later actual backend and restore checks.
The full #53 issue remains open for policy retention, GC and clean-cluster runbook.
