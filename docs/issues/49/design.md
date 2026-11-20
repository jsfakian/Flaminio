# Issue #49: Check real block histories under existing chaos faults — design

Approved 2026-11-13; not implemented. [Spec](spec.md), [intent](intent.md).
Only step3/existing faults and reliable harness evidence: **#49 stays open** for new disk/
cluster faults, dm-log-writes, live upgrades and scheduling/lab locking. No runtime storage
behavior changes or mutated build is deployed by this slice. Exact v1 bytes: [contracts](contracts.md).

The facts it starts from:
- `sim/check.rs` exports COp/CKind/check_register, unique-value registers and optional writes,
  with a128-operation assertion and unbounded memoized search. It cannot consume a long lab
  history directly or return Undecided on budget exhaustion.
- `cmd_chaos/chaos_fault` use unseeded RANDOM, lose fault status in command substitution and
  kill only after fio ends (`scripts/lab.sh:1386–1461`). `write-load.sh` writes/verifies4GiB
  at64KiB using fixed seed7410. It checks integrity, not freshness or acknowledged-write loss.
- Lab workflow is dispatch-only with take-over approval and workflow concurrency; no manual/
  workflow shared lock exists. Keep it dispatch-only; no schedule or runtime mutation feature.

## Workload and durable recorder (S1–S4, S9, S15, S17)

Add the Go tool `control/cmd/flaminio-history` and its library `pkg/blockhistory` (both under
`make lint`'s gofmt roots, `api meta pkg control`), using `golang.org/x/sys/unix` for aligned
mmap, O_DIRECT Pread/Pwrite, `flock` and `CLOCK_BOOTTIME`. `go.mod` makes `golang.org/x/sys` a
direct requirement (it is indirect today), so `go mod tidy -diff` stays clean. `lab.sh` gains
`history_build`, as `meta_build`: it cross-compiles the tool for linux/amd64 into
`$LOCAL_STATE_DIR/bin`, and `cmd_chaos` copies it to each data VM as `$HOME/flaminio-history`,
the lab user's home, executable: `vssh … "cat > ~/flaminio-history.tmp && chmod 0755
~/flaminio-history.tmp && mv ~/flaminio-history.tmp ~/flaminio-history"` (`cmd_meta up` copies
`~/flaminio-meta` with `cat` alone, as `meta.sh` then runs `sudo install -m 755`; nothing
installs this copy). Every remote call names the tool by that absolute path, `sudo` included, so
none depends on a `PATH` or `secure_path` lookup. The workstation's own `seal` runs a host build.
One command owns each segment device; validate `/dev/ublkb[0-9]+`, block offsets/counts,
run/segment IDs and bounded output paths. Local file mode0600, exclusive creation; no shell
interpolation of device or history values.

4096-byte value: magic/version,256-bit run nonce, logical block index and monotone unique write ID,
then deterministic SHA256-expanded bytes. Ids carry their segment: the high 32 bits of every write
ID and op ID a segment's client allocates are its segment ID (1..n), the low 32 bits a counter from
1 in that client, so no two segments' clients allocate the same ID and none reads another's file.
The checker rejects an Invoke whose ID names another segment (contracts). Validate the complete
block against that encoding, not only its CRC/header. Distinct IDs give distinct whole values; nonce
collision probability is a declared identifier assumption, not mathematical global uniqueness. Never
accept another run/block or partial value. Foreign read records retain bounded digest/header for
diagnosis, never silently substitute a known value.

