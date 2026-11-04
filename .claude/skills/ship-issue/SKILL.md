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
and honours the human gates each enforces:

```
intent + spec ─▶ spec review loop ─▶ [human approves spec]
  ─▶ design ─▶ design review loop ─▶ [human approves design]
  ─▶ implement + tests ─▶ code + security review loop ─▶ conformance ─▶ draft PR
  ─▶ [lab checks, on request] ─▶ PR ready
```

Loop rules: `review-loop.md`, next to this file. Branches, commits and
resuming: `docs/issues/README.md`. Agents, models and resuming within a
stage: each stage's skill.

## Steps

1. Resolve the issue as `issue-to-spec` step 1 does. If the issue's
   folder or branch exists, start at the first stage not yet approved
   (`decisions.md` `## Approvals`); never redo an approved stage.
2. Invoke `issue-to-spec`. It **stops for approval**. Honour that stop:
   the human gates are the point, not something to optimize away.
3. On explicit approval, invoke `spec-to-design`. It **stops for
   approval**.
4. On explicit approval, say that everything up to a draft pull request
   now runs unattended, then invoke `ship-spec <n>`.
5. Relay `ship-spec`'s result plainly: what each loop found in how many
   rounds, anything still open, failures; the draft PR's URL; the lab
   checks still to run. Point at `docs/issues/<n>/report.md`.
