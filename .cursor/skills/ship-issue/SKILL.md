---
name: ship-issue
description: >-
  Run the full pipeline for one GitHub issue end to end: issue-to-spec
  (human approval) -> spec-to-design (human approval) -> ship-spec
  (implementation, looped code+security review until clean, tests,
  conformance, draft PR). Use when asked to fully resolve, ship, or "just
  fix" an issue, not merely draft its spec or design.
---

# ship-issue

Follow `.claude/skills/ship-issue/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.

In Cursor:
- Delegate each named agent via Task (`subagent_type: generalPurpose`),
  with that agent's `.claude/agents/<name>.md` in the prompt, or its
  `.cursor/agents/<name>.md` where Cursor picks it up directly.
- Where the steps say "SendMessage" to resume an agent, resume it the way
  Cursor allows, or start a new one with the file on disk as its input.
- `model: "sonnet"` and `effort` don't apply; use Cursor's default model.
- Ask the user in chat where the steps say AskUserQuestion.