Use4 selected blocks in different chunks, one in chunk0 and others outside fio's range.
The run's first segment initializes each with an acknowledged unique write before any
read/overlap. Every later segment opens with one read of every block, issued together with no
write in flight, and writes a block only once its opening read was answered with success. That
read is the segment's first operation on the block: it closes the deadline of the previous
segment's unanswered writes (protocol §9), and an acknowledged write the failover dropped shows
as an older value. Then each one-second burst starts two writes/two reads per block
concurrently, then drains and reads a checkpoint value. At most16 operations globally in flight.
Persist invocation frames as a batch and fdatasync before issuing any syscall. The I/O is
blocking O_DIRECT `Pread`/`Pwrite`, one goroutine per operation: after its batch's fdatasync each
goroutine samples the clock, appends its operation's Issued frame through the recorder
(contracts) and then enters `Pread`/`Pwrite` directly, so the stamp is the last point before the
syscall, not after the issue. Nothing is synced for the Issued frames before the next batch, as a
client SIGKILL does not lose a completed `write(2)`. When `Pread`/`Pwrite` returns, its goroutine
samples the clock and appends its Return before it does anything else. The recorder buffers
nothing: each Issued and each Return frame is one `write(2)` of its own, made when it is
appended, and a batch's Invoke frames are written before its fdatasync; the kill's gate (below)
reads the file, so a Return held back would show an answered write as outstanding. Response records
include the captured same-node CLOCK_BOOTTIME timestamp/result and validated value ID; serialize
append/fdatasync in one recorder. Failed recorder stops issuance and fails the run. An
incomplete invocation frame was never issued; a lost response after kill is conservative
Unanswered, not a fabricated ack. The client holds an exclusive `flock` on its history file
while it runs; the kernel drops it when the client's last process exits. Once it holds the lock
it writes its PID to `client-pid` in its segment directory (mode 0600, exclusive creation), the
one process the kill helper signals. **The client runs as root:** its unit runs `sudo
"$HOME/flaminio-history" run …`, as `write-load.sh` runs `sudo fio` (`/dev/ublkb1` is
`root:disk`). Its history, `client-pid` and `kill-ready` are therefore root's (uid 0, mode 0600),
in a segment directory the lab user owns: before each segment, on that segment's VM, the
harness runs `mkdir -p` for the run directory `~/.flaminio-lab/chaos-<h>/` (idempotent; a
failover moves the next segment to another VM) and then `mkdir -m 0700` for the segment
directory, both as the lab user without `sudo`, before the client's unit starts; and `run`
creates only its own files, each with `O_CREAT|O_EXCL` (an existing name, a symlink included, is
refused), never a directory, and refuses a missing parent. Every command that reads or ends the
history runs with `sudo`: `end`, `status`, `kill-at`, the kill helper's and
`stop-client`'s reads, checks and signals, and the collection.

**Kill phase (S9).** On SIGUSR1 from the kill helper (Fault evidence, below), the client runs its
bursts back to back, without the one-second pause. An Invoke's `local ns` is stamped when its
batch is formed, before its fdatasync; an Issued frame's by its operation's goroutine just before
that goroutine's syscall. S9's in flight means issued and unanswered: `in_flight_at_kill` (below)
counts writes issued before the kill with no successful Return. Reads, the checkpoint reads
included, and operations invoked but not yet issued do not count. Much of each back-to-back
cycle has no write issued and unanswered (the burst's Invoke fdatasync, the checkpoint read's
fdatasync and I/O), so the kill is gated on the history: `kill-at` (Fault evidence, below) sends
nothing until it reads, in the history's tail, a Write with an Issued frame and no Return; the gap
from that read to the kill is microseconds against a replicated write's milliseconds. Once every
operation of the first such burst has its Issued frame appended, the client creates `kill-ready`
in its segment directory; if that creation fails (`EEXIST` included), the client stops issuing
and exits non-zero, and the run fails. Once it has created `kill-ready` the client never exits on
its own except on a recorder failure, which makes it exit non-zero at once, whether it is still
issuing or already waiting, so its `.rc` fails the run (below) and a write whose Return could
not be recorded never passes as S9's evidence. At its first failed operation (the kill's
errors), or after 1,000 back-to-back bursts (20,000 operations) with none, it stops issuing,
keeps its answers and the `flock`, and waits for the SIGKILL or SIGTERM. After 1,000 bursts with
no kill the run fails: the kill did not come.

Version1 segment file: bounded header(run nonce, segment ID, node, boot_id, block list),
length+CRC32C framed records `Invoke/Issued/Return/Checkpoint/End`, monotone record
sequence, op ID, block, local ns, kind/result/value. Hard limits: header4KiB, frame 136 bytes
(contracts), segment files 128 MiB per run, every other kept file (S14) 8 MiB together (below).
The harness passes each client `--max-bytes`: 128 MiB less the earlier segments' sealed lengths.
Before each batch the client reserves its Invokes, their Issued frames and Returns, a Checkpoint
and End; it stops issuing before a batch that would pass the budget, and the run fails as
undecided. The checker refuses, as undecided, a manifest whose sealed lengths total more than
128 MiB (contracts). No per-run operation count: slicing at checkpoints bounds the search, and
128 MiB is about 300,000 operations (three frames each), some 4 h at the paced 20 per second.
Limit bounds are shared with checker; no payload-bearing unbounded records. CRC detects
truncation/corruption, not malicious artifact forgery. Process-kill tests validate sync ordering.

