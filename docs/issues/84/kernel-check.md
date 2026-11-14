# Issue #84: the kernel check — appendix

Part of the [design](design.md): what `scripts/checks/block-contracts.sh` and
`scripts/guest/block-contracts.sh` do, and the interfaces their implementers and the script
tests build against. Accepted with the plan, 2026-11-13; nothing here has run.

## Procedure

One run measures one cell: the buffer mode and queue count NODE's node already runs with. The
check changes three things, in this order: it creates one volume, attaches it on NODE, and
writes under 32 MiB to it. It deletes and detaches nothing, starts, stops and formats no node,
signals no process, keeps no state that any run reads, and reads no node setting from its
caller. The operator restores the lab with the command the check prints.

The host script sources `lib.sh` and extracts `metactl` alone from `lab.sh`, as
`scripts/checks/resync.sh:10` does; it calls `metactl`, `run_on_vm` and `vms_of`, nothing else.
Its run id is `bc-<unix seconds>-<8 hex digits from /dev/urandom>`, the run's volume id, checked
against `^bc-[0-9]+-[0-9a-f]{8}$` before each use.

**Before changing anything,** in this order; the first that fails ends the run as `refused`
with the reason in brackets:

1. `CONFIRM=yes` (`confirm`); no argument (`arguments`); NODE is one of `vms_of all` (`node`).
2. `metactl volumes` and `metactl tombstones` answer (`metadata`), and neither lists an id that
   matches the pattern (`leftover`, with that id in `found`). Other ids are not looked at.
3. Both data nodes are alive in `metactl nodes`, asked every 2 s for at most 60 s
   (`nodes-not-alive`; a listing that fails is `metadata`).
4. The guest `state` on NODE answers `none`. `leftover` is `leftover`; a volume id is
   `attached`, with it in `found`; `invalid` is `attached`; no answer is `nodes-not-alive`.

**The run,** each step once, with no retry:

5. `metactl create <run> 1G 2 1 256M <NODE>`. Anything but success is `fail`/`setup`.
6. `metactl attach <run> <NODE>`. A failure, or a token outside `^[A-Za-z0-9+/=]+$`, is
   `fail`/`setup`.
7. `node.sh attach` on NODE with that volume and token. If it fails, the guest `state`
   decides: `none` is `unsupported`/`node-refused`; any other answer, the run's own id and no
   answer included, is `fail`/`guard`, with `found` only for a valid id that is not the run's.
8. The guest `run` on NODE with `RUN=<run>`. Its JSON line and its exit 0, 1 or 3 are the
   result. No JSON line, or any other exit, is `fail`/`interrupted`.
9. From step 5 on, however the script ends (its EXIT trap, which only prints): the result,
   then `restore: VOL=<run> make vol-delete`, except after `AlreadyExists` in step 5, where
   that volume is not the run's.

Whenever the result is `interrupted` (step 8), or the guest `state` answered `leftover` (step
4 or 7), the check also prints the "by hand" text below, in this order: for `interrupted` the
wait, then the two commands that must print nothing, then the two guest paths to remove.

**The guest `state`** changes nothing and prints one word: `leftover` if `/run/flaminio-bc` or
`/sys/kernel/tracing/instances/flaminio-bc` exists; else, from the control socket's `status`,
`none` for `ok none`, the id for `ok <valid id> /dev/ublkb<n>`, `invalid` for any other answer.
It exits 3, printing nothing, when the socket does not answer.

**The guest `run`** requires `RUN` to match the pattern, else exit 2 with nothing done. It
makes `/run/flaminio-bc` with `mkdir`, without `-p` (root, mode 700); if that fails it ends
`fail`/`guard` and removes nothing: the directory and the trace instance are another run's.
Both guest paths are fixed: `RUN` is only compared with the `status` answer.

- **Children:** every command that opens the device is a foreground child, run as
  `sudo timeout -k 10 60 <command>`, so `timeout` can signal it. There is no background job,
  and the script signals nothing.
- **Deadline:** before each of steps 2–7 below, 300 s after its start, it ends `fail`/`guard`.
- **What is bounded:** each command that opens the device, by 70 s unless the kernel cannot
  kill it, and each `status` call, by the socket's 180 s (`scripts/guest/common.sh:26`). The
  run as a whole has no stated bound: a step runs several such commands, and the deadline is
  checked between steps only. It goes on without ssh; its end is the directory going.
- **Trap,** set once the `mkdir` has succeeded, on EXIT, HUP, INT, TERM and PIPE, run once the
  current command has ended: remove the trace instance and the directory. Nothing else. A run
  that is killed, or whose child cannot be killed, leaves both, and the next run is
  `refused`/`leftover`.
- **Identity guards,** before each of steps 3–6: `status` answers exactly `ok <RUN>
  /dev/ublkb<n>`, and that path, the only source of the device, is a block device; `blockdev
  --getsize64` is 1073741824; no holder, no mount. A failure ends the run `fail`/`guard`.
