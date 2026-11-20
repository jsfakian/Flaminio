# Settle brief — open questions, batch 2026-11-12

On 2026-11-13 the user (Ioannis Sfakianakis) decided: "For all open questions go with the
recommendations." Every plan already records the approval of 2026-11-13. This step settles what
that approval left open. You record decisions; you do not review or redesign the plans.

## What counts as open

In the issue's `docs/issues/<n>/` (read `decisions.md` first, then spec, design and appendices):
- a question marked "Still open after approval", "stays open", "no recommendation", or left in an
  Open questions section or numbered Q list without a settled answer;
- a change made after approval and marked "not accepted" (for example a risk found after
  approval, or an approved answer changed later).

## How to settle each

- If the plan states a recommendation or a preferred option anywhere: take it.
- If it does not: choose the option that is simplest and safest given the plan's own rules,
  `AGENTS.md`'s Principles and the code as it is today; never one that weakens fencing,
  durability, authorization or a stated invariant. Write one sentence of reason. If the question
  is a measurement or evidence to gather later (a benchmark, a hardware result), the decision is
  the plan's own procedure for gathering it, not a guessed result.
- Never settle a question without a plan recommendation by choosing an option that narrows the
  intent or a spec rule. Choose nothing in this plan instead, name the ADR or later plan that
  decides before implementation, and say the intent's promise stands until then (the user asked
  for #63 to be made "in line with the others" after it was settled by narrowing).
- For a change after approval marked "not accepted": accept the plan's current text, unless it
  states a better recommended alternative.

## Recording

1. `decisions.md`: add a section `## Settled at the user's instruction of 2026-11-13` with one
   bullet per item: the question, the answer, and either "the plan's recommendation" or
   "recommended by Claude, no recommendation in the plan: <reason>"; for an accepted change after
   approval, say it is accepted under this instruction.
2. At each place the plan said the item was open or not accepted, replace that marker with
   "Settled 2026-11-13 (user's instruction): <answer> (see decisions.md)". Change nothing else.
3. Check the worktree first: if `git status --short -- docs/issues/<n>/` shows changes, skip the
   issue and say so.
4. One commit per issue, only your files, with the `-- <paths>` form:
   `git -C <worktree> add <files>` then
   `git -C <worktree> commit -m "Settle the open questions of #<n>" -m "Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>" -- <files>`.
   Don't push. `git diff --check` clean first. Don't touch `report.md`, code, `docs/adr/`, other
   issues' folders or `opencode.json`. If an issue has nothing open, make no commit.

## Final message

One line per issue: `#<n> <commit> — <k> settled (<p> the plan's, <c> recommended by Claude)`,
or `#<n> nothing open`, or `#<n> skipped — <reason>`. Then list every item you settled without a
recommendation in the plan, with your answer, so the user can check them. Nothing else.
