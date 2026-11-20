# Plan review: issue #45, round 10 (incremental), design-code and design-security
Reviewed commit: db80c683dc784dd984688a5c3e29c34f72467c62 (HEAD; the diff c8d0892..9a68aeb, design.md and decisions.md).

## Round-9 findings
- R9-1 (Medium): fixed. Get with ModRevision == rev, then KeepAliveOnce; ErrLeaseNotFound is ErrLost; other errors pass through; displaced-owner case on real etcd. A takeover between the Get and the keepalive is harmless: the keepalive extends only the old lease, and the next Renew's Get answers ErrLost; writes compare the ownership revision. See M1 for a residual.
- R9-2 (Low): fixed in design (send time, 4 s); see L1, L2 (the test and the evaluation point).
- R9-3 (Low): fixed (three outcomes, fail closed, tests).
- R9-4 (Low): fixed (lease and revision, newer revision wins, refused txn keeps registration and revokes its own grant); see L3.

## Findings

### L1 - Low: the test "a renewal answered 3 s late" cannot happen as specified
design.md:~123 and the test row: renewals run with a 1 s context, and the etcd `retry` is bounded by it, so an answer can never arrive 3 s after its send. Either the test (a fake KV that answers after 3 s ignoring the context) exercises a path the real code cannot reach, or the context is not what the design says. Fix: test with a renewal answered at the context limit (about 1 s late) and a fake clock, and state that the send time is taken before the `Get` of `Renew` (the earlier, conservative time), not after the keepalive's own send.

### L2 - Low: where the 4 s bound is evaluated is not stated, and the acquisition's start is not guaranteed
(a) If the bound is checked only when a renewal returns or at 1 s ticks, the owner can credit up to 1 s past send+4, i.e. to the TTL, which is what R9-2 removed. Fix: check it before each credit and each write (and before B0/T are used), by the monotonic clock, not at the renewal.
(b) "or of the acquisition": the acquisition puts on the instance's cached lease, which may have been last extended long ago, and is extended only by the keepalive after commit. If that keepalive fails, the lease can expire sooner than acquisition-send + 5 s, so the claim "the key cannot expire before send + 5 s" is false for that case. Fix: count the acquisition as a successful renewal only when its post-commit keepalive succeeded (send time of that keepalive), else treat the ownership as not held, or grant a fresh lease for the ownership key.

### L3 - Low: dropping the registration "at rev" must be conditional
design.md etcd Renew: "the registration at rev is dropped" on ErrLost. A Renew running late while the same instance acquires again (new lease, newer revision, registered under `e.mu`) must not delete the newer registration. Say: dropped only if the registration is still the same lease and revision; add a test (late ErrLost for an old revision after a new registration).

### Nits
None.
