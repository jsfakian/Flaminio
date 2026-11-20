# Plan review: issue #15, round 5 (incremental), design-code and design-security
Reviewed: git diff baee5c8..95a1dba -- docs/issues/15/ (no report.md).

## Round-4 findings
- R4-B1 (`down` per mode): fixed. design.md S10 Scripts, Order and Tests agree. #74 design.md:139-142, :209-210, :290 still say `del` in both modes; the plan has the second plan to land adjust them. The change is not reflected in #74 itself, which is expected.
- R4-B2 (id ceiling): fixed in substance (no wrap, ceiling at 1048575, refusal text, fake-kernel test). Opens R5-1 and R5-2.
- R4-B3 (`--privsep` failure order): fixed. Answer, Detach, purge, counter, end of entry, then `delete`; test with a blocking `delete`.
- R4-B4 (6.8 case): test added, risk stated, not accepted. Opens R5-3.
- R4-N1: fixed in spec, design and contract (revision 5). Opens R5-4 (Nit).

## Findings

### R5-1 - Low
**"At most 40 ids skipped per attach" contradicts "a held id moves to the next at once".**
design.md:104-112 and :690-692 skip a held id immediately, with no 250 ms wait. Only the retry of an unheld id is paced. So the 40 bound (deadline / pace) holds for unheld retries only. Held ids can be skipped faster than 40 in one deadline. An implementer will test the wrong bound, or add a pace to held skips that the S2 test ("moves to the next at once") forbids. The ceiling test relies on the same loop.
Fix: say the 40 bounds only the unheld tries, and that held skips are bounded by the deadline and the ceiling. Or pace every add and drop "at once".

### R5-2 - Low
**The ceiling test does not cover the edges.**
design.md:696-698 tests "counter at 1048575 and that add failing". Not covered: a held skip at 1048575 (which moves to 1048576 inside one attach, so the first refusal's text and code must be the ceiling's, not the last add error); `first_id` itself above 1048575 at start (a recorded or present id 1048575 gives 1048576); and the attach where the add at 1048575 succeeds. The spec (S2, "refused, with its reason") does not name the error code either.
Fix: add those cases to the test, and name the code in the spec (S2: for example `Unavailable` or `ResourceExhausted`).

### R5-3 - Low
**The 6.8 test is the plan's only check for a possible rule violation, and there is no fallback if it fails.**
design.md Risks (:932-939) and the 6.8 test (:722-724). If 6.6-6.8 hold `ublk_ctl_mutex` across `DEL_DEV`'s wait (true in the 6.1 source: ublk_drv.c:1758-1781, with `ADD_DEV` at :1673 on `mutex_lock_killable`), the device thread blocks inside `ADD_DEV`. `add_device`'s deadline and the 250 ms pace cannot run, so the attach gets no answer and the S2 deadline fails, on the supported kernels. The test then fails, but the design names no change for that outcome, and the risk is "not accepted", so the plan ends in an unresolved state.
Fix: state what follows from a failing test (for example the attach's `ADD_DEV` moves to a thread the deadline can abandon, or the delete is deferred until the device's last close, or a decision for the user), or make the 6.8 check a gate before approval. The "Risks" text also says "#74 read 6.8.12's wait as one an exit can cut short"; say whether that already settles `ADD_DEV`'s wait.

### R5-4 - Nit
**Contract revision 5 is named by commit message, not by hash.** device-lifetime-contract.md:10-16: "#79 must pin revision 5", but the commit is "Address the review of the design of #15, round 4", and #79 can pin only a hash. Round 3 fixed the same gap with `e46dd6b`; fill the hash for revision 5 once it exists.

Not findings (checked): the 6.1 source citations for `GET_DEV_INFO`, the release and the mutex are accurate; spec, design and contract agree on the changed-reply wording; the kernel-global-lock risk is stated consistently (design Risks, decisions R4-B4, 6.8 test, "not accepted"). The orphaned `node-scripts-test.sh` bullet in design.md Order (:612-614) is set after a blank line, outside its list; format only.
