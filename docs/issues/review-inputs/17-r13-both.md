Reviewed commit: 3d16fc15fbfbe7973d44a5c535c636d0775cf184 (diff 62a3ee9..c19d1d4)

R12-B1 (clock): fixed in substance; two Lows remain below (R13-L1, R13-L2).
R12-B2 (control.rs gets Node, wait as a Node method): fixed in substance; one Low remains (R13-L3).

## R13-L1 - Low: the fake clock and Node's condvar are built in the wrong order
design.md "The clock" (~:240-246), Contracts. Node is built holding an Arc<dyn Clock>, and the fake clock's advance needs a clone of Node's condvar, "taken through a cfg(test) accessor". The clock exists before the Node, so it cannot take the clone at construction. Also, "a clone of which" needs the condvar to be an Arc<Condvar>, and Contracts says only "a Condvar".
Fix: say Node's condvar is an Arc<Condvar>, and the fake clock holds a set-after-build slot (Mutex<Option<Arc<Condvar>>> or OnceLock) that the test fills from the accessor. Or the fake could be built from the condvar first and Node given it by a cfg(test) constructor.

## R13-L2 - Low: "only the maintenance waits read the clock" splits the main thread's wait from the test clock
design.md "The clock" says the main thread's wait for a maintenance start reads the clock. That wait is on the entry's waiter queue with METADATA_LIMIT, as #15's (:249), not on Node's condvar, and today's code reads Instant::now() for it (ublk.rs:858). A fake clock's advance wakes only the condvar, so that wait either runs on real time (and the "only the maintenance waits" sentence is wrong) or has no wake on advance. No test needs it today.
Fix: drop the main thread's wait from the clock's readers, or say its deadline stays on Instant::now().

## R13-L3 - Low: the Attached replacement is not traced through today's other users
design.md :208-212, Files touched. Attached (ublk.rs:682) is also made in serve_node (:722), read by its startup wait (:752), and passed to the attach and detach helpers (:838, :939) and to control.rs's detach (:72, :192). The design says "#15's registry replaces it" and control::spawn takes an Arc<Node>, but not that Node must exist before control::spawn and before the startup attach wait (:751), nor what replaces the :752 check. This is #15's rewrite, but #17 now depends on the order.
Fix: one line: Node is built first in serve_node and handed to spawn and to the startup wait; or say that #15's design already covers these users.

No new security finding. The wait holds the registry lock only inside wait_timeout (released while blocked), the removal and notify_all happen under the one lock, and the 100 ms slice bounds a missed wake (an advance's notify_all without the lock can fall between the check and the wait).
