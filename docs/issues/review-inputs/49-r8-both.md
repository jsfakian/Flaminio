# Plan review: issue #49 (Chaos test suite), round 8, design-code and design-security

Scope: `git diff 1e3fba0..073c74c -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49` (design.md:69-76 and the S9/S10 test row;
decisions.md round-7 entry). Checked against `scripts/lib.sh` `run_detached` (:163-173), the
S17 row (design.md:343, spec.md:126, :155), the kill helper and `kill-at` text (design.md:251-303)
and the earlier exclusive-creation rule (design.md:32).

Counts: Low 1, Nit 1.

## Earlier findings

- R7-C1: fixed. design.md:72-74 has the harness create the segment directory as the lab user
  without `sudo` before the unit, and `run` create only its files, never a directory, refusing a
  missing parent; tests in the S9/S10 row (script test and main_test.go). No conflict with S17:
  its "temporary file" is the device (spec.md:126), and the history lives in a directory the
  script makes; workload_test.go uses temporary files the same way. Nothing else in the plan
  creates or expects `run` to create the segment directory.
- R7-S1: fixed. The main_test.go list now refuses a `SUDO_UID` that is missing, empty, 0, not a
  strict decimal (`1000x`, `-1`) or with no passwd entry (lookup injected), before the open.
- Nit (blank line ending the table): fixed. The S9/S10 row is followed directly by the S11–S13
  row (design.md:339-340).

### R8-B1 — Low
**`run`'s exclusive creation, now part of the R7-C1 fix, has no test, and with `run` as root it
is what keeps a planted file or symlink from being written.** design.md:73-74 says `run`
"creates only its own files (exclusive creation)" (also :32, :68) in a directory the lab user
owns. Since the client runs as root (:69), an `O_CREAT|O_TRUNC` open, or any open without
`O_EXCL`, of a `history`, `client-pid` or `kill-ready` path where a symlink or stale file already
stands would follow it and write as root (`O_CREAT|O_EXCL` fails on any existing name, a
symlink included). The only new test is "a missing parent directory refused" (S9/S10 row,
design.md:339). The S3/S4 row (:337) checks only that `client-pid` is written once the
`flock` is held. So an implementation without `O_EXCL` passes every listed test.
**Fix:** add to main_test.go or recorder_test.go: `run` with an existing `history`, an existing
`client-pid` or `kill-ready`, and a symlink at each of those names, each refused, the symlink's
target unchanged.

## Nits

1. design.md:73: `mkdir -m 0700` creates only the last component, and nothing in the plan
   creates the run directory `~/.flaminio-lab/chaos-<h>/` (`run_detached` makes only
   `~/.flaminio-lab`, lib.sh:170, and only when a unit starts). As written, the first segment's
   `mkdir` fails, or a `sudo mkdir` of the run directory brings back R7-C1's failure one level
   up. Say the harness creates `chaos-<h>` the same way, as the lab user without `sudo`, before
   the first segment (e.g. `mkdir -p` then `mkdir -m 0700` for the segment).

Reviewed commit: c73bc7d8626895ddf79b3f33889ab5b2c5471001
