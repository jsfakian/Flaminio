---
name: ship-issue
description: >-
  Run one Flaminio GitHub issue end to end: intent and spec (reviewed, then
  human approval) -> design (its own review loop on the whole plan, then
  human approval) -> implementation with a code + security review loop ->
  tests, lab checks and results -> PR. Every review loop fixes and
  re-reviews until no Low-or-worse findings remain. Use when asked to ship,
  resolve, "continue" or "just fix" an issue, not merely to draft its spec
  or design.
---

# Ship issue (Flaminio)

```
intent.md + spec.md ─▶ plan review loop ─▶ [human approves spec]
      ─▶ design.md ─▶ design review loop (whole plan) ─▶ [human approves design]
      ─▶ implement ─▶ code + security review loop ─▶ tests, lab, results ─▶ PR
```

The two human gates are the point, not overhead: never pass one without the
user's own reply in this conversation. Everything after the design gate runs
unattended until the PR is open.

If the issue is mid-pipeline (an approved spec or design already on its
branch, or a previous session left it part-way), start at the matching step:
read the branch (`git log main..`), `docs/issues/<n>/` and the issue's comments
first, and do not redo or re-litigate approved stages.

## Where things go

- **Branch:** `issue/<n>-<slug>` from `origin/main` (a short kebab-case slug
  from the title). When another issue's branch is checked out in the main
  clone, work in a worktree: `git worktree add ../flaminio-<n> -b
  issue/<n>-<slug> origin/main`. Never switch the branch of a checkout the lab
  is being driven from: `make deploy` copies that working tree.
- **Plan:** `docs/issues/<n>/intent.md`, `spec.md` and `design.md`. Read two
  earlier issues' documents before writing (`docs/issues/9/`, `docs/issues/10/`), and
  match their structure, terseness and numbering (spec rules S1, S2, …; a
  `## Tests` section listing store / engine / node / sim / lab checks).
- **Results** (when the spec has measurements):
  `docs/benchmarks/<date>-<slug>.md`, with the data, scripts and command
  lines that produced it in `docs/benchmarks/data/<date>-<slug>/`; the
  performance plan's item and Status updated.
- **Commits,** in the repo's style (`git log --oneline origin/main`), never
  squashed:
  1. `Design <what> (#<n>)`: intent, spec, design. Its body summarises the
     plan and says how many review rounds it took ("Reviewed in N rounds").
  2. `<Imperative summary> (#<n>)`: the implementation and its tests.
  3. `Address the <first|second|…> code review of #<n>` (or "the code and
     security reviews"), one per fix round, each body listing the findings
     and what changed, with the regression test for each.
  4. `Record the <…> results, and address …` when measurements land.
  Every commit ends with the attribution line this session requires.

## Severity

Every reviewer finding carries one: **Critical / High / Medium / Low / Nit**.
A loop is **clear** when a round, run by fresh reviewers, reports no Low-or-worse
finding. Nits are fixed when free or noted; they never block and never become
an issue.

**Every review loop runs 5 rounds at most while only Low and Nit findings are
left**, and as many as it takes while anything worse is (plan, design, and code
and security loops alike). After each round from the 5th on:
- **only Low and Nit:** fix those that are cheap, with no further round; the
  loop ends there. The Lows left open are listed (in the design commit's body
  for the plan loops, in the PR body for the code loop) and shown to the user,
  with why each was left;
- **any Medium, High or Critical:** fix them and run another round with fresh
  reviewers, and go on until a round reports only Low and Nit. Do not stop to
  ask the user for these; tell them, in a line, which round the loop is in.

## Steps

### 1. Intent and spec

1. Resolve the issue (`gh issue view <n> --comments`, and its section of
   `docs/followups.md`). If none is named, list open ones (`gh issue list
   --state open`), drop those blocked by their "Depends on", and propose the
   lowest-numbered unblocked one.
2. Research the code and write `docs/issues/<n>/intent.md` (why, with code facts and
   measured numbers; what we want; out of scope) and `docs/issues/<n>/spec.md`
   (terms, behaviour rules, what does not change, tests). A subagent may draft
   them; give it the issue, the files to read and the convention above.
3. Run the **plan review loop** (below) on intent + spec.
4. Show the user the spec's substance (rules, scope, open questions, the
   review rounds) and **stop** for approval. On requested changes, edit, run
   the plan review loop again on what changed, and ask again.

### 2. Design

1. Write `docs/issues/<n>/design.md`: the approach per spec rule, the files and
   functions touched, data and format changes, crash safety and ordering
   arguments, risks, the test plan mapped to every spec rule, and the
   measurement plan.
2. Run the **design review loop**: the plan review loop below with the
   design checks added, on the whole plan (intent, spec and design together,
   so a design finding that changes the spec is caught). It is a loop of its
   own, after the spec's: fix `design.md` (and the spec where a finding
   reaches it) for every Low-or-worse finding, re-review with fresh
   reviewers, until clear.
3. Show the user the design's substance and both loops' rounds, and **stop**
   for approval. On requested changes, edit, run the design review loop again
   on what changed, and ask again. On approval, commit the plan (commit 1),
   and say that from here everything runs unattended until the PR.

### 3. Implement

