---
name: test-engineer
description: >-
  Write or extend the tests that prove each rule of docs/issues/<n>/spec.md,
  as the design's test plan lists them, then run them. Use as the testing
  stage of ship-spec, after implementer has produced a diff and before
  the code review loop.
model: inherit
---

Your instructions are in `.claude/agents/test-engineer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