- **Zero guard,** before each of steps 3–5: every range the step will write reads as zeros, as
  a new volume does; else `fail`/`guard`. Step 6 has none: step 3 wrote its range.
- **Writes:** every `dd` that writes the device has `oflag=direct conv=nocreat`, so a path
  that is gone is never created as a file.

The steps, with random data from a file in that directory:
1. Read the mode and queue count from the command line of unit `flaminio-node`'s main process
   (`--zero-copy`, `--queues N`; else `copy`, 1), and the limits and the `mq/` count under
   `/sys/block/<dev>/`.
2. Start a trace: the instance `/sys/kernel/tracing/instances/flaminio-bc`, event
   `block:block_rq_issue`, filtered to the ublk device and NODE's data disk (`data_disk`).
3. Write 8 × 1 MiB at offset 0, then `fsync` (`conv=nocreat,fsync`): the empty flush.
4. Write 2 × 512 KiB from 256 KiB before the first chunk boundary with `O_DSYNC`
   (`oflag=direct,dsync`): FUA writes, the first across two chunks.
5. Write 1024 × 4 KiB with `O_DSYNC` at 512 MiB from CPU 0, then the same at 768 MiB from
   CPU 1 (`taskset`), one after the other.
6. `blkdiscard -o 0 -l 1M`: "Operation not supported" is the pass; success is
   `discard-accepted`; any other error is `guard`.
7. Read every written range back (`iflag=direct`) and compare it with the source.
8. Stop the trace. Count the ublk device's issued requests by their `rwbs` field: writes (`W`),
   flush or FUA (`F`), discards (`D`); and the data disk's `F` requests. Lost events are the
   `overrun` and `dropped events` counts of the instance's `per_cpu/*/stats`.

A run passes only if all hold. Limits: `logical_block_size`, `physical_block_size` and
`minimum_io_size` 4096, `optimal_io_size` 65536, `write_cache` `write through`, `fua` 0,
`discard_max_bytes` 0, `write_zeroes_max_bytes` 0, and as many `mq/` queues as `--queues`.
Trace: no lost event, issued writes ≥ 2058 (one per write call, so the trace saw the device),
flush-or-FUA 0, discards 0. Step 6 refused; step 7 equal. And when the data disk reads `write
back`, at least one `F` request reached it: the store's flush, and the proof that this trace
shows an `F`.

**What the operator runs by hand.** After every run that created its volume:
`VOL=<run> make vol-delete`, the existing target. After `interrupted`, first wait until
`/run/flaminio-bc` is gone on NODE; if it stays, it is a guest `leftover`. After a guest
`leftover`, on NODE, once neither `sudo grep -lsaz '^RUN=bc-' /proc/[0-9]*/environ` (a live
guest script, which runs as `bash /tmp/tmp.<x>`) nor `pgrep -f /dev/ublkb` (one of its
children) prints anything: `sudo rmdir /sys/kernel/tracing/instances/flaminio-bc` and
`sudo rm -r /run/flaminio-bc`. `pgrep -f` can also match an unrelated command line that
contains that path: such a match only makes the operator look at it before going on. After a
host `leftover`: `VOL=<found> make vol-delete`.

## Interfaces

- **Check CLI:** `NODE=a|b CONFIRM=yes scripts/checks/block-contracts.sh` (`NODE` defaults to
  `a`), no arguments: no volume id, device path, size, offset or mode can be passed in. Exit 0
  `pass`, 1 `fail`, 2 `refused` (nothing was changed), 3 `unsupported`. Only 0 is a pass.
