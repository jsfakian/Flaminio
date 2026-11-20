## Earlier findings
- R10-1: not fixed as written. `mod registry` is the right shape, but `pub(super)` is too narrow (R11-B1).
- R10-2: fixed in the text (own mid-wait string, `DeadlineExceeded` by prefix). The files list lacks the node.go mapping (R11-B3).
- R10-3: fixed (log-line counting dropped, `EndMaintenance` calls and `Engine::maintenance()` used).
- R10-4: partly fixed. The condvar wait, the barrier and the router's own removal are named. The fake-clock wiring is open (R11-B2).
- R10-5: fixed (own channel, receiver kept).

### R11-B1 - Medium: `pub(super)` on `ublk::registry` items reaches only `ublk`, not `control.rs` or `node.rs`
design.md:196-204, :512-516, :435-437, :816-820. The module tree (main.rs:10-21) has `cli`, `control`, `node`, `proto`, `remote`, `replica`, `ublk`, `volume` as siblings under the crate root. `mod registry` inside `ublk.rs` makes `pub(super)` mean "visible in `ublk`". The main thread (`serve_node`, ublk.rs) and the router are in `ublk`, so they are fine. But the design says "the main thread, the router and the socket's code reach entries only through those methods", and S9/S12 have root's `maintenance end` and the client-attach waits running on the socket's threads in `control.rs`. `control.rs` cannot name `ublk::registry`'s `pub(super)` items. It compiles only if `ublk` re-exports them or wraps them in `Node` methods, and neither is said. The "compiler enforces it" claim also leans on the choice of visibility.
Fix: say that `control.rs` reaches the registry only through `Node`'s own `pub(crate)` methods (the wait, the reserve, the lookups), which call the `pub(super)` registry methods. Or make the registry's methods `pub(crate)` and keep the private fields and constructor. Name the choice in the files list.

### R11-B2 - Low: the fake clock's wake of the condvar is not wired
design.md:668-677. "The fake clock's advance notifies that condvar": the clock is injected and has no reference to the registry's condvar. A `Condvar::wait_timeout` also takes a real duration, not a deadline on the injected clock. The design does not say that the wait is a loop (read state, read clock, `wait_timeout(real remaining)`). Under a fake clock that never wakes it, the 30-s case at :665 would hang or end on real time.
Fix: state the loop, and that the node's clock type has a `cfg(test)` hook (a callback or an `Arc<Condvar>`) that `advance` calls, or that the test calls `notify_all` itself after `advance`. Take the wait's real timeout from the injected clock's remaining time.

### R11-B3 - Low: the files list names only the 30-s mapping for `control/csi/node.go`
design.md:430 against :142-147, :583-585. The plugin now also maps the `the attach's time ran out waiting for the maintenance target to end` prefix to `DeadlineExceeded`, and the files list says only "the 30-s answer's mapping to `Unavailable`". An implementer reading the list can skip the second mapping, and the plugin test at :583 then fails.
Fix: add the second prefix and `DeadlineExceeded` to the node.go line.

Reviewed commit: 8a53a4b33bd9f69c52828fbe428851722c86cdc8
