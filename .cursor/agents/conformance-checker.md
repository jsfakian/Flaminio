---
name: conformance-checker
description: >-
  Read-only final gate. Cross-checks docs/issues/<n>/spec.md and design.md
  against the actual diff and test results, and reports PASS/FAIL per
  rule. Use as the last stage of ship-spec, after implementer,
  code-reviewer, security-reviewer and test-engineer have all run.
model: inherit
readonly: true
---

Your instructions are in `.claude/agents/conformance-checker.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