- **Guest CLI:** `block-contracts.sh state|run`, as above. `run` reads `RUN` alone from its
  environment and prints one JSON line on stdout (the result's fields from `kernel` on), its
  log on stderr; exit 0, 1 or 3 as the check's, 2 for a bad `RUN`. `SYSFS_ROOT`,
  `TRACEFS_ROOT`, `PROC_ROOT` and `WORK_ROOT` replace those trees only under
  `FLAMINIO_SCRIPT_TEST=1`, as `CONTROL_SOCKET` does.
- **Result:** one JSON object, on stdout and in `$LOCAL_STATE_DIR/block-contracts/<run>.json`,
  a file no run reads:

```json
{"schema": 1, "run": "bc-…", "node": "fl-a", "result": "pass|fail|unsupported|refused",
 "reason": "", "found": "<volume id>", "restore": "VOL=bc-… make vol-delete",
 "kernel": "<uname -r>", "libublk": "<version in dataplane/Cargo.lock>",
 "virt": "<systemd-detect-virt>", "faults": "none", "mode": "copy|zero-copy", "queues": 2,
 "advertised": {"<each limit above>": "<as read>"},
 "issued": {"writes": 2058, "flush_or_fua": 0, "discards": 0, "lost": 0},
 "discard_ioctl": "EOPNOTSUPP|accepted|<other errno>", "readback": "equal|differs",
 "backing": {"device": "/dev/…", "write_cache": "<as read>", "flush_or_fua": 1}}
```

- **Result rules:** `reason` is empty on a pass, else one of: `confirm`, `arguments`, `node`,
  `metadata`, `leftover`, `nodes-not-alive`, `attached` (refused); `setup`, `guard`,
  `interrupted`, `advertisement`, `flush-or-fua-issued`, `discard-issued`, `discard-accepted`,
  `readback`, `trace-incomplete`, `backing-not-flushed` (fail); `no-block-trace`,
  `node-refused` (unsupported). `found` is a volume id only if it matches the id pattern
  (`leftover`: the check's own pattern), else empty. `restore` is empty when the run's volume
  was not created.

## Script tests

`scripts/tools/block-contracts-test.sh`, run by `make lint`. The host script runs against fake
`metactl`, `run_on_vm` and `vms_of` that record every call, as `lab-deletion-test.sh` does. The
guest script runs with fake sysfs, tracefs, proc and work trees and stubs on `PATH` (`sudo`,
`dd`, `blkdiscard`, `blockdev`, `findmnt`, `timeout`, `taskset`), as `node-scripts-test.sh`
does. Both scripts take the time from `date +%s` and wait with `sleep`; the tests stub both,
with a `sleep` that advances the stub `date`, so the timed fixtures take no real time. Only the
all-good fixture exits 0. In every fixture, no call is a `delete`, a `detach`, a `node.sh up`
or `down`, or a signal.

| Fixture | Must give |
|---|---|
| no `CONFIRM`; an argument; NODE not a data VM | `refused` with `confirm`, `arguments`, `node`; no call at all |
| `metactl volumes`, `tombstones` or `nodes` fails | `refused`/`metadata`; no create |
| `volumes` lists `bc-1-0123abcd`; the same id only in `tombstones` | `refused`/`leftover`, `found` that id; no create |
| `volumes` lists `wbench` and `bc-x` | they are ignored; the run goes on |
| a node alive only at the third `nodes` listing; a node never alive | the run goes on; `refused`/`nodes-not-alive` after 60 s, no create |
| guest `state` answers `leftover`; a valid id; `invalid`; nothing (exit 3) | `refused` with `leftover`, the two commands printed before the two paths; `attached` and `found`; `attached`; `nodes-not-alive`; no create |
| the caller's environment has `FORMAT=1 ZERO_COPY=1 QUEUES=4 NO_VERIFY=1` | the calls are those of the all-good fixture |
| create answers `AlreadyExists`; another error; nothing | `fail`/`setup`; `restore` empty for `AlreadyExists` only; no attach |
| `metactl attach` fails; its token has a space or a quote | `fail`/`setup`; no `node.sh attach` |
| `node.sh attach` fails and `state` answers `none` | `unsupported`/`node-refused`; no guest `run` |
| `node.sh attach` fails and `state` answers another id; the run's id; `invalid`; `leftover`; nothing | `fail`/`guard`, `found` for the other id only; for `leftover`, the two commands printed before the two paths; no guest `run` |
| the guest `run` prints no JSON line; exits 255 | `fail`/`interrupted`; the restore line is printed, then "wait until `/run/flaminio-bc` is gone" with no number of seconds, then the two commands, then the two paths |
| the host script gets TERM after the create | the restore line is printed; no further call |
| guest `run` with a `RUN` outside the pattern (`../x`, `bc-1-zz`, one with a space) | exit 2; no directory, trace instance or write |
| guest `state` with a work directory, or with the trace instance, present | `leftover`; nothing removed |
| guest `run` with the work directory present, and the trace instance too | `fail`/`guard`; no write; the directory and the trace instance are left |
| `status` answers `ok <other> /dev/ublkb1`, or a path outside `/dev/ublkb<n>` | `guard`; no write |
| a wrong size; a mount; a holder; non-zero data in a range to write | `guard`; no write from that step on |
| `status` names another volume from step 6 on | `guard`; no `blkdiscard` |
| the device path is absent when step 4 writes | no file is created there; `guard` |
| a step's command ignores TERM | the recorded call is `sudo timeout -k 10 60 <command>`, `timeout` after `sudo`; the run ends `fail` |
| the guest `run` gets TERM during step 5 | no later step starts; the instance and the directory are gone; nothing was signalled |
| 300 s have passed before step 4 | `guard`; no write from step 4 on |
| `write back`; `fua` 1; a discard limit; an `mq/` count other than `--queues` | `advertisement` |
| trace lines `FF`, `FWS`, `WFS` for the ublk device | `flush-or-fua-issued` |
| a trace line `DS`; `blkdiscard` succeeding; `blkdiscard` failing otherwise | `discard-issued`; `discard-accepted`; `guard` |
| 2057 writes traced; a lost event | `trace-incomplete` |
| no `F` on a `write back` data disk | `backing-not-flushed` |
| no `block_rq_issue` event | `unsupported`/`no-block-trace` |
