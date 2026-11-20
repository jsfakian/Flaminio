# Issue #84 — plan review round 5 (incremental), focus: design-code

Diff reviewed: `git diff 317ef09..e342b46 -- docs/issues/84/ ':!docs/issues/84/report.md'`.
The reviewer wrote no file; this is its returned report, saved by the orchestrator.

## Earlier findings

- R4-C1 (Low): fixed. kernel-check.md "Script tests" says both scripts read `date +%s` and wait
  with `sleep`, and the tests stub both, with a `sleep` that advances the stub `date`; this
  covers the 60 s nodes-alive wait and the "300 s have passed before step 4" fixtures. The stub
  list also has `sudo` and `timeout`.
- R4-C2 (Nit): fixed. "keeps no state that any run reads" in kernel-check.md and design.md.

Other changed lines are consistent with each other and with decisions.md.

## Findings

None of Low or worse.

## Nits

### R5-C1 — Nit

`pgrep -f /dev/ublkb` can also match unrelated processes whose command line contains that
string (for example an operator's own ssh command). Harmless: a false positive only makes the
operator wait longer or look closer. Fix: say so, or narrow the pattern.

Reviewed commit: 4a0b4f8b7d857963ed7a18e65a0c9d871eb66258
