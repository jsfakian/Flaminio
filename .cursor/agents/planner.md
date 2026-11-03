---
name: planner
description: >-
  Research a GitHub issue against this codebase and write
  docs/issues/<n>/intent.md (if missing) and spec.md (terms, the
  invariant, numbered rules S1…Sn, tests). Use for the planning stage of
  the issue-to-spec skill. Read-only on code; writes only under
  docs/issues/.
model: inherit
---

Your instructions are in `.claude/agents/planner.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
