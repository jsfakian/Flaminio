# Plan review: issue #49 (Chaos test suite), round 11, design-code and design-security

Scope: `git diff 6911634..10b8100 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. The diff touches the segment end step
(design.md:120-121), the kill helper (design.md:263-266), the S9/S10 test row (design.md:347) and
decisions.md's round-10 entry. I checked these against the kill phase (design.md:81-95),
`run_detached` (scripts/lib.sh:163-173), `chaos.sh kill-node` (scripts/guest/chaos.sh:12-15) and
the ublk device setup (dataplane/crates/flaminio-io/src/ublk.rs:906-919).

Counts: Medium 1, Low 1.

## Earlier findings

- R10-B1: fixed for the case it named. The helper refuses a `kill-ready` that exists before
  SIGUSR1, and after the wait it requires a regular file owned by uid 0 (design.md:263-266). At a
  kill, a `.rc` that is present and non-zero now fails the run (design.md:120-121). The S9/S10 row
  and decisions.md record both changes. The new `.rc` rule misfires, though (R11-B1), and the
  helper's checks follow symlinks (R11-B2).

### R11-B1 — Medium
**The new rule "at a kill, a `.rc` that is present and non-zero also fails the run" can fail a
correct kill. The plan never says what the client does after the kill's errors.** `kill-at`
SIGKILLs `flaminio-io`, which serves `/dev/ublkb1`. The device is set up without
`UBLK_F_USER_RECOVERY` (ublk.rs:906-919), so the kernel fails the client's in-flight I/O with
errors within milliseconds. `in_flight_at_kill` counts on exactly this ("answered with an error
after it", design.md:163-165). The client then "stops issuing at its first failed operation"
(design.md:93). The plan does not say whether it then waits or exits, or with what code. The
harness SIGKILLs the client's unit only later. First the helper writes `kill-mark`, confirms the
node gone and runs `reset-failed` (today `kill-node` sleeps 1 s there, chaos.sh:13-15). Then the
harness makes another `vssh` round trip. An implementer can easily have the client exit non-zero
after its first failed operation, and `run_detached`'s wrapper would then write `.rc`
(lib.sh:172-173) before the unit SIGKILL. Every killed segment would then fail the run. Before
this change, any `.rc` was ignored at a kill, so the client's behaviour here did not matter. The
new test, "a client exiting non-zero before a kill fails the run" (design.md:347,
decisions.md:510), does not pin the window either. If its fake client exits before the helper
runs, the helper's live-PID and `flock` check (design.md:261-263) already fails the run, so the
test passes even without the new `.rc` rule.
**Fix:** design.md:93: once it has created `kill-ready`, the client never exits on its own. At its
first failed operation it stops issuing, keeps its answers and the `flock`, and waits for the
SIGKILL or SIGTERM. Only then does a `.rc` present at a kill (any value) mean that the client
quit early, and the run fails. S1/S2 or S3/S4 row: after `kill-ready`, a failed operation stops
issuance and the client keeps running. S9/S10 row: a fake client that exits non-zero after
SIGUSR1 and before the unit SIGKILL fails the run. A fake client that gets errors after `kill-at`
and keeps running until the unit SIGKILL passes.

### R11-B2 — Low
**Both new `kill-ready` checks can be bypassed with a symlink.** design.md:264 names
`sudo test -e`, which follows symlinks. A dangling symlink at `kill-ready` passes that check, and
the client's `O_EXCL` create then fails with `EEXIST`. The post-wait check, "a regular file owned
by uid 0" (design.md:265), names no command. `test -f`/`-O` follow symlinks, and so does
`stat -L`. So by the end of the wait, the lab user can rename into the 0700 segment directory,
which that user owns, a symlink to a root-owned regular file (for example `/etc/passwd`). The
check then passes, the kill goes ahead, and only R11-B1's backstop is left. That backstop holds
only if the client exits before the unit SIGKILL. The lab user has `sudo`, so this matters little
in practice. Still, the check the design names does not do what the R10-B1 fix claims.
**Fix:** design.md:264: refuse if `sudo test -e PATH || sudo test -L PATH`. design.md:265: check
without following symlinks, for example `sudo stat -c '%F:%u' -- PATH` (no `-L`) equal to
`regular empty file:0`. S9/S10 row: a dangling symlink planted before SIGUSR1 is refused. After
the wait, a symlink to a root-owned regular file is refused.

## Nits

- design.md:265-266: "if either check fails" follows three conditions: the pre-signal check, the
  10 s wait and the post-wait type/owner check. Say "if `kill-ready` is not there within 10 s, or
  is not a regular file owned by uid 0". For the pre-signal refusal, "sends nothing more" should
  be "sends nothing".

Reviewed commit: e208909c848af9ccb392c6676470a71bb1ceff71
