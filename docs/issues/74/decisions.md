# Issue #74 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. On 2026-11-13 the plan was approved before its plan review cleared
(review status then: addressing_round_2).

- **After approval:** the round-3 review fixes (2026-11-13) came after the approval; they are
  recorded under "Choices, plan review round 3 (after the approval)".
- **After approval:** the round-4 review fixes (2026-11-13) came after the approval too; they are
  recorded under "Choices, plan review round 4 (after the approval)".
- **After approval:** the round-5 review fixes (2026-11-13) came after the approval too; they are
  recorded under "Choices, plan review round 5 (after the approval)".

## Settled at approval

- **The proposed architecture** ([adr-proposal.md](adr-proposal.md), Decision; design.md, ADRs):
  accepted with the plan: the root helper that keeps ublk control and starts the unprivileged
  I/O process, the vendored libublk patch, and `rustix` as a direct dependency.
- **Q1, `PR_SET_IO_FLUSHER`** (spec.md, Open questions; design.md, Risks): no recommendation in the
  plan. Settled 2026-11-13 (user's instruction): the I/O process keeps no capability and the
  risk is accepted (see below).
- **Q2, the libublk change** (spec.md, Open questions; adr-proposal.md, Decision, Dependencies): the
  plan's Proposed choice is settled: a vendored libublk 0.4.8 with the two public functions,
  offered upstream. Who maintains the vendored copy: no recommendation in the plan. Settled
  2026-11-13 (user's instruction): this project, in this repository (see below).
  Changed after approval (round 3): the patch gains a third change, a failed ADD_DEV that sends
  no DEL_DEV.
- **Q3, another ublk server's disk** (spec.md, Open questions; design.md, Risks): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): no other ublk server on a
  data node is this step's deployment rule (see below).
- **Q4, untrusted contents reaching root parsers and udev rules** (spec.md, Open questions;
  design.md, Risks): no recommendation in the plan. Settled 2026-11-13 (user's instruction):
  accepted as today's residual, for a later plan of #74 (see below).

## Settled at the user's instruction of 2026-11-13

- **Q1, `PR_SET_IO_FLUSHER`:** the I/O process keeps no capability. Its queue threads run without
  `PR_SET_IO_FLUSHER`, and the risk of memory reclaim waiting on the device a thread serves is
  accepted for this step, stated in ARCHITECTURE §15.5 and `docs/followups.md` as the design's
  Docs plan. Recommended by Claude, no recommendation in the plan: keeping `CAP_SYS_RESOURCE`
  would break S1's empty capability sets, which this step exists for, while the risk is to
  availability only and never acknowledges a write.
- **Q2, who maintains the vendored libublk copy:** this project. The copy stays in
  `dataplane/vendor/libublk/`, changes only through this repository's reviewed pull requests, and
  is replaced by the upstream release once upstream takes the change. Recommended by Claude, no
  recommendation in the plan: no one else owns it, and the repository's usual review is the
  simplest owner, with no new process.
- **Q3, another ublk server's disk:** "no other ublk server on a data node" is this step's
  deployment rule, stated in `docs/followups.md` with Q1–Q4 as the design's Docs plan; marking
  the helper's devices waits for a step that needs another ublk server on a data node.
  Recommended by Claude, no recommendation in the plan: the lab and #12's pod run no other ublk
  server, the rule needs no code, and under it the invariant's ublk-disk check leaves no foreign
  ublk disk to name.
- **Q4, untrusted contents reaching root parsers and udev rules:** accepted as today's residual in
  this step, listed in `docs/followups.md` as the design's Docs plan, and left to a later plan of
  #74 to close. Recommended by Claude, no recommendation in the plan: a raw block volume's bytes
  reach the same parsers and rules today without the helper, so this step widens nothing, and
  closing it needs host udev and LVM configuration outside this step's boundary.

## Scope (plan review round 1)

- **The plan is step 1 of #74:** the issue's step 1 and the I/O half of its step 2 (intent.md).
  Profiles, storage binding, NetworkPolicies, the agent's privileges, controller images, the
  release track and the disclosure work stay open in #74. The release track (old S7–S11), the
  disk registry, the claim action, the ledger, the helper's metadata identity, ring handoff and
  io_uring restrictions were cut, not guarded. Findings on cut text are resolved by the cut.
- **The boundary:** a root parent `flaminio-helper` keeps `/dev/ublk-control` and `CAP_SYS_ADMIN`,
  and starts `flaminio-io node --privsep` as its only child, unprivileged, connected by an unnamed
  socketpair on the child's stdin. Chosen as the simplest the kernel facts below allow.

## Choices, per finding

- **R1-C1, R1-S3 (High):** no `UBLK_F_UNPRIVILEGED_DEV`. The helper adds ordinary devices, passes
  `/dev/ublkcN`, keeps every control command, and sends START_DEV with the child's pid. Recovery
  and raw block opens stay as today; zero copy is refused under the helper (security: the uapi
  warning in Risks).
- **R1-C2, R1-S2 (High):** no ring handoff. The I/O process creates every ring itself, the store's
  included, and enables its single-issuer rings in the submitting thread. No opcode list, FD
  whitelist or second-stage seccomp in this step (step 3). One descriptor per reply.
- **R1-S1 (High):** new rule S6: the CSI node plugin checks the device path and the kernel's name
  for it before `blkid`, `mkfs` or `mount`; tests with a lying data plane. The agent and claim job
  do not read the data plane's answers (only `control/csi/node.go:59`, `:74`, `:140`, `:180` do).
- **R1-C3, R1-S6 (High, Medium):** resolved by the cut: no registry and no runtime disk open. The
  helper opens the one disk path the node's unit names, at its start, as the node does today; a
  `/dev/disk/by-id` path survives renumbering. No `dev_t` is pinned.
- **R1-C4:** #43 (install), #16 (helper identity), #73 (audit), #80 (quotas) are no longer used.
  `Resize` and `Recover` are dropped; #48 adds recovery requests (design, Dependencies). #12
  packages the helper; the lab does not wait for it.
- **R1-C5, R1-S9:** resolved by the cut: the helper reads no metadata, checks no token and has no
  network identity; no `auth.go` change. Its authority is the devices it created for its child.
- **R1-C6, R1-S5:** resolved by the cut: agent privileges and the claim action leave this step; the
  agent keeps its root probes.
- **R1-C7, R1-S4:** no named helper socket. The helper binds the control socket as today (root,
  0600) and passes it; the I/O process never writes `/run/flaminio`. The startup token is 0400,
  owned by the I/O user, in a 0711 directory (lab).
- **R1-C8, R1-S8:** resolved by the cut: no ledger and no files to version. Ownership is the helper's
  in-memory table for one lifetime; a failed add deletes its device; leftovers after a crash are
  stopped by the kernel and deleted by the administrator, as today.
- **R1-C9:** the switch is the command line (`flaminio-helper ... -- flaminio-io node --privsep`);
  the lab defaults to it (`PRIVSEP=1`, 0 with `ZERO_COPY=1`); direct mode is listed in S7.
- **R1-C10, R1-S13:** every rule's tests are named with file and case; the lab is written as
  commands with pass criteria and restore. Rows for cut rules went with them.
- **R1-C11:** capability sets derived from the operations: bounding emptied before the spawn,
  {`SYS_ADMIN`, `SETUID`, `SETGID`} for it, {`SYS_ADMIN`} after; no `DAC_OVERRIDE`. Tested.
- **R1-C12:** every privileged call site is listed in design facts with its fate.
- **R1-C13, R1-C14, R1-S10, R1-S11, R1-S16:** resolved by the cut of the release track.
- **R1-S7:** resolved by the cut: no epochs or tokens at the helper; stop and delete need only
  ownership; recovery is #48's.
- **R1-S12:** no host service. The lab runs the helper in the node's systemd unit; #12 runs it in
  the data-plane container. Retained authority is listed in design Risks.
- **R1-S14:** requests carry no descriptor (closed and refused); replies at most one.
- **R1-S15:** resolved by the cut (no release or crypto-erase). The helper closes its disk copy after
  passing it, so the exclusive claim ends with the I/O process.
- **Nits (code N1–N4, security):** `uring.rs:123–154`; "incarnation" removed (the step uses no
  epoch term); ARCHITECTURE §5.1 named as text to change; no `SO_PEERCRED`/pidfd peer check
  (unnamed socket); "public" control socket gone; `add_device` cited; "registry" gone.
- **Socket placement:** the helper socket is the child's stdin because `OwnedFd::from_raw_fd` and
  `std::env::remove_var` are `unsafe` (edition 2024) and the workspace forbids `unsafe_code`
  (`dataplane/Cargo.toml:39–40`); `Stdio::from(OwnedFd)` and `stdin().as_fd()` are safe.
- **Cross-plan (#71):** superseded in round 3; see "Choices, plan review round 3", R3-S3.

## Choices, plan review round 2

- **R2-C1, R2-S1 (Medium):** the helper's main thread only waits on the child's pidfd; a server
  thread, started after the capability drop, answers requests and may block in the kernel. The
  child's exit therefore ends the helper whatever the server thread waits on (design, Owned
  devices, outcome 4; test `exits_with_its_child_while_blocked`). Replies need no
  `MSG_DONTWAIT`: a blocked send stops only the server thread.
- **R2-C2, R2-S3, R2-S8:** ownership and cleanup are stated once, in design "Owned devices", with a
  test per outcome. Ownership is the helper's open descriptor of `/dev/ublkcN` (the pin), which
  keeps the id from being freed (`fs/char_dev.c:348–355`, `:496`; `ublk_drv.c:2150–2164`); chosen
  over a nonce in `ublksrv_flags`, which needs a GET_DEV_INFO before each command and still has a
  window between check and command. A failed attach stops, joins, drops and deletes; the child's
  exit deletes owned devices within 5 s; adds are paced to one per 250 ms. The helper's own death
  leaves devices for the administrator, as stopping the root node does today.
- **R2-S2 (Medium):** intent.md, "Which step", says once that Kubernetes stays privileged in direct
  mode until #12 adopts the helper (#12's proposed ADR 0007), and that #48's recovery is not
  covered and must go through the helper. The ADR and design defer to it.
- **R2-C3:** `--privsep` drops the `--device` requirement (`node.rs:210`) and path messages; S2 says
  "disk or ublk device path" (`/dev/urandom` stays).
- **R2-C4, R2-S4:** SIZE at most 2^63 − 4096, tested.
- **R2-C5:** one client mutex from request to reply; a `start` reply later than `START_LIMIT` is
  fatal, so a queued `stop` waits at most that long.
- **R2-C6:** START_DEV's pid is the spawned child's (`Child::id`), tested on the fake kernel.
- **R2-C7:** `lab.sh:1503–1513` compares `--privsep` too.
- **R2-C8:** a watcher thread in the I/O process exits on the helper socket's hangup (chosen over a
  lab check, so it holds under any supervisor).
- **R2-C9:** partition scanning suppressed: stated under Risks and in `docs/lab.md`; kernel test
  `partition_scan_suppressed`.
- **R2-S5:** the CSI plugin reads at most 4096 printable bytes of an answer.
- **R2-S6:** stated in the invariant and Risks; denying those ioctls joins step 3's scope. No simple
  mechanism closes it in this step.
- **R2-S7:** resolved by a cut: with `PRIVSEP=1` `node.sh` refuses `ATTACH`, which no lab target
  sets, so no token file is written and `/run/flaminio` keeps its mode.
- **Nits:** std's real order (`setgid`, `setgroups(0)` with `EPERM` ignored, `setuid`) and why
  `CAP_SETGID` matters; the ADR names UID 0; Q4 names udev rules; titles say step 1; the
  `bench-reads.sh` change is worded as a replacement; affinity is set after `start`.
- **Cross-plan (#71 round 2):** superseded in round 3; see R3-S3 below.

## Choices, plan review round 3 (after the approval)

- **R3-S1 (Critical), R3-C3:** verified in libublk 0.4.8: `UblkCtrlInner::new` returns on a failed
  ADD_DEV (`ctrl.rs:1258`, also `new_async` `:1301`) and the drop (`:1085–1096`) sends DEL_DEV for
  `dev_info.dev_id` (`:1459`, `:1626–1634`), which still holds the requested id (`:1134`; the kernel
  copies `dev_info` back only on success, `ublk_drv.c:2498`). Fix: a third change in the vendored
  patch marks the handle deleted when that step fails, so the drop sends nothing; the id stays in
  the protocol (the lab's fixed `--id 1`). The entry's handle is disowned; a pin is closed before
  the helper's own DEL_DEV, sent on a fresh handle. Tests: `failed_add_sends_no_delete`,
  `foreign_device_survives_add_of_its_id`, `add_of_an_owned_id_changes_nothing`,
  `delete_closes_pin_before_del_dev`. Today's direct-mode `add_device` (`ublk.rs:902–934`) hits the
  same path with a fixed `--id`; the patch changes that too, and `node.sh up` now deletes a
  leftover first.
- **R3-S2:** outcome 4 now shuts the helper's socket end down, waits up to 4 s for the current
  request, and sends each DEL_DEV on its own thread; one `start` per owned device. An `add` that
  outlasts the 4 s is left, as stated. Tests named in outcome 4 and `second_start_refused`.
- **R3-S3:** #71's rule (its contracts, "Under #74's `--privsep`"): the helper request for the
  telemetry socket is a later step of #74, in neither plan's present step; `--telemetry` with
  `--privsep` is refused at parse by whichever plan lands second; `node.sh` dies with
  `TELEMETRY=1 needs PRIVSEP=0`.
- **R3-S4:** `node.sh down` stops the unit before `del`, in both modes; a root `del` of an owned
  device while the unit runs is unsupported, as stated in outcome 6.
- **R3-C1, security nit 1:** the stale approvals text is removed; the ADR's sections, the design's
  ADR line and spec Q1–Q4 are marked as settled or still open in place (approval brief 5b).
- **R3-C2, security nit 4:** the pin keeps `UB_STATE_OPEN` set; #48 must close it before
  START_USER_RECOVERY (intent, ADR).
- **R3-C4:** `flaminio-io/tests/privsep.rs` `exits_when_the_helper_goes` runs the binary with a
  socketpair as stdin and closes the other end: exit 1 with `the helper is gone`.
- **Security nit 2:** the ADR's Consequences name the disk descriptor's ioctls. **Nit 3:** a
  `delete` of a held device is stated as unbounded. **Code nit:** the approval's workflow state is
  dated.
- **The `io` key (coordinator, from #71's round 4):** stated under design Risks with the
  follow-up "metadata clients check the `meta` URI SAN" (`pkg/pki/pki.go:300–310`,
  `flaminio-metaclient/src/lib.rs:85–92`). Not closed here: the key's usages are #16's.

## Choices, plan review round 4 (after the approval)

- **R4-S1, R4-S2 (Medium): device cleanup in one place** (design, Owned devices). The helper adds
  only at the id its own command line names (`--id`, the lab's 1); the child names none. So it
  owns at most one device, and the only device a stop can leave is `/dev/ublkc<ID>`, which
  `node.sh down` deletes after stopping the unit. Chosen over SIGTERM handling, which needs a
  signal crate (rustix 1.1.5 has no safe signal mask) and a `KillMode` change. At the child's
  exit, `closing` is set before the socket is shut down; the server thread checks it before each
  request, so queued requests are never served, and an `add` finishing after it deletes its own
  device. Tests: `queued_requests_after_child_exit_ignored`, the `down` case of
  `node-scripts-test.sh`, `add_uses_the_configured_id`. `add_of_an_owned_id_changes_nothing`
  (round 3) is replaced by `device_limit`.
- **R4-C1:** verified: libublk retries a failed control command with its legacy opcode
  (`ctrl.rs:1500–1520`, `:1571–1582`); Ubuntu kernels lack `CONFIG_BLKDEV_UBLK_LEGACY_OPCODES`
  (kernel-noble `debian.master/config/annotations:2199`; this host's `/boot/config-6.8.0-138-generic`)
  and refuse it with `EOPNOTSUPP` (`ublk_drv.c:1712–1714`). Replies keep libublk's errno; tests
  check what failed and what survived, not the errno. No fourth patch.
- **R4-C2:** spec S3 and the ADR say a failed add deletes no device but the one it added.
- **R4-C3:** the watcher takes an exit hook; `exits_when_the_helper_goes` leaves the node blocked on
  a FIFO outside any helper call, so only the watcher can end it.
- **R4-C4:** the worst case is (b) + (d), 9 s; the kernel tests allow 15 s.
- **R4-C5:** `ublk.sh up` deletes a leftover `/dev/ublkc0` when its unit is inactive; tested.
- **Nit, and #71's notes:** the `io` key text names #71's Prometheus metadata job and its
  `meta.flaminio.invalid` DNS SAN; a node under the helper shows `stale` and raises
  `FlaminioTelemetryLost`, by #71's rules.

## Choices, plan review round 5 (after the approval)

- **R5-B1:** the server thread also checks `closing` after each `recvmsg`, under the table lock,
  and drops a request read then unanswered; queued requests are therefore never served.
  `request_read_after_closing_dropped` covers a request read after `closing` is set (round 6;
  `queued_requests_after_child_exit_ignored` covers requests queued before it).
- **R5-C1:** one device per helper follows from one volume per node; #15's several volumes need one
  owned id per volume, which changes the ADR. Stated in design Risks and ADR Consequences;
  followups item 5 gains that step (Docs).
- **R5-C2:** each kernel test uses `--id` 100 plus its index, never the lab's 0 or 1, checked free
  in `/sys/class/ublk-char` first.
- **R5-C3:** `refuses_a_missing_or_bad_id` (no `--id`, `-1`, `1048576`).
- **Nit:** the invariant says "at most one bounded device at a time".

## Kernel facts

Read in the Linux 6.8.12 tree on this host (`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`,
read only):
- `drivers/block/ublk_drv.c`: unprivileged devices clear recovery and refuse user copy, a
  `CAP_SYS_ADMIN` creator gets the flag cleared (2391–2414); owner-only opens apply to unprivileged
  devices only (768–795); ordinary devices need `CAP_SYS_ADMIN` per control command (2852–2854);
  the character device opens once (1356–1365); its mmap checks only the mm (1376–1390) and its
  data commands only the fetching task (1746–1769); an unprivileged server suppresses partition
  scans (1693–1694, 2288–2290); START_DEV takes the pid from `data[0]` and waits for the queues
  (2252–2281); START_USER_RECOVERY needs the character device closed (2740–2744); zero copy is
  cleared for every device (2483); a dying server's device is stopped (1279–1297, 1540–1550);
  `ublk_remove` stops first (2226–2228); DEL_DEV waits for the id to be freed (2553–2568); disks
  are named `ublkb%d` (2277); `GET_QUEUE_AFFINITY` reads the tag set's map (2345–2346); id bound
  (503).
- `net/core/scm.c:107–111`: io_uring files refused in `SCM_RIGHTS`.
- `io_uring/register.c:229–236`: enabling an `R_DISABLED` single-issuer ring binds the enabler.
- `io_uring/io_uring.c:4113–4129`: `io_uring_disabled` and `io_uring_group`; `:3955` and
  `rsrc.c:41–49`: an unprivileged ring's registered buffers count against `RLIMIT_MEMLOCK` (the
  code registers none: no `register_buffers` call in `dataplane/crates`).
- `kernel/sys.c:2706–2708`: `PR_SET_IO_FLUSHER` needs `CAP_SYS_RESOURCE`.
- `kernel/cred.c:406–413`, `fs/exec.c:1360–1362`: the parent-death signal is cleared on a
  credential change, so the design does not rely on it.
- `block/fops.c:600–631`: the exclusive claim belongs to the open file.
- `net/unix/af_unix.c:1649–1650`: both ends of a socketpair carry the creator's credentials.
- `block/blk-mq-sysfs.c:114`: `cpu_list`.
- Round 2: control commands run in io-wq (`ublk_drv.c:2925`); STOP_DEV cancels fetches of a
  never-started device (1663–1680, 1497–1512); the id is freed at device release (2150–2164), and
  an open character device holds the device (`fs/char_dev.c:348–355`, `:496`, `:608–624`);
  removal returns the `ublks_max` slot (2226–2233; limit 504, 2439); no `dev_sectors` check
  (566–607) and `(loff_t)sectors << 9` (`block/bdev.c:427`); seqpacket reports the peer's close
  (`net/unix/af_unix.c:3201–3204`, `:873`); no partition rescan when suppressed
  (`include/linux/blkdev.h:244–248`, `block/genhd.c:343–349`, `block/ioctl.c:556–562`); `SG_IO`
  write-class commands and reservations need only a writable descriptor
  (`drivers/scsi/scsi_ioctl.c:251–341`, `block/ioctl.c:262–275`).
- Rust std (nightly-2026-10-28 source) `sys/process/unix/unix.rs:307–330`: `setgid`, then
  `setgroups(0)` ignoring `EPERM`, then `setuid`.

**Not verified here:** whether newer kernels refuse zero copy or auto buffer registration for
unprivileged devices (moot: the helper sets neither); whether the `scm.c` refusal is in 6.1 or
6.6 stable (moot: no ring is passed); whether newer kernels check START_DEV's pid (the helper
sends the right one); every behaviour on the lab's newer kernel, which `make privsep-check` shows.

## Library facts

- libublk 0.4.8: `new_with_info` is private (`io.rs:889`), its open loop `io.rs:910–927`;
  `set_params` (`ctrl.rs:2532`) and `disown` (`ctrl.rs:2790`) are public; `start_dev`
  (`ctrl.rs:2637–2651`); the drop deletes (`ctrl.rs:1085–1096`); `init_queue_thread` ignores
  `prctl`'s result (`ctrl.rs:2874–2884`). Queues read `dev_id`, `flags`, `max_io_buf_bytes`,
  `nr_hw_queues`, `queue_depth` from `dev_info` (grep of `io.rs`).
- libublk-rs-sys 0.1.2 `ublk_cmd.h:223–224`: zero copy is withheld from unprivileged devices
  because a server can leak uninitialized kernel memory.
- rustix 1.1.5 (locked through async-io) has safe `set_no_new_privs` (`thread/prctl.rs:631`),
  `remove_capability_from_bounding_set` (`:489`), `clear_ambient_capability_set` (`:725`),
  `set_capabilities` (`thread/libcap.rs:136`), `pidfd_open` (`process/pidfd.rs:29`), and
  `sendmsg`/`recvmsg` with `ScmRights` (`net/send_recv/msg.rs:712`, `:789`).
