# Plan review brief — batch 2026-11-12

You review the draft plan of one GitHub issue of Flaminio. You do not edit the plan or the
code. Your agent instructions (checklists, severities) apply; this brief adds the batch's context.

## Context
- The plan is in the issue's worktree at `docs/issues/<n>/`: `intent.md`, `spec.md`,
  `design.md`, and any appendix in that folder (`adr-proposal.md`, contract files,
  `decisions.md`). They are unapproved drafts written in batch mode. The spec had no review
  loop of its own, so **round 1 covers the whole folder** (except `report.md`): the plan
  checklists on intent and spec, the design checklists on design and appendices.
- Verify every claim against the code in that same worktree (`AGENTS.md` has the map).
- Nothing in the plan is implemented; the tests it names are future tests. Judge whether they
  would prove the rules, not whether they exist.
- Not findings: the lack of a human approval; a choice labelled Proposed. Findings: a
  mechanism left unspecified, a claim the code contradicts, a contradiction with an accepted
  ADR (`docs/adr/`, status Accepted) or with `AGENTS.md`'s Principles, a rule without a test.
- Other issues' plans are drafts in their own worktrees (map:
  `/home/jsfakian/Documents/src/flaminio/docs/issues/batch-2026-11-12-manifest.json`). Read a
  dependency's plan only where this plan relies on it, and flag contradictions (protobuf
  field numbers, size bounds, shared contracts). ADRs 0007–0011 and the audit under
  `/home/jsfakian/Documents/src/flaminio/docs/audits/2026-11-12/` are proposals.
- Read-only: no `make`, no lab or network commands, nothing that changes the worktree.
- This batch clears a plan only at zero comments, Nits included. So report every
  Low-or-worse finding, and a Nit only when it is real (a wrong name, a broken link, wording
  that would mislead an implementer). Don't pad; don't restate taste.

## Rounds
- Round 1: the whole plan folder.
- Later rounds: only `git diff <reviewed>..HEAD -- docs/issues/<n>/ ':!docs/issues/<n>/report.md'`.
  First say for each earlier finding (file given in your prompt) whether it is fixed, then
  what the revision broke. Read other text or code only where the changed lines depend on it.

## Output
Write the full report to the output file your prompt names (create it with a shell heredoc):
one `### <ID> — <Severity>` section per finding, most severe first, each with a summary, the
evidence (`file:line` or the document's section) and a fix. IDs: `R<round>-C<k>` (code
focus), `R<round>-S<k>` (security focus), `R<round>-B<k>` (both focuses). In later rounds,
start with a `## Earlier findings` list (id: fixed / not fixed, one line). End with
`Reviewed commit: <git rev-parse HEAD>`.

Your final message is only: the reviewed commit, the count per severity, then one line of at
most 30 words per finding (id, severity, what is wrong). With no findings at all, the final
message is `CLEAR — zero comments including Nits` and the reviewed commit.

## The issue's own text
The GitHub issue and its comments are cached in `/tmp/flaminio-batch-issues-comments.json`
(`data.repository.issues.nodes`, by `number`); read its "Done when" from there, not the network.
