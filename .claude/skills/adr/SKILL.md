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

# ADR

Rules: `docs/adr/README.md`. Read it, the index, and one existing ADR for
the format before writing.

## Create

1. Next number: the highest `docs/adr/NNNN-*.md` + 1 (never reuse one).
2. `docs/adr/NNNN-kebab-case-title.md`, in the existing format:
   `# NNNN. <The decision as a statement>`, then `- **Status:**` and
   `- **Date:**`, then `## Context`, `## Decision`, `## Consequences`.
   Add `## Options` when more than one was genuinely considered (each
   with honest pros and cons), and `## Confirmation`: how review, tests,
   the simulator or CI catch drift from it.
3. Fill it from evidence: link the issue, spec, design, benchmarks and
   ARCHITECTURE sections; consequences include the bad ones and the risks
   accepted (as ADR 0006 does).
4. Status:
   - **Accepted** only if the user decided it in this conversation.
   - Otherwise **Proposed**. If the choice is genuinely open, ask the
     user (AskUserQuestion) before writing instead of picking silently.
5. Add its row to the index table in `docs/adr/README.md`.
6. If code or docs already contradict it, list the follow-ups at the
   end. Don't refactor in the same step unless asked.

## Supersede

Accepted ADRs are not edited. Write a new ADR whose context says what
changed; then change only the old one's status line to
`Superseded by NNNN` and its index row.

## Review a change against ADRs

For a diff or design: list the ADRs it touches; for each, check its
Decision (and Confirmation, where there is one). Report `consistent`,
`contradicts NNNN: <how>`, or `needs a new ADR: <decision>`.
