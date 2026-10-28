# Issue #5: zero-copy local reads fail with EINVAL — design

## Root cause

Found on fl-a on 2026-10-28, with `vol1` replicated on fl-a and fl-c:

- `scripts/checks/reconcile.sh` with `ZERO_COPY=1`: the first read came after 3.2 s, and one
  block of every chunk after 7.1 s. Without zero copy, the same check took 0.2 s and 0.9 s.
- The node's log, for every failed local read:
  `local read: device read returned -22, not 4096`. One such line comes from the fast path and
  one from the queued path. Then `local read failed`, the request timeout 3 s later, and a new
  ISR without fl-a.
- `/sys/block/*/queue/dma_alignment` reads 3 on `ublkb1` and 511 on `vdb`, the disk under the
  store. The ublk device takes direct I/O into any buffer aligned to 4 bytes. The disk, opened
  `O_DIRECT`, needs 512, and the kernel refuses anything else with EINVAL before any I/O.
- A direct read of `ublkb1` into a buffer 4 bytes past a page boundary took 3.006 s and gave the
  same log lines. At 0 and 512 bytes past it, the read took under a millisecond.
- A bpftrace probe on `blkdev_read_iter` for ublk devices, during the check, showed every
  misaligned direct read. All came from the check's own `dd iflag=direct`, which is uutils
  coreutils 0.10.0 on these VMs. Its buffer starts 0x6b0 bytes into a page.
  `dd iflag=direct` on `/dev/vdb` itself fails with "Invalid input".

So the attach is not the cause. Right after an attach, the check's `dd` is simply the only
reader. Copy mode never showed the problem, because there the disk reads into the frontend's
own page-aligned buffer.

## Frontend: copy through the staging slot (S1)

`zc_read_plan` (`dataplane/crates/flaminio-io/src/ublk.rs`) submits one `ReadFixed` per device
read of the plan, into the request's pages at `at + r.buf_offset`, then awaits them all. The
change is in that await loop. A read that returned `-EINVAL` is done again through the tag's
staging slot:

1. An `opcode::Read` on the disk's descriptor, into the slot's memory at
   `at + r.buf_offset`, at the same device offset and length. The slot is page-aligned, and
   `at` and `r.buf_offset` are multiples of the store's 4096-byte block, so the disk accepts it.
2. `stage_to_request` for the same range: a `ReadFixed` from the staging memfd into the
   request's pages. The memfd is not opened `O_DIRECT`, so the kernel copies with no alignment
   rule.

The request offset of each device read, `at + r.buf_offset`, is computed once. The
`ReadFixed`, the staged `Read` (as the slot offset) and `stage_to_request` all use that value,
so none of them can drop either term. The lab probe tests a non-zero `at`. With one-block
pieces, its `r.buf_offset` is 0, so this rule is what covers `r.buf_offset`.

These staged reads run one after another, after the zero fills. They are rare, so being
sequential costs nothing that matters. A staged read that fails fails the plan, which fails the
local read (S2). Other errors are not retried. The existing line
`local read: device read returned {got}, not {expected}` moves after the retry decision. It is
logged for an error other than EINVAL, and for a failed staged read, with "(staged)" appended.

The slot's memory is reached through a new `Staging::addr(i, at, len) -> *mut u8`. It asserts
`i < slots` and `at + len <= slot`. It does not borrow the slot: the kernel writes it while no
`with_slot` closure can run on that slot. Each slot belongs to one tag, and only that tag's task
uses it. The zero fills write other ranges of the same slot: the gaps of the plan, which never
overlap its device reads.

The first staged read in a process logs once, from a `static AtomicBool`: "zero copy: a read's
pages are not aligned for <device> (its DMA alignment); such reads are copied through the
staging area". A line for every read would flood the log under a workload that is misaligned
on purpose.

The copy-mode frontend (`local_read`, `fast_read_copy`) is unchanged. The disk reads into its
own buffers there.

## Engine: report the failure (S2)

`dataplane/crates/flaminio-node/src/engine.rs`. A pending local read was admitted on the
target's current local session (`*session == msg.session`), so its `sref.epoch` is the epoch of
the target that sent it.

`local_read_done` releases the plan and removes the pending read, as now. Then, on both paths,
it returns early if `p.sref.epoch` is not `rt.sm.epoch()`, before it touches `rt.reads`. The
engine purges `local_reads` only on detach, so a report can outlive the target that made the
request. A new target numbers its sessions and requests from 1 again, so without this check, a
stale report could remove the new target's entry from `rt.reads` and drop its read. The
success path then builds its `FromReplica` with `epoch: p.sref.epoch`.

