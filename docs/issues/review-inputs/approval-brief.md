# Approval brief — batch 2026-11-12

On 2026-11-13 the user (Ioannis Sfakianakis) decided: accept every plan in the batch now, with the
recommended answer to each open question, and keep running the review loops after approval (later
fixes are recorded as changes made after approval). You record that decision for the issues in
your prompt. You do not review or revise the plans.

## For each issue in your list

1. Find its worktree and branch in
   `/home/jsfakian/Documents/src/flaminio/docs/issues/batch-2026-11-12-manifest.json`, and its
   review status in `docs/issues/batch-2026-11-12-reviews.json` (same folder). Run
   `git -C <worktree> status --short -- docs/issues/<n>/`: if anything is modified or untracked
   there, another agent is working on it: skip the issue and say so.
2. Read `decisions.md` (if present), `spec.md`, `design.md`, and any appendix (`adr-proposal.md`,
   contracts files) to find every open question: sections named Open questions or Questions, items
   numbered Q1…, and choices labelled Proposed that the plan leaves for a human. For each:
   - the plan states a recommended answer or a Proposed choice: it is settled at approval with
     that answer;
   - the plan states no recommendation: record "no recommendation in the plan; stays open". Never
     invent an answer.
3. `decisions.md` (create it as `# Issue #<n> — decisions` if missing). Under `## Approvals`
   (create the section if missing; keep any line already there, and don't duplicate one), add:

   ```
   - **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
   - **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
   ```

   then one paragraph: "Approved with the recommended answer to each open question (see Settled at
   approval). The plan's review loop continues after approval; later fixes are recorded as changes
   made after approval. Nothing is implemented." If the issue's review status is not `clear`, add:
   "Approved before its plan review cleared (status: <status>)." Then a section
   `## Settled at approval` listing each open question, the answer chosen (or "stays open"), and
   where the question is in the plan (file and section).
4. `adr-proposal.md`, if present: change only its status line to "Accepted with the plan,
   2026-11-13; numbered and moved into docs/adr/ when implemented." Never edit `docs/adr/`.
5. `intent.md`, `spec.md`, `design.md`: only if one has an explicit status line saying draft, not
   approved or unapproved, change that line to "Approved 2026-11-13; not implemented." Change
   nothing else in them.
5b. A plan must not contradict its own approval. Wherever `spec.md`, `design.md`, a contracts
   file or another appendix still presents a question or choice you settled as open or Proposed
   (an Open questions section, a Risks item, an ADR section, a header such as "Everything here is
   Proposed"), add one line at that place: "Settled at approval, 2026-11-13: <answer> (see
   decisions.md, Settled at approval)." For a whole-file header, change "Proposed" to "Accepted
   with the plan, 2026-11-13". A question that stays open gets "Still open after approval". Change
   nothing else in those files. (A reviewer found this contradiction in a plan approved without
   this step.)
6. Commit, one commit per issue, staging nothing else: `git -C <worktree> add <the files you changed>`
   then `git -C <worktree> commit -m "Record the approval of the plan of #<n>" -m "Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>" -- <the same files>`.
   The `-- <files>` matters: another session commits in the same checkout, so commit only your
   paths. If git reports an index lock, wait a few seconds and retry. Don't push. Don't touch
   `report.md`, other issues' folders, code, `docs/adr/`, or `opencode.json`.
7. Before committing, `git -C <worktree> diff --check` on your files is clean and any relative link
   you added resolves.

## Final message

One line per issue: `#<n> <commit> — <k> settled, <m> left open` (name the open ones briefly), or
`#<n> skipped — <reason>`. Then list any Proposed ADR in `docs/adr/` (for example 0007–0011) that a
plan depends on, by issue, since accepting those is a separate step. Nothing else.
