# Plan review: issue #49 (Chaos test suite), round 6, design-code and design-security

Scope: `git diff 659b0e0..0d080dc -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. Checked against `origin/main`'s `scripts/lib.sh`
(`run_detached`, :163-173), `scripts/lab.sh` (`cmd_meta up`, :517), `scripts/guest/meta.sh` (:13),
`scripts/guest/write-load.sh` (:9), `scripts/guest/bench-writes.sh` (:99), `scripts/guest/chaos.sh`.

Counts: High 1, Low 4, Nit 3.

## Earlier findings

- R5-C1: fixed as a mechanism (the gate, design.md:244-268, decisions round 5), but its owner
  check refuses the history a root client writes (R6-B1).
- R5-C2: fixed (contracts.md:49-61, design.md:51-55, :66-68; tests reworded, design.md:307-308).
- R5-C3: fixed (design.md:309 adds the three decoder cases).
- Nit 1: fixed (absolute `$HOME/flaminio-history`, design.md:25-28, :244-247), but the copy is
  never made executable (R6-C1).
- Nit 2: fixed (contracts.md:33-35, :54-55).
- Nit 3: fixed (strict PID, whole cgroup line, pidfd). The order (pidfd_open, /proc checks, gate,
  pidfd_send_signal) is sound: if the PID dies and is reused after pidfd_open, the checks may read
  the new process, but the signal through the pidfd fails with ESRCH and kill-at refuses.
- Stale text from rounds 3-4: none in design.md or contracts.md; decisions.md marks R4-B1's
  Issued timing corrected, round 4's zero-window and PID-reuse risks replaced/withdrawn.

### R6-B1 — High
**The gate accepts only a history owned by `SUDO_UID`, but the client most likely runs as root,
so its file is root-owned and every gated kill is refused: every killed round fails.**
design.md:256-258, decisions.md:330. The plan never says which uid runs `flaminio-history run`
(design.md:157). `run_detached` starts the unit as the lab user (`--uid=$VM_USER`,
lib.sh:171), `/dev/ublkbN` is root:disk, and every lab access to a device goes through `sudo`
(write-load.sh:9 `sudo fio`, bench-writes.sh:99, bench-reads.sh:92); the helper itself checks
the client's `/proc/<pid>/exe` "with `sudo`" (design.md:239), which is needed only for another
uid's process. A client run as `sudo "$HOME/flaminio-history" run …` creates the history (and
`client-pid`) owned by uid 0, and `fstat`'s owner is never `SUDO_UID`. No test uses the real
client's ownership: main_test.go's fixtures are written by the test's user.
**Fix:** state the client's uid (root via `sudo`, as fio, or the lab user with named device
access) and make the owner rule match it, e.g. accept a regular file owned by uid 0 or by
`SUDO_UID`, or require the owner to equal the client PID's uid read from `/proc/<pid>/status`
(the helper already checked that PID holds the file's lock). Add the accepted owner to
main_test.go and a script-test assertion that the client is started with that uid. State `end`'s
and `status`'s privilege to match (they open the same 0600 file).

### R6-S1 — Low
**`kill-at` opens the lab user's path as root before it checks the file type, so a FIFO (or a
device node) there blocks it or has open-time side effects, outside the 10 s bound.**
design.md:258-262, decisions.md:381 ("a hostile file can at most make it send the kill or
refuse"). `open(O_RDONLY)` of a FIFO waits for a writer indefinitely; `fstat` comes after, and
the 10 s bound covers only the poll. `O_NOFOLLOW` guards only the last component: a symlinked
`chaos-<h>` directory passes the lexical check (the owner check still limits what is read). The
test list's "not a regular file" case would hang with a FIFO.
**Fix:** open with `O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_NOCTTY|O_CLOEXEC` (or `openat2` with
`RESOLVE_NO_SYMLINKS`), then `fstat`; name a FIFO and a symlinked directory component in
main_test.go's `--history` refusals, each refused without blocking.

### R6-C1 — Low
**The tool's copy is never executable.** design.md:25-28: `vssh … "cat > ~/flaminio-history"`
creates a 0644 file (umask), and every call runs it by that path, so `sudo
"$HOME/flaminio-history" …` fails with EACCES. `cmd_meta up` (lab.sh:517), the cited model, works
only because `meta.sh` then runs `sudo install -m 755` (meta.sh:13).
**Fix:** copy with `cat > ~/flaminio-history.tmp && chmod 0755 … && mv … ~/flaminio-history`
(or `install -m 755`), and have the script test's fake `vssh` assert the mode step.

### R6-C2 — Low
**The gate relies on each Return being appended as soon as its syscall returns, which the
design does not require.** design.md:56-58 says only "serialize append/fdatasync in one recorder";
the gate (design.md:259-262) and its named risk (decisions.md:370-375, "the recorder's append
latency") assume a prompt `write(2)` per Return. A recorder that buffers Returns until the drain
or the next batch's fdatasync (a natural batching choice) makes answered writes look outstanding
for a whole cycle, so the gate fires with nothing in flight and `in_flight_at_kill` is zero.
**Fix:** state that each operation's goroutine appends its Return (one `write(2)`, unbuffered)
right after its syscall returns, before it does anything else; recorder_test.go: a Return is in
the file (read by another fd) before the next batch's Invoke fdatasync and with no later append.

### R6-C3 — Low
**`kill-ready`'s new condition no longer matches its test.** design.md:74-76 creates
`kill-ready` once every operation of the first back-to-back burst has its Issued frame, and an
Issued frame precedes its syscall (contracts.md:50-56); workload_test.go (design.md:307) still
asserts "`kill-ready` appears only with an operation outstanding at the backend", which a correct
client can violate (all Issued appended, no goroutine yet in the backend) and which no longer
states the rule.
**Fix:** reword the case: in the kill phase, `kill-ready` appears only after the first
back-to-back burst's Issued frames are all in the file, and not before SIGUSR1.

## Nits

1. design.md:260, decisions.md:331: "the last 64 KiB of whole frames": 64 KiB is not a multiple
   of 136. Say "the last 481 frames, at offsets header_end + 136k up to the file's size", and that
   a header with a bad magic, CRC or a payload length over 4096 is refused.
2. design.md:256: "`SUDO_UID`'s `~/.flaminio-lab/…`": under `sudo`, `~` and `$HOME` are root's.
   Say the home comes from the passwd entry of `SUDO_UID` (missing or 0 refused).
3. design.md:249: PIDs are below `pid_max`, so the bound is at most 4,194,303
   (`PID_MAX_LIMIT` - 1), not 4,194,304.

Reviewed commit: a08bad2029c1f07798ae29b4f48183cf1e628ab9