A segment ends in three named steps, so End has one writer and nothing follows it:
1. **The client is stopped.** At a kill, the harness sends SIGKILL to the client's unit, waits
   up to 30 s for it to be not active (`inactive` or `failed`: a unit whose main process a
   SIGKILL ended is `failed`), then runs `sudo systemctl reset-failed` on it, as `chaos.sh
   kill-node` does for `flaminio-node`. Before the move back, `chaos.sh stop-client` sends
   SIGTERM to the client's PID alone (Fault evidence, below), so the `run_detached` wrapper
   survives and writes `.rc`: the client stops issuing, waits up to 30 s for its outstanding
   answers, and exits, non-zero when its recorder failed or an answer is still outstanding. The
   harness waits up to 45 s for that `.rc`; past it, it SIGKILLs the client's unit as at a kill,
   names the client and fails the run (S13). The final segment's client exits after its final
   read. Unless SIGKILLed at a kill, a client's `.rc` must be 0, or the run fails; at a kill, a
   `.rc` that is present (any value) means the client quit before its unit's SIGKILL, and fails
   the run. The harness's wait can end on a unit that wrote no `.rc` (`run_detached`, below),
   returning 1; only after the SIGKILL at a kill is that not a failure. A client still running
   30 s after a SIGKILL is named and fails the run (S13), and its segment gets no End: undecided.
