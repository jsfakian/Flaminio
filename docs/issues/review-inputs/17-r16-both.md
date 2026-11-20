## Earlier findings
- R15-C1: fixed in substance (named `startup_attempt`, `Err(answer)` feeds the loop, `done()` re-checked after the wait, interval a parameter, the three tests listed). One Low remains in the same area (R16-C1).

### R16-C1 — Low: the first attempt's hand-off to the retry thread and the concurrent test's outcomes are underspecified
design.md "startup `--attach` is a client attach" (:229-250) and the Tests case (:739-751); ublk.rs:745-767.
1. The first attempt now runs in `serve_node` before `control::spawn` and returns `Result<Receiver<String>, String>`, but the spawned thread's loop starts from `answer.recv()`. The design does not say how the first attempt's `Err(answer)` reaches the thread (the loop must start from the Result, not a Receiver, else a failed first attempt is lost, logged by nobody and never retried).
2. The concurrent test is racy as written. Whichever of the startup thread and the socket client reserves first wins; the loser's reserve is refused with #15's "already attaching" answer. That is consistent with S5 as narrowed (a client-vs-client conflict, not a maintenance refusal), but the test says only "the other is refused", and for the startup thread "returns Err and stops at done()": if the socket client wins, the device entry may not exist yet when the startup `Err` returns, so the retry stops at the `done()` after the sleep, not immediately. The test needs the cfg(test) waiter count (already used at :771) to line both up, and must assert both orders (socket first, startup first) and the exact answer text.
Fix: say the loop starts from the first attempt's `Result` (an `Err` is logged and goes down the same `done()`/sleep path); in the test, name the refusal as #15's "already attaching", run both winners, and assert the startup retry ends at the `done()` after the interval.

## Security
No new finding. `startup_attempt` uses the socket's checks; the wait is bounded by 30 s and the deadline, and retries by `STARTUP_ATTACH_LIMIT`.

Reviewed commit: 98bfbe286165ab3f29542e27716bd5759398a5ad
