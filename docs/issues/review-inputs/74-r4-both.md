# Issue #74, plan review round 4 (incremental), design-code and design-security

Scope: `git diff 7245c69..2040d58 -- docs/issues/74/ ':!docs/issues/74/report.md'` in
`/tmp/flaminio-plans-2026-11-12/74` (HEAD f9732b9 adds only report.md). Checked against libublk
0.4.8 (`~/.cargo/registry/.../libublk-0.4.8/src/ctrl.rs`), the 6.8.12 tree
(`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`), the worktree's scripts, and #71 at 9dea4a9.

## Earlier findings
- R3-S1 (Critical): fixed. The patch's `mark_deleted()` on a failed lifecycle step makes the drop's `del()` return at `is_deleted()` (ctrl.rs:1626–1629), so no DEL_DEV goes out (ctrl.rs:1085–1096, :1258, :1301). The three tests can fail without the patch. The errno the tests expect is wrong on the lab's kernel (R4-C1).
- R3-C1: fixed. The approval marks agree with decisions.md in spec, design, ADR and intent.
- R3-S2: partly fixed. Each device gets its own DEL_DEV thread and a single `start`. Requests queued before the shutdown are still served (R4-S1), and the lab's stop never runs outcome 4 (R4-S2).
- R3-S3: fixed. The parse refusal, its message and the `node.sh` rule match #71 contracts.md:38–45, and followups.md names the owner.
- R3-C2: fixed (intent, ADR Consequences; ublk_drv.c:1356–1372, :2740–2744).
- R3-C3: fixed. The handle is disowned, and a pin is closed before the DEL_DEV on a fresh `new_simple`. `delete_closes_pin_before_del_dev` tests the order.
- R3-C4: partly fixed. The new test passes even if the watcher is never wired to exit (R4-C3).
- R3-S4: fixed. `down` stops the unit before `del`, `up` runs `down` first, and outcome 6 says a root `del` while the unit runs is unsupported.
- Code nit (workflow state): fixed (dated). Security nits 1–4: fixed.

### R4-S1 — Medium
**Requests queued before the shutdown are still served, so a compromised child leaks up to 4 devices at exit.**
- Outcome 4 (a) shuts the helper's end down, and (b) waits only for the "current request" (design.md:104–111). Nothing stops the server thread from reading the next message.
- On a SEQPACKET socket, shutdown only stops waiting: `__skb_try_recv_datagram` still dequeues queued messages, and EOF comes only once the queue is empty (net/core/datagram.c:103–105; af_unix.c:2361–2366). Up to `max_dgram_qlen` (10, af_unix.c:965, :3591) requests can be queued.
- Attack: the child queues 4 `add`s, then about 6 more (refused with `device limit` while 4 are owned), and exits. Step (c) empties the table. The server thread then serves the queued `add`s: the limit allows 4 new devices within the 5 s of (d), and nothing deletes them. The child picks ids other than 1, so `node.sh` never deletes them either (R4-S2).
- The new "only while the child lives" (design.md:48–49) is therefore false. `requests_after_child_exit_refused` tests only sends made after the shutdown.

**Fix:** before (a), the main thread sets `closing` under the table lock. The server thread checks it before each request and stops reading. An `add` that completes after `closing` deletes its own device instead of recording it. Step (b) waits for the server thread to stop, bounded, rather than for the current request. Test: `queued_requests_after_child_exit_ignored` (fake `Kernel`: the child queues six `add`s unread and exits; no device remains).

### R4-S2 — Medium
**The lab's stop never runs outcome 4, and `node.sh` deletes only id 1. A compromised child's other devices survive every stop.**
- `node.sh` runs the unit with `systemd-run`'s defaults (node.sh:65): `KillMode=control-group`, so the stop sends SIGTERM to the helper and the child at once.
- The helper installs no SIGTERM handling (design.md:330), so it dies by the signal. That is outcome 5 ("no cleanup runs"), on every `make node-down`, `node-up` and `node-fault`.
- Outcome 5 says the remaining devices "wait for `flaminio-io del --id N`, which `node.sh` runs after stopping the unit" (design.md:120–121). But `node.sh` deletes only `id=1` (node.sh:13, :28–29; design.md:186–188), while the protocol accepts any ID up to 1048575, or −1.
- So the child adds up to 4 devices at other ids and waits for the next stop, and each stop leaks them. Leaked devices hold `ublks_max` slots (64, ublk_drv.c:504). After about 16 stops, every ADD_DEV on the host fails `EACCES` (:2439), the node's own id 1 included, until root deletes them one by one.
- This is R2-S3/R3-S2's leak across restarts on the path the lab actually uses. rustix 1.1.5 offers no safe signal mask (`kernel_sigprocmask` is `unsafe`, and there is no `signalfd`), so a handler is not free under `forbid(unsafe_code)`.

