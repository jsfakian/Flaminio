# Plan review: issue #15, round 4 (incremental), design-code and design-security

Scope: git diff 28711dd..e46dd6b -- docs/issues/15/ (without report.md), and what it depends on.

## Earlier findings
- R3-B1: fixed. The walk terminates (10 s deadline, 250 ms pace, counter only rises). It cannot take a held id: the kernel refuses a duplicate add, and a held id is skipped. `GET_DEV_INFO` answers for a deleted, still-open device: the id is freed when the disk's last close drops the `cdev_dev` reference (taken at START_DEV), and the idr lookup checks no state. A libublk handle opened without `for_add` does not delete on drop (`ctrl.rs:1089`). Opens R4-B2, R4-B4.
- R3-C1: fixed for #74's order; scope of `down` opens R4-B1.
- R3-C2: fixed.
- R3-C3: fixed for the normal path; the failure path opens R4-B3.
- R3-N1, R3-N3: fixed. R3-N2: fixed in design, spec and contract (one wording Nit below).

### R4-B1 — Low
**`node.sh down`: the new text and test do not say direct mode only, and they collide with #74's assertion in the shared file.**
design.md S10 Scripts (about :482) and Tests (:854) have `down` run `flaminio-io cleanup` after the stop, with no mode. The records and the flock are direct mode's only (:561), and "Order" says cleanup replaces #74's delete "in direct mode" (:603). Under `PRIVSEP=1` the cleanup finds nothing, so #74's `del /dev/ublkc<ID>` must remain (#74 design.md:139-142, outcome 5). #74's test asserts "`down` deletes `/dev/ublkc<ID>` after the stop" and "stops the unit before `del`" (#74 design.md:142, :290). #15's "as #74's own case asserts" is inexact: #74's case names `del`, #15's names `cleanup`.
Fix: say the Scripts bullet and the test are per mode: direct mode stops then `cleanup`; `PRIVSEP=1` stops then `del`. Have the second plan to land adjust #74's direct-mode assertion, and keep a `PRIVSEP=1` case for `del`.

### R4-B2 — Low
**A failure unrelated to the id now burns ids with no ceiling, and no rule covers the kernel's id range.**
design.md:103-113: an unheld id is skipped after 4 tries, so `ublk_drv` missing, `ublks_max` reached or `ENOMEM` burn up to 4 ids per second, about 40 per failed attach, and the counter never goes back until a restart. The kernel's ublk minor space is 2^20 (`UBLK_MAX_UBLKS`); the record grammar allows 7 digits (design.md:224). A node that fails persistently (kubelet and the CSI retry) can pass the kernel's range, after which every attach fails (`EINVAL` or the like) until a restart. Before this round a failure burned nothing. No rule or test covers the ceiling.
Fix: bound it, for example when the counter passes the kernel's limit, or a failure the walk cannot tell from a held id, wrap to `first_id` computed afresh (present and recorded ids), or reuse an unheld id after a pause. Or lower the burn (one id per attach: skip an unheld id only on the attach's last try). Test the ceiling on the fake kernel.

### R4-B3 — Low
**The `--privsep` failure path still ends the entry after an unbounded `delete`.**
design.md:587-589: "A failure after `add` is #74's outcome 2 (`stop`, join, `delete`), then the entry ends." R3-C3 moved the entry's end before `delete` for a normal stop, because `delete` waits for the last close with no bound. In the failure path the attach stays unanswered and the entry stays starting until `delete` returns. The device was never published, but udev or blkid can hold it, and the S2 rule "answer the attach, send `Detach`, remove the record, end the entry" (:209-210) puts the answer first.
Fix: in the failure path too: answer the attach, purge the router, remove the counter and end the entry, then `delete`. The blocking-`delete` test (:843-846) covers it.

### R4-B4 — Low
**The load-bearing claim ("`GET_DEV_INFO` answers for a deleted, open device") is tested only on the lab's 7.0.**
design.md:696-698 uses `DEL_DEV_ASYNC`. On the supported 6.6-6.8 (ADR 0003; Centaur's 6.8 runs the disowned path) `DEL_DEV` is synchronous and blocks until the last close; the same walk matters there, and the fake kernel cannot prove the claim.
Fix: add the 6.8 case: the test's `DEL_DEV` on its own thread with `/dev/ublkb<N>` held open; the attach takes N + 1 within its deadline.

## Nit
- spec.md:142-145, design.md (step 4 of the device check) and contract: "answered as that state is when first seen" cannot hold for "none" (first seen: an attach, not `Unavailable`) or "another lifetime" (not a step 1 state). Say "the state's code: stopping `FailedPrecondition`; none, starting or another lifetime `Unavailable`", without "when first seen".

Not findings (checked): the `--privsep` order of steps 7 and 8 holds. Under `--privsep` there is one volume and the helper owns one id. The helper answers one request at a time (#74 design.md:249). A re-attach's `add` waits behind `delete`, and the caller's retry finds the starting entry through `status`. Contract revision 4 matches spec and design on codes.

Reviewed commit: baee5c8a1d498579e308e35c05d6bc31b4f7f7d8
