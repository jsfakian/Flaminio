# Plan review: issue #46 (Scrubbing and repair), round 1, focus: design-security

Scope: `docs/issues/46/` (intent, spec, design, decisions) in `/home/jsfakian/Documents/src/flaminio-46`.
The plan is step 1 only: a local read-only pass. It adds no repair, no RPC, no frame, no token and
no format change, so the repair questions of the brief (source choice, stale epoch, deposed
target, acknowledgement) have nothing to review here; the findings are on the pass's waits and
pins, the control socket's new lines, and the lab fixture that writes the raw disk.
Paths below are relative to `dataplane/crates/` unless they start with `docs/` or `scripts/`.

### R1-S1 — Medium
**A write load on the chunk starves a real finding, and the pass never ends.**
The recheck is valid only if the chunk's mutation generation did not change during the second
read; the generation is per chunk and advances at every write admission and commit
(design.md:35-39). "Unrelated changes may force a retry", and nothing bounds the retries:
"Continual overlap can postpone completion" (design.md:41-46). So a block that is really bad, in
a chunk a client writes steadily (4K QD32 keeps a write admitted or in flight at all times), is
retried forever: it is never reported, the pass never reaches an end state of S5, and since a
node runs one pass at a time and the next is due one period after the last *ended* (S8), the
disk gets no further pass. Any tenant with a steady workload can do this to the whole disk's
scrub. The plan's own check is exposed: S11 keeps fio only cluster-disjoint from the corrupted
block (design.md:96-97), not chunk-disjoint (default chunk 256 MiB, `control/csi/controller.go:71`),
so S11.2 can hang.
Fix: decide overlap against the block itself, not the chunk (for example one watched
slot/cluster/block range that write admission and apply mark; `touched` sets are per cluster,
`flaminio-store/src/store.rs:211,224,244`), and state what bounds the retries and how a pass
ends when they do not converge (an S5 outcome, or the block listed as unverified). Add a test:
a damaged block in a chunk under continuous writes to other blocks is reported and the pass ends.

### R1-S2 — Medium
**The executor of the pass's read does not exist where the pass mostly runs.**
The design sends reads to "the daemon's device executor" and says "Node/ublk already execute
planned local reads asynchronously" (design.md:16, 55-57). Only the ublk queue tasks do
(`flaminio-io/src/ublk.rs:1293`, `:1467`). A replica-only node is one thread in `drive_until`
(`flaminio-io/src/node.rs:344-376`); its replica reads are synchronous `read_batch`
(`flaminio-node/src/replica/mod.rs:399`); `BlockDevice` has asynchronous commits only
(`flaminio-store/src/device.rs:19-73`), and `UringDevice` has no asynchronous read
(`flaminio-uring/src/uring.rs`). `flaminio-uring` is not in the touched list (design.md:130-132).
Implemented with what exists, the read blocks the engine thread, and on the disk a scrub is
for (slow or retrying sectors) that stalls heartbeats and sessions: the membership change S6
and S7 forbid. The engine also moves from the main thread to the queue threads at attach and
back at detach (`ublk.rs:966-975`, `:889-895`); who completes a ticket outstanding across that
move is not said ("process generation" does not cover it).
Fix: name the mechanism (an asynchronous read on `UringDevice` completed through the node's
eventfd, or a reader thread on the raw fd), the ticket's owner across attach and detach, and
add the crate to the touched list. Tests: a held read on a replica-only node with heartbeats
and replica requests answered; attach and detach with a ticket outstanding.

### R1-S3 — Medium
**"Pins that cluster" is not what the read-lifetime mechanism does; a long pin quarantines the disk.**
The design pins scrub tickets and the fixture's lease "using the existing read-lifetime
mechanism" (design.md:34, 103). That mechanism is one counter: while any plan is outstanding,
every data cluster freed at a checkpoint is blocked from reuse, and all are unblocked only at
a `guard` call that finds the counter at zero (`flaminio-store/src/store.rs:391`, `:723-725`,
`:1913`, `:1918-1922`, `:2043`; `state.rs:239-246`; `bitmap.rs:65-83`). The fixture holds its pin
for the whole check, under fio: every cluster that whole-cluster rewrites, discards or
deletions free in that time stays unallocatable, up to `OutOfSpace` and failed writes, which
the check would then read as its own failure. A harness that dies leaves the pin, and the
quarantine, until the node restarts; nothing bounds it. The same holds for a scrub ticket if it
is kept while a recheck waits for overlapping writes (not said), and for a pass at a rate the
disk cannot reach (one read always in flight: the counter is never seen at zero).
Fix: specify a per-cluster pin for the fixture (the pinned clusters stay blocked, the rest are
released as today), at most one lease, dropped when the node stops or on an explicit bound;
say that a scrub ticket is released before a recheck waits and that the driver lets the
counter reach zero between reads. Test: a pin held while other clusters are rewritten and
checkpointed; free space returns.

