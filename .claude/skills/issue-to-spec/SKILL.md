---
name: issue-to-spec
description: >-
  Turn a GitHub issue (#n) into docs/issues/<n>/intent.md and spec.md
  (terms, the invariant, numbered rules S1…Sn, tests) via the planner
  agent, then stop for human approval. First stage of the issue -> spec ->
  design -> ship pipeline. Use when asked to spec out, scope, or plan an
  issue before implementing it.
---

# Issue → Spec

Stage 1 of 3 (`issue-to-spec` → `spec-to-design` → `ship-spec`). Produces
`docs/issues/<n>/intent.md` and `spec.md`, and stops for human approval.
Never chain into `spec-to-design` automatically. Conventions:
`docs/issues/README.md`.

## Steps

1. Resolve the issue:
   - `args` is a number, `#n`, a GitHub URL or `docs/issues/<n>` → that
     issue.
   - No `args` → list the open issues (`gh issue list --state open`,
     with their milestones) and ask which one, unless the user already
     named one in this conversation.
2. Branch: if on `main`, create `issue/<n>-<short-slug>` from an
   up-to-date `origin/main`. If already on another issue's branch with
   uncommitted work, stop and ask.
3. If `docs/issues/<n>/spec.md` exists, resume it: revise in place, never
   regenerate. Take settled choices from `decisions.md`.
4. Delegate to the `planner` agent (Agent tool, `subagent_type:
   planner`) with the issue number and folder. It writes `intent.md` (if
   missing) and `spec.md`. The spec states behaviour as numbered rules in
   about 150 lines at most; implementation facts (file:line, flags, exit
   codes, test vectors) go in the design. When an edit would grow it past
   that, cut something first.
5. Commit the plan: `Draft the plan of <what> (#n)`.
6. Show the spec's full content in the conversation.
7. **Stop.** Ask whether the spec is approved as written or needs
   changes. Do not invoke `spec-to-design` in the same turn, even if the
   spec looks fine. The approval must come from the user's own reply.
8. On changes: continue the same `planner` with SendMessage (it keeps
   what it has read), or edit directly; show the diff, commit
   (`Address the <first|second|…> review of the plan of #n`), ask again.
   Record each choice the feedback settles, with its reason, in
   `docs/issues/<n>/decisions.md` under `## Choices` (create the file in
   the style of `docs/issues/14/decisions.md`).
9. On approval: add `- **Spec:** approved by <user>, <YYYY-MM-DD>.` under
   `## Approvals` at the top of `decisions.md`, and commit it.

**Batch mode** (only when the user explicitly asks for many specs at
once): one `planner` per issue in parallel, each on its own branch via a
worktree (`isolation: "worktree"`), no per-issue stop, no approvals. The
user reviews them later.

## On approval

Tell the user they can run `/spec-to-design <n>` (or just ask you to
continue; don't do it unprompted).
