# Issue #84: Validate the exported block durability contract — design

[Specification](spec.md) · [Decisions](decisions.md).
Approved 2026-11-13; not implemented.

The facts it starts from:
- Node ublk params: `types` is `UBLK_PARAM_TYPE_BASIC` alone, `attrs: 0`, block shift 12
  (`ublk.rs:936–967`). With no discard parameters the kernel keeps the discard and write-zeroes
  limits at 0 and refuses both itself. The standalone `ublk` command's device (`ublk.rs:166–230`)
  does advertise discard; it is not the exported volume device and is outside this contract.
- Copy and zero-copy node tasks both decode `op_flags & 0xff` alone, no-op FLUSH/DISCARD, answer
  a READ/WRITE larger than the buffer with EINVAL and any other op with EOPNOTSUPP
  (`ublk.rs:1220–1227,1279–1281,1454–1456`). Zero copy answers `UBLK_IO_F_NEED_REG_BUF` with EIO
  first; it registers without `UBLK_AUTO_BUF_REG_FALLBACK`, so the kernel fails such a request
  itself (`ublk.rs:1367–1386`). `start_sector << 9` is unchecked; the engine refuses zero
  length, misalignment to 4 KiB and an end past the volume as EINVAL (`engine.rs:1698–1709`).
- With no volatile cache advertised, the block layer completes an empty flush itself and clears
  `REQ_PREFLUSH`/`REQ_FUA` on writes before the driver (`submit_bio_noacct`, `block/blk-core.c`;
  `Documentation/block/writeback_cache_control.rst`). A cacheless ublk server is handed neither
  `UBLK_IO_OP_FLUSH` nor `UBLK_IO_F_FUA`.
- Engine submission splits at chunks; completion aggregates pieces (`engine.rs:1685,1880`).
- The store's device trait defines durability after flush; crash tests model persisted and
  volatile writes (`flaminio-store/src/device.rs`, `tests/crash_consistency.rs`). The node's
  device flushes with `fdatasync` (`flaminio-uring/src/uring.rs:211–214`).
- The current simulator checks per-block histories. No test here establishes arbitrary
  multi-block atomicity or actual kernel ordering merely by inspecting attrs. Its
  `ack_before_shrink` and `late_join` mutations are the early acknowledgements
  (`flaminio-repl/src/target.rs:63,73`; `sim/tests.rs:396,409`).
- Lab: kernel 7.0 (`docs/lab.md:17`), libublk 0.4.8 (`dataplane/Cargo.lock`). A node serves one
  attached volume, at the path its control socket answers to `attach` and `status`
  (`control.rs:5,104–106`), and refuses a second attach (`control.rs:94–96`). Buffer mode and
  queue count are fixed at its start; `node.sh up` and `delete_volume` can start a node with
  the caller's settings (`scripts/guest/node.sh:44–57`; `scripts/lab.sh:679,1742–1751`).

## One frontend contract (S1, S5, S8)

