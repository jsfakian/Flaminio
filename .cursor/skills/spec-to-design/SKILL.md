---
name: spec-to-design
description: >-
  Turn an approved docs/issues/<n>/spec.md into design.md (how each rule
  is built, files touched, contracts, tests, lab checks, risks, ADRs) via
  the designer agent, then stop for human approval. Second stage of the
  issue -> spec -> design -> ship pipeline. Use after a human has approved
  a spec produced by issue-to-spec.
---

# spec-to-design

Follow `.claude/skills/spec-to-design/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.

In Cursor:
- Delegate each named agent via Task (`subagent_type: generalPurpose`),
  with that agent's `.claude/agents/<name>.md` in the prompt, or its
  `.cursor/agents/<name>.md` where Cursor picks it up directly.
- Where the steps say "SendMessage" to resume an agent, resume it the way
  Cursor allows, or start a new one with the file on disk as its input.
- `model: "sonnet"` and `effort` don't apply; use Cursor's default model.
- Ask the user in chat where the steps say AskUserQuestion.
