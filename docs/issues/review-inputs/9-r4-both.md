# Plan review: issue #9, round 4 (incremental), design-code + design-security
Reviewed commit: 325e11c5b0e5ee395c775f3a949db4f00eb8706f (HEAD). Diff 22fca47..37b29e0.

## Round-3 findings
- R3-A: fixed in substance (check before restart, never detaches). Gaps: R4-A, R4-B.
- R3-B: fixed (detach, then `trap - EXIT`; ssh bounded by ConnectTimeout, ConnectionAttempts and ServerAliveInterval, lib.sh:28). One residual: R4-D.
- R3-C: fixed. int(10000*F/R)/100 is exact: 10000*F is exact for integer or half medians, one rounding in the division, so an exact 0.9 gives 9000. A printed 90.00 means >= 0.9. Mode and POLL_US on the next line decide "without spinning". No other consumer parses the share column.
- R3-D: fixed in substance (two messages, partial vector, vol-delete check). Gaps: R4-B, R4-C.
- R3-N1: fixed.

## New findings

### R4-A (Low): `node.sh attached` grammar and ids not specified
control.rs:104-107 answers `ok V /dev/ublkbN` or `ok none`. A volume may be named `none` (valid id), so the "or `none`" output is ambiguous with no volume. The design does not say that the output is parsed against `^ok none$` or `^ok (<id>) /dev/ublkb[0-9]+$`; any other answer (`error: ...`, empty) must die, not be read as "nothing attached" (that would let the restart drop a volume). V is printed in a message and compared to VOL; node.sh already has `volume_id`'s regex, so apply it to V before printing.
Fix: specify the two patterns, die on anything else, print the id only after the regex; print an unambiguous token for none (for example an empty line or `-`), and add a script test (CONTROL_SOCKET is supported, common.sh:23) for ok none, ok V path, a volume named none, an error answer.

### R4-B (Low): the stop message names a command that does not exist as written
design step 3: "Detach V first." There is no make target for it (Makefile has none), and `lab.sh vol detach` takes VOL from the environment and a node argument defaulting to `a` (lab.sh:720-721), so for fl-b it needs `VOL=V scripts/lab.sh vol detach b`. For a CSI volume (pvc-...) with a mounted device, node.sh down refuses too.
Fix: name the runnable command in the message, and check it as a string in the script test.

### R4-C (Low): `bench_isr` edge cases
(a) A record with zero chunks (a failed parse or empty output) gives K=0 of N=0 and passes vacuously; the spec's vectors do not include it. Refuse N=0. (b) The placement message states one cause ("placed while NODE was draining or not alive"). On the `get` path (step 7) the volume may be old, and its replicas moved by drain or failover; say "placed or moved". (The advice to delete is safe: the name is bench-.)

### R4-D (Low): interrupted-run check may be racy
The warm-up fio runs in the foreground over ssh (run_on_vm, no tty). After Ctrl-C the remote fio may keep /dev/ublkb1 open until it sees the closed pipe, and the node's detach waits up to 30 s for the device (control.rs:205-211), while the trap ignores failures. The check "node.sh attached says none" right after the interrupt could fail or flake.
Fix: the check polls `attached` for about 40 s, or the design states that the trap's detach is the only guarantee and the check waits for it.

## Nits
None.
