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

# spec-to-design

Follow `.claude/skills/spec-to-design/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.
