# Issue #16 plan review, round 8 (incremental), design-code and design-security
Reviewed: 3c27987..2d18860.

## Round-7 findings
- R7-1: fixed. flaminio-net uses smol 2 (Cargo.toml:29), and tests/session.rs:138-139 already drives session::accept with smol::spawn under smol::block_on, so smol::spawn for the connector, the server half first, then the connector's task works: the connector's drop gives the drain its EOF. Async::new on a try_clone is sound: it sets O_NONBLOCK itself, and epoll keys registrations on (fd, open file description), so a dup's fd is a new entry; the original's Async deregisters its own fd on drop and leaves the clone's. smol::Timer with futures_lite::future::or (both available) bounds the drain.
- R7-2: fixed. `find ... || echo ... >&2` makes the exit status harmless under set -e; the new case (aged dir holding a file) covers the refused rmdir.

## New findings

### R8-1 - Low - What the drain does with a read error is not stated
Design Tests, reconnect_after_expiry_refused. The drain exists to keep the server from closing with unread client data (the reset cause). If the connector still closes with data unread, the clone's read returns ConnectionReset, not EOF; and the 5 s timeout is "a failure". The text does not say whether a read error ends the drain quietly. If an implementer unwraps it, the test turns flaky on the very reset it removes. The assertions are on accept's error and the connector's error text, not on the drain.
Fix: say the drain only delays the close: EOF, any read error, or timeout ends it, and none of them is asserted (the timeout may be logged); the assertions stay on accept's error and the connector's text.

## Nits
None.
