---
name: ship-spec
description: >-
  Implement an approved docs/issues/<n>/design.md end to end and
  unattended (implementation, a code-review +
  security-review loop that fixes findings and reviews again until clean,
  tests, a conformance check against the spec), then open a draft PR.
  With "lab", run the design's lab checks, record them and mark the PR
  ready. Third and final stage of the issue -> spec -> design -> ship
  pipeline. Use only after a human has approved both spec and design.
---

# Ship spec

Stage 3 of 3. No human checkpoint inside it up to the draft pull request.
Runs `implementer` → review loop (`code-reviewer` +
`security-reviewer`, with `implementer` fixes, until clean or capped) →
`test-engineer` → `conformance-checker`, writes `docs/issues/<n>/report.md`
and opens a draft PR. The lab checks are a separate run: `/ship-spec <n>
lab` (step 9).

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
entries as settled unless the code proves one wrong. A fix that adds a
mechanism records why here.

## Context resets

A long run can lose its context (compaction, a resumed session). Files
and commits survive; what you had read doesn't. On resume, take progress
from `git log origin/main..HEAD` (the commits name the review rounds),
`git status`, `units.md` (if units were used), `decisions.md` and
`report.md`; continue from the next unfinished step and re-read only what
that step needs. A contradiction settled in `decisions.md` stays settled.

## Agents

- **Reviewers run read-only:** `code-reviewer` and `security-reviewer`
  have no Edit or Write. Use these named agents, not `Explore` or persona
  agents from plugins.
- Round 1 uses the default model; every later (delta) round invokes both
  with `model: "sonnet"` and `effort: "medium"`.
- **One implementer holds the whole change** by default: Flaminio's
  worst bugs sit between components (an ordering, a crash point, a
  fence), and one agent sees both sides. Units (step 2) are the fallback
  for a change too large for that. Each fix round gets a fresh
  implementer; reviewers are always new agents.
- **Independent agents run in parallel:** the two reviewers of a round,
  always; the Rust and Go implementers when the design allows (step 2).
- This stage itself never reads the code: its context holds the spec,
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
     → two implementers in one message, one for the data plane
     (`dataplane/`), one for the control plane (Go), each with its file
     scope. Safe only because the design's "Contracts" pins the proto,
     control-socket or wire format both build against. Neither runs
     `make fmt`, `make lint` or tests (they touch the whole tree); run
     `make fmt lint` once both finish.
   - **Units, the fallback,** for a change too large for one agent (about
     15 files or more on one side, or a design that says so): small
     ordered units of one module and its wiring, dependencies first (e.g.
     `api/proto` + `make proto` → `meta/service` → CLI; `flaminio-repl`
     → `flaminio-store` → `flaminio-node` → `flaminio-io`; scripts and
     their tests last). Write them to `docs/issues/<n>/units.md` first
     (one line each: name, files, design sections, rules) and keep it
     current; run one fresh `implementer` per unit, in order, each brief
     naming its unit, sections, rules and files (earlier units are on
     disk). Never brief one to read everything first.
   Check `git status` lists the design's files, then commit everything:
   `<What the change does> (#n)`.
3. **Review loop,** at most 3 rounds:
   a. Run `code-reviewer` and `security-reviewer` in parallel, new
      agents each round, each given `spec.md`, `design.md` and
      `decisions.md`:
      - **Round 1, the whole change** (`git diff $B`), so problems
        between components are seen. Only when the diff is too large to
        review well in one pass (about 3 000 changed lines, or units were
        used) split it: one pair per part (a unit, or the data plane and
        the control plane), each checking its calls into the other parts
        against the design's "Contracts".
      - **Rounds 2 and 3, delta reviews:** the last fix commit
        (`git diff HEAD~1`), a check that each earlier finding is fixed,
        and anything the fix broke. They don't re-audit unchanged code.
      - Each reviewer reports **every Critical, High and Medium
        finding,** most severe first, each with evidence (file:line), the
        failure it causes and a fix; then **at most 5** Lows and Nits, as
        a short list. Their checklists say where to look; they are not
        quotas.
   b. Both report no Medium or worse findings → clean: go to step 4.
      Lows and Nits are fixed in the same way only when they are cheap
      and certain; otherwise listed in the report.
   c. Otherwise run one fresh `implementer` (one per side when the Rust
      and Go fixes don't touch each other's files, in parallel), scoped
      to the files the findings point at, with the exact findings and an
      instruction to fix only those (smallest change, no scope creep; if
      a finding looks wrong, fix what it points at or say plainly why
      not). Fixes **cut before adding**: prefer removing a rule or
      mechanism to guarding it. Record in `decisions.md` each choice the
      fix settles and each accepted risk, with the finding that raised
      it. Commit: `Address the <first|second|third> code [and security]
      review[s] of #n` (as the history does). Repeat from (a).
   d. Findings still open after round 3 → stop here: skip to step 6 with
      the rounds that ran, and step 8 surfaces them to the human. Don't
      test or ship with open findings, and don't loop forever.
   **Shared root causes:** when one root cause shows up in several
   issues' reviews, fix it once, in an issue of its own the others depend
   on, instead of patching each.
4. Run `test-engineer` with `spec.md` and `design.md`; wait for pass or
   fail per rule. Commit its tests: `Test <what> (#n)`.
5. Run `conformance-checker` last, on the final diff and the test
   results.
6. Write `docs/issues/<n>/report.md`:
   - `## Review loop`: the rounds that ran, each round's findings and
     what the fix changed; final state (clean, or capped with N open)
   - `## Tests`: pass or fail per rule, and the `make` targets run
   - `## Conformance`: the checker's table verbatim (only if steps 4–5
     ran)
   - `## Lab`: the design's lab checks, each `pending`
   - `## ADRs`: those relied on, and any proposed by the design
   Commit it with the docs the design lists (`docs/followups.md`,
   `docs/lab.md`, …) if not already in.
7. **Only if the loop converged clean and conformance is PASS:**
   a. If the design proposed an ADR, set it to **Accepted** (status line
      and index row); the design approval covered it.
   b. Push the branch and open a **draft** PR (`gh pr create --draft`).
      Title: `<What the change does> (#n)`. Body, in the style of PRs
      #31 and #32: `Closes #n.` (or `Part of #n.` and why it stays open,
      when the change is one step of the issue); then `## Why`,
      `## What changes`, `## Results` (pending lab checks named) and
      `## Tests`. It stays a draft while lab checks are pending; with
      none, mark it ready (`gh pr ready`).
   c. Watch CI (`gh pr checks <pr> --watch`, in the background) and
      report the result. A failing check goes back to step 3c as a
      finding.
   No remote or `gh` not authenticated → stop after the local commits and
   say so. Never merge.
8. Report plainly: clean and PASS → point at `report.md`, give the PR
   URL, say it's a draft awaiting the lab checks (or ready, awaiting
   review), not merged. Capped with open findings, or conformance FAIL →
   **don't ship**: skip step 7, say exactly what's open, and ask how to
   go on (keep iterating, a follow-up issue, an accepted risk).

## 9. Lab checks (`/ship-spec <n> lab`, on request only)

The lab is shared, some checks take hours, and some reformat disks or
delete volumes. Run them only when the user asks.
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
6. Restore the lab as the design says (e.g. volumes recreated and
   filled), then, if every check passed, `gh pr ready`. A failed check:
   don't mark it ready; report it and ask.
