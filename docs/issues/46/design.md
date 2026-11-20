# Issue #46: A local read-only scrub pass — design

Approved 2026-11-13; not implemented. Changed after approval by the plan review's rounds 1 to
4: see [decisions](decisions.md).
[Spec](spec.md), [intent](intent.md).
This implements step 1 and node-local counts only; **#46 stays open** for comparison, repair,
read-triggered repair and disk-health integration.
Settled at approval, 2026-11-13: Q1 to Q6, each with its recommendation (see decisions.md,
Settled at approval).

The facts it starts from (paths under `dataplane/crates/`):
- `ReadPlan::verify` checks mapped CRCs (`flaminio-store/src/store.rs:366–379`); `digests`
  reads metadata rather than media (`:1283`). `fsck(true)` is synchronous/offline (`:2176`).
- `plan_read/finish_read` protect planned clusters against reuse (`:1901–1922`), but require
  an epoch, resolve the current visible layer only and permit concurrent in-place mismatch.
  A scrub needs distinct layer-aware plans without granting ordinary reads an epoch bypass.
- **That protection is one store-wide counter** (`reads_in_flight`, `:391`). While it is not
  zero, a checkpoint keeps every data cluster freed since the last one from reuse, and `guard`
  lifts that only at zero (`:723–725`, `:2043`; `state.rs:239–245`). It fits a read of
  microseconds, not a pass or a lease held for minutes.
- **The map and the medium change at one point.** `execute_finish` applies a durable batch's
  records (`State::apply`, `state.rs:283–471`) and then writes deferred writes in place,
  synchronously, on the store's owner (`store.rs:872–881`). A batch in flight or staged
  changes neither (`:848–849`). No record removes a layer: layers are only pushed
  (`state.rs:360`). Layers and mappings are in `state.rs:73–138`.
- **No asynchronous local read exists outside a ublk queue task** (`flaminio-io/src/ublk.rs:1293`),
  whose ring lives only while a volume is attached. A node without one runs the engine in
  `drive_until` (`flaminio-io/src/node.rs:344–376`), and its replica reads are synchronous
  (`flaminio-node/src/replica/mod.rs:391–405`). `BlockDevice` and `UringDevice` have no read
  that returns before it is done. The engine moves to the queue threads at attach and back at
  detach (`ublk.rs:968–975`, `:1053–1063`); nobody drives it while the device is added.
- No scrub scheduler/status exists. The control socket is root-only, mode 0600
  (`flaminio-io/src/control.rs:51`). The store opens its device without `O_DIRECT` under
  `--buffered` (`flaminio-uring/src/uring.rs:128–136`) and says which (`is_direct`,
  `:163–165`). The queue threads already read through its descriptor (`AsRawFd`, `:287–291`;
  `ublk.rs:1306–1313`).
- **`unsafe` is forbidden** in the workspace (`dataplane/Cargo.toml:39–40`), `flaminio-io`
  included. `flaminio-uring` is the one crate that may use it
  (`flaminio-uring/Cargo.toml:19–23`).
- **#74 (clear):** under `--privsep`, the lab's default once it lands, the I/O process opens
  no disk path and gets one disk descriptor from the helper, once
  (`origin/main:docs/issues/74/spec.md`, S2, S3, S7; `design.md:54–56`, `:153–163`).

## Store cursor, coverage and recheck (S1–S6, S10)

Add `flaminio-store/src/scrub.rs`: `ScrubCursor`, `ScrubTicket`, `ScrubResult` and store-only
`start_scrub/next_scrub/finish_scrub` methods. Keep all metadata decisions on the existing
store owner; the device read happens on the reader (next section), which returns the CRC-32C of
each block it read and nothing else. No epoch/token is needed for this local check, and no
client/replication method gains unfenced access.

**Cursor.** At start, capture for each chunk slot in use its incarnation and its layer count,
bounded by the store's slot/layer limits; do not copy payloads or maps.
- **Incarnation:** a counter per slot, in memory, kept in `State` beside `chunks` and not in
  `Chunk` (which `ChunkDelete` drops), advanced by `ChunkCreate` and `ChunkDelete`. A slot
  deleted and created again, under the same volume id and index or another, is not the
  captured chunk: the cursor leaves it, and nothing late is reported for it.
- **Layers only grow,** so a layer's index is its identity. The issue that adds layer removal
  extends the cursor.
- **The walk:** each captured layer's cluster positions once, in order, each read planned
  against the map as it is then. An entry that is Unmapped or Zero by then is passed over; so
  is a chunk or a layer created after the start. Every mapping in place from the pass's start
  to its end is visited.

