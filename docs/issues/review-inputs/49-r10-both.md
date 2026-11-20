# Plan review: issue #49 (Chaos test suite), round 10, design-code and design-security

Scope: `git diff 23ade95..6911634 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`: design.md:72-79 (the run directory on each VM),
design.md:91-95 (a refused `kill-ready`), the S3/S4 and S9/S10 test rows (design.md:342, :344),
and decisions.md's round-9 entry. I checked these against the segment end steps
(design.md:110-123) and the kill helper (design.md:256-306).

The run-directory change is consistent with the rest of the plan. `mkdir -p` is idempotent, so a
move back to the first VM reuses its run directory. The segment directory is still made without
`-p`, so it is new and empty on each VM. The helper's path check (a path under
`~/.flaminio-lab/chaos-<h>/`) matches on every VM.

Counts: Low 1.

## Earlier findings

- R9-B1: fixed. design.md:72-77 now runs `mkdir -p` for the run directory before each segment,
  on that segment's VM, then `mkdir -m 0700` for the segment directory, both as the lab user. The
  S9/S10 row (design.md:344) fakes two VMs, and decisions.md records the change.
- R9-B2: fixed as worded. design.md:91-93 says that a failed `kill-ready` creation makes the
  client stop issuing and exit non-zero. The S3/S4 row (design.md:342) sends SIGUSR1 with a file,
  then a symlink, already at that name. The harness side of "the run fails" is still open: see
  R10-B1.

### R10-B1 — Low
**"The run fails" on a refused `kill-ready` does not hold at a kill. The harness ignores the
client's exit code there, and the helper proceeds on the planted file.** The kill helper checks
only that `kill-ready` exists (design.md:263). So when a file is already at that name, the helper
goes on to `kill-at` at once, before the burst has its Issued frames. `kill-at` fires at the
first outstanding write (design.md:294), and the harness then SIGKILLs the client's unit
(design.md:111). This can happen before the client tries to create `kill-ready`, and then it
never sees `EEXIST`. If the client does see `EEXIST` first and exits non-zero, the end step still
excuses it: "Unless SIGKILLed at a kill, a client's `.rc` must be 0", and at a kill a missing or
any `.rc` is not checked (design.md:120-122). In both orders the segment ends normally. The gate
still found an outstanding write, so `in_flight_at_kill` passes, and the run passes. The new
sentence at design.md:93 claims an outcome that nothing enforces. Only the client's exit code is
tested (design.md:342, `recorder_test.go`). This is not a safety break: `kill-at`'s gate still
keeps the kill under load. A planted file also needs the lab user or root, because the segment
directory is new and mode 0700.
**Fix:** design.md:263: before it sends SIGUSR1, the helper refuses (`sudo test -e`) if
`kill-ready` already exists. After the wait, it requires `kill-ready` to be a regular file owned
by uid 0, as the client creates it. If either check fails, it sends nothing and the run fails.
design.md:120: at a kill, a `.rc` that is present and non-zero also fails the run. S9/S10 row
(design.md:344): a `kill-ready` planted before SIGUSR1 is refused and nothing is killed, and a
lab-user-owned `kill-ready` after the wait is refused.

Reviewed commit: 6911634be985ff7fffd4f0843409a2d9772757d1
