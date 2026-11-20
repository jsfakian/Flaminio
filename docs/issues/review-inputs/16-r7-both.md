# Issue #16 plan review, round 7 (incremental), design-code and design-security
Reviewed: 05ea0e1..126b7e2.

## Round-6 findings
- R6-1: fixed. The scan is of $root, -maxdepth 1 -type d, anchored to pki, meta-pki, agent-pki, io-pki (the full set of directories in the design's list), removed with rmdir. -name matches the whole basename, so `pki.*` cannot match `meta-pki.*.new`; a final directory never ends in `.new`; every build is under the lock that step 6 holds, so no live temporary directory of another name exists. The test now keeps a fresh temporary directory, final directories and another name's aged .new file.
- R6-2: fixed in the design (see R7-1 for how the drain can hang or fail).

## New findings

### R7-1 - Low - The drain is specified without the two things that make it work
Design Tests, reconnect_after_expiry_refused. (a) "reads it to the end" returns only when the connector closes. If the test awaits the server half and the drain before polling or dropping the connector (one task, or a sequential join), the drain waits for an EOF that comes only when the connector is dropped: a hang. The connector must be a separate task or thread, or be read and dropped first. (b) try_clone dups the descriptor, so O_NONBLOCK, set by the async runtime on the shared open file description, stays set; a blocking std read on the clone returns WouldBlock. The test must `set_nonblocking(false)` on the clone, or drain with the runtime (a second tokio/async wrapper), and bound the drain with a read timeout so a failure is an error and not a hang. Both texts remain asserted; accept's error is still produced by session::accept (it takes `io` by value, so the clone holds the socket open after its drop, as intended).
Fix: add to the paragraph: the connector runs in its own task, the clone is made blocking (or read through the runtime), and the drain has a timeout.

### R7-2 - Low - The cleanup commands as written can fail the install, against "without failing the install"
Design Installing, step 6. `find ... -exec rmdir {} +` exits nonzero when an rmdir is refused, and `find -delete` exits nonzero on a failed unlink; "logged, without failing the install" holds only if the root shell is not under `set -e`, or the commands end in `|| echo ... >&2`. The design does not say. Also the test named does not cover a refused rmdir (a temporary directory that holds a file).
Fix: write `|| log` after each find (or say the exit status is ignored), and add a case: an aged temporary directory with a file in it stays, the install succeeds, a line is logged.

## Nits
None.
