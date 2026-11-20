# Issue #74, plan review round 2, design-code

## Earlier findings
Round 1 code findings: R1-C1 fixed (ordinary devices, no unprivileged flag; 6.8.12 `ublk_drv.c:2391-2414`, `:768-795` re-read and agree). R1-C2 fixed (no ring handoff; libublk queue, store rings made by the child; `uring.rs:123-154` and `from_file`). R1-C3, R1-C5, R1-C6, R1-C8 resolved by the cut. R1-C4 fixed. R1-C7 fixed (no named socket; control socket bound by the helper). R1-C9 fixed (switch, `PRIVSEP`, direct mode listed; but see R2-C7). R1-C10 mostly fixed (see R2-C2, R2-C6, R2-C8). R1-C11 fixed. R1-C12 fixed (call-site list matches `ublk.rs:129-131`, `:141`, `:168-176`, `:626`, `:902-934`, `:1050-1052`, `control.rs:202`, `cli.rs:99`, bench, disktool). R1-C13, R1-C14 resolved by the cut. Nits fixed.
Cut text: registry, ledger, release track, claim action, Resize/Recover, SLSA, verify.sh are gone from intent, spec, design, adr-proposal (grep). Only the three document titles still carry "and verifiable release artifacts" (R2-C9).

Kernel claims re-read in the 6.8.12 tree and found right: START_DEV takes the pid from `data[0]` and waits (2252-2262); `capable(CAP_SYS_ADMIN)` per control command (2852-2854); data commands check only the fetching task (1746-1769); owner-only opens are for unprivileged devices (768-795); partition scan suppressed when a daemon is unprivileged (2288-2290, 1688-1696); recovery needs the char device closed (2740-2744); zero copy cleared (2483); DEL_DEV waits for the id (2553-2568); ids and `ublkb%d` naming. libublk 0.4.8: `new_with_info` private with the 3 s open loop (io.rs:889-927), `create_queue_handlers` private and uses `&self` for affinity and json (ctrl.rs:2886-2942), `disown`, `set_params`, `wait_for_buffer_registration`, `set_io_thread_idx`, `init_queue_thread` public; queues read only `dev_id`, `flags`, `max_io_buf_bytes`, `nr_hw_queues`, `queue_depth` from `dev_info`; no mlock without the opt-in flag. rustix 1.1.5 has the named safe calls.

### R2-C1 — Medium
S5 ("the helper exits when its child exits") is false while the helper sits in a blocking kernel call, and the Risks text justifying it ("as those calls block the root process today") is wrong. START_DEV waits in `wait_for_completion_interruptible(&ub->completion)` until every queue fetched (`ublk_drv.c:2252-2262`); if the child dies first nothing completes it, and the helper, one thread polling socket and pidfd (design.md:29, 140), never sees the pidfd. DEL_DEV waits until the id is freed (`:2553-2568`), i.e. until a mounted device is unmounted. Today these block one thread of the node (`ublk.rs:1050`, the `UblkCtrl` drop), and systemd restarts a crashed node; under the helper a child crash or OOM-kill during start, or while a deposed device stays mounted, leaves the unit active with no I/O process. The only stated release is stopping the unit (design.md:218-220). No test covers it (`exits_with_its_child` only exits normally).
Fix: a watchdog thread, created after step 7 (it inherits the reduced capabilities and needs none), blocked on the pidfd and calling `process::exit` with the child's status; or run the kernel commands off the serving thread. Add a kernel test: kill the child while `start` waits with no queue fetching, the helper exits 137 within a bound. Correct the Risks sentence.

