---
name: spec-to-design
description: >-
  Turn an approved docs/issues/<n>/spec.md into design.md (how each rule
  is built, files touched, contracts, tests, lab checks, risks, ADRs) via
  the designer agent, review the whole plan in a loop until clear, then
  stop for human approval. Second stage of the
  issue -> spec -> design -> ship pipeline. Use after a human has approved
  a spec produced by issue-to-spec.
---

# Spec → Design

Stage 2 of 3. Produces `docs/issues/<n>/design.md` and stops for human
approval. Never chain into `ship-spec` automatically.

## Preconditions

The spec is approved: `decisions.md` has `**Spec:** approved` under
`## Approvals`, or the user approved it earlier in this conversation (or
tells you to proceed now). Otherwise say so and stop. **Batch mode**
(only when the user explicitly asks): one `designer` per issue in
parallel against unapproved specs, no per-issue stop.

## Steps

1. Resolve the spec from `args` (`<n>`, `#n` or a path), or the most
   recently approved spec in this conversation. Work on its
   `issue/<n>-…` branch.
2. If `design.md` exists, resume it: revise in place, never regenerate.
3. Delegate to the `designer` agent (Agent tool, `subagent_type:
   designer`) with the spec path. Give it `decisions.md`; its entries are
   settled. The design carries the implementation facts the spec leaves
   out (file:line, flags, exit codes, test vectors).
4. Commit it: `Design <what> (#n)`.
5. **Design review loop** (`.claude/skills/ship-issue/review-loop.md`),
   a loop of its own after the spec's, on the whole plan (intent, spec
   and design together, so a design finding that changes the spec is
   caught): two fresh `plan-reviewer`s per round, in parallel, focus
   `design-code` and `design-security`. Fixes go through the same
   `designer` (SendMessage), and reach the spec where a finding does.
6. Show the design's full content, including the rule → test mapping,
   the lab checks and `## ADRs`, with both loops' rounds. If the design drafted an ADR, show it too and say that
   approving the design accepts it.
7. **Stop.** Ask whether the design is approved as written or needs
   changes. Do not invoke `ship-spec` in the same turn. On changes: the
   same `designer` revises, commit, run the loop again on what changed,
   show the diff, ask again. Record each settled choice, with its
   reason, in `decisions.md`.
8. On approval: add `- **Design:** approved by <user>, <YYYY-MM-DD>.`
   under `## Approvals` in `decisions.md`, and commit it.

## On approval

Tell the user they can run `/ship-spec <n>` (or just ask you to
continue). From there it runs unattended up to a draft pull request; say
so, so they don't expect another checkpoint until it reports back.
