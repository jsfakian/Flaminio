# Issue #74 plan review: round 2 (incremental), security focus

Scope: `git diff 9d1db41..96d792d -- docs/issues/74/ ':!docs/issues/74/report.md'` in
`/tmp/flaminio-plans-2026-11-12/74`. I checked it against the worktree's code, libublk 0.4.8 and
the Rust std source in the local toolchains, and the Linux 6.8.12 tree
(`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`, read only). The kernel claims in
decisions.md that I spot-checked hold: `ublk_drv.c` 503, 1356–1390, 1540–1550, 1693–1694,
1746–1769, 2226–2228, 2252–2290, 2391–2414, 2483, 2553–2568, 2740–2744, 2852–2854;
`scm.c:107–111`; `af_unix.c:1649–1650`; `sys.c:2706–2708`; `io_uring.c:4113–4129`;
`cred.c:406–413`; `fops.c:600–631`.

## Earlier findings
- R1-S1: fixed. S6 checks the path pattern and the sysfs rdev. The unbounded answer read is new (R2-S5).
- R1-S2: fixed. There is no ring handoff.
- R1-S3: fixed. Devices are ordinary, so recovery and raw block keep working.
- R1-S4: fixed. The socket is an unnamed socketpair, and no helper socket is in `/run/flaminio`.
- R1-S5: resolved by the cut. Agent privileges are listed as open in intent.md:16–17.
- R1-S6: resolved by the cut. The unit names the path, and no dev_t is pinned.
- R1-S7: resolved by the cut. There are no epochs or tokens at the helper.
- R1-S8: resolved by the cut, but the in-memory ownership table can go stale (R2-S8).
- R1-S9: fixed. The helper has no network, metadata client or token check.
- R1-S10, R1-S11, R1-S16: resolved by the cut. Step 4 stays open (intent.md:21–22).
- R1-S12: fixed for #43. The new #12 assumption contradicts #12's draft (R2-S2).
- R1-S13: fixed. Tests are named per rule; new untested paths are listed below.
- R1-S14: fixed. Requests carry no descriptor, and a reply carries at most one.
- R1-S15: fixed. The helper closes its copy, so the O_EXCL claim ends with the I/O process.
- Security nits: fixed ("public", "incarnation" and "registry" are gone; `add_device` is cited).
- R1-C1, R1-C2, R1-C7, R1-C9, R1-C10, R1-C11, R1-C12, N1–N4: fixed.
- R1-C3, R1-C5, R1-C6, R1-C8, R1-C13, R1-C14: resolved by the cut.
- R1-C4: fixed for #43, #16, #73 and #80. #12 and #48 are still assigned work their drafts lack (R2-S2).

### R2-S1 — Medium
**The helper does not exit with its child while it is blocked in a kernel call (S5).**
S5 says the helper exits when its child exits (spec.md:47). The design polls the pidfd only
between requests and runs each request to completion (design.md:42, 139–140). On 6.8.12, ublk
control commands never run inline. They return `-EAGAIN` under `IO_URING_F_NONBLOCK`
(`ublk_drv.c:2925–2926`), so io-wq runs them, while libublk's synchronous call waits for the
completion. START_DEV waits, interruptibly, for every queue to fetch (`:2262`). DEL_DEV waits
until the last reference is gone (`:2567`), and a mounted device holds that reference
(design.md:65–66). The child's death does not wake the helper: SIGCHLD with SIG_DFL is
discarded. The helper therefore stays the unit's MainPID with a dead child, and systemd never
restarts it. A realistic case: a deposed target whose filesystem is still mounted (failover
after a partition) has the helper waiting in DEL_DEV for hours, and the child crashes. Today the
whole root process would die and restart, and its replicas would come back. Under the helper,
the replicas stay down, and `ensure_nodes` (`scripts/lab.sh:781–786`) sees an active unit and
leaves it alone. The Availability risk (design.md:218–220) covers only stopping the unit by
hand. `exits_with_its_child` tests only an idle helper.
**Fix:** run kernel commands on one worker thread. The main thread keeps polling the pidfd and
the socket, and exits with the child's status when the child exits. Process exit interrupts the
io-wq wait (DEL_DEV has already set `UB_STATE_DELETED` and removed the device,
`ublk_drv.c:2543–2546`). Send replies with `MSG_DONTWAIT`, so that a child that never reads
cannot block the helper either. Add `exits_with_its_child_while_blocked`: the child sends
`start` on a device whose queues never fetch, then exits, and the helper must exit with its
status within a bound.

