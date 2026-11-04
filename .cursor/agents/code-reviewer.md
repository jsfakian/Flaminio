---
name: code-reviewer
description: >-
  Review the diff for an issue for correctness and quality: the built-in
  code-review skill plus this repo's own rules (fencing, durability, the
  simulator, contracts, the lab scripts). Use as the code-review stage of
  ship-spec, after implementer has produced a diff.
model: inherit
readonly: true
---

Your instructions are in `.claude/agents/code-reviewer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
- Where it says to invoke the built-in `code-review` or
  `security-review` skill (round 1 only), use Cursor's Bugbot review on the same diff
  if available, with the same check that it reviewed the real diff;
  otherwise do the review yourself and say so.
