# Review loops

One set of rules for the three loops of the pipeline: the **spec loop** (`issue-to-spec`), the
**design loop** (`spec-to-design`) and the **code loop** (`ship-spec`). Each skill says which
loop it runs and with which reviewers; everything else is here.

## Severity and when a loop ends

Every finding carries one: **Critical / High / Medium / Low / Nit**. A round is **clear** when
it reports no finding of Low or worse; the loop ends at the first clear round.

- **Every Low-or-worse finding is fixed,** Lows included, and another round follows. Nits are
  fixed when free or noted; they never block.
- **From round 5,** tell the user, in one line per round, which round the loop is in and
  what is still open.
- **Round 10 still has findings of Low or worse:** stop and ask. The plan or the change
  likely needs rethinking, not another patch.

## Rounds

- **Reviewers are new agents every round,** so no round anchors on an earlier verdict. Use
  the named agents (`plan-reviewer`, `code-reviewer`, `security-reviewer`), never `Explore` or
  persona agents from plugins.
- **Round 1 reviews the whole thing:** the plan (intent and spec; in the design loop, intent,
  spec and design together) or the change (`git diff $(git merge-base origin/main HEAD)`,
  generated files such as `api/gen` and `meta/testdata/*.jsonl` excluded).
- **Every later round is incremental:** only the files the last fix changed
  (`git diff HEAD~1 --stat`), and in them only that fix (`git diff HEAD~1 -- <files>`). The
  reviewer checks that each earlier finding is fixed and what the fix broke, reading other
  code only where the changed lines call into it or are called from it. It doesn't re-audit
  unchanged files or lines.
- **Models:** round 1 uses the default model; incremental rounds `model: "sonnet"`, `effort:
  "medium"`.
- **Every reviewer gets** the issue number, the checkout, the paths of `intent.md`, `spec.md`,
  `design.md` and `decisions.md` (its entries are settled unless the code proves one wrong),
  the round's diff and its list of earlier findings, and the instruction to verify claims
  against the code rather than trust the documents.
- **What a reviewer reports:** every Low-or-worse finding, most severe first, each with
  evidence (file:line, or the document's section) and a fix; at most 5 Nits. Checklists say
  where to look; they are not quotas.

## Fixing a round

- **One commit per round's fix, before the next round,** so the next reviewer reads exactly
  what changed. Plan loops: `Address the <first|second|…> review of the plan of #n` (or `of
  the design of #n`); code loop: `Address the <first|second|…> code [and security] review[s]
  of #n`, the body listing each finding and what changed.
- **Cut before adding:** prefer removing a rule or mechanism to guarding it.
- **Code fixes come with a regression test that fails without the fix,** and the local checks
  (`make lint test`, plus `make sim` / `make contracts` where they apply) pass before the
  commit.
- **A fix that changes the plan's rules** updates `docs/issues/<n>/` in the same commit.
- **`decisions.md`** records each choice a fix settles, each accepted risk, and why a finding
  does not apply, with the finding that raised it.
- **A finding that looks wrong:** fix what it points at, or record in `decisions.md` why it
  does not apply. Don't argue with it inline.
- **Log each round** in `docs/issues/<n>/report.md` under `## Review rounds`: the loop, the
  round, findings by severity, what changed.

## Shared root causes

When one root cause shows up in several issues' reviews, fix it once, in an issue of its own
that the others depend on, instead of patching each.
