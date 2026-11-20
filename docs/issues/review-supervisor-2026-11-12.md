# Review-batch supervisor instructions

User scope update, 2026-11-20: #4, #5, #7, #8, #10, #11 and #30 are explicitly removed from
the remaining review queue as completed merged implementations. Their verified disposition
is `cleared_completed`; preserve historical reviews/approvals and do not claim a new review.
The terminal statuses are `clear` and `cleared_completed`. Continue only #62–#69 (8 issues).

This continues the user's task: after creating every missing intent/spec/design, run incremental
correctness/codebase-consistency and security review/address loops for **each issue** until
there are zero comments, including Nits. The original59draft sets were completed before review;
new confirmed F09 became #86 and receives the same drafts/reviews. These are draft-plan reviews,
not authorization to implement runtime features, accept ADRs, push/merge or run shared lab.

Read AGENTS.md, `.claude/skills/issue-to-spec/SKILL.md`,
`.claude/skills/spec-to-design/SKILL.md`, `.claude/skills/ship-issue/review-loop.md`,
`.claude/agents/{planner,designer,plan-reviewer}.md`. User's zero-comment/no-round-cap instruction
overrides the normal review caps/Nit exemption. Batch drafting has no approval stops; never
fabricate approvals. Existing accepted ADR files and user opencode.json remain untouched.

The root assigns one active supervisor a numbered batch. Root plus supervisor plus at most two
active child reviewers/fixers is the concurrency limit of4. Do not oversubscribe or launch hidden
external agent processes. The skills explicitly delegate fresh plan reviewers each round;
use collaboration tools. Safety paths get separate correctness/security reviewers; otherwise a
fresh combined reviewer is allowed. Never reuse a reviewer from an earlier round of that issue.
No unavailable Claude model/Skill-tool claims: use the inherited available model and role files.

Issue/worktree/branch mapping: `batch-2026-11-12-manifest.json`; source issue/comments cache:
`/tmp/flaminio-batch-issues-comments.json` (`data.repository.issues.nodes`). Re-scan actual files
and git status/HEAD. Read dependency plans in their mapped worktrees; they are not implemented
or accepted merely because files exist. Audit source/evidence and Proposed0007–0011 live in the
main checkout `docs/audits/2026-11-12/` and `docs/adr/`. Code is unchanged from8aaa7a3 except
historical branch differences; verify current source facts rather than trusting any plan.

Round1 reviews the whole intent/spec/design/local appendices and relevant code/dependencies.
Later rounds review only `git diff <previous-reviewed>..HEAD` and affected call/dependency paths,
confirm each earlier finding and check revision fallout. Record actual reviewed commit and any
reviewed dependency commits. If another session changes scoped documents/source, invalidate the
snapshot and re-review changes. Another Claude session was expected to resume at20:00 Athens;
never overwrite its concurrent work. Unrelated report/ledger-only changes do not alter semantics.

Fix every finding coherently using planner/designer roles (original author available where
helpful), with one issue-scoped fix commit per round. Preserve existing work/decisions and
historical approval records; flag revised text unapproved rather than inventing acceptance.
Use Proposed architectural choices; a lack of human approval alone is not a technical defect,
but an unspecified mechanism/contradiction cannot be hidden as a release gate. Do necessary
research and concrete draft resolution before any question. If genuinely irreducible, continue
all unaffected issues and return the precise blocker; never label unresolved findings clear.

Record every real round in issue report.md under `## Review rounds` with reviewer names,
reviewed hash, severities/evidence, changes/resolutions. `/tmp/flaminio-review-log.py` is an
optional helper; it appends provided actual JSON to report and root ledger. Author-direct reports
already exist for76/79 and must not be duplicated. The root ledger is an index; committed issue
reports are authoritative. Update `batch-2026-11-12-reviews.json` statuses/round references and
batch progress document; commit checkpoints in main only on the expected audit branch. Stage
explicit files only. Use require_escalated for authorized git metadata/outside-root writes;
auto-review is enabled. Never touch opencode.json.

Clear means actual fresh required reviewers returned no comments including Nits on the current
technical plan. A log-only final report commit does not require another technical review. Keep
implementation tests explicitly future; docs-only edits need link/whitespace checks, not invented
runtime passes. Local safe repros are allowed if needed; no shared VM/raw-disk/deploy/fault or
benchmark action without separate user request. Baseline full audit suite already passed.

Shared root causes belong once in an appropriate existing/new issue with dependencies, not
inconsistent copies. The user authorized filing necessary new issues; avoid duplicates. Any
newly filed issue also needs all3drafts and review loops, and mapping/cache/ledger entries.
Coordinate shared protocol fields/profiles and exact size bounds: known pre-draft collisions
are recorded in batch progress. Preserve per-method auth, model checks+caught mutant for every
new replication/membership/recovery safety rule, versioned format contracts and no metadata
round trips/locks added to steady-state data I/O.

Send root concise progress at meaningful checkpoints and at least every45s during long work
(current issue/round, finding or uncertainty, next step), allowing user-facing updates. Do not
end merely at the first review or normal cap. Finish the assigned batch through zero-comment
rounds, returning concise issue→clear commit/round/report mapping and precise remaining blockers.
Do not claim unreviewed issues complete. The root will assign the next batch or remaining work.
