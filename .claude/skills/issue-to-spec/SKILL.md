---
name: issue-to-spec
description: >-
  Turn a GitHub issue (#n) into docs/issues/<n>/intent.md and spec.md
  (terms, the invariant, numbered rules S1…Sn, tests) via the planner
  agent, review it in a loop until clear, then stop for human approval. First stage of the issue -> spec ->
  design -> ship pipeline. Use when asked to spec out, scope, or plan an
  issue before implementing it.
---

# Issue → Spec

Stage 1 of 3 (`issue-to-spec` → `spec-to-design` → `ship-spec`). Produces
`docs/issues/<n>/intent.md` and `spec.md`, and stops for human approval.
Never chain into `spec-to-design` automatically. Conventions (branch,
commits, resuming): `docs/issues/README.md`.

## Steps

1. Resolve the issue:
   - `args` is a number, `#n`, a GitHub URL or `docs/issues/<n>` → that
     issue.
   - No `args` → list the open issues (`gh issue list --state open`,
     with their milestones), drop those blocked by their "Depends on"
     and those with a branch or plan already (`git branch -a --list
     'issue/<n>-*'`, `docs/issues/<n>/`), and propose the lowest-numbered
     one left, unless the user already named one.
2. Branch: the issue's branch, as the README's Branch convention says.
   If it exists (locally, on `origin`, or in a worktree:
   `git worktree list`), resume it; never start a second plan.
3. If `docs/issues/<n>/spec.md` exists, resume it: revise in place, never
   regenerate. Take settled choices from `decisions.md`.
4. Delegate to the `planner` agent with the issue number and folder. It
   writes `intent.md` (if missing) and `spec.md`, about 150 lines at
   most.
5. Commit the plan: `Draft the plan of <what> (#n)`.
6. **Spec review loop** (`.claude/skills/ship-issue/review-loop.md`):
   one fresh `plan-reviewer` per round, focus `spec`. Fixes go through the
   same `planner` (SendMessage), so it keeps what it has read; settled
   choices go to `docs/issues/<n>/decisions.md` under `## Choices`
   (create it in the style of `docs/issues/14/decisions.md`).
7. Show the spec's full content in the conversation, with the loop's
   rounds (findings by severity, what changed) and anything left open.
8. **Stop.** Ask whether the spec is approved as written or needs
   changes. Do not invoke `spec-to-design` in the same turn, even if the
   spec looks fine. The approval must come from the user's own reply.
   On changes: the same `planner` revises, commit, run the loop again on
   what changed, show the diff, ask again. Record each choice the
   feedback settles, with its reason, in `decisions.md`.
9. On approval: add `- **Spec:** approved by <user>, <YYYY-MM-DD>.` under
   `## Approvals` at the top of `decisions.md`, and commit it.

**Batch mode** (only when the user explicitly asks for many specs at
once): create a worktree per issue first (README's Branch convention),
then one `planner` per issue in parallel, each briefed with its
worktree's path. Commit each plan in its worktree; no review loop, no
per-issue stop, no approvals. The user reviews them later.

## On approval

Tell the user they can run `/spec-to-design <n>` (or just ask you to
continue; don't do it unprompted).
