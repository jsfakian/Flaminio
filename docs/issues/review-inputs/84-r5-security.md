# Issue #84 — plan review round 5 (incremental), focus: design-security

Diff reviewed: `git diff 317ef09..e342b46 -- docs/issues/84/ ':!docs/issues/84/report.md'`
(checkout `/tmp/flaminio-plans-2026-11-12/84`; HEAD adds only `report.md` to `e342b46`). The
code was read only. The two operator commands were tried on the workstation against harmless
`sleep` processes (GNU grep 3.7, procps-ng 3.3.17); nothing ran against the lab or the worktree.

## Earlier findings

- R4-S1: fixed. The 370 s bound is gone from the appendix, the decisions and the operator's
  text; no instruction relies on a number of seconds. The form is written once, as
  `sudo timeout -k 10 60 <command>`, and its fixture row asserts `timeout` after `sudo`. The
  180 s is `scripts/guest/common.sh:26`. The operator's wait, "until `/run/flaminio-bc` is
  gone", holds: the trap is the only thing that removes the directory, and it runs with no
  foreground child left.
- R4-S Nit 1: fixed in the "by hand" paragraph; one line that depends on it was not aligned
  (R5-S1). Both commands do what the text says. `run_on_vm` starts the guest as
  `env RUN=… bash -c …` (`scripts/lib.sh:157`), so the runner, the script and every `sudo` it
  starts carry `RUN=bc-…` in `/proc/<pid>/environ`; `-z` anchors `^` at each variable, so
  `XRUN=bc-…` does not match. An orphan child of a killed script has the device path in its
  arguments. `pgrep` does not match itself, and the node's own command line
  (`scripts/guest/node.sh:45-63`) has no `/dev/ublkb`, so the test can come out empty.
- R4-S Nit 2: fixed. The trap is set once the `mkdir` has succeeded; a `run` that finds the
  directory removes nothing, and its row asserts that the directory and the trace instance are
  left. A kill between the `mkdir` and the trap leaves the directory: a `leftover`, the safe
  side.
- R4-C1 (code reviewer's): fixed. Both scripts take the time from `date +%s` and wait with
  `sleep`; the tests stub both.
- R4-C2 (code reviewer's): fixed. "keeps no state that any run reads", in `kernel-check.md:12`
  and `design.md:104`.

## Findings

None of Low or worse.

## Nits

### R5-S1 — Nit

**What the check prints after a guest `leftover` was not aligned with the new liveness test.**
Step 9 still says that after `interrupted` and after a guest `leftover` the check "prints the
wait and the two guest paths below". The revision made two commands the condition for removing
those paths, and `design.md` sends the operator to "the commands the check printed". Read as
written, the check prints the two paths to remove without the test that no guest process is
live; an operator who clears them under a live run lets that run's trap remove the next run's
trace instance and directory. No fixture row asserts the text printed after a guest `leftover`;
the `interrupted` row asserts the wait only.

- Evidence: `docs/issues/84/kernel-check.md:43-44` against `:104-108`; `:166`;
  `docs/issues/84/design.md:197-198`.
- Fix: in step 9, say that the check prints the wait and, with the two paths, the two commands
  that must print nothing first. Add to the `interrupted` row, and to the row where the guest
  `state` answers `leftover`, that the printed text carries both commands before the two paths.

Reviewed commit: 4a0b4f8b7d857963ed7a18e65a0c9d871eb66258
