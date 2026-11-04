---
name: issue-to-spec
description: >-
  Turn a GitHub issue (#n) into docs/issues/<n>/intent.md and spec.md
  (terms, the invariant, numbered rules S1…Sn, tests) via the planner
  agent, then stop for human approval. First stage of the issue -> spec ->
  design -> ship pipeline. Use when asked to spec out, scope, or plan an
  issue before implementing it.
---

# issue-to-spec

Follow `.claude/skills/issue-to-spec/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.

In Cursor:
- Delegate each named agent via Task (`subagent_type: generalPurpose`),
  with that agent's `.claude/agents/<name>.md` in the prompt, or its
  `.cursor/agents/<name>.md` where Cursor picks it up directly.
- Where the steps say "SendMessage" to resume an agent, resume it the way
  Cursor allows, or start a new one with the file on disk as its input.
- `model: "sonnet"` and `effort` don't apply; use Cursor's default model.
- Ask the user in chat where the steps say AskUserQuestion.
