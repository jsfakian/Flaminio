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

# Ship spec

Stage 3 of 3. No human checkpoint inside it up to the draft pull request.
Runs `implementer` → `test-engineer` → code review loop
(`.claude/skills/ship-issue/review-loop.md`) → `conformance-checker`,
writes `docs/issues/<n>/report.md` and opens a draft PR. The lab checks
are a separate run: `/ship-spec <n> lab` (step 9).

## Preconditions

- `decisions.md` records both `**Spec:** approved` and `**Design:**
  approved` under `## Approvals`, or the user approved both earlier in
  this conversation. Otherwise say so and stop.
- On the issue's branch `issue/<n>-…`, with no uncommitted changes
  outside this issue's work.

## Decision log

`docs/issues/<n>/decisions.md`: every choice the reviews settle and every
accepted risk, each with its reason and the finding that raised it (as
`docs/issues/14/decisions.md`). Every reviewer gets it and treats its
entries as settled unless the code proves one wrong.

## Context resets

Files and commits survive a reset; what you had read doesn't. Take
progress from `git log origin/main..HEAD` (the commits name the rounds),
`git status`, `units.md` (if used), `decisions.md` and `report.md`;
continue from the next unfinished step and re-read only what it needs.

## Agents

- **One implementer holds the whole change** by default: Flaminio's
  worst bugs sit between components (an ordering, a crash point, a
  fence), and one agent sees both sides. Each fix round gets a fresh
  implementer.
- **Independent agents run in parallel:** the reviewers of a round; the
  Rust and Go implementers when the design allows (step 2).
- This stage never reads the code itself: its context holds the spec,
  the design, `decisions.md` and the agents' reports.

## Steps

1. Resolve the design from `args` (`<n>`, `#n` or a path) or the most
   recently approved one; note its sibling `spec.md`. Base:
   `B=$(git merge-base origin/main HEAD)`.
2. **Implement.** Pick the shape from the design alone, and start the
   first implementer at once:
   - **Default: one `implementer` for the whole design.** Its brief names
     the design and the spec; it works step by step, reading only what
     each step touches.
   - **Rust and Go in parallel:** the design says "Parallelizable? yes"
     → two implementers in one message, one for `dataplane/`, one for
     the Go side, each with its file scope, both building against the
     design's "Contracts". Neither runs `make fmt`, `make lint` or tests;
     run `make fmt lint` once both finish.
   - **Units, the fallback,** for a change too large for one agent (about
     15 files or more on one side, or a design that says so): small
     ordered units of one module and its wiring, dependencies first (e.g.
     `api/proto` + `make proto` → `meta/service` → CLI; `flaminio-repl`
     → `flaminio-store` → `flaminio-node` → `flaminio-io`; scripts and
     their tests last). Write them to `docs/issues/<n>/units.md` first
     (one line each: name, files, design sections, rules) and keep it
     current; run one fresh `implementer` per unit, in order, each brief
     naming its unit, sections, rules and files.
   Commit the files the design lists and the implementers reported (never
   `git add -A`): `<What the change does> (#n)`.
3. **Tests.** Run `test-engineer` with `spec.md` and `design.md`; commit
   its tests: `Test <what> (#n)`. Tests come before the review loop, so
   the reviewers check them too.
4. **Code review loop** (`review-loop.md`), on the change and its tests.
   - Round 1: the whole change. Only when it is too large to review well
     in one pass (about 3 000 changed lines without generated files, or
     units were used), split it: one set of reviewers per part (a unit,
     or the data plane and the control plane), each checking its calls
     into the other parts against the design's "Contracts".
   - **Fixes:** one fresh `implementer` per round (one per side, in
     parallel, when the Rust and Go fixes don't touch each other's
     files), scoped to the files the findings point at, with the exact
     findings (Lows included) and an instruction to fix only those.
   - Stopped at round 5 with findings open → skip to step 6, and step 8
     surfaces them.