### R2-S2 — Medium
**The dependencies on #12 and #48 are not in their drafts, and the plan never says that
Kubernetes stays privileged.**
intent.md:25, decisions.md:57 and design.md:224–227 say that #12 runs the helper as the
data-plane container's entrypoint, and that #48 routes recovery through it. #12's draft runs
`flaminio-io node --device ...` directly in a privileged container
(`flaminio-12/docs/issues/12/design.md:41–44`), under its proposed ADR 0007 "A privileged
data-plane pod, until privilege separation lands". No plan owns that switch. #48's draft keeps
recovery inside `flaminio-io` and adds a second root path, `flaminio-io end-kept VOLUME
RECORD_NONCE`, which stops and deletes ublk devices (`flaminio-48/docs/issues/48/design.md:98–113`).
Under `--privsep`, that recovery cannot reach `/dev/ublk-control`. The resulting second
privileged ublk-control path is outside this ADR's boundary. None of the plan's documents says
that after this step every Kubernetes data plane is still direct mode with the whole node (ADR
0007's Consequences). The ADR's consequence "the process that parses network input runs without
capabilities" (adr-proposal.md:58–59) holds only for the lab's systemd unit.
**Fix:** state in intent and ADR Consequences that Kubernetes stays direct and privileged (ADR
0007) until a named owner switches the pod. Record that owner and the pod's requirements in the
planned followups entry. Flag the #48 contradiction to #48: recovery requests go through this
helper, and there is no second root ublk path.

### R2-S3 — Low
**The device bound holds per helper lifetime and per moment, not over restarts or time.**
The invariant says the child can add "at most 4" devices (spec.md:19–20). When the child exits,
the helper exits without deleting what it owns (design.md:42, 47–49). Those devices are not
recoverable here (`ctrl_flags` 0), so they are garbage. Each one keeps its tag set and counts
against `ublks_max`, which on 6.8.12 limits every device on the node, privileged ones included
(64; `ublk_drv.c:504`, `:2439`). Under a restarting supervisor (#12's pod), a compromised child
leaks 4 devices per life. After 16 restarts, no ublk device can be added on the node until an
administrator cleans up. Within one life, add/start/stop/delete cycles have no rate limit. Each
cycle sends uevents and makes udev probe, as root, contents the child chose (Q4). None of this is
tested.
**Fix:** when the child exits, the helper sends DEL_DEV for each owned device before it exits. It
does not wait for the last close (see R2-S1); the kernel frees the device then. Rate-limit `add`
(for example one per 250 ms, the client's retry interval). Add `owned_devices_deleted_on_child_exit`
and an `add` rate-limit case to `serve.rs`.

### R2-S4 — Low
**SIZE allows capacities that overflow the kernel's byte size.**
The design bounds SIZE only as a non-zero multiple of 4096 in a u64 (design.md:150–151), while
the spec says device size is bounded (spec.md:40). 6.8.12 does not check `dev_sectors`
(`ublk_validate_params`, `ublk_drv.c:566–606`). `set_capacity` stores `(loff_t)sectors << 9`
(`block/bdev.c:427`), which is negative for SIZE ≥ 2^63. A compromised child can thus make the
root helper create a disk whose `i_size` is negative, and udev then probes that disk as root.
**Fix:** bound SIZE to at most `i64::MAX & !4095`, or a product limit, and add these values to the
`bounds` test.

### R2-S5 — Low
**The CSI plugin reads an answer of any length from the now-untrusted data plane.**
S6 checks only the device string (spec.md:51–53; design.md:96–101). `ControlSocket.command`
reads the answer with `bufio.Reader.ReadString('\n')` and no limit (`control/csi/dataplane.go:75`)
for up to 60 s (`:41`). The plugin is root, has no memory limit and is `system-node-critical`
(`deploy/csi/driver.yaml:101`, `:105–137`), so the kernel's OOM killer spares it and kills other
pods. Answers also reach gRPC errors verbatim (`dataplane.go:82`).
**Fix:** cap the answer (for example 4 KiB, through `io.LimitReader`) and refuse non-printable
bytes. Add `TestAnswerTooLong` to the S6 tests.

### R2-S6 — Low
**The disk descriptor allows device commands whose effects outlive the process.**
The invariant says the child "holds its disk" (spec.md:18). An unprivileged process holding an
`O_RDWR` whole-disk descriptor can also send SG_IO write-class commands (MODE SELECT, FORMAT
UNIT, START STOP, WRITE LONG; `drivers/scsi/scsi_ioctl.c:251–340`) and persistent-reservation
ioctls (`block/ioctl.c:262–275`). For NVMe, it can send the namespace I/O commands the controller
supports, reservations among them. Saved mode pages (for example the write-cache bit, which `sd`
does not re-read until a rescan) and reservations persist after the process is gone. On a shared
LUN, reservations also reach other hosts. Neither Risks nor step 3's list in intent.md:18–20
names this.
**Fix:** state it in the invariant and Risks. Add "deny SG_IO, IOC_PR_* and NVMe passthrough
ioctls" to step 3's profile scope.

### R2-S7 — Low
**The token file is created world-readable, and stale tokens become reachable.**
The design gives only the end state: `/run/flaminio` 0711 and the token 0400, owned by
`flaminio-io` (design.md:110–111). Today `sudo tee` creates the token 0644, which only the 0700
directory protects (`scripts/guest/node.sh:61–63`). A tee-then-chmod implementation leaves the
token world-readable for a moment in a directory anyone can traverse. Tokens left by earlier
direct-mode runs (0644, named after predictable volume ids) become readable to every local user
once the directory is 0711. Tokens never expire.
**Fix:** create the token with its final mode and owner (`umask 077`, then `install -m 0400 -o
flaminio-io`), and remove `/run/flaminio/*.token` on `up`. Add the modes to `node-scripts-test.sh`.

### R2-S8 — Low
**Ownership is a bare device id, which can name a device the helper did not create.**
"Owned" means "added and not deleted by this helper" (spec.md:13), and the table holds only ids
(design.md:61, 67). A root `flaminio-io del --id N` empties an owned id without the helper
knowing. The lab does exactly that while the unit runs (`scripts/guest/node.sh:27–29`). If
another creator then adds id N, `start`, `stop` and `delete` act on its device. The kernel
records only the creator's uid, root for every root creator (`ublk_drv.c:2418`).
`foreign_device_refused` does not cover a reused id.
**Fix:** put a per-helper random nonce in `ublksrv_flags` at add (libublk `ctrl_target_flags`; the
kernel keeps it, `ublk_drv.c:2455`). Check it with GET_DEV_INFO before each start, stop and
delete, and forget the id on `ENODEV`. Add `reused_id_refused`.

## Nits
- design.md:38–40: std calls `setgid`, then `setgroups(0)`, then `setuid`, and it ignores `EPERM`
  from `setgroups` (library/std/src/sys/process/unix/unix.rs:307–330). Without `CAP_SETGID`, the
  supplementary groups would stay silently. Give the real order and say that `CAP_SETGID` is what
  makes the group drop work.
- adr-proposal.md:44–46 says "one capability retained", and :60 lists only `CAP_SYS_ADMIN`. The
  helper also keeps UID 0, which owns most of the filesystem (design.md:43). Name both.
- spec.md:83–84 (Q4) describes root *parsers*. udev rules also *act* on the contents: LVM
  autoactivation and md incremental assembly need no parser bug. The bytes of a raw block volume
  are tenant-controlled even today. Say so, so that the decision covers it.

Reviewed commit: d762b38432e838e8edb4ea0bd1a164c53bf15eda (diff 9d1db41..96d792d; HEAD adds only report.md)
