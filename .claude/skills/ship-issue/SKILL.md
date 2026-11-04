---
name: ship-issue
description: >-
  Run the full pipeline for one GitHub issue end to end: issue-to-spec
  (human approval) -> spec-to-design (human approval) -> ship-spec
  (implementation, looped code+security review until clean, tests,
  conformance, draft PR). Use when asked to fully resolve, ship, or "just
  fix" an issue, not merely draft its spec or design.
---

# Ship issue

A thin sequencer over the three pipeline skills. It runs them in order
and honours the same human gates each already enforces:

```
issue-to-spec → [human approves spec] → spec-to-design → [human approves design] → ship-spec → [lab checks, on request]
    (HITL)                                  (HITL)                                  (unattended up to a draft PR)
```

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
- **Reviewers run read-only and are new each round** (`code-reviewer`,
  `security-reviewer`), in parallel. One implementer holds the whole
  change (Rust and Go in parallel when the design allows); each fix round
  gets a fresh one. Round 1 uses the default model; delta rounds
  `model: "sonnet"`, `effort: "medium"`. `ship-spec` has the details.
- Use these named agents, not `Explore` or persona agents from plugins.

## Steps

1. Resolve the issue from `args` (number, `#n`, URL), or ask which open
   issue if none was given or named in this conversation.
2. Invoke `issue-to-spec`. It writes the spec, shows it, and **stops for
   approval**. Honour that stop: the two human gates are the point, not
   something to optimize away.
3. On explicit approval, invoke `spec-to-design`. It writes the design,
   shows it, and **stops for approval**.
4. On explicit approval, say that everything up to a draft pull request
   now runs unattended (implementation, a review loop of at most 3
   rounds, tests, conformance), then invoke `ship-spec <n>`.
5. Relay `ship-spec`'s result plainly: clean, or the open findings and
   failures; the draft PR's URL; the lab checks still to run. Point at
   `docs/issues/<n>/report.md`.

If the user starts mid-pipeline (a spec or design already approved),
start at the matching step. Don't re-run a stage that's approved and on
disk, or re-litigate what `decisions.md` settles.