**Fix (one of):**
- The helper accepts only the ids given on its own command line (for example `--id 1`, refusing others and −1 with `error invalid request`), so `node.sh`'s `del` covers outcome 5. Test: `unlisted_id_refused`.
- Or the helper handles SIGTERM through a named safe crate (`signal-hook` or `nix`, checked by cargo-deny): it forwards the signal to the child and runs outcome 4. The unit sets `KillMode=mixed`. Test: kernel `sigterm_deletes_owned` (two owned devices at ids 2 and 3: both gone, exit 143).
- Or state the leak in outcome 5 and Risks with a followups.md entry, and correct "which `node.sh` runs".

### R4-C1 — Low
**On the lab's kernels a taken id fails with `EOPNOTSUPP`, not `EEXIST`.**
- libublk retries every failed control command whose legacy opcode is ≤ 0x12 with that legacy opcode (ctrl.rs:1500–1520, :1571–1582). Ubuntu builds without `CONFIG_BLKDEV_UBLK_LEGACY_OPCODES` (kernel-noble debian.master/config/annotations:2199; this workstation's 6.8.0-138 config). The retry then returns `-EOPNOTSUPP` (ublk_drv.c:1712–1714), and that is the error reported.
- The lab VMs run Ubuntu 26.04 (docs/lab.md:17). So the helper answers `error kernel add: EOPNOTSUPP`, and a test that asserts `EEXIST` (outcome 1 at design.md:90, `pinned_id_not_reused` at :127–128) fails in `make privsep-check`.

**Fix:** either the vendored patch skips the legacy retry, or reports the first attempt's errno; or outcome 1 and the tests check that the add fails and the device survives, not the errno name.

### R4-C2 — Low
**spec S3 "A failed add deletes no device" contradicts outcome 1.** Outcome 1 sends DEL_DEV for the device that the failed add itself created (design.md:91–92), and its test is named `failed_add_deletes_its_device` (:93). The ADR says the same thing as the spec: "a failed add deletes nothing" (adr-proposal.md:36). Read literally, the spec rule makes an implementer leak the half-added device.
**Fix:** "A failed add deletes no device but the one it added" in spec.md:43 and adr-proposal.md:36.

### R4-C3 — Low
**The watcher's exit is still untested (R3-C4).**
- In `exits_when_the_helper_goes` (design.md:122), the test never answers `disk`, so the process is blocked in its first call when the other end closes. It then exits through the "EOF in any call" path (:148), which also prints `the helper is gone`.
- `helper_hangup_is_seen` checks only that the watcher function returns. If the watcher's return is never turned into an exit, both tests still pass. That missing exit is exactly the steady-state case S5 covers: the helper dies while no call is in flight.

**Fix:** the test answers `disk` and `control` (a temporary file and a bound listener) and closes once the control socket accepts a connection. Or give the watcher an exit hook, and have `helper_hangup_is_seen` assert that the hook ran.

### R4-C4 — Low
**`exits_with_its_child_while_blocked` leaves about 1 s of margin.**
- In that test START_DEV never ends, so (b) waits its full 4 s.
- The DEL_DEV in (c) then waits for START_DEV's device reference (ublk_drv.c:2549–2567), which is released only when the helper exits, so (d) also runs its full 5 s.
- The helper exits about 9 s after the child's death, against the test's 10 s bound (design.md:113–115). A loaded VM makes the test flaky.

**Fix:** raise the bound to 15 s, or state the worst case as (b) + (d) and derive the bound from it.

### R4-C5 — Low
**The patch also removes `serve`'s self-healing, and only `node.sh` was compensated.**
- Today a crashed `serve` leaves device 0 behind. The next `make ublk-up` fails, but its failed add deletes the leftover, so a retry works.
- With the patch, `ublk.sh up` (ublk.sh:14–31, `--id 0`, no leftover deletion) fails every time until `make ublk-down`.
- Contracts mentions only `node.sh` (design.md:246–247), and there is no test.

**Fix:** have `ublk.sh up` delete a leftover `/dev/ublkc$id` when its unit is inactive, as `node.sh` does, and add a `node-scripts-test.sh` case; or state the change in docs/lab.md.

## Nits
1. design.md:304: "the impersonation #71 closed for its observer". #71 closed it for its Prometheus metadata job (`meta.flaminio.invalid`, 71/design.md:268–276), not for the observer.

Reviewed commit: f9732b922b4e8822b355884c9b4f4155fa6790f3 (diff 7245c69..2040d58; HEAD adds only report.md)