Extract node parameter construction into a pure `node_params(size: u64, max_io: u32)` in
`ublk.rs`, used by both buffer modes, returning today's values exactly: `types =
UBLK_PARAM_TYPE_BASIC`; `attrs = 0`; `logical_bs_shift`, `physical_bs_shift` and `io_min_shift`
12; `io_opt_shift` 16; `max_sectors = max_io >> 9`; `dev_sectors = size >> 9`; all else zero.
Discard stays unadvertised: no `UBLK_PARAM_TYPE_DISCARD`, no discard or write-zeroes limit.

Add a pure `classify_node_request(op_flags: u32, start_sector: u64, nr_sectors: u32, size: u64,
max_io: u32) -> NodeRequest`, with `NodeRequest::{Read { offset, len }, Write { offset, len },
Flush, DiscardHint, Refuse(errno)}`. It is the only reader of a node request's descriptor: both
tasks call it before staging or engine submission, match its result exhaustively, and complete
`Refuse(e)` with `-e`. The first matching rule decides:

1. **Registration:** `UBLK_IO_F_NEED_REG_BUF` set → `Refuse(EIO)`, in both modes. This is
   today's zero-copy path; the kernel does not set the bit in copy mode.
2. **Flags:** any of bits 8–31 outside `UBLK_IO_F_FAILFAST_DEV`, `_FAILFAST_TRANSPORT`,
   `_FAILFAST_DRIVER`, `_META`, `_FUA` and `_SWAP` → `Refuse(EOPNOTSUPP)`. That refuses
   `_NOUNMAP`, `_INTEGRITY`, `_SHMEM_ZC` and every bit the pinned header does not name. The six
   accepted bits change nothing: FUA asks for what every write already gets.
3. **Op** (`op_flags & 0xff`): `UBLK_IO_OP_FLUSH` → `Flush`, `UBLK_IO_OP_DISCARD` →
   `DiscardHint`, sector fields ignored; both complete 0 with no staging and no engine call.
   `READ` and `WRITE` → rule 4. Any other value (`WRITE_SAME`, `WRITE_ZEROES`, the zone ops, an
   unknown one) → `Refuse(EOPNOTSUPP)`.
4. **Range,** READ and WRITE: `Refuse(EINVAL)` when `nr_sectors` is 0, `start_sector` exceeds
   `u64::MAX >> 9`, offset or length is not a multiple of 4096, length exceeds `max_io`, or
   `offset.checked_add(len)` is `None` or exceeds `size`, as the engine tests it
   (`engine.rs:1702`). Otherwise `Read`/`Write` with the byte offset and length, served by
   today's paths. The engine's own check stays.

No wire, metadata or on-disk field changes are needed for the initial conformance work.
Discard success does not change data or free fencing records. Do not set a zero-after-discard
capability. Raw geometry uses #80's admission contract; this issue tests that the frontend
agrees rather than redesigning it: rule 4 uses only the advertised `size` and `max_io`.

## Completion and ordering evidence (S2–S4)

Build a test driver around the engine adapter with per-request sequence IDs and a trace of
invoke, per-piece completion, effective ISR, persistence completion and final response. It is
an observation tool, not an alternative permission check. Preserve `Target::check_write` and
membership rules. Record errors only after accounting for every piece; never convert a failed
piece into success or pretend previously durable pieces rolled back.

FLUSH gets no ordering rule. A flush covers the writes completed before it was issued
(`writeback_cache_control.rst`), and S2 makes each of those durable before its completion. So
`Flush` stays a no-op that reads no queue state, and a write in flight on another queue is not
its predecessor. No drain, sequence counter or replication rule is added, hence no new
simulator rule: the two existing early-acknowledgement mutations must stay caught. On the node
device the arm is also unreachable (the facts above), so no kernel test can drive FLUSH or FUA
into a task. The classification table proves the tasks' decisions; the kernel check proves the
advertisement and that the kernel issued neither. A unit test pins `attrs == 0`: setting
`UBLK_ATTR_VOLATILE_CACHE` is write-back, out of scope (S8).

FUA writes use the same durable path as ordinary writes: the same `Write`. Test that neither
adapter can acknowledge before effective-ISR commit, including an ISR RPC whose reply is lost
or delayed. No cache flag or write-back option is added as a shortcut to make the test pass.

## Kernel check (S3, S5–S7)

`scripts/checks/block-contracts.sh` (`make block-contracts`) and, in the VM,
`scripts/guest/block-contracts.sh`. Shell and the VM's tools only (`dd`, `blkdiscard`, `cmp`,
sysfs, tracefs): no Rust helper, no new crate, no daemon change. It measures what a cacheless
device lets one observe: the advertised limits, and which requests the kernel issued. Fault
injection, restarts under load and power loss stay with the engine, store and chaos tests.
Virtual device tests do not certify physical PLP. The exact procedure, CLI, result and
fixtures are the appendix [kernel-check.md](kernel-check.md). In short:

- **Three changes, no cleanup:** the check creates one 1 GiB volume named by its random run
  id, attaches it on NODE and writes under 32 MiB to it. It deletes and detaches nothing,
  starts, stops and formats no node, signals no process, keeps no state that any run reads
  and reads no node setting from its caller. It ends by printing the operator's restore command.
- **One cell a run:** the buffer mode and queue count NODE's node already runs with. A node
  that is not running is `refused`; no admission is bypassed.
- **Guards:** the only inputs are `NODE` and `CONFIRM`. The guest script takes the validated
  run id alone and reads the device from the node's own `status` answer, which must name the
  run's volume before each step that writes or discards. Another volume found attached is left
  alone and fails the run.
- **Interrupted or uncertain:** the check stops and says what to run by hand. The next run
  refuses while the metadata service lists a volume or tombstone with the check's id pattern,
  or while NODE still has the guest's directory or trace instance.
- **Oracle:** the limits in sysfs (`write through`, `fua` 0, no discard); a
  `block:block_rq_issue` trace that shows every write call and no flush, FUA or discard at the
  ublk device, while the store's flush reaches the data disk; the discard ioctl refused; the
  data read back equal.

## Files touched

- `dataplane/crates/flaminio-io/src/ublk.rs`: `node_params`, `classify_node_request`, both
  tasks' use of them, and unit cases S1/S3/S5/S8.
- `dataplane/crates/flaminio-node/tests/engine.rs`: per-piece, FUA-path and overlap cases S2/S4.
- `dataplane/crates/flaminio-repl/src/sim/{mod,check,tests}.rs`: not changed, as no ordering
  rule changes; the current per-block contract and mutation coverage S2/S3/S4/S8 stay.
- `dataplane/crates/flaminio-store/tests/{async_commit,crash_consistency}.rs`: cases S2/S6.
- `scripts/checks/block-contracts.sh`, `scripts/guest/block-contracts.sh`: the check S3/S5–S7.
- `scripts/tools/block-contracts-test.sh`: the appendix's fixtures S7, run by `make lint`.
- `Makefile`: the `block-contracts` target, its help line, the script test in `lint`.
- `docs/ARCHITECTURE.md`, `docs/lab.md`, `README.md`, `docs/followups.md`: measured contract S8.

## Contracts

No changed RPCs, control-socket commands, wire frames or disk records. The check reads the
existing `status` answer (`ok <volume> /dev/ublkb<id>` or `ok none`) and adds no command.

- **Exported device** (spec S1–S6): the `node_params` values above; aligned 4 KiB blocks;
  cacheless durable writes; FLUSH and FUA consumed by the kernel, and harmless if delivered;
  discard and write-zeroes not advertised; no multi-block transaction.
- **Request decisions:** `classify_node_request` rules 1–4, in that order. EIO for a
  registration failure and for failed durability, EOPNOTSUPP for a refused flag or op, EINVAL
  for a refused range; never success.
- **Kernel check:** its CLI, exit codes (0 `pass`, 1 `fail`, 2 `refused`, 3 `unsupported`; only
  0 is a pass), guest verbs, result JSON and reasons: [kernel-check.md](kernel-check.md).

## Tests

- S1: ublk unit tests `node_params_are_cacheless_4k_without_discard` and `classification_table`:
  every flag bit 8–31, every op the header names and one it does not, zero length, sector
  overflow, misalignment, the `max_io` and `size` edges, `UBLK_IO_F_NEED_REG_BUF` with every op,
  and the wrapping sum: `start_sector = (u64::MAX >> 9) - 7`, `nr_sectors = 8` →
  `Refuse(EINVAL)`. The tasks themselves need a kernel queue: the kernel check runs each.
- S2/S4: engine `cross_chunk_error_never_reports_success`, `fua_waits_for_effective_isr`,
  `retry_after_partial_completion`, `overlapping_queue_history_is_per_block_linearizable`.
- S3: table rows FLUSH → `Flush` with and without FUA, WRITE with FUA → the same `Write`; the
  params test pins `attrs == 0`. `make sim`: `catches_ack_before_shrink` and
  `catches_late_join` still fail the checker. Kernel, one run per buffer mode: `write through`,
  `fua` 0, no flush or FUA among at least 2058 traced writes. No fake-only result closes this
  rule.
- S5: table row DISCARD → `DiscardHint`, which carries no range, so no engine I/O can come from
  it; the params test has no discard type or limit. Kernel: `discard_max_bytes` 0, the discard
  ioctl refused, the data read back equal.
- S6: store crash campaign retains every acknowledged block; no PLP-based flush-elision path.
  Kernel: an `F` request on a `write back` data disk.
- S7/S8: `scripts/tools/block-contracts-test.sh`: every row of the appendix's
  [fixture table](kernel-check.md#script-tests); only the all-good fixture exits 0. Each
  refusal and guard has a row that fails without it, and no fixture may record a delete, a
  detach, a node start or stop, or a signal.
- Local checks: `make lint test race contracts sim crash-test`; `make miri sanitize` for touched
  unsafe paths, noting Miri does not run kernel io_uring. Run no lab target during drafting.

## Lab

Only with explicit authorization on the shared lab, and with nothing attached on NODE. One run
per buffer mode, about two minutes each; `<run>` is the id the check prints:

```
FORMAT=0 QUEUES=2 make node-up VM=a && make block-contracts NODE=a CONFIRM=yes
VOL=<run> make vol-delete
FORMAT=0 ZERO_COPY=1 QUEUES=2 make node-up VM=a && make block-contracts NODE=a CONFIRM=yes
VOL=<run> make vol-delete
FORMAT=0 make node-up VM=a
```

Pass: two results with `result` `pass` and `queues` 2, one with `mode` `copy` and one with
`zero-copy`. `refused` and `unsupported` are not evidence.

- **What the check changes:** it creates one 1 GiB RF-2 volume, attaches it on NODE and writes
  under 32 MiB to it. The volume stays attached, and NODE refuses any other attach, until the
  operator's `vol-delete`. The check deletes, restarts and formats nothing.
- **What the operator's commands change:** they are existing targets, not part of the check.
  `node-up` restarts NODE's node: other volumes with a replica there lose that member and
  resync it. `vol-delete` detaches and deletes that one id with its replicas, waits for its
  tombstone, and starts a data node it finds stopped, with `FORMAT=0` and the operator's other
  settings (`scripts/lab.sh:1733–1751`).
- **Restore:** the two `vol-delete` lines and the last `node-up`. After an interrupted run, the
  commands the check printed ([by hand](kernel-check.md#procedure)); the next run refuses until
  they are done.

## Docs

Publish the result JSON and the measured cell (kernel, libublk, modes, queues, backing) in
docs/benchmarks. ARCHITECTURE's block claims say that discard is not exported and that the
kernel consumes FLUSH and FUA on the cacheless device. `docs/lab.md` and the Makefile help gain
the target, the two runs, what they change and the restore. Existing #85/#80 defects remain
separately tracked. A model pass is not a statement that hardware power-loss or a newer kernel
has been validated.

## Risks

- **The trace as oracle:** it cannot show how a delivered FLUSH or FUA is handled, since the
  kernel delivers none: that rests on S2 and the table. It relies on the kernel's `rwbs`
  encoding; the write count, the data disk's `F` requests and the fixtures guard the parser. On
  a `write through` data disk that second control is absent, which `backing` records.
- **Unknown flag bits fail closed:** a kernel that sets a new hint bit gets EOPNOTSUPP for that
  I/O until the table grows. Open question: accept that, or keep ignoring unknown bits as today.
  Settled at approval, 2026-11-13: refuse them, as rule 2 states (see decisions.md, Settled at approval).
- **Matrix:** the evidence covers only the measured cell: the lab's kernel, the pinned libublk,
  both modes, two queues, virtual disks. Open question: is evidence on ADR 0003's minimum
  kernel (6.6) and on a physical backing stack required before the issue closes?
  Settled 2026-11-13 (user's instruction): yes, gathered by this kernel check as more cells of
  the matrix (see decisions.md).
- **#83:** as drafted it refuses `flaminio-io node` on the lab VMs. No node runs there then,
  and this check answers `refused`/`nodes-not-alive`. It has to run before #83 lands, or on a
  host #83 admits. Settled 2026-11-13 (user's instruction): on the lab VMs before #83 lands,
  then only on a host #83 admits, with no bypass (see decisions.md).
- **Security:** the lab operator runs the check with the lab administrator's identity. Neither
  script takes a volume, device, size or mode; the run id and the attach token are validated
  before they reach a command or a socket line; the guest's paths are fixed. It needs root in
  the VM for tracefs. The only deletion is the operator's own, of an id printed only if it
  matches the check's pattern.
- **Left by design:** every run leaves its volume attached until the operator deletes it. A
  child the kernel cannot kill, or a `kill -9` of the guest, leaves the guest's directory and
  trace instance. Both make the next run refuse, naming what to run.
- **Scope:** do not fabricate accepted risks or claim a no-op FLUSH bug without evidence.
  FUA/cache changes, discard semantics (advertising it, even as a hint), format changes and
  write-back require further design/model review before implementation.

## ADRs

Consistent with ADR0001/0002/0003 and current fencing/lease core. ADR0003's minimum kernel has
no zero copy: a node whose kernel refuses the zero-copy device gives `unsupported` in that run.
The correction to ADR0005 is Proposed ADR0007 in audit commit 10265b6; do not use its historical
immediate-read shortcut. Proposed ADR0010 in that audit records this contract but is not
accepted. Any new ordering or write-back choice needs a reviewed ADR before shipping. No global
ADR number is allocated here, and this revision drafts none: it adds no architectural choice.

## Parallelizable?

Yes: the Rust unit (`ublk.rs` and its tests, the engine tests) and the shell unit (the check,
its guest script, the script tests, the Makefile and docs) build independently against the
Contracts above. There is no Go unit and no change to core ordering.
