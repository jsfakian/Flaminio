## Earlier findings
- R14-L1: fixed. `Node.start_limit` (METADATA_LIMIT in production, cfg(test) constructor argument); the two time-out cases use ~200 ms; the 30-s cases left use the fake clock.
- R14-L2: fixed in substance (S12 is run by the startup thread through the Node methods; main thread not blocked; two runners converge as two socket clients do: one DetachMaintenance, both wait, the second meets the first's reservation and is refused, and for the startup thread the retry covers that). One Low remains.

### R15-C1 — Low: the startup thread's S12 step has no defined place in the retry loop, and no test seam
design.md :226-236, Tests (the new "startup attach over a maintenance entry" case); ublk.rs:745-767.
Today's loop is `answer.recv()` (the main thread's reply), then `done()`, sleep 5 s, `done()`, then build and send the next request. The plan puts S12's token/epoch check, DetachMaintenance, the up-to-30-s wait and the #15 reserve on the startup thread before `ctl.send`, and says "a wait that runs out fails that attempt". But such an attempt produces no reply on `answer`, so the loop's shape (what it recv()s on, what it logs, how the failure reaches `done()`/the next sleep) is unspecified. Also `done()` is not re-checked after the up-to-30-s wait, so an attempt can go on to reserve after the node attached the volume elsewhere (refused by the reserve, harmless but it logs a misleading answer). The test also cannot be written as stated: the retry is a closure inside `ublk.rs`'s run with a fixed 5-s sleep, and no named function or injectable interval lets a test drive it, observe the main thread's channel and assert it is never blocked. The case "startup retry and a socket client at once over one entry" is not named either.
Fix: say the attempt is a function `startup_attempt(&Node, volume, token, deadline) -> Result<Receiver<String>, String>` (Err is the failed attempt's answer, logged like an answer, and feeds the same `done()`/sleep path); re-check `done()` after the wait; take the 5-s interval as a parameter so the test uses ms; add the startup-plus-socket concurrent case (one DetachMaintenance, one reserve, the other refused or retried) to the test list.

## Security
No new finding. The startup thread uses the same token check and epoch rule as the socket; a refused attempt is bounded by STARTUP_ATTACH_LIMIT and the 30-s wait.

Reviewed commit: a09eb3404043020219b263ccfa1f28dbf8243b2c (the reviewed diff is 324191e..85c678e; a09eb34 only adds report.md)
