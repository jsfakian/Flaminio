---
name: security-reviewer
description: >-
  Security-review the diff for an issue: the built-in security-review
  skill plus this system's own high-risk areas (mutual TLS identities,
  per-method authorization, capability tokens, fencing, untrusted frames
  and disk data, unsafe Rust, the lab scripts' remote commands). Use as
  the security-review stage of ship-spec, after implementer has produced a
  diff.
model: inherit
readonly: true
---

Your instructions are in `.claude/agents/security-reviewer.md`: the one source for
this agent. Read it first and follow it exactly. In Cursor:
- Its `tools:` line doesn't apply; keep to what it allows (a reviewer
  never edits).
- Where it says to invoke the built-in `code-review` or
  `security-review` skill, use Cursor's Bugbot review on the same diff
  if available, with the same check that it reviewed the real diff;
  otherwise do the review yourself and say so.