### R1-S4 — Medium
**No path gets a `lab-tools` build into the lab; the boundary's only test cannot test it.**
The fixture is "only under explicit `lab-tools` feature, absent from release artifacts"
(design.md:99-101; decisions.md:28-31). The lab's nodes run what `make deploy` builds:
`cargo build --release` with default features (`scripts/guest/build.sh:4-6`, `Makefile:426`).
The design names no crate that carries the feature, no command that builds or deploys it, no
step by which the check learns the running node has it, and does not say that S7's measurement
runs on a default build. S11 is not runnable as written. "Production build refuses fixture
commands" is listed under `scripts/tools/scrub-test.sh` (design.md:127), which fakes the
socket and cannot show what the real binary does. Cargo unifies features across the workspace
(`flaminio-net/Cargo.toml:22-26`): one member enabling it puts the fixture in every build.
Fix: name the crate and feature, the deploy command and its variable, the check's first step
(a `fixture` line answered with the usage error stops it before any write), and a default
build for the measurement. Add a `control.rs` test under default features that `fixture pin`
and `fixture release` get the usage error, and a lint that no workspace manifest enables the
feature. Say whether "a local diagnostic socket pair" is two lines on `node.sock` or a second
socket (path, mode).

### R1-S5 — Low
**The restore can put old bytes over a newer write, and a dead harness has no way back.**
The pin stops reuse of a freed cluster; it does not stop an in-place write to the block
(`DeferredWrite`, applied in place after commit, `flaminio-store/src/store.rs:872-881`, `:1791`).
The restore's only conditions are the pin and the same process and incarnation
(design.md:111-115). If the block is rewritten between the save and the restore, the restore
replaces acknowledged data on that replica with the saved bytes, and nothing repairs it. The
design rules other writers out by assumption, with no check. Separately, a harness killed
without its trap (SIGKILL, a lost SSH session) leaves the block corrupt and pinned; a second
run is refused as a duplicate, and no command resumes the restore.
Fix: `fixture release` (or a `fixture check TICKET`) says whether the leased mapping (physical
cluster and checksum) is unchanged since the pin; restore only when unchanged, otherwise skip
and say so. Add a re-entrant `restore` step that works from the saved artifact and ticket.
Script tests for both.

### R1-S6 — Low
**The raw write's guards are not listed.**
The harness writes 4 KiB to the device as root at a "disk/physical offset" taken from a socket
reply (design.md:102-109). The design does not say that the device is the one `data_disk()`
finds (`scripts/guest/common.sh:11-16`) and the one the running node's status names (disk
UUID against the superblock), that the offset is decimal, 4 KiB-aligned and inside the
device's data area, or that `VOLUME`, `CHUNK`, `BLOCK` and the ticket are validated before they
reach a socket line or a command (AGENTS.md, Principles; the precedent is
`scripts/guest/confirm.sh:19-21` and its unit and disk checks).
Fix: list these checks as steps that stop the script before the write, each with a script test.

### R1-S7 — Low
**The design contradicts two approved spec rules and leaves the spec as it was.**
S7: "No session fails, no request times out and no lease lapses because of a pass"
(spec.md:73-75); the design says rate limiting cannot promise that for a slow or hung device
and that the qualification "needs" documenting (design.md:67-68, 146-147). S11.5: "it puts the
saved block back, whatever failed before" (spec.md:118); the design forbids the restore after
a process loss (design.md:113-115, 147-149). decisions.md records neither as an accepted risk
or a change after approval, and S7's engine test still asserts the unqualified rule.
Fix: amend S7 and S11.5 (and S11's script test line) and record both in decisions.md.

### R1-S8 — Low
**The status line's encoding, sources and limit are unspecified.**
`scrub status` answers `ok JSON` (design.md:78-82). The data plane has no JSON encoder (no
`serde` in any crate's manifest) and the design adds no library (design.md:132), so the line
is written by hand. It carries a free-text `reason` (store and I/O error text) and a volume
name; a quote breaks the JSON and a newline breaks the one-line reply that `control()` reads
with `readline()` (`scripts/guest/common.sh:21-30`), which the lab check then parses. The name
has no stated source: the replica service keeps only the name's hash
(`flaminio-node/src/replica/mod.rs:62-73`, `flaminio-node/src/lib.rs:21`). The socket thread's
wait for the engine's answer has no limit (compare `DELETE_LIMIT`, `flaminio-io/src/control.rs:37`,
and #13's `LIST_LIMIT`).
Fix: fixed tokens for `reason`, or an encoder that escapes quotes, backslashes and control
characters; say where a name comes from or report the UUID only; give the three lines an
answer limit below the CSI's. Test: a reason and a name with quotes and newlines still give
one valid line.

### R1-S9 — Low
**Four behaviours the design adds have no named test.**
`--scrub-rate-mib` outside 1…1024 or not a number, and `--scrub-period` of zero or one that
overflows (design.md:61, 70); issuance paused after a backward or failed clock reading
(design.md:64-65); a count overflow ending the pass as failed (design.md:84). The rows for S7
and S5/S8/S9 (design.md:125-126) name none of them.
Fix: add them to those rows.

## Nits

- **N1.** design.md:6 ("The spec's Q1–Q6 remain proposed choices") and :146 ("Open Q1–Q6
  remain") contradict :7, :154 and decisions.md, which settle them.
- **N2.** design.md:95-96, "Refuse uncertain clock/profile assumptions; this test does not
  qualify #83 timing": no state is named, nor where the script reads it; #83 is not a
  dependency of this plan. Name the check or drop the sentence.
- **N3.** #44's spec (Q5, settled) says the case of blocks that differ under equal CRC-32C
  values is "noted on #46". This plan's folder does not hold it; intent.md's Out of scope,
  step 2, is where it belongs.

Reviewed commit: f6bb5830547e1c3a385f0a7308dc27b85a0c959f
