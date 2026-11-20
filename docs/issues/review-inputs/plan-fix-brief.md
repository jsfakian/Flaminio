# Fix brief — batch 2026-11-12

You address one review round on the draft plan of one GitHub issue of Flaminio. Documents
only: you act as the plan's planner and designer and revise it in place.

## Rules
- Read the findings file(s) your prompt names, then the plan folder `docs/issues/<n>/` in the
  issue's worktree. Check each finding against the code before fixing it.
- Fix every finding, Nits included, with the smallest coherent change. Cut before adding:
  prefer removing a rule or mechanism to guarding it. Revise in place, never regenerate a
  file. Keep intent, spec, design and appendices consistent with each other after the change
  (a fix reaches the spec only where the finding does; every rule keeps a test that can fail
  without the change; a new replication, membership or recovery safety rule keeps its
  simulator check and injected bug; a new RPC keeps its `auth.go` rule).
- If a finding is wrong, don't argue inline: record in `decisions.md` why it does not apply,
  with the finding's id, and say so in your final message.
- Edit only files under `docs/issues/<n>/` of that worktree. A drafted ADR stays the
  issue-local `adr-proposal.md`. Never touch code, `docs/adr/`, another issue's folder,
  `opencode.json`, or the main checkout. If a finding needs a change to another issue's plan
  (a shared root cause), don't make it: describe it in your final message.
- Nothing is approved and nothing is implemented: invent no approvals, no accepted risks, no
  test or lab results. Open architectural choices stay labelled Proposed.
- `decisions.md` (create it if missing, with a `## Approvals` section saying none yet):
  record each choice a fix settles, with the finding that raised it.
- `report.md` (create as `# Issue #<n> — report` if missing): append under `## Review rounds`:

  ```
  ### Plan review round <N> — <reviewed commit>

  Scope: correctness against code and security of the draft plan; no runtime implementation.
  Reviewers: <names from your prompt>.

  - **<id> — <Severity>:** <summary> Evidence: <evidence>.

  Changes/resolution: <what changed, per finding>

  Status: needs_incremental_review. Approval remains separate; no approval is inferred.
  ```
- Before committing: `git diff --check` is clean and every relative link you added resolves.
- One commit, staging only the files you changed by name (never `git add -A`):
  `Address the review of the design of #<n>, round <N>`, the body listing each finding id
  and what changed, ending with the line
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`. Don't push.

## Final message
The commit hash, one line per finding (id: what changed, or why it does not apply), and
anything that needs another issue's plan to change. Nothing else.

## If the Write tool refuses `report.md`
Don't work around it. Write the round's entry (starting at its `### Plan review round` line)
to `the path the orchestrator gives you, named <n>-r<N>.md`,
commit the rest, and say so in your final message; the orchestrator appends and commits it.
