## Earlier findings
- R11-B1: fixed in substance. Node, serve_node (main thread) and the router (#15 Node::route, ublk.rs) are all in ublk.rs, so pub(super) plus Node wrappers compiles for them. No router or main-thread code lives in node.rs or control.rs. How control.rs obtains a Node is open (R12-B2).
- R11-B2: partly fixed. The loop is stated (state, clock, wait_timeout slice <=100 ms, notify_all). The clock itself is not defined (R12-B1).
- R11-B3: fixed (both prefixes, Unavailable and DeadlineExceeded, match design.md:140-147 and :293-296).

### R12-B1 - Low: the "node's injected clock" does not exist, and the fake clock's link to the registry's condvar is unspecified
design.md:234-237, :690-697. Today flaminio-io reads Instant::now() directly (ublk.rs:583,739,802; control.rs:130,205). #15's design has no clock on Node. #17 never says which type is injected, who owns it (Node or Registry), or how its advance() reaches the condvar (an Arc<Condvar> in the clock, or a test hook). It also does not say what the production clock is (real Instant). The 100 ms slice makes the test terminate even with no wake, so this is not a hang. But the sentence "the fake clock's advance calls notify_all" cannot be built as written.
Fix: add to Contracts a small clock trait or struct on Node (production: Instant::now; test: a manual clock holding the registry's Arc<Condvar>), and say Node::wait_* take the deadline from it. Or drop the advance notify and say the test relies on the 100 ms slice.

### R12-B2 - Low: how control.rs's threads obtain a Node is not said
design.md:196-206, :537. control.rs takes mpsc::Sender<Control> today (control.rs:41,69,118) and has no Node. The design now has its threads call Node's pub(crate) wrappers (root's maintenance end, client attach's wait, name lookups), and node.rs (--max-maintenance) must hand the limit to Node. #15's design builds Node in serve_node and does not pass it to the socket. Also the wait's condvar must pair with the Mutex<Registry> that #15 keeps inside Node, so the wait is a Node method, not a registry one. The design lists "the 30-s wait" among registry pub(super) methods (:199), which cannot take the guard from outside.
Fix: state that the control server gets an Arc<Node> (or a narrow handle) next to ctl, and that the wait is Node's method locking the registry and using its condvar, with the registry's own methods being the state reads it calls.

Reviewed commit: 62a3ee97014061a377c02e06ef2a3df1217bef0e
