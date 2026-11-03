---
name: ship-spec
description: >-
  Implement an approved docs/issues/<n>/design.md end to end and
  unattended (implementation and its tests, a code-review +
  security-review loop that fixes findings and reviews again until clear,
  a conformance check against the spec), then open a draft PR.
  With "lab", run the design's lab checks, record them and mark the PR
  ready. Third and final stage of the issue -> spec -> design -> ship
  pipeline. Use only after a human has approved both spec and design.
---

# ship-spec

Follow `.claude/skills/ship-spec/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.

In Cursor:
- Delegate each named agent via Task (`subagent_type: generalPurpose`),
  with that agent's `.claude/agents/<name>.md` in the prompt, or its
  `.cursor/agents/<name>.md` where Cursor picks it up directly.
- Where the steps say "SendMessage" to resume an agent, resume it the way
  Cursor allows, or start a new one with the file on disk as its input.
- `model: "sonnet"` and `effort` don't apply; use Cursor's default model.
- Ask the user in chat where the steps say AskUserQuestion.
