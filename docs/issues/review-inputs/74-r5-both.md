# Issue #74, plan review round 5 (incremental), design-code and design-security

Scope: `git diff 2040d58..0564254 -- docs/issues/74/ ':!docs/issues/74/report.md'` in
`/tmp/flaminio-plans-2026-11-12/74`. HEAD 72c2c84 adds only report.md. Checked against libublk 0.4.8
(`ctrl.rs`), the 6.8.12 tree (`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`), this host's
`/boot/config-6.8.0-138-generic` (`# CONFIG_BLKDEV_UBLK_LEGACY_OPCODES is not set`, line 2702), the
worktree's `scripts/guest/{node,ublk}.sh`, `flaminio-io` and #71 at 20af551.

## Earlier findings
- R4-S1 (Medium): fixed in effect. One id caps any leak at `/dev/ublkc<ID>`, which `node.sh` deletes, and a late `add` deletes its own device. The `closing` check still leaves a window (R5-B1).
- R4-S2 (Medium): fixed. The helper adds only at its own `--id` and the request carries no id (design.md:77–79, :247), so a stop can leave only `/dev/ublkc<ID>`. `node.sh down` deletes it after the stop. In the lab only `node.sh` uses id 1 (`ublk.sh` uses 0), so `del --id 1` reaches no other process's device.
- R4-C1 (Low): fixed. `ublk_ctrl_need_retry` (ctrl.rs:1500–1520) and `ublk_check_cmd_op` (ublk_drv.c:1709–1718) confirm it; the tests no longer check the errno.
- R4-C2 (Low): fixed (spec.md:43–44, adr-proposal.md:36–37).
- R4-C3 (Low): fixed. `build` reads `token.pub` after the format and `Store::open`, and before any other helper call (node.rs:255–284). With the FIFO the node blocks in `open`, so only the watcher's hook can end the test.
- R4-C4 (Low): fixed. START_DEV waits before taking `ub->mutex` (ublk_drv.c:2262), and DEL_DEV waits for the id to be freed (:2565), so 9 s holds against the 15 s bound.
- R4-C5 (Low): fixed (design.md:205–208, S7 row).
- Nit (#71's observer): fixed. It matches #71 contracts.md:40–43 and :168.

### R5-B1 — Low
**`closing` is checked before the read, so a request read after it is set is still served. "Never served" (decisions.md:176) and "get nothing served" (design.md:113–116) are false.**
- The server thread checks `closing`, then calls `recvmsg`. The main thread can set `closing` and shut the socket down between those two steps. `recvmsg` still dequeues a queued message after the shutdown (`__unix_dgram_recvmsg`; EOF comes only on an empty queue, af_unix.c:2361–2366). That message is then served.
- The same happens when the server thread waits in `recvmsg` on an empty queue and a grandchild that kept stdin sends just before the shutdown.
- With one id the harm is small. A late `add` makes an ADD_DEV, then deletes its own device. A late `start` holds the server thread for (b)'s 4 s. Neither leaks a device.
- The property the design states does not hold, and `queued_requests_after_child_exit_ignored` cannot exercise the window. "Only while the child lives" (design.md:48–49) has the same gap.

**Fix:** check `closing` under the table lock after each `recvmsg` returns, and drop the request unanswered when it is set. Keep the check at record time for an `add` that was already running. Or reword: at most the request read when `closing` was set is served, and a late `add` deletes its device.

### R5-C1 — Low
**The plan does not say what one device per helper gives up: several volumes per node (followups item 5, #15).**
- One device meets step 1 for today's node, which attaches one volume at a time (`Attached` is an `Option`, ublk.rs:682; phase1-plan.md:328).
- The ADR now fixes "the device it owns: at most one, at the id its own command line names" (adr-proposal.md:34–37). That ADR is binding once accepted.
- Followups item 5 lists its steps (followups.md:127–140) but has no helper step. #15 would meet a binding ADR and a protocol with no id field.

**Fix:** one sentence in the ADR's Consequences and in the design's Risks: the helper owns one device because the node attaches one volume at a time; several volumes need one owned id per volume, which changes this ADR. Add that step to followups item 5 (Docs).

### R5-C2 — Low
**The kernel tests now need fixed ids, and the plan names none.**
- Every `tests/kernel.rs` case starts a helper, and `--id` is now required (Contracts, design.md:235–236). Before this round a test could ask for −1.
- `cargo test` runs tests on parallel threads. Two cases with the same id collide: the second `add` fails, and `foreign_device_survives_add_of_its_id` or `pinned_id_not_reused` fail spuriously.
- `privsep-check` runs with the lab node up (design.md:298). The restore must not touch `/dev/ublkb1` (design.md:303), and `ublk.sh` holds 0. So the tests must avoid ids 0 and 1, and the plan does not say so.

**Fix:** in Lab or Tests, each kernel test uses its own id from a range the lab does not use (for example 100 + its index), checked free in `/sys/class/ublk-char` at start. Or run them with `--test-threads=1`.

### R5-C3 — Low
**The required, bounded `--id` has no test. The one-device argument rests on it.**
- Contracts make `--id` required, 0–1048575 (design.md:235–236). "The only device it can ever have created is `/dev/ublkc<ID>`" (design.md:78–79) and outcome 5 depend on that.
- S1's parse tests are `refuses_uid_or_gid_zero`, `refuses_unless_root` and `needs_a_program` (design.md:277), none of them for `--id`.
- `add_uses_the_configured_id` passes even if a missing `--id` fell back to −1, the I/O process's own default (node.rs:136, "first free"). A kernel-picked id is one that `node.sh` never deletes: R4-S2 again.

**Fix:** `flaminio-helper/src/main.rs`: `refuses_a_missing_or_bad_id`, covering an absent `--id`, `-1` and `1048576`.

## Nits
1. spec.md:21–23: "Through the helper it can add one bounded device … and start, stop and delete it" can be read as one device per helper lifetime, which would break detach and re-attach. S3 and design.md:78 mean one at a time: say "at most one device at a time".

Reviewed commit: 72c2c84591d3051efb86b27c460b0150e64f9f59 (diff 2040d58..0564254; HEAD adds only report.md)
