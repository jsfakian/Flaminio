---
name: adr
description: >-
  Create, supersede, or review Architecture Decision Records in docs/adr/.
  Use when a change picks a language, library, service or protocol,
  alters a cross-cutting rule (replication, fencing, leases, the on-disk
  format, identity and authorization, placement, target environments),
  when asked to "write an ADR" / "record this decision", or to check
  whether a proposed change contradicts an accepted ADR.
---

# adr

Follow `.claude/skills/adr/SKILL.md` exactly: it is the one source for
this skill. Read it first, then run its steps.

In Cursor:
- Delegate each named agent via Task (`subagent_type: generalPurpose`),
  with that agent's `.claude/agents/<name>.md` in the prompt, or its
  `.cursor/agents/<name>.md` where Cursor picks it up directly.
- Where the steps say "SendMessage" to resume an agent, resume it the way
  Cursor allows, or start a new one with the file on disk as its input.
- `model: "sonnet"` and `effort` don't apply; use Cursor's default model.
- Ask the user in chat where the steps say AskUserQuestion.