**Ticket and watch.** `next_scrub` gives out at most one ticket at a time: one read of up to
64 KiB of consecutive stored blocks inside one cluster (a whole cluster at the default size),
with its device offset and the map's checksum of each block. While it is out the store holds
one **watch**: slot, incarnation, layer, cluster, block range, physical cluster, and one mark
per block (16 bits).
- **`State::apply` marks** the watch's blocks that a record covers: a `MapUpdate` or `Discard`
  of the cluster and a `ChunkDelete` of the slot mark all of them, a `DeferredWrite` the blocks
  it writes. Records change the top layer only, so a watch on a lower layer is marked by
  `ChunkDelete` alone.
- **This is S3's evidence:** per block, taken at the one point where the map and the medium
  change. Write admission, the Flying/Staged touched sets and a per-chunk generation are not
  used: none of them changes what a read returns, and a per-chunk signal moves on every write
  to the chunk.

**Pin.** The ticket pins its one physical cluster, and does not count in `reads_in_flight`.
`State` keeps one pin per holder: a slot for the ticket and a slot for the lab fixture's
lease. A cluster is pinned while either slot names it, so a ticket that finishes on the
cluster the lease holds (S11's pass does that) leaves it pinned. `release_pending` frees a
pinned cluster as any other and blocks it in memory, as the quarantine does
(`state.rs:239–245`); `unblock_all` leaves pinned clusters blocked. A dropped pin only empties
its slot: the next `unblock_all` unblocks the cluster if no slot names it, so a client read's
quarantine is never cut short. Every other freed cluster is released as today, so a pass of
any length, with a read always in flight, keeps one cluster from reuse and no more.

**What a read decides,** in `finish_scrub` on the owner, in this order:
1. **The store is poisoned:** the pass ends failed (S5) and nothing pending is reported. A
   failed in-place write leaves the map ahead of the medium (`store.rs:876–880`), which would
   otherwise read as a mismatch twice.
2. **A marked block** was rewritten, discarded or deleted during the read: skipped (S2), and
   counted as skipped.
3. **The read failed:** each of its blocks not decided yet is read again alone (4 KiB, a
   ticket each). A block whose own read fails is **unreadable** (S4), unless marked. Read
   errors never enter the store's write-poisoning paths.
4. **An unmarked block whose CRC matches** is checked.
5. **An unmarked block whose CRC does not match** is read again alone, once. The driver
   carries the first ticket's physical cluster and checksum to the second's planning, because
   no watch exists between the two tickets and the throttle can put time there.
   - **The entry differs then** (another cluster or checksum, or none): the block was
     rewritten or removed: skipped and counted, with no read.
   - **The entry is the same:** the second ticket is issued. Marked then: skipped. Matching:
     checked. Not matching, and unmarked: a **mismatch** (S3).

   So a mismatch is a block unmarked in both reads that failed one checksum twice.

A block is read at most three times (in a 64 KiB read, alone after a failed read, alone after
a mismatch). Every read is charged to the rate, and no step waits for a write or a commit: a
pass over a chunk under continuous writes ends. A mismatch is never reported from one read,
nor for a block a record covered during either read. The ticket's pin and watch end at
`finish_scrub`: after a stop or a deletion the driver still finishes the ticket it holds once
its read completes. A shutdown ends the process, and the pin with it.

## Reader, driver, throttle and schedule (S1, S5–S8)

**The reader** (`flaminio-io/src/scrub_io.rs`): one thread, `flaminio-scrub`, started in
`node.rs`'s `build` beside the network and metadata threads, for the life of the process. It
holds a duplicate of the store's descriptor and one aligned 64 KiB buffer, takes
`ScrubRead { id, offset, len }` on a channel, reads at that offset, sends
`Input::ScrubDone { id, result }` on `Parts.sender` and calls the node's wake, as those
threads do (`node.rs:288–322`). `result` is the CRC-32C of each block read, or the error: no
block data leaves the thread.
- **The descriptor** is a duplicate of the one the store's `UringDevice` holds, taken in
  `node.rs`'s `main` before `build` gets the device. `flaminio-uring` gains one safe method,
  `UringDevice::try_clone_fd() -> io::Result<OwnedFd>` (its `File`'s `as_fd()`, then
  `try_clone_to_owned`), and no new `unsafe`. No path is opened and nothing is asked of
  #74's helper, so it is the same in direct mode and under `--privsep`, where the device
  comes from `UringDevice::from_file`.
- **The read** is `std::os::unix::fs::FileExt::read_exact_at` on a `File` made from that
  `OwnedFd`, into an `AlignedBuf` (`flaminio-uring`). It takes its offset as an argument and
  shares no file position; the reader calls nothing else on the descriptor. `flaminio-io`
  gains no `unsafe`: no `libc` call, no raw descriptor.
  - **All of it or an error.** `read_exact_at` retries an interrupted read, and a read that
    comes back short, or ends at the device's end, is an I/O error. The reader computes a
    CRC only over a buffer the read filled, so a short read is reported as any read error
    is (What a read decides, 3) and never as a mismatch.
  - **Aligned.** Before the read the reader checks that the offset and the length are
    multiples of 4,096, as `O_DIRECT` needs and as every ticket's are. A request that is not
    is a bug: in every build the reader answers it as a read error (`InvalidInput`) and does
    not read. It is a checked condition, not an assertion: no build panics on it.
- **A buffered store has no scrub** (S1). `node.rs` asks `UringDevice::is_direct()`, which
  #74's `from_file` sets from the file's flag. Without it a read would check the page cache, and a
  finding is about the medium: no reader is started, no automatic pass runs, and the socket
  answers as Status says. The same holds on other systems, which run the node for development
  only, without `UringDevice`. The lab's nodes are never buffered, in either mode.
- **#74:** neither plan changes for the other. Whichever of #46 and #74 lands second adds the
  test that a node under the helper runs a pass (below), and the lab check and S7's
  measurement run in the lab's default mode: under the helper once #74 has landed.
- **In `flaminio-node`** the reader is the trait `ScrubReader` (`submit(ScrubRead)`), given to
  the engine when it is built, so `tests/engine.rs` can hold a read. `BlockDevice` is not
  changed; `flaminio-uring` gains only `try_clone_fd`.

**The driver** (`flaminio-node/src/scrub.rs`) is part of the `Engine` and holds the ticket.
- **One step, three callers.** `Engine::scrub_step(now)` scans the cursor and issues a read
  when the throttle allows. `turn` and `flush` (`engine.rs:748–750`) both call it, and so does
  the handling of `ScrubDone`, after `finish_scrub` has decided the read. When nothing is due
  it is one comparison, so it costs `flush`'s callers nothing.
- **Its deadline.** The driver keeps `due`: the throttle's next allowed issue time, or 1 ms
  after a scan that reached its bound. `Engine::next_wakeup` (`engine.rs:621–633`) includes
  `due` only while a pass runs with no read out. After `scrub_step(now)`, `due` is absent or
  later than `now`: a wake at `due` always issues a read or scans, and no caller can spin on
  a time in the past.
- **Who wakes.** `drive_until` waits on `next_wakeup` and calls `turn` (`node.rs:352–354`).
  With a volume attached today no thread reads `next_wakeup`: the step runs at every pump
  (`flush`) and at the 10 ms turn (`ublk.rs:586–595`, `:1178–1186`), so an idle attached node
  advances every 10 ms and at completions. #15's engine thread waits on `next_wakeup` and
  serves a deadline wake with `flush` (`origin/main:docs/issues/15/design.md:256–275`), which
  runs the step. Whichever of #15 and #46 lands second lists the scrub's `due` among that
  thread's deadlines and runs the test below against it, as #15 does for #75's deadline.
- **Across attach and detach** the ticket moves with the engine. The reader and its channel
  depend on no queue ring, and both drivers drain `Parts.inputs` (`node.rs:368–373`,
  `ublk.rs:586–590`). While nobody drives the engine (the device being added, up to 10 s), the
  completion waits in the channel and the ticket's one cluster stays pinned; the next drive
  handles it. No ticket leaks and the pass goes on.
- **At most one read in flight,** of at most 64 KiB. Cursor work in one step is bounded: at
  most 4,096 map entries looked at. A scan that reaches the bound without a stored block
  sets `due` 1 ms later: at most 1,000 scans a second, on any kind of node.
- **Never on the engine's thread:** `fsck`, `read_range` or any device read. No target bulk
  slot, metadata call, replication frame or membership change.
- **Stop** prevents new issue and waits for the outstanding read before publishing Stopped.
  The node's shutdown does not wait for the reader.

**Throttle.** `--scrub-rate-mib N`, N from 1 to 1024, default 32.
- **The window (S7's limit):** every device read, the single-block ones included, is charged
  before issue against a strict trailing one-second byte window; the sum stays at or below the
  configured bytes. Timestamped reads form a bounded deque: at most 16,384 reads of 64 KiB a
  second at the top rate, and reads of at least 4 KiB, so at most 262,144 entries.
- **Pacing inside it:** a read of n bytes is issued no earlier than `next`, and then
  `next = max(next, now − 10 ms) + n / rate`. A driver woken late (the 10 ms turn of an
  attached node) catches up at most 10 ms of reads back to back, so both kinds of node reach
  the rate without a burst of a whole second's bytes.
- **A ceiling:** the reader's round trip bounds what one read in flight reaches. The lab
  records the rate measured.
- **The clock** is the `Instant` the engine passes to `turn` and `handle`, which is monotonic.
  The earlier rule for a backward or failed clock reading is cut: it has no case.
- Foreground and control completions come first; issuing a read is never an engine wait.

This bounds what a pass adds to the device's queue (one read, at the rate), not the device's
service time: S7 as amended, and Risks.

**Schedule.** `--scrub-period-hours N|off`, N from 1 to 87,600, default 336. Both flags take
`--flag VALUE` as `parse` does (`node.rs:150–207`), are in `USAGE`, and are refused when the
value is not a number or is out of range. First due = start + period; each end, including
failed/stopped, schedules end + period. Restart resets schedule/counts; no persistent record or
catch-up storm. Operator starts use the same driver. At most one pass. In the lab,
`SCRUB_RATE_MIB` and `SCRUB_PERIOD_HOURS` reach the flags through `node_env` (`scripts/lab.sh`)
and `scripts/guest/node.sh`, as the node's other flags do.

## Status and local contract (S8, S9)

Lines of the node's control socket (`/run/flaminio/node.sock`, root, mode 0600): `scrub start`,
`scrub stop`, `scrub status`; no arguments/IDs accepted. Each reaches the driver as an `Input`
with a reply channel, as `delete` does (`control.rs:173–190`).
- **Responses:** `ok started PASS`, `ok already-running PASS`, `ok stopping PASS`, `ok idle`,
  or `ok JSON` for status. Malformed line: `error: usage: scrub start|stop|status`.
- **A node without scrub** (a buffered store): `scrub start` and `scrub stop` answer
  `error: scrub unavailable`; `scrub status` answers `ok JSON` as always, with `state`
  `unavailable` and `reason` `buffered`. This is what S1's "its status says so" means.
- **Answer limit:** the socket's thread waits `SCRUB_LIMIT`, 15 s, then answers
  `error: no answer within 15 s`. That is above the 10 s for which an attach leaves the engine
  undriven (`ADD_DEVICE_LIMIT`) and below `DELETE_LIMIT` and the CSI's limit. A start whose
  answer timed out may still start, as a delete may still delete.
- **Status:** disk UUID, process generation (64 random bits drawn at start, 16 hex digits),
  running/last pass start/end/result/reason, rate, checked_blocks, skipped_blocks,
  estimated_remaining, exact mismatch/unreadable totals and details.
- **Encoding:** one line of JSON written by hand (the data plane has no JSON library and gains
  none). Every string in it is a hex id or a fixed token, so nothing needs escaping and no
  input can break the line. `state` is `idle`, `running` or `unavailable`; `result` is
  `complete`, `stopped` or `failed`; `reason` is `operator`, `shutdown`, `poisoned` or
  `buffered`. A store's or device's error text goes to the node's
  log only.
- **Findings** identify the volume by the store's id (32 hex digits), chunk index, layer
  index, block and kind (`mismatch`, `unreadable`). No name: the replica service keeps none
  (`flaminio-node/src/replica/mod.rs:62–67`). The id is the first 16 bytes of SHA-256 over
  `flaminio-volume:NAME` (`flaminio-node/src/lib.rs:21–29`): docs/lab.md gives the rule and a
  one-line command, and the lab check takes the id from the fixture's answer.
- Retain/log at most **1024 findings per pass**, count the rest exactly and report the
  omitted count. Counts are u64 with saturating adds; 2^64 blocks cannot be read, so the
  earlier rule that an overflow fails the pass is cut. No data bytes/checksums/keys are
  logged. A finding stays until the next pass replaces the status, even if fixed since.
- Disk-since-start counters aggregate ended passes (complete, stopped or failed) and their
  findings; process restart clears them.

Add lab commands `lab.sh scrub-start|scrub-stop|scrub-status VM`, corresponding Make targets
using VM selectors; reuse socket validation/quoting. Add `lab.sh scrub-build` and
`make scrub-build`: it asks every data node for `fixture status`, passes when each answers
the socket's usage error (a default build) and fails otherwise, naming the node. No metadata
RPC/proto/on-disk/wire changes; ordinary read/Digest/fsck semantics and faults stay untouched.
`flaminio-repl` is not changed.

## Reproducible corruption fixture and lab (S11)

`scripts/checks/scrub.sh` (host) and `scripts/guest/scrub-check.sh` (guest), run later and only
with explicit authorization: `make scrub-check TARGET=a CONFIRM=yes`. It follows #84's rule
for a check that writes (`origin/main:docs/issues/84/spec.md`, S7): it creates the volume it
works on, takes no volume, device, offset or block from its caller, and starts, stops and
formats no node. It deletes and detaches nothing, and never reformats a device.
- **Inputs:** `TARGET` and `CONFIRM=yes` for a run. For a restore, `RESTORE=<run>` and
  `CONFIRM=yes`: a run id of the check, refused unless it matches the pattern below; the
  restore acts only on that run's artifact.
- **Mode:** the nodes run as the lab runs them: under #74's helper once it has landed
  (`PRIVSEP=1`, its default), in direct mode before. The check passes the nodes no flag.

**The volume.** One 1 GiB RF-2 volume named by the run id, `scrub-<unix seconds>-<8 hex digits
from /dev/urandom>`, checked against `^scrub-[0-9]+-[0-9a-f]{8}$` before each use. The check
attaches it on TARGET, which must have nothing attached, writes 1 MiB at offset 0, and runs
fio writing and verifying from 64 MiB to the end: in the block's chunk, away from its
cluster. The block is chunk 0, block 0, on the replica away from TARGET (CORRUPT below), read
from the volume's record, which must show two replicas of chunk 0, one on TARGET, and full
ISRs. No snapshot, discard or other writer.

**The fixture.** Only the running node knows the live mapping: the maps on disk are as old as
the last checkpoint. The fixture is root-only, on the node's control socket (no second
socket), and built only with the cargo feature `lab-tools`.
- **The feature** is declared by `flaminio-store` (the lease), `flaminio-node` (its `Input`)
  and `flaminio-io` (the lines), each forwarding to the one below. Cargo unifies features
  across the workspace, so any other line that enables it would put the fixture in every
  build.
- **The lint** (`scripts/tools/fixture-feature-lint.sh ROOT`, run by `make lint` on the tree;
  its path lacks the string it looks for) matches the exact string `lab-tools` and fails
  unless both hold:
  - **Manifests:** in every `Cargo.toml` under `dataplane/`, the fuzz workspace's included,
    the string `lab-tools` occurs only as the three declarations: `lab-tools = []` in
    `flaminio-store`, and the two forwards, `["flaminio-store/lab-tools"]` in `flaminio-node`
    and `["flaminio-node/lab-tools"]` in `flaminio-io`. Any other occurrence fails: in a
    `default`, in a dependency's `features`, or in a third feature that forwards to it.
  - **Build lines:** in the Makefile, `scripts/` and `.github/`, no line has `lab-tools`, or
    `--all-features` for the main workspace, but the four this design adds: `lab.sh deploy`'s
    under `LAB_TOOLS=1`; the `cargo test` line of `make test-fixture` (below); the
    `cargo clippy` runs of `make lint` and of `lab.sh lint`. No line of `.github/` is among
    them. The fuzz workspace's own `--all-features` (`Makefile:86`) stays: its manifest cannot
    name the feature.
  - **Exempt: the lint itself, and no other file.** Its script test,
    `scripts/tools/scrub-test.sh`, is linted as any script. No line of it has the string or
    `--all-features`: it composes both from parts at run time, and writes its doctored
    manifests and build lines with `printf` into a temporary tree that the lint is pointed
    at.
- **`make test-fixture`** (new, named so that neither the Makefile's target lines nor a
  workflow that calls it has the string) runs
  `cd dataplane && cargo test --locked --features flaminio-io/lab-tools -p flaminio-store
  -p flaminio-node -p flaminio-io`: the three crates that declare the feature, and no other.
  `make test` calls it after its own `cargo test`.
- **CI.** The job `rust-test` (`.github/workflows/ci.yml:32–41`) gains the step
  `make test-fixture` after its `cargo test --locked`: the fixture's tests and the pin test
  that needs the lease run on every pull request, the rest of the workspace's suite runs
  once, and a failure reproduces locally with the same command (`docs/ci.md`). `docs/ci.md`'s
  CI row and the Makefile's help name the target. `make lint`, a CI job already, runs the
  lint and the clippy run.
- **Build:** `LAB_TOOLS=1 make deploy` makes `lab.sh deploy` pass
  `--features flaminio-io/lab-tools` to `scripts/guest/build.sh`, which already hands its
  arguments to cargo. Every other deploy, `make release` and S7's measurement build without
  it.
- **Lines:**
  - `fixture status`: `ok none` or `ok LEASE`.
  - `fixture pin VOLUME CHUNK BLOCK`: `ok LEASE DISK-UUID VOLUME-ID OFFSET`. Refused: a VOLUME
    outside the check's pattern (the fixture leases no block of any other volume), an unknown
    volume or chunk, a number that is not decimal or is outside the chunk, a block not stored
    in the top layer, a lease already held.
  - `fixture check LEASE`: `ok unchanged` or `ok changed`; `error: no such lease` for a lease
    released, or of another process.
  - `fixture release LEASE`: the same answers, and the lease ends.
  - A build without the feature answers all four with the socket's usage error.
- **The lease** pins the block's physical cluster (the pin above) and holds a watch of that one
  block, marked by `State::apply` as a ticket's is: `changed` once a record covered the block.
  At most one lease; in memory; it ends at its release or with the process. LEASE is the
  process generation, `-`, and a counter. OFFSET is the block's byte offset on the device.
  The answers carry no data, checksum or key, and the fixture has no scrub summary and no
  replication authority.

**Guards before each raw write** (guest; each stops the script with nothing written):
1. **Ids:** the run id, LEASE (`^[0-9a-f]{16}-[0-9]+$`), DISK-UUID, VOLUME-ID and OFFSET
   (`^[0-9]+$`) match their patterns before they reach a socket line or a command.
2. **The node:** exactly the unit's `flaminio-io` runs, and its `--device` resolves to the
   disk `data_disk()` finds (`scripts/guest/common.sh:11–16`; the process checks of
   `confirm.sh`'s `data_plane`, `scripts/guest/confirm.sh:68–76`). Under #74 the unit's
   process is the helper, which holds `--device`, and `flaminio-io` is its child: the guard
   takes `data_plane`'s rule as #74 leaves it.
3. **The disk:** `flaminio-disk inspect` of that device gives DISK-UUID as the superblock's
   disk uuid.
4. **The offset:** a multiple of 4,096, at or above the data area's offset and at least 4,096
   below its end, both from the same `inspect` (data offset, clusters, cluster size).
5. **The lease:** `fixture check LEASE` answers `ok unchanged`.

**Procedure.** Each step that fails ends the run as failed, after step 7 if step 3 began.
1. **Before anything is created:** `CONFIRM=yes`; TARGET is a data VM with nothing attached;
   every data node answers `fixture status` with `ok none` (a usage error is a default build:
   the check says to deploy with `LAB_TOOLS=1` and restart the node); no leftover (below).
2. **Create, attach, write, fio.** Record the volume's epoch and ISRs.
3. **Corrupt** (guest `corrupt`, on CORRUPT): `fixture pin RUN 0 0`; the guards; read the 4 KiB
   with direct I/O; write the artifact (the block, LEASE, OFFSET, DISK-UUID, under
   `/var/lib/flaminio/scrub-check/<run>/`, root, mode 0700) and fsync it and its directory;
   guard 5 again; write the block with one bit flipped, direct, and flush. `fixture check`
   must still answer `ok unchanged`: otherwise the block moved and there is nothing to find.
4. **A pass on CORRUPT** (`scrub start`, then `scrub status` until it ended): complete, with
   one finding: a mismatch, VOLUME-ID, chunk 0, block 0.
5. **A pass on TARGET:** complete, no finding.
6. **Unchanged:** the volume's epoch and ISRs as in step 2, no session lost in TARGET's node
   log since then, and fio's verification passes.
7. **Restore** (guest `restore`; also the EXIT/INT/TERM trap's, and by hand after a killed
   run: `make scrub-check RESTORE=<run> CONFIRM=yes`, which runs it on every data node). It
   works from the artifact alone and passes the guards again.
   - **`ok unchanged`:** write the saved bytes at the saved offset, flush, then
     `fixture release`, which must answer `ok unchanged`: `restored`.
   - **`ok changed`, or `no such lease`** (the node restarted, and the cluster may have been
     reused): nothing is written; a lease that exists is released: `not-restored`.
   - **`ok changed` at the release, after the write:** a write raced the restore:
     `not-restored`. The pin kept the cluster from reuse, so only the run's own volume can hold
     the old bytes.
   - The artifact is removed once the verb has concluded. With no artifact nothing was
     written under a lease (step 3 saves first): the verb releases a lease the node still
     holds and prints `nothing`. `not-restored` fails the run.
8. **After `restored`, a pass on CORRUPT:** complete, no finding. The check says that nothing
   repaired the block.
9. **Always:** it prints `VOL=<run> make vol-delete`, the operator's existing command. It
   deletes the volume and, with it, a block that was not restored.

**Leftovers** refuse the next run, each named with what to run (`RESTORE=<run> CONFIRM=yes`, then
`vol-delete`): a volume or tombstone with the check's pattern in the metadata service, a lease
on a data node, an artifact directory. Only an id matching the pattern is printed in a command.

**Length.** A pass covers the whole disk, the lab's other volumes included: with 16 GiB stored
at 32 MiB/s, about nine minutes, twice on CORRUPT and once on TARGET.

## Files and rule-to-test mapping

| Rules | Files and concrete validation |
|---|---|
| S1, S2 | Store `scrub.rs` tests: layered maps, holes/zero/discard/free clusters; detached/stale/fenced replicas; continuously present blocks exactly once; chunks and layers created during the pass passed over; a slot deleted and created again left alone. |
| S2, S3 | Store property tests interleave in-place/redirected writes, deferred/pipelined commits, discard/delete/recreate and scrub completions on a memory device: no good-block findings. Persistent damage finds exactly the surviving blocks. One damaged block under continuous writes to the other blocks of its cluster and chunk: reported, and the pass ends within three reads a block. An entry changed between a block's two tickets: skipped, counted, no second read. The test's device takes each read after the records it interleaves have been applied, so a write inside a read is seen by it. Two test-only mutations must each fail: `report_first_mismatch` fails the property; `ignore_marks` fails a directed schedule, a block written A, B, A, B with the first B applied inside the first read, A written back between the tickets and the second B applied inside the second read (without the mutation: skipped). |
| S3–S5 | Store tests: a record applied between a ticket and its completion (each kind) marks only the blocks it covers; a device failing one block of a 64 KiB read gives one unreadable block and 15 checked; a read failing once gives none; the store is not poisoned by read errors. An in-place write that fails while a read of its block is out: a failed pass, no finding; an already poisoned store: failed. |
| S2, pin | Store tests: a pin held while other clusters are rewritten and checkpointed: free space returns and the pinned cluster is not allocated; released, it is. A pass with a ticket always out leaves at most the pinned clusters blocked. A dropped pin does not unblock a cluster while a client read is in flight. A lease and a ticket on one cluster, the ticket finished, the cluster freed and checkpointed: still not allocated; after the lease's release it is. |
| S1, S6, S10 | `flaminio-node/tests/engine.rs`, with a `ScrubReader` that holds reads: target/replica-only nodes, any fence; identical client/replication outputs, metadata calls and session state with/without pass; no repair writes. A read held on a replica-only node: heartbeats and replica requests answered. A read held across `Attach`, a gap with no drive, and `Detach`: the ticket is finished, its pin dropped, the pass goes on. Driven as #15's loop (a wake at `next_wakeup`; `flush` unless the 10 ms turn is due): the read is issued and a sparse map is scanned; after every step `next_wakeup` is later than now, and every wake at `due` issues a read or scans. |
| S1, S3, S4 | `flaminio-io` `scrub_io.rs` tests on a temporary file opened as the store opens it (the reader's logic only): block CRCs returned through a duplicate of the descriptor, an error returned, the store's position untouched. A read that reaches past the end of the file returns an error, not CRCs (S3: a short read is never a mismatch; S4: it is a read error). An offset or a length that is not a multiple of 4,096 is answered as an error with nothing read. A descriptor without `O_DIRECT`: no reader, `scrub start` refused, status `unavailable` and `buffered`. `flaminio-uring`: `try_clone_fd` gives a descriptor that reads what the device reads and outlives no claim (unit). Under #74 (added by whichever lands second, beside `flaminio-io/tests/privsep.rs`): a node started with `--privsep` runs a pass to its end. It needs a real PKI directory (that harness's node blocks on `token.pub` before `build` starts the reader) and a `disk` answer that is a file opened with `O_DIRECT`. |
| S7 | Driver fake-clock tests examine every sliding 1 s window including retries/boundaries; one in flight/64 KiB bound and bounded scan work. Driven as each kind of node (woken at `next_wakeup`; 10 ms turns plus completion wakes), an idle node reads within 10% of the configured rate and never above it. `--scrub-rate-mib` of 0, 1025 or not a number is refused. |
| S5, S8, S9 | Node driver/control tests: automatic/off/default and restart schedule; `--scrub-period-hours` of 0, 87,601 or not a number refused; start twice/stop idle/malformed commands; stop/shutdown with a ticket out; exact counters after 1024 details and no payload in logs; a store error whose text has quotes and newlines still gives one valid status line, with the token only; no answer within `SCRUB_LIMIT` gives the error line. |
| S11 (fixture) | `control.rs`, default features: the four `fixture` lines get the usage error. With `lab-tools`: each refusal of `fixture pin`, one lease at a time, `changed` after each kind of record, `no such lease`. `make test-fixture` runs the three crates' tests with the feature; `make test` and CI's `rust-test` job call it. `make lint` and `lab.sh lint` each gain a clippy run with the feature. |
| S8, S11 | `scripts/tools/scrub-test.sh`, added to `make lint`, with VMs, socket, disk tool and disk faked: each guard's stop with nothing written; a default build stops before the create; each leftover's refusal; failed writes/flushes/pass/membership/fio each followed by the restore; restore refused after `changed`, after `no such lease` and at a `changed` release; the restore run again after a killed run, and with no artifact; signal cleanup; the finding's id matched to the fixture's VOLUME-ID; `deploy` passes the feature only under `LAB_TOOLS=1`; `lab.sh scrub-build` passes when every data node answers `fixture status` with the usage error and fails, naming the node, on any other answer. `fixture-feature-lint.sh` passes on the tree, the script test included (which has neither the feature's string nor `--all-features` on any line), and fails on each doctored copy, written at run time into a temporary tree: the feature in a `default`, in a dependency's `features`, in a third feature that forwards to it, in `dataplane/fuzz/Cargo.toml`, on a `cargo build` line of the Makefile, of a script and of a workflow, on a build line of any other `scripts/tools/*.sh` (a copy of the script test itself among them), and `--all-features` on a main-workspace build line. `scripts/tools/node-scripts-test.sh`: `SCRUB_RATE_MIB` and `SCRUB_PERIOD_HOURS` reach the flags. |
| S11, S12 | Lab procedure above, docs/help fixture checks, unchanged offline digest/fsck tests and format compatibility. |

Touched: store `scrub.rs/state.rs/bitmap.rs/store.rs/lib.rs`; node `scrub.rs/engine.rs` plus
tests; daemon `scrub_io.rs/node.rs/ublk.rs/control.rs`; `flaminio-uring/src/uring.rs`
(`try_clone_fd`); the `lab-tools` feature in the three crates' manifests;
`.github/workflows/ci.yml` (the `rust-test` step) and `docs/ci.md` (its CI row);
`scripts/lab.sh`, `scripts/guest/node.sh`,
`scripts/guest/scrub-check.sh`, `scripts/checks/scrub.sh`, `scripts/tools/scrub-test.sh`,
`fixture-feature-lint.sh` and `node-scripts-test.sh`; the Makefile (`test-fixture`,
`scrub-build`, the scrub and check targets, `test`, `lint`, and its help) and README/docs.
`BlockDevice` and `flaminio-repl` are not touched. Reuse the existing aligned buffers; no new
library/cipher dependency and no new `unsafe`.
Implementation checks `make test lint race contracts`, sanitizer quick run for ticket lifetimes.
No tests, corruption, raw writes or lab actions ran while drafting this design.

## Measurement, docs, risks and ADRs

Lab measurement later, on a default build (no `lab-tools`) and in the lab's default mode
(under #74's helper once it has landed). Its first step is `make scrub-build`, which must
pass; docs/lab.md's procedure starts with it. Then: performance-plan durability/cache
settings, three interleaved with/without-pass runs for 4K QD32 and 1M reads/writes, scrub on
target then peer; record rate/length, foreground throughput/p99, binaries/raw logs. Keep
32 MiB/s only if impact meets the spec's 10% criterion; otherwise record evidence and propose
a lower default. No result invented.
Settled at approval, 2026-11-13: Q4's 10% criterion (see decisions.md, Settled at approval).
Document in docs/lab.md, dataplane/README.md, architecture/roadmap: the commands;
`SCRUB_PERIOD_HOURS=off` for measurements; restart forgetting; the 1024-detail cap; no repair;
how a finding's volume id maps to a name; the check, its `LAB_TOOLS=1` build and its guarded
restore.

**Risks: named, not accepted.**
- **A device that stalls.** The engine never waits for a pass's read, but a read of a failing
  sector can make the device stall its other I/O, and that can cost a request, a session or a
  member. S7 is amended to say so; isolating classes of I/O on one device belongs to #75.
- **A restore refused.** After a node restart, or once the block changed, the check writes
  nothing (S11 step 5 as amended): the block stays wrong in the check's own volume until the
  operator's `vol-delete`.
- **A raw write beside a live store.** A write to the block between a guard and the raw write
  is not excluded, only confined: the pin keeps the cluster from reuse, so the bytes land in
  the run's own volume or in a free cluster. The fixture leases only volumes with the check's
  pattern.
- **The fixture in a build.** Nothing but the lint (manifests and build lines) and the
  default-features test keeps `lab-tools` out of a release build; a running node tells the
  two builds apart only by `fixture status`, which `make scrub-build` asks. The measurement's
  runs are the existing bench targets, so that first step is the operator's to run. Its
  feature boundary is reviewed before implementation, as settled at approval.
- **A leaked lease** (a killed run) keeps one cluster from reuse until `RESTORE=<run>` or the
  node's restart.
- **A buffered node has no scrub.** It is a development mode; the lab and #74's helper run
  direct unless told otherwise.
- **A block rewritten during its read** is skipped, not checked, in that pass (S2).

CRC-32C does not authenticate a malicious replica; the audit's proposed 0009 and #50 own that
separate contract. No replica-safety/format rule changes, no new ADR is required;
accepted fencing/etcd ADRs remain unchanged. Pure cursor/store tests and daemon/script
integration can proceed separately against the ticket/status contracts.
Settled at approval, 2026-11-13: Q1 to Q6, each with its recommendation, and the lab-only pin
helper, its feature boundary reviewed before implementation (see decisions.md, Settled at
approval; its mechanism was specified by the plan review's round 1).
