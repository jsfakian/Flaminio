# Issue #74, plan review round 6 (incremental), design-code and design-security
Diff 0564254..b1913c0 in /tmp/flaminio-plans-2026-11-12/74.

## Round-5 findings
- R5-B1: fixed in the text (design.md:112-116), but the test does not cover it (finding 1 below).
- R5-C1: fixed. ublk.rs:682 is `Attached` (an Option); Risks, ADR Consequences and followups step are present.
- R5-C2: fixed. /sys/class/ublk-char/ublkc<ID> is right for 6.8: class name "ublk-char" (ublk_drv.c:488), device name "ublkc%d" (:2177-2181). The directory exists only while the module is loaded; absent is the correct "free".
- R5-C3: fixed. Bound 1048576 is right: UBLK_MAX_UBLKS = 1<<MINORBITS = 1048576 (ublk_drv.c:49, :503), the kernel refuses dev_id >= that (:2426).
- Nit: fixed.

## Findings
### 1. Low: no test can reach the window the new post-recvmsg check closes
design.md:124-125: `queued_requests_after_child_exit_ignored` is described as "the child queues six adds unread and exits". That covers requests queued before `closing`, which the pre-read check already stops, not a request dequeued between the check and the `recvmsg` return (or while blocked in `recvmsg`). The test passes without the new check, so the R5-B1 rule has no test that can fail without it. The same bytes also race: if the server thread is running, it may serve some of the six before `closing` is set.
Fix: have the server loop take its receive as an injectable step (fake in `serve.rs`), and test that a receive which sets `closing` before returning a valid `add` leaves no kernel call and no reply. Name that test in the design and the table, and make the six-queued case start with the server paused or wait on a latch.

### 2. Nit: parse test boundaries
`refuses_a_missing_or_bad_id` refuses -1, 1048576 and a missing id; add that 0 and 1048575 are accepted, so an off-by-one the other way is caught.