### R2-C2 — Medium
Error paths of an attach after `add` are unspecified (design.md:81-86 describes only the success path; the helper cleans up only a failed `add`, design.md:56-62). If `from_cdev`, queue setup or `start` fails, nothing says the I/O process stops its queue threads, closes the char device and sends `delete`. The id stays owned: the fixed `--id` (node.sh passes `--id "$id"`) then gives EEXIST on every later `add` until the node is restarted and the leftover deleted by hand, and each leak counts toward the limit of 4. Note that DEL_DEV cannot finish while the child still holds `/dev/ublkcN` (the cdev refcount, see design.md:61) and queue threads blocked in FETCH hold it; today's `run_target` has the same hole, so the plan must say how the threads are woken (the node's eventfds) before `delete`. No test (`failed_add_deletes_its_device` covers only the helper's own add).
Fix: specify "on any error after `add`: end the queue threads, drop the `UblkDev`, `delete`, report the first error", and add a `privsep.rs` test with a fake helper (start answers `error kernel start`) that expects `delete` and thread exit, plus a kernel-test variant.

### R2-C3 — Low
`node.rs:210` requires a non-empty `c.device` (usage error otherwise); the design says `--device` is refused under `--privsep` (design.md:75) but not that this validation, `format_if_asked(&dev, &c.device, ...)` (`node.rs:255`, its messages name the path) and `Shared.device` (`ublk.rs:1002`, logged at `:1549`) change. As written `node --privsep` without `--device` exits with the usage text. Also `/dev/urandom` (`cli.rs:52`, used by `--format`) is a device path the child opens; S2's "never opens a device path" should say "disk or ublk device path".
Fix: list the parse change and the path-free messages; test `privsep_parses_without_device` and `--format --force` under `--privsep` on a loop file through `from_file`.

### R2-C4 — Low
Spec S3 and the invariant say device size is bounded, the Contracts bound only QUEUES, DEPTH, IO_BUF and ID; SIZE is "a non-zero multiple of 4096 (u64)" (design.md:149-151). The test `bounds` cannot fail for size. A compromised child can ask for a 2^63-byte disk that the root CSI plugin then probes.
Fix: pick a bound (a configured maximum volume size, or the disk's size) and test it; or drop "device size" from S3.

### R2-C5 — Low
Several I/O-process threads call the helper: `run_device`'s thread (add, start, delete), the detach handler (`control.rs:202`) and the deposition thread (`ublk.rs:626`). The protocol is strictly one request, one reply on one SEQPACKET socket (design.md:138-140), but the client's serialisation is not stated. Without a lock a stop's reply can be read by the thread that sent start; with a lock a stop (the way to abort a stuck attach) waits behind a `start` that blocks (R2-C1). `control::spawn`/`detach` also need a client handle that they lack today.
Fix: state a mutex around request+reply in `privsep.rs`, and that detach during `start` is answered by the helper's death or a bounded `start` wait; test two threads issuing calls against a fake helper.

### R2-C6 — Low
S3's rule that START_DEV carries the child's pid has no test that can fail on the 6.8 kernel (it does not check the pid, `ublk_drv.c:2252-2281`); the claim that newer kernels check it is "from memory" (design.md:222). Only the lab's newer kernel would reveal a wrong pid, and then late. Where the pid comes from (the spawned child's pid, kept with the pidfd) is not stated.
Fix: the fake `Kernel` records the pid argument; test `start_uses_the_child_pid` in `serve.rs`; state the source.

### R2-C7 — Low
`lab.sh:1513` decides whether a running node can be reused from the unit's ExecStart flags (`--zero-copy|--no-verify|--queues|--poll-us|--defer-taskrun`); `--privsep` is not in that list, so a node left in direct mode by `PRIVSEP=0 make node-up` is reused by a default (hardened) bench run, and the reverse. The design lists `lab.sh:679` only. The S8 claim "default lab runs under the helper" then depends on history.
Fix: add `--privsep` (and the helper's name) to the compared flags, plus a script test in `node-scripts-test.sh`/`lab-deletion-test.sh`.

### R2-C8 — Low
S5's second half ("when the helper dies, its supervisor ends the child") has no test, and an idle child never learns that the helper is gone (it reads the socket only during a call; PDEATHSIG is cleared on the credential change, decisions.md). Under #12 this relies on PID 1 dying.
Fix: either have the child treat EOF on the helper socket as fatal (a reader thread), with a unit test, or add a lab check `systemctl kill --kill-who=main -s KILL` that the child is gone.

### R2-C9 — Low
Behaviour change not stated as a rule or tested: with no privileged queue daemon the kernel sets `GD_SUPPRESS_PART_SCAN` (`ublk_drv.c:2288-2290`), so a raw block volume carrying a partition table no longer shows partition nodes (the plan mentions it only as a kernel fact, design.md:20-21). Raw block volumes are tenant-visible.
Fix: record it in spec S3 or Risks and `docs/lab.md`/followups, and add a lab or kernel check of the expectation (no `ublkb<N>p1` for a partitioned device) so a later kernel change shows.

## Nits
- `intent.md:1`, `spec.md:1`, `design.md:1`: the titles still say "and verifiable release artifacts", which this step excludes; say "step 1".
- `design.md:114`: "`bench-reads.sh:99`: `pgrep -x flaminio-io`" reads as the current code; line 99 is `pgrep -f 'flaminio-io node' | head -1`, which matches the helper first (lowest pid). Say "replace with".
- `design.md:85`: the sysfs `cpu_list` exists only after START_DEV creates the disk, so the affinity is set after `start`, later than libublk's pre-start `set_thread_affinity`; say so (threads fetch unpinned until then).

Reviewed commit: d762b38432e838e8edb4ea0bd1a164c53bf15eda