1. Implement the design with its tests (one `implementer` subagent, or two
   with disjoint file scopes when the design's parts are independent and their
   interface is fixed in the design). Record the base commit first.
2. Local checks: `make lint` and `make test`; `make sim` when `flaminio-repl`
   or the simulator changed; `make meta-interop` when the metadata format or
   protocol changed. Linux-only code (io_uring, ublk): `make vm-lint` and
   `make vm-test` once the lab is free.
3. Commit (commit 2): the reviewers see committed changes only.

### 4. Code and security review loop

Each round, run **two fresh reviewers in parallel** against `<base>..HEAD` in
the issue's checkout, each given the spec and design paths:
- **Code review:** correctness against the spec rules, races and ordering,
  crash consistency, error paths, resource leaks and hangs, test coverage of
  every rule, simplification.
- **Security review:** what a peer, a tenant, a fenced or stale session, or a
  malformed frame or request can make the node do (panics, out-of-range
  access, unbounded memory or waits), authentication and authorization
  (certificates, SPIFFE ids, capability tokens, epochs), data integrity and
  isolation between volumes, secrets.

Not clear → fix every Low-or-worse finding with a minimal change and a
regression test that fails without the fix (no scope creep; if a finding
looks wrong, fix what it points at or record why it does not apply), run the
local checks, commit (commit 3), and run a new round, until the loop ends
(**Severity**). Fixes that change the plan's rules update `docs/issues/<n>/` in
the same commit.

### 5. Tests, lab and results

1. Run every test and lab check the spec lists. The lab is shared: one issue
   at a time, `SSH_IDENTITY=~/.ssh/id_rsa_mac` from centaur, deploying from
   this issue's checkout (`scripts/lab.sh` deploys the tree it lives in).
   Campaigns that take hours run under `nohup`, logged, from the empty state
   the performance plan's rules require.
2. Check each "Done when" and spec rule against what was measured, and record
   pass or fail for each. A failing rule is fixed and re-tested (one more
   review round when the fix is large or security-relevant), or surfaced to
   the user.
3. Write the results document and update the performance plan, followups and
   docs the change affects; commit (commit 4).

### 6. PR

Only when every loop is clear or ended with Lows and Nits only, and every
rule and "Done when" passes (or the user accepted what does not):
1. Push the branch and `gh pr create`. Title: the change in the imperative
   with `(#<n>)`, or `(part of #<n>)` when the issue stays open. Body:
   `Closes #<n>` (or `Part of #<n>.` with why it stays open), then `## Why`,
   `## What changes`, `## Results` (tables) and `## Tests`, as in PR #28,
   ending with the session's PR attribution line.
2. Wait for CI (`gh pr checks <pr> --watch`); fix failures as in step 4.
3. Report: the PR URL, what each review loop found in how many rounds, the
   results against "Done when", and anything left. **Do not merge** unless
   the user asks; then merge with a merge commit (`gh pr merge --merge`), as
   every earlier PR was.

## Plan review loop

Used at both gates: on intent and spec before the spec gate, and as the
design review loop (with the design checks below) before the design gate.
Each round, run **two fresh reviewers in parallel** (new subagents every
round, so no round anchors on an earlier verdict). Give each the documents'
paths, the issue, and the checkout; tell it to verify every claim against the
code, not to trust the documents:
- **Plan code review:** facts (every file, function, number and behaviour
  claimed is what the code does); whether the rules achieve the issue's
  "Done when"; missed cases (concurrency, ordering, restarts and crashes,
  epochs and fencing, failure paths, defaults and compatibility, format
  versions); feasibility; consistency between intent, spec and design; every
  rule has a test; scope that belongs in another issue.
- **Plan security review:** the trust boundaries the change touches (peers,
  tenants, the control socket, the metadata service, administrators), what a
  malicious or stale party could do under the new rules, data integrity and
  loss, denial of service (unbounded queues, waits, memory, priority a client
  can claim), crash consistency as an integrity property, and whether the
  tests cover the abuse cases.

**At the design gate,** each reviewer also checks the design itself:
- **Design code review:** every spec rule has an approach, and every approach
  traces to a rule (nothing designed that the spec does not ask for); the
  files, functions, types and call paths named exist and work as the design
  assumes; the change fits the code's structure and idioms, and is the
  simplest that meets the rules; ordering, concurrency and crash-safety
  arguments hold step by step (what is durable when, what a crash between
  any two steps leaves); data, format and protocol changes, their versions
  and compatibility; error paths and cleanup; the test plan names concrete
  tests for every rule, each able to fail without the change; the measurement
  plan is runnable as written.
- **Design security review:** each new or changed input, message, field,
  state and wait in the design: who controls it, how it is validated and
  bounded, what happens when it is malformed, stale, replayed or out of
  order; changes to authorization, fencing or epochs; resources a party can
  exhaust; and whether the design's tests exercise those paths.

Not clear → fix the documents for every Low-or-worse finding (or record in
the document why a finding does not apply), and run a new round with fresh
reviewers on the result. Keep a short log per round: findings by severity,
and what changed.

The loop runs until clear, or, from the 5th round on, until only Lows and
Nits are left (**Severity**); those are listed and the gate goes ahead.
