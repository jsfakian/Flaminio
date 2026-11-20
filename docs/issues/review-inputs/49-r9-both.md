# Plan review: issue #49 (Chaos test suite), round 9, design-code and design-security

Scope: `git diff c73bc7d..a89f70a -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49` (design.md:72-77, the S3/S4 and S9/S10 test rows;
decisions.md round-8 entry). Checked against the segment model (spec.md:11, :71-73: one segment
per attachment on one node, the target alternating), the kill phase (design.md:83-93), the
segment end steps (design.md:107-128), the helper's path validation (design.md:254-279) and
`run_detached` (scripts/lib.sh:163-173).

No conflict between `O_CREAT|O_EXCL` and the rest of the plan: nothing reopens or resumes a
history with `run`. `end` (design.md:120), `status`, `kill-at` (openat2, :279) and the collection
(`sudo cat`, :325) only open an existing file, and `run` never restarts within a segment. A
segment directory made with `mkdir -m 0700` (no `-p`) fails if it already exists, so no stale
file can stand in it. The kill helper's validation (a path under `~/.flaminio-lab/chaos-<h>/`)
matches the run directory the harness now creates.

Counts: Low 2.

## Earlier findings

- R8-B1: fixed. design.md:75-77 has `run` create `history`, `client-pid` and `kill-ready` with
  `O_CREAT|O_EXCL`, an existing name or symlink refused; the S3/S4 row (design.md:340) tests an
  existing file and a symlink at each name, the target unchanged.
- Nit (run directory never created): fixed in part. design.md:72-75 has the harness create
  `~/.flaminio-lab/chaos-<h>/` with `mkdir -p` as the lab user, but only "before the first
  segment" (R9-B1).

### R9-B1 — Low
**The run directory is created "before the first segment", but segments run on different VMs.**
A segment is one attachment on one node (spec.md:11) and the target alternates, with the
workload "on the new target after each failover" (spec.md:71-73). So the second segment's client
runs on another VM, where nothing has made `~/.flaminio-lab/chaos-<h>/`. design.md:72-75 says the
harness creates the run directory "before the first segment" and each segment directory with
`mkdir -m 0700`, which creates only the last component. Taken as written, the harness makes the
run directory once, on the first target's VM. Then at the first failover the segment's `mkdir`
fails on the new target and the run stops. A `sudo mkdir -p` workaround would bring back R7-C1
(root-owned directories). The S9/S10 test (design.md:342) checks only "before the first
segment", so a one-VM fake passes.
**Fix:** design.md:72-75: "before each segment, on that segment's VM, `mkdir -p` the run
directory, then `mkdir -m 0700` the segment directory" (`mkdir -p` is idempotent). S9/S10 row:
segments on two faked VMs, the run directory made as the lab user on each before its segment
directory. Update the round-8 entry in decisions.md to match.

### R9-B2 — Low
**What `run` does when it cannot create `kill-ready` is not said, and the test reads as a check
at startup.** `kill-ready` is created only in the kill phase, after SIGUSR1, once the first
back-to-back burst is issued (design.md:91-92), long after `run` has started. The S3/S4 case
"`run` with an existing ... `kill-ready` ... refused" (design.md:340, decisions.md round 8)
suggests a refusal at start. But design.md:75-77 refuses the name only when it is created.
Nothing says that an `EEXIST` there stops issuing and makes the client exit non-zero. The
"failed recorder" rule (design.md:64-65) covers only the recorder's writes. A client that logs
the error and keeps going leaves a planted `kill-ready` in place. The helper's 10 s wait for that
file (design.md:261) then passes before the burst is issued. The `kill-at` gate still keeps the
kill under load, so this is not a safety break.
**Fix:** design.md:91-92: "if creating `kill-ready` fails, the client stops issuing and exits
non-zero (the run fails)". S3/S4 row: the `kill-ready` case sends SIGUSR1 with that name already
present (a file, then a symlink). It expects the client to exit non-zero and the symlink's target
to be unchanged.

Reviewed commit: 23ade95fbe805f35264bc76524475eed60ac50e6
