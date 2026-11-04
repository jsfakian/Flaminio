# Review loops

The orchestrator's rules for the three loops: the **spec loop** (`issue-to-spec`), the
**design loop** (`spec-to-design`) and the **code loop** (`ship-spec`). Reviewers don't read
this file: their agent file and their brief carry what they need.

## Severity and when a loop ends

- **Critical:** data loss or corruption, a broken safety rule (fencing, durability,
  linearizability), an authorization or identity bypass.
- **High:** wrong behaviour in a realistic case, or a safety rule without its simulator check.
- **Medium:** wrong in an edge case; a rule without a test that fails without the change;
  contract or doc drift that would mislead.
- **Low:** a minor case where the plan or code is wrong or untested.
- **Nit:** clarity, naming, wording, style. Never blocks.

A round is **clear** when it reports nothing of Low or worse; the loop ends there. Every
Low-or-worse finding is fixed and another round follows; Nits are fixed when free.

- **Plan loops stop after round 3:** what is still open goes to the human gate that follows,
  listed with the plan.
- **The code loop stops after round 5** with findings open: don't ship; ask (ship-spec
  step 8).

## Rounds

- **Reviewers are new agents every round** (`plan-reviewer`, `code-reviewer`,
  `security-reviewer`), never `Explore` or plugin personas.
- **How many:** two per round (code and security focus) when the change touches a **safety
  path**: `flaminio-repl`, `flaminio-store`, `flaminio-format`,
  `flaminio-net`, `flaminio-uring`, `flaminio-io` (ublk), `meta/service`, `pkg/pki`,
  `pkg/captoken`. Otherwise one, with both focuses. The spec loop always has one.
- **Round 1 reviews the whole thing:** in the spec loop, intent and spec; in the design loop,
  the design and its consistency with the approved spec (spec lines only where the design
  changes them); in the code loop, `git diff $(git merge-base origin/main HEAD)` without
  generated files (`api/gen`, `meta/testdata/*.jsonl`).
- **Later rounds are incremental:** `git diff <reviewed>..HEAD`, where `<reviewed>` is the
  commit the last round reviewed (recorded in its log entry). The reviewer says for each
  earlier finding whether it is fixed, then what the fix broke, and reads other code only
  where the changed lines call into it or are called from it.
- **Models and effort:** round 1 the default model, with the built-in `code-review` skill at
  `high` effort on a change touching a safety path, `medium` otherwise. Later rounds
  `model: "sonnet"`, `effort: "medium"`; a `security-reviewer` on a safety-path change keeps
  the default model.
- **The brief** gives the issue number, the checkout, the round and its focus, the diff range
  (for a part of a split change, the part's paths too), the `code-review` effort (round 1
  of the code loop), the paths of `intent.md`, `spec.md`, `design.md` and `decisions.md`, and
  the earlier findings. Don't paste documents the reviewer can read.

## Fixing a round

- **One commit per round's fix, before the next round** (messages: `docs/issues/README.md`),
  the body listing each finding and what changed. Stage only the files the agents reported,
  never `git add -A`.
- **Cut before adding:** prefer removing a rule or mechanism to guarding it.
- **Code fixes come with a regression test that fails without the fix,** and `make lint test`
  (plus `make sim` / `make contracts` where they apply) pass before the commit.
- **A fix that changes the plan's rules** updates `docs/issues/<n>/` in the same commit.
- **`decisions.md`** records each choice a fix settles, each accepted risk, and why a finding
  does not apply, with the finding that raised it. Don't argue with a finding inline.
- **Log each round** in `docs/issues/<n>/report.md` under `## Review rounds`: the loop, the
  round, the commit reviewed, findings by severity, what changed.
- **From round 3,** tell the user in one line per round what is still open.

## Shared root causes

When one root cause shows up in several issues' reviews, fix it once, in an issue of its own
that the others depend on, instead of patching each.
