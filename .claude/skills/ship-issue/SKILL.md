---
name: ship-issue
description: >-
  Run the full pipeline for one GitHub issue end to end: issue-to-spec
  (spec review loop, human approval) -> spec-to-design (design review
  loop, human approval) -> ship-spec (implementation and tests, code and
  security review loop until clear, conformance, draft PR). Use when asked to fully resolve, ship, or "just
  fix" an issue, not merely draft its spec or design.
---

# Ship issue

A thin sequencer over the three pipeline skills. It runs them in order
and honours the same human gates each already enforces:

```
intent + spec ─▶ spec review loop ─▶ [human approves spec]
  ─▶ design ─▶ design review loop (whole plan) ─▶ [human approves design]
  ─▶ implement + tests ─▶ code + security review loop ─▶ conformance ─▶ draft PR
  ─▶ [lab checks, on request] ─▶ PR ready
```

The review loops' rules (rounds, severities, when a loop ends):
`review-loop.md`, next to this file.

## Context resets

A long run can lose its context mid-stage (compaction, a resumed
session). Never regenerate a stage whose output exists: an existing
`spec.md` or `design.md` is resumed or revised in place. Take progress
from the issue folder, `git log origin/main..HEAD` and `git status`, and
treat `decisions.md` as settled.

## Agents

- **Drafters are resumed, not re-spawned:** `planner` and `designer`
  write the spec and design, one per issue; on feedback, continue the
  same agent with SendMessage so it keeps the code it has read.
- **Reviewers run read-only and are new each round:** `plan-reviewer`
  for the spec and design loops, `code-reviewer` and `security-reviewer`
  (in parallel) for the code loop. Models per round: `review-loop.md`.
- **One implementer holds the whole change** (Rust and Go in parallel
  when the design allows); each fix round gets a fresh one.
- Use these named agents, not `Explore` or persona agents from plugins.

## Steps

1. Resolve the issue from `args` (number, `#n`, URL). With none given
   or named in this conversation, propose the lowest-numbered open issue
   not blocked by its "Depends on".
2. Invoke `issue-to-spec`. It writes the spec, shows it, and **stops for
   approval**. Honour that stop: the two human gates are the point, not
   something to optimize away.
3. On explicit approval, invoke `spec-to-design`. It writes the design,
   shows it, and **stops for approval**.
4. On explicit approval, say that everything up to a draft pull request
   now runs unattended (implementation and tests, the code review loop
   until clear, conformance), then invoke `ship-spec <n>`.
5. Relay `ship-spec`'s result plainly: what each loop found in how many
   rounds, anything still open, failures; the draft PR's URL; the lab
   checks still to run. Point at
   `docs/issues/<n>/report.md`.

If the user starts mid-pipeline (a spec or design already approved),
start at the matching step. Don't re-run a stage that's approved and on
disk, or re-litigate what `decisions.md` settles.
