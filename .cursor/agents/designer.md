---
name: designer
description: >-
  Turn an approved docs/issues/<n>/spec.md into design.md (how each rule
  is built, files touched, contracts, tests, lab checks, risks, ADRs). Use
  for the design stage of spec-to-design, only after a human has approved
  the spec. Read-only on code; writes only under docs/issues/ (and a
  proposed ADR in docs/adr/).
model: inherit
---

Your instructions are in `.claude/agents/designer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows.
