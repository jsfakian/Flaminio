# Issue #74, plan review round 3, design-code (reviewed 7245c69)

## Round-2 findings
All nine fixed in the plan text; verified against the 6.8.12 tree (/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble; the eve-kernel under ~/Documents is 6.1):
- R2-C1: START_DEV waits without ublk_ctl_mutex (ublk_drv.c:2262), so the cleanup DEL_DEV on a fresh handle is not blocked; exit_group kills the io-wq worker. Fixed; test named.
- R2-C2: STOP_DEV on a DEAD (never started) device skips del_gendisk but still runs ublk_cancel_dev. Fixed.
- R2-C3..C9 fixed. SIZE cap is right (set_capacity at :550, `(loff_t)sectors << 9`, no check in ublk_validate_params). EEXIST for a held id is right (ublk_alloc_dev_number maps ENOSPC to EEXIST). An open /dev/ublkcN pins the id (ublk_cdev_rel frees it, :1443). The 6.8 START_DEV does not check the pid, so `start_uses_the_child_pid` is the only guard. std setgroups(0) EPERM-ignore, START_LIMIT (ublk.rs:703), lab.sh flag list, bench-reads.sh:99 and node.sh ATTACH all match.

## New findings

### R3-C1 - Medium: the approval left the plan contradicting itself
- decisions.md:5-6 still says "None yet. Nothing in this plan is approved ... stays Proposed, and spec Q1-Q4 are open", directly above the approval lines (:8-13).
- adr-proposal.md still says "Decision proposed for review" (:27), "Root parent ... (proposed)" (:48), "Accepted only if approved (spec Q1-Q4)" (:68) and "Nothing here is approved or run" (:78). Its Status line (:3-5) repeats "numbered and moved into docs/adr/ ..." twice.
- design.md:296 "New, Proposed".
- spec.md "Open questions" Q2 still asks whether to upstream the library; decisions.md says the vendored copy is settled and only the maintainer is open. Q1, Q3, Q4 stay open in decisions.md though the brief says every question got its recommended answer; the ADR line "Accepted only if approved" cannot hold for them.
Fix: delete the "None yet" paragraph; retitle the ADR sections ("Decision", "Accepted risks: Q1, Q3, Q4 open, Q2 settled"); drop ":68 only if approved" and ":78 Nothing here is approved"; design.md:296 "New, Accepted with the plan"; mark Q2 in spec as settled/maintainer open.

### R3-C2 - Low: the pin blocks #48's recovery and the plan no longer says so
The round-2 text closed the helper's copy because recovery needs UB_STATE_OPEN clear (ublk_drv.c:2730-2744, `ublk_ch_open` is exclusive, :1356). The pin is the same open file passed with SCM_RIGHTS, so while the helper holds it, the dead server's file is not released and START_USER_RECOVERY returns EBUSY and a new server's open fails. The ADR says only that the helper's lifetime must change for #48.
Fix: name the pin in the ADR Consequences and intent's #48 dependency (#48 must close or hand over the pin before recovery); no test needed now.

### R3-C3 - Low: libublk handle Drop deletes the device
libublk 0.4.8 `Drop for UblkCtrlInner` (ctrl.rs:1085-1096) calls DEL_DEV for a handle that added a device unless disowned. The table holds "libublk handle" per entry. Any drop of that handle (outcome 1 error path, a panic, forgetting the entry, the cleanup taking entries) issues DEL_DEV, possibly with the pin still open, which then waits for the id and blocks the server thread (or the 5 s cleanup). The design does not say the handle is disowned or that pin-close always precedes its drop.
Fix: state "the entry's handle is dropped only after its pin is closed, and cleanup uses fresh `new_simple` handles; the entry's handle is disowned (`disown`, ctrl.rs:2790) or dropped last". Test in serve.rs: `delete_closes_pin_before_del_dev` (the fake Kernel records order).

### R3-C4 - Low: outcome 5 and the watcher are barely tested
S5 says the I/O process exits when the helper's socket closes. `helper_hangup_is_seen` checks only that the watcher returns; nothing tests that the process exits 1 with "the helper is gone", and no kernel test kills the helper with SIGKILL and sees the child go (the lab check from R2-C8's alternative was not adopted). `foreign_device_refused` is listed for outcome 5 but exercises ownership, not the death.
Fix: a tests/kernel.rs test `child_exits_when_helper_is_killed` (SIGKILL the helper; the child's pid is gone within 2 s).

## Nits
- decisions.md:13 "Approved before its plan review cleared (status: addressing_round_2)" is workflow state, not a decision; keep it out of the plan or date it.