5. **Final checks and conformance.** Run `make lint test race hygiene
   contracts` (and `make sim` when `flaminio-repl` changed), then
   `conformance-checker` on the final diff and the results. A FAIL for a
   missing test or doc goes back once to `test-engineer` or
   `implementer`, then one incremental review round and a conformance
   re-run; a second FAIL is reported (step 8).
6. Write `docs/issues/<n>/report.md`:
   - `## Review rounds`: every loop's rounds (kept up to date during the
     loops), and the final state
   - `## Tests`: pass or fail per rule, and the `make` targets run
   - `## Conformance`: the checker's table verbatim (absent when the
     loop stopped with findings open)
   - `## Lab`: the design's lab checks, each `pending`
   - `## ADRs`: those relied on, and any proposed by the design
   Commit it with the docs the design lists (`docs/followups.md`,
   `docs/lab.md`, …) if not already in.
7. **Only if the loop ended clear and conformance is PASS:**
   a. If the design proposed an ADR, set it to **Accepted** and add its
      row to `docs/adr/README.md`'s index; the design approval covered it.
   b. Push the branch and open a **draft** PR (`gh pr create --draft`),
      titled and shaped as the README's Pull request convention says,
      with the pending lab checks named under `## Results`. It stays a
      draft while lab checks are pending; with none, mark it ready
      (`gh pr ready`).
   c. Watch CI (`gh pr checks <pr> --watch`, in the background) and
      report the result. A failing check goes back to the loop as a
      finding.
   No remote or `gh` not authenticated → stop after the local commits and
   say so. Never merge unless the user asks; then with a merge commit
   (`gh pr merge --merge`), as every earlier PR was.
8. Report plainly: clear and PASS → point at `report.md`, give the PR
   URL, what each loop found in how many rounds, and that it's a draft
   awaiting the lab checks (or ready, awaiting review), not merged.
   Stopped with findings open, or conformance FAIL →
   **don't ship**: skip step 7, say exactly what's open, and ask how to
   go on (keep iterating, a follow-up issue, an accepted risk).

## 9. Lab checks (`/ship-spec <n> lab`, on request only)

The lab is shared, some checks take hours, and some reformat disks or
delete volumes. Run them only when the user asks, one issue at a time.
`make deploy` copies the working tree it runs from: run from the main
checkout with the issue's branch checked out, or from the issue's
worktree with `LOCAL_STATE_DIR=<main checkout>/.lab` (the lab's PKI,
known_hosts and markers live there, untracked).
1. Check the lab is free: the hosts' load (`ssh rhythm uptime`,
   `ssh rhythm-backup uptime`) and that nobody else's run is going. If
   not, say so and stop.
2. List what the checks will remove (volumes, formatted stores: e.g. the
   write campaign formats fl-b and refuses while another volume has a
   replica there) and ask before deleting anything that isn't the
   check's own scratch volume.
3. Deploy the branch to the VMs the checks use (`make deploy VM=…`).
4. Run each check of the design's lab section. Long ones run detached
   (`setsid nohup make … > .lab/<log> 2>&1 &`) with a background waiter,
   so a dropped session doesn't kill them; report progress on request.
5. Record the results: `report.md` `## Lab`, the PR body's `## Results`,
   and the documents the design names (`docs/benchmarks/<date>-….md` with
   its data under `docs/benchmarks/data/`, `docs/performance-plan.md`,
   `docs/followups.md`). Commit `Record the lab checks of #n` and push.
6. Check each "Done when" and spec rule against what was measured, and
   record pass or fail for each. A failing rule is fixed, gets one
   incremental review round and a conformance re-run, and is re-tested;
   or it is surfaced to the user.
7. Restore the lab as the design says (e.g. volumes recreated and
   filled), then, if every check passed, `gh pr ready`. A failed check:
   don't mark it ready; report it and ask.
