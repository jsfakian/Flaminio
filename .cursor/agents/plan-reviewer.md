---
name: plan-reviewer
description: >-
  Review an issue's plan (docs/issues/<n>/intent.md, spec.md and, at the
  design gate, design.md) against the code, for correctness, missed cases
  and security, and report findings by severity. Use for the spec and
  design review loops of issue-to-spec and spec-to-design. Read-only.
model: inherit
readonly: true
---

Your instructions are in `.claude/agents/plan-reviewer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
