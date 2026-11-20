Reviewed commit: 324191e (worktree HEAD; the diff 3d16fc1..958b9f2 is the round-13 fix, 324191e only adds the report)

Round-13 findings
- R13-L1 (clock built before Node): fixed. Arc<Condvar>, cfg(test) accessor, OnceLock slot filled after Node is built. An advance before the slot is filled is a missed wake, bounded by the 100 ms slice; the test fills the slot before the waiters start, so no defect.
- R13-L2 (main thread wait vs clock): fixed. Only the 30-s wait reads Clock; the main wait stays on METADATA_LIMIT (ublk.rs:858 verified).
- R13-L3 (Attached users): fixed in substance. ublk.rs lines (:682,:722,:749,:752,:838,:939) and control.rs (:72,:192) verified. The #15 citations (:14-18, :61, :71, :79-89, :226-233, :365-374, :726) match #15's design.md. One Low remains below (R14-L2).

## R14-L1 - Low: two 30-s real-time test cases; make the limit a parameter
design.md Tests (~:707-711), decisions.md 85. The two time-out cases wait out METADATA_LIMIT on real time: 30 s each (60 s if serial) in `make test`, which every contributor and CI runs. The main thread's wait is a timeout on the entry's waiter queue (#15 :79-89), so it needs no fake clock, only a shorter limit.
Fix: carry the limit as a Node field (default METADATA_LIMIT, set from NodeConfig, or a cfg(test) constructor argument), and let the test use ~200 ms. Production is unchanged. If the cases stay at 30 s, mark them so a quick run can skip them.

## R14-L2 - Low: the startup retry's path through S12 is wrong or unspecified
design.md :224-226. It says a maintenance entry "does not stop the retry, whose attach goes through S12". The startup attach and its retries are sent to the main thread over ctl_tx (ublk.rs:745-767), not through control.rs's socket thread. But S12's "end the maintenance entry, wait up to 30 s, then reserve" is the client's socket thread (design.md table :287, :290), and the design says it must not run on the main thread (blocks attaches for 30 s). So with a maintenance entry present, the startup attach's behaviour is not defined: it may end the maintenance target from the main thread, block it, or be refused.
Fix: state it. The simplest: the main thread's device attach refuses ("volume is in maintenance") while the volume has a maintenance entry. The retry, which does not stop on a maintenance entry, then retries every 5 s up to STARTUP_ATTACH_LIMIT. Add that case to the test list (startup attach refused during maintenance, then retried; retry does not stop on a maintenance entry).

## Security
No new finding. The OnceLock and accessor are cfg(test); the production surface is unchanged.
