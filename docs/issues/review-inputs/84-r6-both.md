# Issue #84 — plan review round 6 (incremental), focuses: design-code and design-security

Diff reviewed: `git diff e342b46..edb8d8d -- docs/issues/84/ ':!docs/issues/84/report.md'`
(checkout `/tmp/flaminio-plans-2026-11-12/84`; HEAD adds only `report.md` to `edb8d8d`). The
diff is wording in `kernel-check.md` only. Read only; nothing was run against the lab or the
worktree.

## Earlier findings

- R5-S1 (Nit): fixed. Step 9 now carries only the result and the restore line
  (`kernel-check.md:41-43`). The sentence after it (`:45-47`) says the check prints, for
  `interrupted` the wait, then the two commands that must print nothing, then the two guest
  paths, and names where a guest `leftover` comes from (step 4 or 7). Three fixture rows assert
  the order: the precheck `leftover` (`:165`), the `leftover` after a failed attach (`:170`)
  and `interrupted` (`:171`). `design.md:197-198` ("the commands the check printed") now
  matches what the appendix says is printed.
- R5-C1 (Nit): fixed. `kernel-check.md:111-112` says `pgrep -f` can match an unrelated command
  line and that such a match only makes the operator look at it before going on.

## What the revision changed, checked against the rest

- Step 4's `leftover` is a precheck refusal, before step 9's "from step 5 on". Moving the
  sentence out of step 9 removes that mismatch; a `refused` run still changes nothing and
  prints no restore line (`:117-119`, `:144-145`).
- Step 7's `leftover` keeps the reason `guard` (`:36-38`, `:170`); the new sentence keys on the
  guest `state` answer, not on the reason, so both cases are covered without a new reason.
- The wait is printed for `interrupted` only, as the "by hand" paragraph has it (`:106-107`);
  after a guest `leftover` the wait is the "once neither … prints anything" condition
  (`:108-110`). Row `:171` puts the restore line first, as step 9 does.
- The `pgrep` sentence agrees with "once neither … prints anything": the rule stays, and an
  unrelated match is the operator's to judge. It is true of the lab: a command passed to `ssh`
  with `sudo` or a second command leaves a `bash -c` whose command line has the pattern.
- `design.md:112-114` and `:197-198`, and `decisions.md` (Review84-R4-S1), say the same thing as
  the changed lines; the anchors `kernel-check.md#procedure` and `#script-tests` exist.
- The three rows can fail without the change: a host script that prints the paths without the
  two commands, or before them, fails each of them.

## Findings

None.

## Nits

None.

Reviewed commit: 8179fef8eed07eb27175060cb9d6d07db4071095
