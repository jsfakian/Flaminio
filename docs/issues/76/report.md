# Issue #76 — report

## Review rounds

### Plan review round 1 — 08a26343a321ff9d78f94446c18cd0c0fa79932f

Scope: draft plan/codebase consistency and security; no runtime implementation.
Reviewers: review76_code_r1 (design-code), review76_sec_r1 (design-security).

- **R1-C1 — Medium:** the 64-root/8-transition budget excluded raw-header discovery and delegated
  legacy deletion to full List/deleteChunks loops (`service.go:548–577`). **Addressed in plan:**
  four bounded discovery sources, shared operation/work budgets including backend retries,
  bounded keys/value APIs and a proof-based single-CAS unpublished rollback followed by paged
  orphan metadata cleanup. No recovery/compatible-create by-name delegation remains. Legacy
  cleanup resumes from remaining keys, including across crashes and more than one pass.
- **R1-C2 — Low:** DeleteVolume preserves Creating when marking Deleting (`service.go:539–542`);
  root GC could turn a legitimate interrupted deletion into a missing-root corruption report.
  **Addressed in plan:** classify Deleting first; recognized never-published deletion resumes
  without a root, published deletion stays on its conservative path, and only nondeleting
  new-format Creating requires a live root. Add both-flags/root-GC regression cases.
- **R1-S1 — High:** mixed delete/progress-Put GC fails under real etcd NOSPACE (pinned 3.7.3
  `etcdserver/apply/capped.go:37–40`, `storage/quota.go:151–173`). **Addressed in plan:** eligible
  root deletion is the fence, orphan plan cleanup compares root absence/per-key revisions, and
  all GC transactions are delete-only. Fixed-cardinality birth-revision CAS prevents even an
  initial transaction's transparent retry resurrecting the root after GC. Embedded-etcd real
  alarm/reply-loss/restart tests replace a fake quota assumption; operator space/alarm recovery
  remains explicit and live admitted operations cannot be revoked for pressure alone.
- **R1-S2 — High:** a progress-compare mutant does not test the destructive expected-incarnation
  fence. **Addressed in plan:** add distinct missing-header/incarnation-compare and public-by-name
  substitution mutants, plus the actual Go CAS schedule paused after an old read, followed by
  legitimate deletion/recreation/publication and release of the stale deleter. Both model and
  backend tests must catch replacement damage; retain progress/early-publication mutants too.
- **R1-S3 — Medium:** full-prefix discovery fetched all plan values/headers before processing
  limits. **Addressed in plan:** separate roots/plans; ScanKeys latest pages <=64 keys/32KiB wire,
  bounded point reads <=8KiB value/16KiB wire, ordered Mem index with no full-map copy/sort/MVCC
  clone or global expiry sweep in these APIs. Add million-key/abandoned-root/header fixtures,
  incremental memory and per-pass work/continuation/fairness assertions, including all new helpers.
- **R1-S4 — Medium:** unbounded PreferNode is persisted twice and can break atomic admission's
  byte cap. **Addressed in plan:** empty-or-ValidID (ASCII1–128), canonical identity<=1024 bytes
  and root/header/chunk<=8192 before any write, with strict persisted validation and bounded
  diagnostics. Tests cover 300KiB/invalid/maximum valid input, malformed persisted identity and
  no staging on rejection. Atomic transactions retain the <=512KiB cap.

Proof checked for the narrower legacy exception: initial Creating claim epoch0/no target;
publication clears Creating and starts the positive claim epoch in one transaction
(`service.go:409–473`); readHeader/Attach refuse hidden records before token issuance
(`:275–289,696–744`). Existing #30 first-epoch/monotonic-revision fencing still governs earlier
incarnation tokens. Unknown/nonzero-epoch/target-bearing states have no shortcut; pre-existing
tombstones and authorization/protection preconditions remain. No paged tombstone API is added.

Validation: inspected the referenced code and pinned etcd source; ran document whitespace,
relative-link and rule/line-budget checks. Runtime/model/NOSPACE tests listed above are required
future implementation tests, not results of this documentation fix.

Status: needs_incremental_review. All six findings have proposed plan resolutions; no fresh
review/all-clear, runtime verification, approval or accepted availability risk is inferred.

### Plan review round 2 — b084261ae6575bdd0e3f3806e18dd2453bfa6111

Scope: incremental draft review from R1's reviewed commit through its fix; no implementation.
Reviewers: review76_code_r2 (design-code), review76_sec_r2 (design-security).
Both confirmed all six R1 findings resolved. The birth fence and unpublished proof were judged
sound; the following executable-proof/cursor gaps remained. Security independently confirmed
the birth-mutation gap, with no additional ABA/quota/protocol finding.

- **R2-C1 — High:** the new birth-revision fence had a functional backend regression but no
  explicit simulator invariant/caught mutant. **Addressed in plan:** NoRootResurrection records
  retired operation IDs; model the original transaction's initial commit, lost reply, root GC
  and transparent backend replay. missing_birth_compare must yield a counterexample. The same
  saved-transaction schedule replays actual Go transactions on memory and etcd; progress-fence
  mutants and a positive backend test do not substitute for this proof.
- **R2-C2 — Medium:** fetching four 64-key pages but processing only 64 items could advance cursors
  past unexamined tails and starve stable entries. **Addressed in plan:** retain each <=64-key
  source tail plus fetched next/done; cursor advancement follows only examined keys. Do not
  overwrite/refetch or wrap until its tail is consumed. Cancellation preserves an unexamined
  entry, and missing/error/skipped examined entries consume the same item budget. Restart may
  safely rescan, never checkpoint fetched-but-unexamined progress. All four sources populated,
  mid-page pause/budget exhaustion and wrap/restart fixtures must prove eventual eligible work
  attempts without skipping tails while retaining the existing memory/work limits.
- **R2-S1 — High:** the tombstone exception lacked its own independently checked model predicate
  and a caught weakened_unpublished_classifier mutant. **Addressed in plan:** NoUnprovedTombstoneBypass uses
  logical-incarnation publication/Attach/token history independently of mutable flags, plus the
  recognized-writer/exclusion/Creating/zero-epoch/empty-target proof. Creating-only classifier
  mutation must fail on unsupported or previously published/authorized shapes. Replay actual Go
  publication/Attach/token issuance before unsupported-shape injection, name replacement and
  delayed prior-incarnation tokens; valid never-published rollback is still allowed.

Validation: document whitespace, relative links, rule references, required invariant/mutant
names, issue-only scope and line-budget checks. No model/backend/NOSPACE/runtime/lab test was
executed; these additions specify required implementation checks rather than claim results.

Status: needs_incremental_review. All three R2 findings have proposed resolutions; R3 remains
necessary. No approval, accepted risk, runtime verification or all-clear is inferred.

### Plan review round 3 — 9642649278ca6bced116d11f417ca27e45e6f025

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r76c3 (incremental design-code), r76s3 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed all R2 findings resolved: independent root-resurrection and tombstone-bypass invariants/caught mutations, actual backend replays, retained page tails and examination-based cursor fairness. Affected code/dependency paths checked; #78 technical9f0a4b0 current f672280 adds report only. Zero comments including Nits. Runtime implementation/tests and approval remain separate.

Status: clear. Approval remains separate; no approval is inferred.
