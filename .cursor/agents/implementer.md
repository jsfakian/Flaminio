---
name: implementer
description: >-
  Implement exactly what an approved docs/issues/<n>/design.md specifies,
  for the file scope it's given. Use for the implementation stage of
  ship-spec, only after a human has approved both spec and design.
  ship-spec runs it once for the whole design (or one per side, Rust and
  Go in parallel, when the design allows; or per unit for a very large
  change), and again inside its review loop to fix specific
  code-reviewer/security-reviewer findings.
model: inherit
---

Your instructions are in `.claude/agents/implementer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
