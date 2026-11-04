# Issues

GitHub issues are the tracker. Each issue we build gets a folder named after its number, with
its plan and the pipeline's records:

```
docs/issues/<n>/
  intent.md     # why, what we want, out of scope: from the GitHub issue and the evidence
  spec.md       # planner: terms, the invariant, numbered rules S1…Sn, tests
  design.md     # designer: how each rule is built, contracts, tests, lab, risks
  decisions.md  # approvals, settled choices and accepted risks; reviewers take them as given
  units.md      # ship-spec: the implementation units, only for a change too large for one agent
  report.md     # every loop's review rounds; tests, conformance, lab checks
```

## Pipeline

`/issue-to-spec <n>` (with a spec review loop) → human approves `spec.md` →
`/spec-to-design <n>` (with a design review loop on the whole plan) → human approves
`design.md` → `/ship-spec <n>` implements it with its tests (one agent, or Rust and Go in
parallel), runs a code- and security-review loop until clear and a final conformance check,
then opens a **draft** pull request. The loops' rules:
[`.claude/skills/ship-issue/review-loop.md`](../../.claude/skills/ship-issue/review-loop.md). Lab
checks, which need the shared lab and can take hours, run later on request
(`/ship-spec <n> lab`); their results go into the pull request, which is then marked ready.
`/ship-issue <n>` runs the whole thing, stopping at the same two approvals.

Approvals are recorded in `decisions.md`, under `## Approvals`, with the date. Nothing is
implemented before both the spec and the design are approved.

## Conventions

- **Branch:** `issue/<n>-<short-slug>`, from an up-to-date `origin/main`, checked out in the
  main checkout (commit or stash other work first; nothing uncommitted is carried over). A
  worktree (`git worktree add ../flaminio-<n> -b issue/<n>-<slug> origin/main`) only when
  several issues are worked on in parallel; remove it once its pull request is merged. The
  plan, the code, each review round and the lab results are commits on the branch, pushed with
  a pull request.
- **Resuming:** an issue's branch or worktree that exists is resumed, never started again
  (`git branch -a --list 'issue/<n>-*'`, `git worktree list`); progress is in the folder,
  `git log origin/main..HEAD` and `decisions.md`.
- **Commits:** `Draft the plan of <what> (#n)`, `Address the review of the plan of #n, round
  N`, `Record the approval of the spec of #n`, `Design <what> (#n)`, `Address the review of
  the design of #n, round N`, `Record the approval of the design of #n`, `<What the change
  does> (#n)`, `Test <what> (#n)`, `Address the code and security reviews of #n, round N`,
  `Accept ADR NNNN (#n)`, `Record the lab checks of #n`.
- **Pull request:** title `<What the change does> (#n)`; body `Closes #n.` (or `Part of #n.`
  when a step of the issue stays open), then `## Why`, `## What changes`, `## Results`,
  `## Tests`.
- **The issue's step:** when a change is one step of an issue, the intent says which, and the
  issue stays open.
- **A shared root cause** found in several issues' reviews is fixed once, in an issue of its
  own that the others depend on.
- `docs/followups.md`, `docs/performance-plan.md` and `docs/lab.md` are updated in the same
  change when the issue's state, results or lab procedure change.