With `ok = false`, after its log line, it removes the read from `rt.reads`, keyed
`(local, req)`, as a reply would. Then:
- if a newer target fenced the local replica (`fenced_epoch(p.sref)` above `p.sref.epoch`),
  it answers `Stale`, as the success path does. The store refuses to plan a read at an epoch it
  was fenced past, so a fence makes reads fail: that must depose the target, not send the read
  to another member;
- otherwise it calls `rt.sm.on_read_failed(us, local, p.sref.epoch, p.session, p.req)`, then
  `self.drain(now, &volume)` so the retried read's output goes out.

The target checks the epoch again, as `on_reply` does, so neither check depends on the other.

The engine test for a stale report has to see these numbers, and it is an integration test, a
separate crate. The engine therefore gets
`#[doc(hidden)] pub fn local_read_ids(&self, lr: &LocalRead) -> Option<(u64, u64, u64)>`, which
returns a pending read's session epoch, session and request.

`OpResult::Unreadable`, the new result, becomes `IoError::Unreadable` ("no replica could serve
the read"). The frontends answer it with EIO, as they answer every error except `Invalid`.

## Target: fail the read over (S2)

`dataplane/crates/flaminio-repl/src/target.rs`. New method:

```rust
/// The client read `req`, sent to `from` at `epoch` on `session`, failed there without a
/// reply (a local read, docs/5). It is sent again to another ISR member, and `from` stays a
/// member.
pub fn on_read_failed(&mut self, now: Time, from: NodeId, epoch: Epoch, session: u64, req: u64)
```

1. It ignores the call, as `on_reply` ignores a reply, when:
   - the target was deposed;
   - `epoch` is not the target's;
   - `from` has no session `session`;
   - that session has no request `req` in flight;
   - `client_read_of(from, req)` is `None`. Only client reads come from the frontend, so the
     request stays in flight, untouched.
2. It removes the in-flight entry and the `ReadOp`, and releases the read's lock. Client reads
   hold no bulk slot.
3. It queues the read again at the front, as `Work::Client`, and records `op → from` in `avoid`.
4. It calls `self.pump(now)`.

`Target` gets a field `avoid: BTreeMap<u64, NodeId>`, holding the member each re-queued client
read must not use. In `try_start`, `Op::Read` asks `self.source(i, self.avoid.get(&op).copied())`.
When that finds no member, a read with an `avoid` entry ends as `OpResult::Unreadable` if the
chunk's ISR has no member other than the avoided one, members being removed not counted. Any
other read waits, as it does today.

`OpResult::Unreadable` is a new variant: "A read that no ISR member could serve: the one that
failed it was the only member left." `OpResult::Failed` keeps its meaning, "the target was
deposed". The simulator and the checker treat the new variant like `Failed`, as an operation
without a result.

The `avoid` entry goes once the read is sent to another member: if that member fails it too
(its session drops, say), the read may go back to the local replica rather than end as
unreadable while the local replica is still a member. It also goes on `Unreadable`, and on
`depose`, which clears the whole map. Under `cfg(test)`, `avoid_len()` lets the unit tests
check this. The check for another member shares `source`'s rule for members being removed
(`may_read_from`), so the two cannot disagree.

The session stays open and the member stays in the ISR, so `fail_peer` and `member_failed` are
not involved. A fast read that fails already falls back to the queue (`finish_fast_read` with
`ok = false`). It then becomes a queued local read, and if that fails too, this path applies.

## Simulation

`sim::Config` gets `local_read_fail_pct`, the percent of the target's client reads from its own
replica that fail. It is drawn last in `Config::random`, from `[0, 0, 5, 20]`, so every earlier
field of every seed keeps its value. It is 0 in the hand-written scenarios.

In `deliver`, a `Body::FromReplica` reply is a candidate when all of these hold:
- the run is in chaos (`self.chaos`), as for the random message loss in `send`. The final reads
  after settling must all complete;
- it comes from the target's own node (`p.from == p.to`);
- its epoch is the target's (`m.epoch == t.epoch()`);
- it is `Response::Data`, for a request that `client_read_of` says is a client read.

Only for a candidate, and only when `local_read_fail_pct > 0`, the simulator draws
`self.rng.pct(local_read_fail_pct)`. On a hit, it calls
`t.on_read_failed(local, p.from, m.epoch, m.session, m.req)` instead of `t.on_reply`. Seeds with
a rate of 0 draw nothing new, so they run exactly as before. That includes the hint seed of
`catches_fast_reads_across_a_fence`. Seeds with a rate above 0 run differently. `make sim` is
rerun, and the table of first-catching seeds in `docs/replication-protocol.md` §9 is updated
where a seed moved.

The checker already proves that every completed read returns the latest acknowledged write, so
it covers reads that went elsewhere. An unreadable read has no result, which the checker
allows.

## The lab check (S3)

`scripts/checks/reconcile.sh`:
- **Before restarting anything:** it fails unless `NODE` is in `replicas`, the replicas of
  `VOL` read from the metadata service. It also computes `LOCAL_CHUNKS`, the indices `c ≥ 1`
  of the chunks whose `replicas` include `NODE` (`jq` over `.volume.chunks`), and passes them to
  the remote part with the other variables.
- **On `NODE`, after `dd` reads block 0:** a `python3` snippet runs the probe.
  1. It reads the first block of each chunk in `LOCAL_CHUNKS`, page-aligned, until one is not
     all zeros.
  2. It reads the two blocks that end chunk `c − 1` and start chunk `c`, with `O_DIRECT`. The
     first read goes into an anonymous mapping, 4 bytes in, and the second page-aligned.
  3. It prints the chunk and fails if the reads differ or no chunk qualified. Chunk `c` is on
     `NODE`, so its piece is a local read.

  The range crosses a chunk boundary, so the read has two pieces, the second at `at = 4096`.
  The block holds data, so the plan has a device read, not only a zero fill.
- **Times:** the check takes the time before the attach as `t0` (`date +%s.%N`). At the end,
  it reads `sudo journalctl -u flaminio-node --since "@$t0" -o cat` on `NODE`.
  - It fails, printing the lines, if any matches
    `grep -E 'local read(:| failed)|on a local read|timed out'`. These are the failure lines
    from `zc_read_plan`, `local_read_done`, the copy-mode checksum mismatch and the target's
    request timeout. The startup lines, `zero copy: local reads go straight into the requests`
    and `EXPERIMENT: local reads are not checked`, do not match.
  - If the log has the zero-copy startup line, it also fails unless one line contains
    `copied through the staging area`. The mode comes from the node itself, so the check
    cannot skip this by mistake. The check restarts `NODE`'s node, since `NODE` holds a
    replica, so the one-time line is new.

## Protocol document (S5)

`docs/replication-protocol.md`:
- **§5:** a failed local read goes to another ISR member at once, and the local replica stays a
  member. With no other member, the read is unreadable. The rule that members being removed
  never serve reads still holds.
- **§9:** the injected local read failures, and the updated seed table.

## Decisions

- **Copy the misaligned reads, rather than advertise the disk's DMA alignment on the ublk
  device.** A `UBLK_PARAM_TYPE_DMA_ALIGN` of 511 would make the block layer refuse misaligned
  direct I/O with EINVAL, as the raw disk does. But the device would then refuse, with zero
  copy only, requests it serves in copy mode. One of them is the lab check's own `dd`. Zero
  copy must not change what the device accepts. The copy costs one refused submission, a
  device read and a memory copy, for the misaligned reads only.
- **Keep the local replica in the ISR when one of its reads fails.** That is what the issue
  asks. Dropping it never repaired anything: a resync copies only the writes the member missed,
  not an extent it cannot read. A disk that fails altogether still leaves the ISR through its
  failed writes. A disk that fails only reads costs each read one fast failed attempt, not a
  3-second stall.
- **A new result, `Unreadable`, rather than reusing `Failed`.** `Failed` means the target was
  deposed, which the engine reports as "the volume moved to another node". That would mislead
  for a read whose only replica failed.
- **Only the local replica fails over this way.** A remote replica would need a new protocol
  response for a failed read. It is out of scope (intent), and remote reads still fail over
  through the request timeout.
- **Only EINVAL leads to the staged copy.** EINVAL is how the kernel refuses an unusable buffer
  for direct I/O. EIO and the rest are disk failures, and copying would not help them. If an
  EINVAL ever comes from a bug, such as a misaligned device offset, the staged read fails too,
  and S2 takes over.
- **No checksum verification of the staged data.** Zero copy does not verify, and the staged
  data, although it passes through the process, is part of a zero-copy read.
- **Log the first staged read only.** This keeps the log readable. A count can come later, if
  someone needs one.

## Risks

- **A misaligned workload under zero copy pays a refused submission and a copy per device
  read.** This is still far cheaper than the 3-second stall it replaces. A workload like that
  is better off in copy mode, and the log line says so by naming the cause.
- **`avoid` holds an entry for every client read waiting to retry.** At most one per queued
  client read, and every way an operation ends removes it.