2. **`flaminio-history end --history PATH`**, on that VM, with `sudo` (the history is root's),
   takes the file's `flock` without
   waiting and refuses while it is held. It truncates a torn tail, appends End (the same boot's
   clock, checked against the header; the kill's two samples when the target was killed) and
   fsyncs the file and its directory (contracts). With the client gone no Return comes after End;
   late answers are not kept.
3. **`flaminio-history seal --run-dir DIR`**, on the workstation once every segment's file is
   collected, writes `FLRUN001` with each file's sealed length.

The next segment's client starts only after step 2. No end mark from an unreachable/powered-off
VM is invented; #56 must design that boundary.

Parameterize fio OFFSET_BYTES, default0 unchanged for other checks; chaos uses4096 so block0
is reserved while still sharing chunk0. Other history blocks are in chunks32/48/63 of the
16GiB volume; all lie beyond fio's offset+4GiB range. Validate disjoint exact byte intervals;
fio verify-only reuses the same offset. Never let fio overwrite a history block. Without
`OFFSET_BYTES`, `write-load.sh` builds today's fio command, for `node-fault` and `drain-test`
(S18).

## Bounded checker and semantics (S5–S8, S16, S17)

Add the decoder, slicer and classification as the library module `flaminio_repl::sim::history`
(`sim/history.rs`), with its unit tests, which `cargo test` runs, and `examples/check_history.rs`
beside `examples/sim.rs` as a thin `main` over it. It shares the register search of
`sim/check.rs`, not a second differently defined checker. `std` only: CRC32C is a table-driven
function in the module, tested against RFC 3720's vectors and against the Go recorder's frames
(S16); no SHA-256 is needed, as `FLRUN001` seals each segment by its length. Extend search with
explicit state budget/result and configurable initial value; existing simulator wrapper
preserves its semantics and mutation coverage. No new dependency.

Map valid value IDs to register values. Construct per-block precedence from local invocation/
return timestamps within each segment and explicit End→next segment boundaries; never compare
nodes' ns. Compress those partial-order endpoints to checked integer ranks. Old Unanswered
writes remain optional, with their latest effect at the first later-segment response on that
block, exactly the simulator rule. They cannot reappear after that response. Error writes are
optional; error reads impose no value. S10's live-target errors: per segment, the checker lists
every error Return earlier than its End's second kill sample (same boot, same clock), or every
error Return when the segment ended without a kill; the harness fails the run when a list is not
empty. Per killed segment it also counts `in_flight_at_kill`, the writes issued before the kill's
first sample (an Issued frame's `local ns` earlier than it) with no successful Return: none, or
an error answered after the second sample. Reads and operations with no Issued frame are not
counted. A write answered with success after the second sample is not counted: a SIGKILL takes
effect after `pidfd_send_signal` returns, so the target may have answered it first (S9).

Split only at proven quiescent checkpoints with no unresolved operation, carrying its read value
into the next search slice. A kill's pending writes carry into the next segment until their
deadlines, its opening reads' answers, are resolved; never cut them away at End. Bound each
slice≤128 operations and search≤1,000,000 memo states; exhaustions return Undecided, never Pass.
Burst/drain pacing keeps normal slices small; missing checkpoint, malformed sequence, overflow,
missing segment/End, segment IDs not 1..n, required initialization/write acknowledgement, a later
segment's opening read or final block read fails validation.

CLI (Go): `flaminio-history run --device DEV --segment N --history PATH --run-nonce HEX
--node NAME --blocks LIST --max-bytes N`; `--run-nonce` is the header's 32-byte `run_nonce` as
64 lowercase hex digits, `--segment` 1..1024, `--node` the VM's name, context only, an id of
`valid_id`'s grammar (`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`, at most 128 bytes, the header's
bound), `--max-bytes` the segment file's byte budget, at most 128 MiB.
`end --history PATH`, `seal --run-dir DIR`, `status --history PATH` (prints the count of
successful Returns), `clock` (prints the boot id and `CLOCK_BOOTTIME` ns) and `kill-at --pid PID
--exe PATH --unit UNIT --history PATH` (Fault evidence, below; `--exe` an absolute path, `--unit`
a `.service` name, `--history` the segment's history file). Rust:
`check_history --manifest PATH`. Verdict JSON: version, `verdict` pass|violation|undecided; for a
violation `kind`, `block` and the operations (segment, op ID, kind, times); per segment
`in_flight_at_kill` and `live_target_errors`; exits0/1/2 respectively. Same retained
manifest/history yields deterministic verdict anywhere.

**Kinds** (S8), decided on the history once the search finds a block not linearizable. W1 is
*known newer* than W0 when W0's latest effect (its answer; for an unanswered write, its
deadline) precedes W1's invocation, or a read of W0's value was answered before a read of W1's
value was invoked (within a segment by its times, across segments by S4). For a read R of W0's
value while some W1 known newer than W0 was acknowledged, or its value read, before R was
invoked, the first bullet that matches names the kind, so the kind is deterministic (S8):
- **a write that resurfaced** when W0 is unanswered (it took effect after its deadline);
- **a stale read** when W1's own value is read again after R;
- **a lost acknowledged write** when W1 was acknowledged and its value is never read after R.

**Foreign content** is S6's: the foreign flag, another run's or block's value; and, as an
addition to S6, a value whose write was invoked after the read was answered is reported as
foreign content too. Any other violation is reported as a stale read,
with the operations the search could not place: every violation holds a read it cannot place.
The S16 fakes lie on the last burst before the final read, so a stale read is followed by a read
of the newer value. In a lab history, a stale read whose newer value was overwritten before any
read shows as a lost write: the kinds name what the history shows, not a hardware cause.

**The faults around a violation** come from the harness: at each confirmed inject and heal it
samples the target's clock (`flaminio-history clock` on the target's VM) and logs that mark in
the fault log. The report joins the verdict's operations with the faults whose marks, on the
same boot, surround them. Context only: no verdict reads the fault log (S11).

CI fake devices implement dropped acknowledged write (within a segment, and at a segment
change), stale read, late resurfaced optional write across segments, other-block and torn data.
Same workload/recorder/checker must pass without each lie and fail with it, with the lie's kind.
No production bug flag or shared-lab mutated daemon. Where it runs: `scripts/dev/history-lies.sh`,
beside `meta-interop.sh`, called by `make contracts` (CI's `contracts` job; `docs/ci.md` says
so, S19). It builds `check_history` and runs the Go test file `pkg/blockhistory/lies_test.go`,
behind the build tag `historylies` (so `make race` does not build it), with the checker's path;
the test fails, never skips, when that path is empty or the binary is missing.

## Fault evidence, seed and healing (S9–S14, S18)

**Its own volume (S9).** Before anything else (before it detaches, attaches or writes),
`cmd_chaos` refuses a `VOL` that `valid_id` refuses or that is named `pvc-…` (the CSI driver's),
`bench-…` (#9's S7) or `bc-…` (#84's check). Before each segment's client and each fio job, it
reads `node.sh attached` on that VM and requires `volume $VOL`; the device is the one node.sh
attaches at, `/dev/ublkb1`. Otherwise it stops, writing nothing. `node.sh attached` is #9's (its
design reads the control socket's `status` answer); whichever of #9 and this change lands first
adds it as #9 defines it. At each round's start, `metactl get` must show, for the chunk of every
history block (offset / `VOL_CHUNK`), `replicas` equal to the round's target and replica, as
`chunks_on` reads them; otherwise the round fails before its load.

Refactor chaos_fault to explicit status-returning calls, no status-bearing command inside log
substitution. Fixed PRNG xorshift64* with nonzero SEED, printed/stored before first selection;
only fault kinds are reproducible, I/O interleaving is not. Keep existing four kinds/durations.

Before changing a fault, append the intended action and its owned resource to the run's heal ledger
(contracts; not `FLRUN001`); after, probe effect then log confirmed state. Node restart: exact unit
inactive then active/service ready. Partition: owned firewall rules plus connectivity probe. Delay:
owned wg0 netem100ms rule, the root qdisc with the fixed handle `49:`; `chaos.sh slow on` adds (or
replaces) it only when wg0's root qdisc is the kernel's default (handle `0:`) or already that
netem, and otherwise refuses, changing nothing, so the fault is not confirmed and the run fails
(today it first deletes any root qdisc); `chaos.sh slow off` deletes the root qdisc only when it
is that netem (today it deletes any). Witness outage: both expected
units stopped, then healthy cluster membership/RPC after restore. Kill: exact daemon instance gone,
not an ignored kill error. Probe inability fails. Store fault times only as context, never as
cross-node ordering authority.

Before the kill: fio has ended, every ISR is full, and the workload progressed since the last
heal. After that heal is confirmed the harness reads the target's count of successful answers
(`flaminio-history status`, with `sudo`) twice, the second time just before the kill, and
requires that it grew (S10). Both reads are on the target's VM, ordered by the harness: no clock
is compared.

**The kill helper** is `chaos.sh kill-node`, on the target's VM, given the segment directory
(validated: a path under the run's guest directory `~/.flaminio-lab/chaos-<h>/`). It signals the PID
in `client-pid` there (a decimal number, read with `sudo cat`: the file is root's) alone, never
the unit: under `run_detached` the unit's main process is a bash wrapper, which SIGUSR1 would end.
First, with `sudo` as for the kill, it checks that the PID is live, that `/proc/<pid>/exe` is the
copied `$HOME/flaminio-history`, and that `/proc/locks` shows that PID holding the `flock` on the
segment's history file (its inode); otherwise it refuses and the run fails. It refuses, too,
sending nothing, if anything is at `kill-ready` (`sudo test -e PATH || sudo test -L PATH`, so a
dangling symlink counts). It sends SIGUSR1 (`sudo kill`) and waits up to 10 s for `kill-ready`;
if it is not there by then, or is not a regular file owned by uid 0 as the client creates it
(checked without following symlinks: `sudo stat -c '%F:%u' -- PATH` must print
`regular empty file:0` or `regular file:0`), it sends nothing more and the run fails. It then
reads `flaminio-node`'s main PID
(`systemctl show --property=MainPID --value`), which must be a nonzero decimal number, and runs
`sudo "$HOME/flaminio-history" kill-at --pid PID --exe
"$HOME/flaminio/dataplane/target/release/flaminio-io" --unit flaminio-node.service --history
PATH` (root:
 `node.sh` starts the node as root, and signalling it needs that; `$HOME` is expanded
by the lab user's shell before `sudo`). In one process, in this order, `kill-at`:
1. refuses, before any `/proc` read, a `--pid` that is not a strict positive decimal (digits only,
   no sign or leading zero) greater than 1 and at most 4,194,303 (`PID_MAX_LIMIT` - 1: PIDs are
   below `pid_max`): `kill(2)` with 0 or a negative PID signals a group or every process;
2. opens the PID with `pidfd_open`, then checks, as the client's PID is checked, that
   `/proc/<pid>/exe` is that binary (`node.sh`'s) and that `/proc/<pid>/cgroup`'s unified line is
   exactly `0::/system.slice/<UNIT>`, compared whole, never as a substring; otherwise it refuses,
   sending nothing (the run fails);
3. gates the kill on an outstanding write. `--history`, the segment's history file, must be an
   absolute path under `<home>/.flaminio-lab/chaos-<h>/`, `<home>` being the home in the passwd
   entry of `SUDO_UID` (under `sudo`, `~` and `$HOME` are root's; a `SUDO_UID` missing, not a
   number, 0 or with no entry is refused), with no dot, dotdot or empty component (the helper's
   check of the segment directory). It is opened with `openat2`, flags
   `O_RDONLY|O_NONBLOCK|O_NOCTTY|O_CLOEXEC` and `RESOLVE_NO_SYMLINKS`, so no component is a
   symlink and a FIFO does not block the open, and refused unless `fstat` then shows a regular
   file owned by uid 0, as the client (root, above) creates it: a file the lab user made is that
   user's and is never read as the history. A root-owned file in that tree is one root made in
   the lab user's directories, or one that user can already read and write: no component is a
   symlink, the user cannot hard-link there a file they could not read and write (systemd's
   default `fs.protected_hardlinks = 1`), nor make a device node (`mknod` needs `CAP_MKNOD`), and
   `kill-at` prints nothing it reads from the file, only acts on it. It reads
 the header (at most 4,112 bytes; a bad magic or CRC, or
   a payload length over 4,096, is refused) to find where frames start, then polls, at most every
   millisecond, the last 481 whole frames (65,416 bytes), at offsets `header_end + 136k` up to
   the file's size, using only frames whose CRC32C holds, until some Write has an Issued frame
   and no Return there. Past 10 s without one it refuses, sending nothing, and the run fails;
4. reads `CLOCK_BOOTTIME`, sends SIGKILL with `pidfd_send_signal` on that pidfd and reads the
   clock again, with nothing between them, and prints the boot id and both samples. The signal
   reaches only the process the pidfd opened: if that process exited after `pidfd_open`, the call
   fails, nothing is signalled, `kill-at` refuses and the run fails, so a reused PID is never
   signalled.

The helper writes the samples to `kill-mark` in the segment directory, confirms that instance gone
and runs `reset-failed`, as today; `end` copies both samples into End after checking their boot id.
Signal, samples and kill run on one VM, the kill between the samples in one process, with no exec,
`sudo` or D-Bus call between them.
Before the move back, `chaos.sh stop-client`, given the segment directory (validated the same way),
makes the same checks and sends SIGTERM to that PID alone (`sudo kill`), never the unit. The
evidence is the history's: each killed segment's `in_flight_at_kill` must be at least one, or the
run fails (S9). No block trace is taken: the history records what was in flight. After failover
start next segment only after S4 End and ordinary fresh-epoch readiness.
 The final segment makes its opening reads, then,
once every ISR is full after the final failover/move-back, reads every history block with no write
in flight.

`chaos-heal RUN_DIR`: `RUN_DIR` must be a direct child of `$LOCAL_STATE_DIR/chaos`, named
`run-<16 hex>`, not a symlink. It reads that run's heal ledger, the only copy, on the
workstation, and refuses the whole ledger before any remote command when a line fails the
contract's validation. For each action without its `healed` line it stops the exact run
client/fio unit, restores owned partition/netem rules and original node/witness service states,
verify each. The harness starts the client and fio units under the ledger's names (an optional
unit name for `run_detached`; other callers keep today's). Given a unit name, `run_detached`'s
poll also ends when the unit is not active (`inactive` or `failed`) and wrote no `.rc` (it reads
the unit's state before `.rc`, in the same round trip), returning 1: a failure unless the harness
SIGKILLed that unit at a kill. Repeat safely; never delete unrelated
qdiscs/firewall rules, kill arbitrary PIDs or restart a service merely because its name looks
similar. Trap EXIT/INT/TERM calls it; after a lost connection, the operator runs it again from
the same workstation, on the same ledger. Fetch history/fault/seed/verdict on every exit (the
segment files are root's: `sudo cat`, by their validated paths); incomplete collection/cleanup
 is failure with pending actions, not Pass. No disk format.

**No secret is kept (S14).** The harness never writes `metactl attach`'s output or `ENV_VARS`
to the run directory. It keeps the segment files, `FLRUN001`, the heal ledger, the fault log,
the seed, the verdict, the report and fio's summary lines, and no journal excerpt; the Lab
workflow uploads that directory alone. After its last write the harness sums the sizes of every
kept file but the segment files, and fails the run, keeping the files and appending that reason
to the report, when they pass 8 MiB.

## Files and rule-to-test mapping

| Rules | Test file: cases |
|---|---|
| S1, S2 | `pkg/blockhistory/value_test.go`: complete-value uniqueness, a torn or other-block value refused. `workload_test.go`, on an aligned temporary file and a controlled blocking backend: actual overlapping reads/writes; the first segment writes each block first; a later segment's opening reads answered before its first write to each block; two segments' clients on one file allocate disjoint IDs; in the kill phase, `kill-ready` appears only after SIGUSR1, once every Issued frame of the first back-to-back burst is in the file; each operation's Issued frame follows its batch's fdatasync and precedes its operation's syscall (the backend records the order) and its Return; the file, End's frame included, never passes `--max-bytes`, and the client stops issuing before it would. |
| S3, S4 | `pkg/blockhistory/recorder_test.go`: invoke fsync before issue; Issued before its operation's syscall, after its batch's fdatasync, and before its Return; each Issued and Return its own unbuffered `write(2)`: a Return is in the file, read through another fd, before the next batch's Invoke fdatasync and with no later append; random process kills, every value on the file has its record; `end` after a torn trailing frame decodes; `end` refused while the client holds the `flock`; `client-pid` written only once the `flock` is held; `run` with an existing `history` or `client-pid`, or a symlink at either name, refused at start, the symlink's target unchanged; SIGUSR1 with `kill-ready` already present (a file, then a symlink): the client stops issuing and exits non-zero, the symlink's target unchanged; after `kill-ready`, a failed operation stops issuance and the client keeps running, holding the `flock`, until signalled, and a recorder failure makes it exit non-zero at once, without waiting for a signal, both while it is issuing after `kill-ready` and on a Return appended after its first failed operation, once issuance has stopped; more than a torn frame after the last good one refused; boot mismatch refused; nothing after End. |
| S5–S8 | `sim/history.rs` unit tests: CRC32C vectors; the simulator register cases; concurrent writes; optional deadlines; quiescent cuts/carry state; each kind by hand, within and across segments (an acknowledged write in segment k, an older value read first in segment k+1: lost); every malformed/missing/limit case of S7, an ID of another segment, an Issued for an unknown op, an Issued whose kind or block differs from its Invoke's, an Issued with a nonzero field it must leave zero, a second Issued, an Issued after its Return, a Return with no Issued, an End with one kill sample 0 and not the other or the second earlier than the first (each undecided), a missing opening read, segment IDs with a gap, sealed lengths totalling more than 128 MiB; `in_flight_at_kill`: the only write issued before the first sample answered with success after the second gives zero, answered with an error after it one, and an error between the samples is a live-target error; when the only operations unanswered at the first sample are unissued or reads, zero; a write issued after the first sample, zero; with the samples further apart than every operation's latency, writes issued before the first and answered with success between them give zero, and one more issued before the first and never answered gives one; deterministic second verdict. Model-correct generated histories pass; altered answers fail. |
| S9, S10 | `scripts/tools/chaos-history-test.sh`, the VMs faked: segments on two faked VMs: on each, before its segment, the run directory made with `mkdir -p` and then the segment directory, both as the lab user without `sudo`, before the client's unit starts; the tool's copy written to a temporary name, made mode 0755 and moved into place (the fake `vssh` asserts the `chmod 0755`); the client's unit runs `sudo "$HOME/flaminio-history" run`, and `end`, `status`, the helper's `client-pid` read and checks, its SIGUSR1, `stop-client`'s SIGTERM and the collection run with `sudo`; fio/history block disjointness; full ISR, progress (`status` twice) and fio before the kill; a `kill-ready` planted before SIGUSR1 (a file, then a dangling symlink) refused with nothing sent, and after the wait one owned by the lab user, or a symlink to a root-owned regular file, refused; a fake client that creates `kill-ready` and then exits 0 after `kill-at`'s SIGKILL and before its unit's SIGKILL fails the run, and one that hits a recorder failure after `kill-at` fails the run, and one that gets errors after `kill-at` and keeps running until the SIGKILL passes; the kill helper's order (signal, `kill-ready`, the main PID read, `kill-at` given the segment's history and gated on an outstanding write, `kill-mark`), a main PID of 0 or not a number refused; the fake unit's wrapper survives the helper's SIGUSR1 and only the client's PID is signalled; a `client-pid` that is not a live `flaminio-history` holding the history's lock refused; a SIGKILLed client unit with no `.rc`, the fake reporting it `failed`, ends the harness's wait, and `reset-failed` follows; `stop-client` signals only the client's PID, and a SIGTERMed client that exits non-zero fails the run; a fake client that ignores SIGTERM is SIGKILLed after 45 s, named, and fails the run; `in_flight_at_kill` zero fails; live-target errors fail; the final read of every block; a refused `VOL` (invalid, `pvc-`, `bench-`, `bc-`) before any remote command; `node.sh attached` naming another volume stops the run; a history block's chunk off the target or the replica fails the round. `control/cmd/flaminio-history/main_test.go`: `kill-at`, with a recorded clock, pidfd and signal: `pidfd_open` before any `/proc` read, then the checks, the gate, and clock, `pidfd_send_signal`, clock in that order with nothing between; a fixture history whose tail has no outstanding write (none issued, or every issued write answered) waits and refuses at the 10 s bound, sending nothing, and one with an outstanding write kills; a torn or CRC-bad last frame is not read as an Issued; a `--pid` of 0, -1, 1, `+5`, `05` or not a number refused before any `/proc` read; a PID not live, not the binary, or whose cgroup line holds the unit only as a substring or under another slice, refused with nothing sent; a process that exits after `pidfd_open` is not signalled and `kill-at` refuses; a `--history` outside the run tree, with a dotdot component, a symlink as its last component or as a directory component, a FIFO (refused at once, the open not blocking), another file that is not regular, or owned by a uid other than 0 (`SUDO_UID`'s included; the test injects `fstat`'s owner, as it runs unprivileged), refused with nothing sent, and a regular file owned by uid 0 accepted; the run tree's home taken from `SUDO_UID`'s passwd entry, not `$HOME`, and a `SUDO_UID` missing, empty, 0, not a strict decimal (`1000x`, `-1`) or with no passwd entry (the lookup injected) refused before the open; a header with a bad magic, a bad CRC or a payload length over 4,096 refused; `run` with a missing parent directory refused. |
| S11–S13 | `scripts/tools/chaos-history-test.sh`: each fault's failed injection/probe/heal, seeded sequence, interrupts at every phase, missing SSH, duplicate heal; a ledger with a foreign unit, VM, kind or nonce, or a `RUN_DIR` outside the run tree, refused before any remote command; all four faults and every client/fio stop attempted; `slow on` over an unrelated root qdisc on wg0 refused, that qdisc kept. |
| S14, S15 | `scripts/tools/chaos-history-test.sh`: hostile IDs/paths/sequence/counts rejected, output bounded, all exits retain raw evidence or fail collection visibly; a segment left out of `FLRUN001` fails; each client's `--max-bytes` is 128 MiB less the earlier segments' sealed lengths; kept files other than the segment files passing 8 MiB together fail the run, the files kept; the fake `metactl attach` prints a fixture token, and no kept or uploaded file holds it. `control/cmd/flaminio-history/main_test.go`: refused flags and values; a `--node` that is not an `fl-` name accepted; no shell evaluation of data. |
| S16 | `scripts/dev/history-lies.sh` (`make contracts`) runs `pkg/blockhistory/lies_test.go` (tag `historylies`) with `check_history`: each lie named by its kind, the lost write within a segment and at a segment change; identical nonlying device passes, including across-segment resurfacing. |
| S17 | `scripts/dev/history-lies.sh` also runs the tool and the checker under `env -i` (no lab variable) on a temporary file. |
| S18, S19 | `scripts/tools/node-scripts-test.sh`: `write-load.sh` without `OFFSET_BYTES`, fio stubbed, builds today's command; unchanged simulator mutations/other lab checks; Make help/README/ci docs state coverage/exclusions and Lab remains dispatch-only. |

Touched: new Go tool/library/tests (`control/cmd/flaminio-history`, `pkg/blockhistory`),
`go.mod`; repl checker module and history example/tests; `scripts/lab.sh`, `scripts/lib.sh`
(`run_detached`'s optional unit name), guest `chaos.sh`/`write-load.sh` and new checker tests;
`scripts/dev/history-lies.sh`; Makefile (`contracts` calls `history-lies.sh`; `lint` runs
`scripts/tools/chaos-history-test.sh`, beside the other script tests), Lab artifact upload and
docs.
Implementation `make test sim lint race contracts`; no lab or live mutation runs in ordinary CI.

## Lab, docs, risks and ADRs

Later explicit authorization/exclusive manual lab use: four seeded rounds, history under faults
and each target kill, verify fio, final reads and replay history on workstation. Report segments,
confirmed/healed faults, actual pending writes at kills, checker budget/verdict and run duration
versus old20–30min; repeat intent's fio blind-spot experiment on scratch file, never a disk.
Where: on the lab VMs before #83 lands; afterwards only on a host #83 admits, with no bypass
(as #84 settled for its check). #83 clock and #84 per-block limits are not certified by this
slice; the audit follow-up's campaigns on #76–#84 remain #49 follow-on work (intent, Out of
scope). No new disk fault, schedule, global lock or all-clear production claim.

Update lab/ci/README claim now to integrity+errors for old runs, and to checked sampled blocks only
after a recorded history run. Retain raw artifacts with seed/binaries; do not extrapolate
sampled-block coverage to whole-volume linearizability. Local recorder survives client death, not
VM/disk loss; #56 owns that extension. Administrative concurrent lab users remain excluded by
procedure until a real shared lock exists. No new ADR/runtime safety rule; Q1–Q5 were settled at
approval. Workload/decoder/fault harness can proceed against v1 records and budgets; integrate
checker/recorder fake-lie tests before any authorized lab run. Settled at approval, 2026-11-13:
Q1–Q5 as recommended (see decisions.md, Settled at approval).
