# Issue #74, step 1: the privileged helper and the unprivileged I/O process — design

[Spec](spec.md). [Decisions](decisions.md). Step 1 of #74 ([intent](intent.md)). Approved
2026-11-13; not implemented. [ADR proposal](adr-proposal.md).

**The facts it starts from:**
- **The node is one root process.** `flaminio-io node` opens its disk (`node.rs:403–404`;
  `flaminio-uring/src/uring.rs:123–154`, the store's two rings at 138–143), adds a ublk device per
  attach (`add_device`, `ublk.rs:902–934`) and runs it with libublk's `run_target`
  (`ublk.rs:1050–1052`; START_DEV with its own pid, libublk 0.4.8 `ctrl.rs:2637–2651`). Detach
  (`control.rs:202`) and deposition (`ublk.rs:626`) stop it; the `UblkCtrl` drop deletes it
  (`ctrl.rs:1085–1096`). It binds the control socket, root only, 0600 (`control.rs:46–52`).
- **Other direct opens:** `serve`/`del` (`ublk.rs:129–131`, `:141`, `:168–176`), `cli.rs:99`,
  `flaminio-bench/src/main.rs:167`, `flaminio-disktool/src/main.rs:162`; the lab unit (`node.sh:45–65`).
- **libublk** opens `/dev/ublkcN` only through a `UblkCtrl` (`io.rs:889–927`, private) and sends
  START_DEV only with a local `UblkDev`; `set_params` (`ctrl.rs:2532`), `disown` (`:2790`) are public.
- **The kernel** (6.8.12; decisions.md lists each claim read): an ordinary device's control
  commands need `CAP_SYS_ADMIN` (`ublk_drv.c:2852–2854`) and run in io-wq (`:2925`); mmap and
  data commands on `/dev/ublkcN` check none (`:1376–1390`, `:1746–1769`); an unprivileged server
  only loses partition scans (`:1693–1694`, `:2288–2290`). `UBLK_F_UNPRIVILEGED_DEV` clears
  recovery (`:2391–2406`); `SCM_RIGHTS` refuses io_uring descriptors (`net/core/scm.c:107–111`).
- **The CSI node plugin** runs `blkid`, `mkfs`, `mount` as root on the device string the control
  socket answers (`control/csi/node.go:58–78`, `:107–126`, `:180–200`), read with no length limit
  (`dataplane.go:75`). No other Go component reads the data plane's answers.

## The helper (S1, S2, S5)

- **Start, in order.** Steps 1–7 run on the only thread (capabilities are per thread). A failure
  prints `flaminio-helper: <reason>` and exits 1 before any child exists.
  1. Parse (Contracts); refuse unless the effective UID is 0 (`must start as root`).
  2. Open `--device` `O_RDWR|O_EXCL|O_CLOEXEC`, with `O_DIRECT` unless `--buffered`; refuse a
     non-block file (`PATH: not a block device`).
  3. Bind `--control` as `control.rs:46–52` does: create the parent, remove the path, bind, 0600.
  4. `socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC)`.
  5. `no_new_privs`; clear the ambient set; empty the bounding set (`CAP_SETPCAP` last, up to
     `cap_last_cap`); effective = permitted = {`SYS_ADMIN`, `SETUID`, `SETGID`}; inheritable empty.
  6. Spawn `PROGRAM ARG...` with `std::process::Command`: `.uid()`, `.gid()`, `.env_clear()`, stdin
     = the child's socket end. In the child std calls `setgid`, then `setgroups(0)`, which works
     because the helper holds `CAP_SETGID` (std ignores its `EPERM`), then `setuid`; the kernel
     clears permitted and effective when every UID becomes non-zero (std
     `sys/process/unix/unix.rs:307–330`). Keep the child's pid (`Child::id`) and open its pidfd.
  7. Close the child's end; effective = permitted = {`SYS_ADMIN`}.
  8. Start the server thread (it inherits {`SYS_ADMIN`}), which answers requests (next section).
     The main thread waits on the pidfd; when the child exits, it runs Owned devices' outcome 4 and
     exits with the child's code, or 128+N for signal N, whatever the server thread is doing.
- **Retained:** UID 0 (owner of `/dev/ublk-control`, `/dev/ublkcN`, the disk node and `/run`) and
  `CAP_SYS_ADMIN`. No `CAP_DAC_OVERRIDE`, network, metadata client or token check.
- **Who may call:** only the child, or a process it hands its stdin to, with the child's authority
  and only while the child lives (outcome 4). The socket has no name; the child's end is its stdin.
  `SO_PEERCRED` would name the helper itself (`net/unix/af_unix.c:1649–1650`), so it is not used.

## Requests (S3, S4)

- **`disk`, `control`:** the reply carries the descriptor, then the helper closes its copy; a second
  request gets `error already sent`. The exclusive claim belongs to the open file description
  (`block/fops.c:600–631`): it ends when the I/O process closes it.
- **`add`:** within the limit and pace of Owned devices, `add_device`'s libublk builder (name
  `flaminio`, the helper's `--id`, depth, queues, buffer, `ctrl_flags` 0), patched so that a failed ADD_DEV sends
  no DEL_DEV (Contracts), then `disown()`; `set_params` with
  `node_params(size, max_io_buf_bytes)` (the values of `ublk.rs:949–963`, moved to the helper's
  library and shared with direct mode); open `/dev/ublkc<DEV>` `O_RDWR|O_CLOEXEC`, retrying 3 s for
  udev (as `io.rs:910–927`). The reply carries that descriptor; the helper keeps it as the pin.
- **`start`:** at most once per owned device (else `error already started DEV`): START_DEV with the
  pid kept at step 6 (`ublk_drv.c:2252–2281`). 6.8 would restart a stopped device (`:2265–2269`).
  It waits until every queue fetched, so the I/O process asks after its queues registered, where
  libublk's `start_dev` does.
- **`stop`:** STOP_DEV. **`delete`:** as Owned devices states.
- **Refusals:** an id not owned: `error not owned DEV`, no kernel call. A kernel error:
  `error kernel OP: ERRNO` (Owned devices, Errors); `start` and `stop` keep the device owned.
- **Reading:** `recvmsg` with `MSG_CMSG_CLOEXEC`, a 129-byte buffer and room for ancillary data. Any
  ancillary data, `MSG_TRUNC` or `MSG_CTRUNC`: close what arrived, `error invalid request`. A
  zero-length read: the child closed its end; the server thread stops reading. The server loop
  takes its receive step as a parameter: production passes this `recvmsg` with its ancillary-data,
  truncation and zero-length checks; `request_read_after_closing_dropped` passes a closure, called
  without the table lock held, as production's receive is.

## Owned devices (S3, S5)

The one statement of ownership and cleanup; the spec and the other sections defer to it.
- **One id:** the helper adds devices only at the id its own command line names (`--id ID`); the
  child names none. So it owns at most one device, and the only device it can ever have created
  is `/dev/ublkc<ID>`, which `node.sh` deletes (outcome 5).
- **Ownership:** the device is owned while the helper holds its descriptor of `/dev/ublkc<ID>` (the
  pin). An open character device holds the device (`fs/char_dev.c:348–355`, `:496`), and its id is
  freed only when the device is released (`ublk_drv.c:2150–2164`). The table (pin, libublk handle,
  `closing`) is shared by the two threads under a lock never held across a kernel call.
- **The libublk handle:** disowned at `add`, so its drop sends nothing (`ctrl.rs:1085–1096`). The pin
  is closed only right before the helper's own DEL_DEV, which goes through a fresh
  `UblkCtrl::new_simple` handle; in that moment only a trusted root process could free and reuse
  the id. A failed ADD_DEV sends no DEL_DEV either (the patch; without it libublk's drop deletes
  whatever device holds the requested id, `ctrl.rs:1134`, `:1258`, `:1626–1634`, `:1459`).
- **Errors:** libublk retries a failed control command with its legacy opcode (`ctrl.rs:1500–1520`,
  `:1571–1582`), which kernels built without `CONFIG_BLKDEV_UBLK_LEGACY_OPCODES` refuse
  (`ublk_drv.c:1712–1714`), Ubuntu's among them: there a reply's ERRNO is `EOPNOTSUPP` whatever the
  first failure was. Tests check what failed and what survived, never the ERRNO.
- **Limit and pace:** an `add` while the device is owned gets `error device limit`; at most one
  `add` per 250 ms, the helper waiting out the rest; one `start` per owned device (Requests).
- **Outcomes**, each with its test (`serve.rs` on a fake `Kernel`, or `tests/kernel.rs`):
  1. **`add` fails.** At ADD_DEV (the id is taken, `ublk_drv.c:2134–2137`): no device is deleted.
     After it (`set_params` or the open): close what was opened and DEL_DEV the device this add
     created, and only it; if that DEL_DEV fails too, the reply ends `; device ID left`.
     `failed_add_sends_no_delete`, `failed_add_deletes_its_device`, `failed_cleanup_leaves_it`;
     kernel `foreign_device_survives_add_of_its_id` (a device the test adds as root at the
     helper's id survives the child's `add`, which fails).
  2. **The attach fails after `add`** (`from_cdev`, a queue thread, `start`): the I/O process sends
     `stop` (STOP_DEV cancels every fetch even before a start, `ublk_drv.c:1663–1680`, `:1497–1512`,
     so its queue threads end), joins them, drops the `UblkDev`, sends `delete`, and reports the
     first error. `privsep.rs`: `failed_attach_stops_then_deletes`.
  3. **`delete`:** the helper removes the entry, closes the pin, then DEL_DEV. Afterwards the device
     is not owned, whatever DEL_DEV answered. DEL_DEV waits for the last reference (`:2553–2568`):
     a held device (mounted, or built into dm or md by udev) holds it with no bound, as today's
     drop does, and a detach's `stop` waits behind it. `delete_forgets_the_id`,
     `delete_closes_pin_before_del_dev`.
  4. **The child exits** (any cause, also while the server thread waits in START_DEV or DEL_DEV): the
     main thread (a) sets `closing` under the table lock, then shuts the helper's socket end down;
     the server thread checks `closing` under the table lock before reading each request and again
     after each `recvmsg` returns, and stops, dropping a request read then unanswered, so requests queued
     before the shutdown (a seqpacket socket still delivers them, `net/core/datagram.c:87–106`, `:241`)
     and a process that kept the child's stdin (it has no `CLOEXEC`) get nothing served; an `add`
     that finishes after `closing` is set deletes its own device instead of recording it; (b)
     waits up to 4 s, longer than an add's 3 s open retry, for the server thread to stop; (c)
     closes the pin and sends DEL_DEV from a cleanup thread; (d) exits after at most 5 s more.
     DEL_DEV removes the device and returns its `ublks_max` slot before it waits (`:2543–2546`,
     `:2226–2233`), so an exit that cuts its wait short leaves no live device. A server thread still
     inside an `add` after (b) may leave that device, which `node.sh` deletes (outcome 5). The worst
     case is (b) + (d), 9 s. `child_exit_deletes_owned`, `add_in_flight_at_exit_deleted` (a slow
     open), `queued_requests_after_child_exit_ignored` (the server loop paused on a latch; the
     child queues six `add`s and exits; the latch is released: no device remains, no kernel call
     after the exit), `request_read_after_closing_dropped` (the server loop's receive is an
     injectable step; the injected receive sets `closing`, then returns a valid `add`: no kernel
     call and no reply, which only the check after `recvmsg` catches); kernel `exits_with_its_child` (exit
     7: 7), `exits_with_its_child_while_blocked` (the child sends `start` with no queue fetching and
     is killed: the helper exits 137 within 15 s, `ublkc<ID>` gone), `requests_after_child_exit_refused`
     (a grandchild that kept stdin gets `EPIPE` and adds nothing), `held_device_does_not_block_cleanup`
     (the test holds `/dev/ublkb<ID>` open: `ublkc<ID>` gone and the helper exited within 15 s).
  5. **The helper dies or the unit stops:** stopping the unit signals both processes at once
     (systemd's default `KillMode=control-group`), so no cleanup runs; the I/O process exits (the
     watcher, below); a started device is stopped by the kernel when its server dies
     (`:1540–1550`). The only device left can be `/dev/ublkc<ID>`, and `node.sh down` deletes it
     after stopping the unit; `node.sh up` runs `down` first (lab node). A new helper owns
     nothing until its own `add`, which fails while that device remains (outcome 1).
     `node-scripts-test.sh`: `down` deletes `/dev/ublkc<ID>` after the stop;
     `flaminio-io/tests/privsep.rs`: `exits_when_the_helper_goes`; `foreign_device_refused` (start,
     stop and delete of a root-added device at another id are refused).
  6. **An id reused:** impossible while owned (the pin); once forgotten, never acted on. A root `del`
     of the owned device while the unit runs is unsupported: it waits for the pin, and a first
     `start` would build a disk on the removed device. `node.sh down` stops the unit first.
     `pinned_id_not_reused` (a root `del` of the owned id waits, and the test's `add` at that id
     fails, until the child's `delete`).

## The I/O process under `--privsep` (S2, S3, S7)

- **Parse:** refuses `--device`, `--control`, `--buffered`, `--zero-copy`, `--id` (`--X is the
  helper's under --privsep`) and no longer requires `--device` (`node.rs:210`); stdin must be an `AF_UNIX`
  `SOCK_SEQPACKET` socket (`--privsep: stdin is not the helper socket`). Exit 1, as today. Messages
  that named the path (`format_if_asked`, `node.rs:255`; `Shared.device`, `ublk.rs:1002`, `:1549`)
  say `helper disk`. It opens no disk or ublk path; `/dev/urandom` for `--format` stays (`cli.rs:52`).
  `--telemetry` (#71) with `--privsep` is refused at parse (`--telemetry needs direct mode;
  telemetry under --privsep is a later step of #74`): whichever of #71 and #74 lands second adds
  that check and its test beside the other `--privsep` checks.
- **Disk:** `disk`, then `UringDevice::from_file(File)`: `direct` from the file's `O_DIRECT` flag,
  size by seeking to the end, both rings as `open` builds them (`open` = open + `from_file`).
- **Control:** `control`, sent right after `disk`, before the store opens; `control::spawn` later
  takes that bound `UnixListener` (direct mode binds as today).
- **The client** (`privsep.rs`, one `Arc<Helper>` for `run_device`, `control::spawn` and the
  deposition thread): one mutex held from request to reply. A `start` reply not received within
  `START_LIMIT` (`ublk.rs:703`) is fatal: the process exits 1, and outcome 4 cleans up, so a `stop`
  queued behind a stuck `start` waits at most that long. A watcher thread, started right after
  parsing, runs a hook when stdin reports `POLLRDHUP` (seqpacket reports the peer's close,
  `af_unix.c:3201–3204`, `:873`); `main` passes a hook that exits 1 with `flaminio-io: the helper
  is gone`, as EOF in any call does. A wrong descriptor count, `MSG_CTRUNC` or a line outside the
  grammar fails the call; received descriptors are closed.
- **Each attach** (`run_device`): `add QUEUES DEPTH IO_BUF SIZE`, retried on `error kernel add:` every 250 ms until
  `add_device`'s deadline; `UblkDev::from_cdev` with the reply's values; queue threads as libublk's
  `create_queue_handlers`, without a `UblkCtrl`; `start` once `wait_for_buffer_registration`
  returns; only then, as `cpu_list` exists once the disk does, each queue thread's affinity from
  `/sys/block/ublkb<DEV>/mq/<q>/cpu_list` (the map `GET_QUEUE_AFFINITY` reads,
  `ublk_drv.c:2345–2346`; threads fetch unpinned until then; unreadable: logged, unpinned); the
  reply `ok /dev/ublkb<DEV>` as today; after the queue threads end, drop the `UblkDev`, `delete`.
  Errors: outcome 2.
- **Detach, deposition:** `stop` replaces `kill_dev()` at `control.rs:202` and `ublk.rs:626`.
- **Rings:** the I/O process creates all its rings; a `--defer-taskrun` ring (`ublk.rs:1077–1091`)
  is enabled by its submitting thread. Needs `kernel.io_uring_disabled` 0 (ADR 0003), or 1 with
  `kernel.io_uring_group` = the I/O GID (`io_uring/io_uring.c:4113–4129`).

## The CSI node plugin (S6)

- **`control/csi/device.go`, `checkUblkDevice(dev)`:** `dev` matches
  `^/dev/ublkb(0|[1-9][0-9]{0,6})$`; `Lstat(dev)` is a block device; `/sys/class/block/ublkb<N>/dev`
  holds its rdev's `MAJ:MIN`. Only ublk names disks `ublkb%d` (`ublk_drv.c:2277`).
- **Where:** `Node.CheckDevice func(string) error` (nil: `checkUblkDevice`), called in `device()`
  before both returns (`node.go:65`, `:78`) and on the Status answer in `NodePublishVolume`
  (`node.go:180–187`). Failure: `codes.Internal`, `data plane answered %q: <reason>`.
- **The answer:** `ControlSocket.command` reads at most 4096 bytes through `io.LimitReader`
  (`dataplane.go:75`); no newline within them, or a byte outside printable ASCII, fails the call
  with `data plane: answer too long` or `data plane: answer not printable`.

## The lab node (S7, S8)

- **`node.sh up`:** `PRIVSEP` defaults to 1, or 0 with `ZERO_COPY=1`; both 1 dies `ZERO_COPY=1 needs
  PRIVSEP=0: the helper refuses zero copy`. With 1, `ATTACH` dies `ATTACH needs PRIVSEP=0; attach
  with make vol-attach` (no lab target sets it), and #71's `TELEMETRY=1` dies `TELEMETRY=1 needs
  PRIVSEP=0` (added by whichever of #71 and #74 lands second). With 1: the system user
  `flaminio-io` (created if missing); `flaminio-helper` and `flaminio-io` installed root-owned 0755
  in `/usr/local/lib/flaminio/` (that user cannot traverse the build user's home);
  `/etc/flaminio/pki` 0750 and `io.key` 0640, group `flaminio-io`; the unit runs `flaminio-helper
  --uid U --gid G --device DEV --id 1 -- /usr/local/lib/flaminio/flaminio-io node --privsep <other
  args, without --id>`.
- **`node.sh up` runs `down` first**, always (today only when the unit is active, `node.sh:44`).
  **`down`**, in both modes: the mount check as today, stop the unit, then `del` a remaining
  `/dev/ublkc$id` (today `del` runs first, while the node runs, `node.sh:27–29`).
- **`ublk.sh up`** (`scripts/guest/ublk.sh:13–33`), when its unit is inactive: dies if `$mnt` is
  mounted, else deletes a leftover `/dev/ublkc0` before starting. Today a crashed `serve` left
  device 0, and the next failed `add` deleted it through libublk's drop; with the patch it no
  longer does, so without this step `make ublk-up` would fail until `make ublk-down`.
- **`pki.sh:10`:** the directory 0750, not 0700, so that group holds. **`lab.sh:679`:** `node_env`
  passes `PRIVSEP` when set. **`lab.sh:1503–1513`:** `--privsep` joins the compared flags, first in
  `want` unless zero copy, so a node in the other mode is restarted. **`confirm.sh:71–86`**
  (`data_plane`): one `flaminio-io`, the unit's MainPID or its child. **`bench-reads.sh:99`:** replace
  `pgrep -f 'flaminio-io node' | head -1`, which finds the helper first, with `pgrep -x flaminio-io`.

## Files touched

- `dataplane/crates/flaminio-io/tests/privsep.rs` (new): outcome 5.
- `dataplane/crates/flaminio-helper/` (new): `src/lib.rs` (protocol, `node_params`), `src/main.rs`,
  `src/serve.rs` (requests, Owned devices, over a `Kernel` trait), `src/kernel.rs`, `tests/kernel.rs`: S1–S5.
- `dataplane/Cargo.toml` (the member; `rustix` 1.1.5 direct, already locked; `[patch.crates-io]
  libublk = { path = "vendor/libublk" }`); `dataplane/vendor/libublk/` (0.4.8 plus Contracts);
  `flaminio-uring/src/uring.rs` (`from_file`); `flaminio-io/src/{node,ublk,control,cli}.rs`,
  `privsep.rs` (new client): S2, S3, S5, S7.
- `dataplane/fuzz/`: target `helper_request`, its harness in `src/lib.rs`, `regressions/helper_request/`;
  it uses `flaminio-helper` with `default-features = false`, whose default feature `ublk` alone
  brings libublk and `node_params`: S4.
- `control/csi/{device.go,device_test.go}` (new), `node.go`, `dataplane.go`, `dataplane_test.go`,
  `csi_test.go`: S6.
- `scripts/guest/{node,pki,confirm,bench-reads}.sh`, `scripts/lab.sh`, `scripts/checks/privsep.sh`
  (new), `scripts/tools/node-scripts-test.sh`, `Makefile`: S7, S8.
- `docs/ARCHITECTURE.md`, `docs/lab.md`, `docs/followups.md`, `docs/performance-plan.md`, `AGENTS.md`: Docs.

## Contracts

- **Helper:** `flaminio-helper --uid UID --gid GID --device PATH --id ID [--buffered] [--control
  PATH] -- PROGRAM [ARG...]`; UID, GID 1–4294967294; ID 0–1048575 (`ublk_drv.c:503`); PROGRAM
  absolute; `--control` default
  `/run/flaminio/node.sock`. Exit 1 for its own errors, else the child's (Owned devices, 4).
- **I/O process:** `flaminio-io node --privsep [node flags]`; stdin is the helper socket.
- **Protocol:** one `SOCK_SEQPACKET` message per request and reply, ASCII, fields joined by one
  space; requests at most 128 bytes with no descriptor, replies at most 256 bytes; the server
  thread answers one request at a time, in order.

  | Request | Reply |
  |---|---|
  | `disk` / `control` | `ok` + the disk / the listening control socket |
  | `add QUEUES DEPTH IO_BUF SIZE` | `ok DEV QUEUES DEPTH IO_BUF FLAGS` + `/dev/ublkc<DEV>` |
  | `start DEV`, `stop DEV`, `delete DEV` | `ok` |
  | any | `error TEXT`, no descriptor |

  Numbers: decimal, no sign or leading zero. QUEUES 1–64
  (`node.rs:203`); DEPTH 1–4096; IO_BUF a multiple of 4096, at most 1048576 (`node.rs:222`); SIZE a
  multiple of 4096 from 4096 to 9223372036854771712 (2^63 − 4096: 6.8 does not check
  `dev_sectors`, `ublk_drv.c:566–607`, and stores `(loff_t)sectors << 9`, `block/bdev.c:427`); DEV
  at most 1048575. The `add` reply holds the kernel's `dev_id` (the helper's ID),
  `nr_hw_queues`, `queue_depth`, `max_io_buf_bytes`, `flags` (u64), the fields libublk's queues
  read. TEXT: `invalid request`, `already sent`, `device limit`, `not owned DEV`, `already started
  DEV`, or `kernel OP:
  ERRNO` (OP `add`, `set-params`, `open`, `start`, `stop`, `delete`; ERRNO a name such as
  `EOPNOTSUPP`, Owned devices, Errors), with `; device ID left` as in Owned devices, 1.
- **libublk patch:** `UblkDev::from_cdev(tgt_name: String, dev_info: sys::ublksrv_ctrl_dev_info, cdev:
  std::fs::File, dev_flags: UblkFlags, ops: F) -> Result<UblkDev, UblkError>` (`new_with_info`
  without its open loop, one thread per queue); `UblkCtrl::start_dev_for(&self, pid: i32) ->
  Result<i32, UblkError>` (read the device info, START_DEV with `pid`; no parameters, JSON or wait);
  and in `UblkCtrlInner::new` and `new_async`, when the device lifecycle step fails, `mark_deleted()`
  before returning, so the drop sends no DEL_DEV for a device it did not add (`ctrl.rs:1258`,
  `:1301`, `:1626–1634`). The patch applies to direct mode too: its fixed-id `add` no longer
  deletes a leftover, which `node.sh up` now deletes first (lab node).
- **CSI:** `Node.CheckDevice`, the 4096-byte answer and their errors. **Lab:** `PRIVSEP`;
  `make privsep-check` (VM, default a).
- No proto, metadata RPC, `auth.go`, wire frame or on-disk change.

## Tests

| Rule | Test (file: case) | Level |
|---|---|---|
| S1 | `flaminio-helper/src/main.rs`: `refuses_uid_or_gid_zero`, `refuses_unless_root` (given the euid), `needs_a_program`, `refuses_a_missing_or_bad_id` (no `--id`, `-1`, `1048576`: exit 1 at parse, never a kernel-picked id; `0` and `1048575` accepted); `tests/kernel.rs`: `credentials_after_start` (helper `CapEff`=`CapPrm`=`0x200000`, `CapInh`/`CapBnd`/`CapAmb` 0, `NoNewPrivs` 1; child: all UIDs and GIDs the test's, no groups, every `Cap*` 0, `NoNewPrivs` 1) | unit; kernel |
| S2 | `tests/kernel.rs`: `child_cannot_open_paths` (the disk path, `/dev/ublk-control`, an added `/dev/ublkc<DEV>`), `refuses_a_non_block_device`; `flaminio-io/src/node.rs`: `privsep_refuses_helper_flags`, `privsep_parses_without_device`, `privsep_needs_the_helper_socket`; `privsep.rs`: `disk_and_control_arrive_once`; `flaminio-uring`: `from_file_matches_open`; `flaminio-io/src/cli.rs`: `format_through_from_file` (`--format --force` on a temp file) | unit; kernel |
| S3 | `serve.rs`, fake `Kernel`: `only_owned_devices`, `device_limit`, `add_uses_the_configured_id`, `add_is_paced`, `fixed_flags`, `start_uses_the_child_pid`, `second_start_refused`, `already_sent`; `lib.rs`: `bounds` (SIZE 2^63 refused, 2^63 − 4096 accepted); `tests/kernel.rs`: `owned_device_lifecycle` (add, `from_cdev`, one queue, start: `/sys/class/block/ublkb<DEV>` appears; stop; delete: `ublkc<DEV>` gone), `partition_scan_suppressed` (`blockdev --rereadpt /dev/ublkb<DEV>` fails: `include/linux/blkdev.h:244–248`, `block/genhd.c:343–349`) | unit; kernel |
| S4 | `lib.rs`: `grammar` (signs, leading zeros, double spaces, unknown ops, 129 bytes), `round_trip`; `serve.rs` on a socketpair: `descriptors_in_a_request_are_closed` (a pipe's write end: the read end sees EOF; no state change), `truncated_request_refused`; `privsep.rs`: `unexpected_descriptors_fail_the_call`; fuzz `helper_request` (no panic; what parses re-formats to the same bytes) | unit; fuzz |
| S5 | The outcome tests of Owned devices (outcome 1's also prove S3); `privsep.rs`: `calls_from_two_threads_stay_paired`, `start_without_reply_times_out` (a short limit), `helper_hangup_is_seen` (the hook runs when the other end closes, no call in flight); `flaminio-io/tests/privsep.rs`: `exits_when_the_helper_goes` (the test answers `disk` with a sparse temporary file and `control` with a bound listener, and runs with `--format --force --pki DIR` whose `token.pub` is a FIFO no one writes, so the node then blocks outside any helper call; the test closes its end: exit 1 with `the helper is gone` within 2 s, which only the watcher can produce) | unit; kernel |
| S6 | `device_test.go`: `TestUblkPath` (refuses `/dev/sdb`, `-o`, `/dev/ublkb01`, `/dev/ublkb1/../sda`, a trailing space, 8 digits), `TestRdevMatchesSysfs`; `dataplane_test.go`: `TestAnswerTooLong`, `TestAnswerNotPrintable`; `csi_test.go`: `TestStageRefusesAForeignDevice`, `TestPublishBlockRefusesAForeignDevice` (no `FSType`, `Format` or `Mount` call); `TestNode` answers `/dev/ublkb1`, checked by the path rule alone | unit |
| S7 | `privsep_refuses_helper_flags` (`--zero-copy`, `--id` too; `--telemetry` once #71 has landed); `node-scripts-test.sh`: `up` runs `down` first and the helper by default with `--id 1`, `--privsep` right after `node`, `ublk.sh up` deletes a leftover `/dev/ublkc0` when its unit is inactive, direct with `ZERO_COPY=1`, dies with both, with `ATTACH` or with `TELEMETRY=1`; `down` stops the unit before `del`; `confirm.sh` accepts the helper's child, refuses another parent | unit; script |
| S8 | Lab, below | lab |

`make test`, `make lint` (script tests, clippy), `make race` (CSI), `make hygiene` (cargo-deny on
`rustix` and the vendored libublk), `make contracts` (fuzz lockfile). No new `unsafe` (`rustix`, std),
so `miri` and `sanitize` gain nothing. The kernel tests are `#[ignore]` (root, `ublk_drv`).

## Lab

- **Run:** `make deploy`, `make node-up` (under the helper), `make privsep-check`, `make vol-create`,
  `make csi-deploy`, `make csi-test` (provision, attach, I/O), `make node-fault` (restart),
  `make data-node-up` once and `make drain-test` (drain); then `PRIVSEP=0 make node-up` and
  `make bench-reads`, whose log shows the node restarted in verify mode. Pass: each exits 0.
- **`make privsep-check` (fl-a):** a 1 GiB sparse file in `/var/tmp` on a loop device; the kernel
  tests, built as the user, run as root with `--ignored`, the test child as UID and GID 65534.
  Each test gives its helper its own `--id`, 100 plus the test's index, never the lab's 0 or 1,
  and first checks that `/sys/class/ublk-char/ublkc<ID>` is absent (else it fails, naming the id).
  Then, with the node running (else: `make node-up first`, exit 1): the unit's MainPID is
  `flaminio-helper` with `CapEff` `0000000000200000`; its one child `flaminio-io` runs as
  `flaminio-io`, `CapEff` 0, `NoNewPrivs` 1.
- **Restore:** `privsep-check` removes, on any exit, only the ublk devices that appeared during its
  run (`/sys/class/ublk-char` before and after; `flaminio-io del`), its loop device and file; it
  touches neither the data disk nor `/dev/ublkb1`. The other targets clean up as today
  (`docs/lab.md`); none reformats unless `FORMAT=1`. `PRIVSEP=0 make node-up` returns to direct mode.

## Docs

- **`docs/ARCHITECTURE.md` §5.1** (lines 229–232): the helper keeps `/dev/ublk-control` and passes
  the disk, the control socket and each `/dev/ublkcN`; the I/O process is its child. **§15.5:** what
  stays privileged, and Q1. **`docs/lab.md`:** `PRIVSEP`, the `flaminio-io` user, the PKI modes,
  `make privsep-check`, no partition nodes for raw block volumes, `node.sh` and `ublk.sh` deleting a
  leftover device before they start. **`docs/followups.md`:** Q1–Q4, zero
  copy under the helper, the disk ioctls for step 3, the `meta` URI SAN check, the telemetry request
  as a later step, intent's #12 and #48 dependencies with the pod's requirements (Risks), and in
  item 5 (more than one volume per node, #15) a step giving the helper one owned id per volume. **`docs/performance-plan.md`:** zero-copy runs use direct mode; no new
  numbers. **`README.md`:** none.

## Risks

- **Safety:** no replication, epoch, fencing or durability change; the store gets the same exclusive
  `O_DIRECT` descriptor; the write path gains no step. A helper crash ends the I/O process.
- **Who must not:** see the invariant; S1–S4 test it. The I/O UID is dedicated: after exec the
  process is dumpable again.
- **The `io` key:** the unprivileged I/O process holds `io.key`, whose certificate has the server
  and client usages and the node's IP. The metadata clients of the data plane and the Go
  components check only the CA and the address (`pkg/pki/pki.go:300–310`;
  `flaminio-metaclient/src/lib.rs:85–92`), so that key can answer as the metadata service at the
  node's address. #71's Prometheus metadata job checks the `meta` DNS SAN `meta.flaminio.invalid`,
  so it is not affected. The root process holds the key today, so this is not new, but it lessens
  what this step buys. Follow-up (`docs/followups.md`): metadata clients check the `meta` URI SAN.
- **The disk descriptor:** writable, it also allows SCSI write-class commands through `SG_IO`
  (`drivers/scsi/scsi_ioctl.c:251–341`), persistent reservations (`block/ioctl.c:262–275`) and NVMe
  passthrough, whose effects can outlive the process or reach other hosts on a shared LUN. The
  root process can do the same today; denying those ioctls is step 3's.
- **One device per helper:** the helper owns one device because the node attaches one volume at a
  time (`Attached` is an `Option`, `ublk.rs:682`). Several volumes per node (`docs/followups.md`
  item 5, #15) need one owned id per volume, which changes this rule and the ADR.
- **Retained, unchanged:** the helper's UID 0 and `CAP_SYS_ADMIN`; the privileged CSI node plugin; the
  root agent; whoever writes the node's unit, or #12's pod spec through #43's operator.
- **Spec Q1–Q4** (Q2's vendoring settled at approval, 2026-11-13, see decisions.md, Settled at
  approval; Q1, Q3, Q4 and Q2's maintainer: Settled 2026-11-13 (user's instruction): no
  capability kept and the risk accepted; no other ublk server on a data node; today's residual,
  for a later plan; this project maintains the copy (see decisions.md)): `PR_SET_IO_FLUSHER` needs `CAP_SYS_RESOURCE` (`kernel/sys.c:2706–2708`;
  libublk ignores the failure, `ctrl.rs:2874–2884`); another ublk server's disk passes the CSI
  check; root parsers and udev rules read and act on untrusted contents; the libublk copy needs an
  owner. Until step 3 the I/O process may use any io_uring operation its UID allows.
- **Behaviour changes:** zero copy is refused under the helper, as a zero-copy server can leak
  uninitialized kernel memory (libublk-rs-sys 0.1.2 `ublk_cmd.h:223–224`; 6.8 has none,
  `ublk_drv.c:2483`). The kernel scans no partitions for an unprivileged server (`:2288–2290`): a
  raw block volume's partition table makes no `ublkb<N>p1` on the host; tenants see the whole device.
- **Availability:** while START_DEV or DEL_DEV waits, the server thread answers nothing else. The
  `start` limit and the child's exit (Owned devices, 4) bound a START_DEV; a `delete` of a held
  device is unbounded, as today's drop is (outcome 3).
- **Compatibility:** the control socket's path, owner, mode and lines are unchanged. The lab's newer
  kernel is shown by `privsep-check`. From memory, newer kernels check START_DEV's pid against the
  fetching process; the helper sends that process's pid.
- **Dependencies:** intent's "Which step" states #12 and #48. #12's pod needs `CAP_SYS_ADMIN`,
  `CAP_SETUID`, `CAP_SETGID`, `CAP_SETPCAP`, `/dev/ublk-control`, the disk, new `/dev/ublkc*` nodes,
  and an init (the helper handles no SIGTERM as PID 1), and its start must delete a leftover
  `/dev/ublkc<ID>` as `node.sh up` does (outcome 5). #16's renewal keeps `io.key` 0640.
  **#71's telemetry socket:** as #71's contracts state, the helper request that would bind it is a
  later step of #74, in neither plan's present step; until then telemetry needs direct mode (the
  parse refusal and `node.sh` rule above), and by #71's rules a node under the helper shows as
  `stale` and raises `FlaminioTelemetryLost`.

## ADRs

- **0001** consistent (Rust). **0003** consistent: devices stay ordinary, so `UBLK_F_USER_RECOVERY`
  stays available to #48; zero copy stays an opt-in of direct mode; its io_uring sysctl holds; its
  seccomp profile is step 3. **0002, 0004–0006:** not touched.
- **New, accepted with the plan, 2026-11-13:** [adr-proposal.md](adr-proposal.md): the unprivileged
  ublk server is the child of a root helper that keeps ublk control; the vendored libublk patch;
  `rustix` as a direct dependency.

## Parallelizable?

Yes. The helper, the I/O client and the libublk patch build against Contracts; the protocol module
(`flaminio-helper/src/lib.rs`) lands first, as both sides use it. The CSI check and the lab scripts
depend only on Contracts.
