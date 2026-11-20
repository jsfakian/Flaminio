# Plan review: issue #45, round 9 (incremental), design-code and design-security
Reviewed commit: c8d089254ddea5a38151139b22c23e93b8aa43f9 (HEAD; the diff 26b8a54..45d15b9 plus report.md).

## Round-8 findings
- R8-1 (Medium): fixed in specification (signature, lease rule, cadence, files, four kvtest cases), but the etcd form does not meet its own scoping rule: see R9-1.
- R8-2 (Low): fixed (design.md:381 x, renewal, y; one B0 read; no gap).
- R8-3 (Low): fixed in intent (three ways to end); residual R9-2, R9-3.
- R8-N1 (Nit): fixed in the ownership paragraph; a new long line remains at design.md:55 (not a finding).

## Findings

### R9-1 - Medium: the etcd Renew does not check the key's revision, so it disagrees with Mem and fails its own test
design.md:112-116 gives etcd "no lease registered at rev: ErrLost; else KeepAliveOnce on that lease". Nothing reads the key. When another instance B acquires (replacing the key with its own lease L_B), the old owner A still has L_A registered at rev_A. L_A is alive (an overwritten key does not revoke its lease), so A's KeepAliveOnce succeeds and Renew returns nil, and A keeps L_A alive indefinitely (a leaked lease). The same happens if the key is deleted while its lease lives. Mem returns ErrLost in both cases (entry Rev != rev, mem.go:70-99 has no lease at all). So:
- the kvtest case "with the old revision after another owner's acquisition: ErrLost" (design.md:381) fails on real etcd as specified; the two backends disagree;
- "no renewal succeeded for 5 s" and "ErrLost" (design.md:122-128) never fire for a displaced owner on etcd; the ownership then ends only at the next refused compare (the pre-round-8 behaviour, safe but not what R8-3 wanted).
Fix: in the etcd form, check the key without a write before the keepalive: Get the key and require ModRevision == rev (ErrLost if absent or different), then KeepAliveOnce on the registered lease (a keepalive of one's own lease after a passing check is harmless if the key changes in between). Revoke or drop the registration when ErrLost is answered by the revision check. Say that ErrLeaseNotFound (rpctypes) is the "gone" case and every other error, including retry-exhausted Unavailable, is returned as it is (the `retry` helper, etcd.go:26-41, wraps errors the design must not turn into ErrLost). Add the displaced-owner case to the etcd run explicitly (key replaced by a second KV's acquisition on the same etcd, the first KV's lease alive).

### R9-2 - Low: the 5 s no-renewal bound equals the TTL, so the owner can outlive its key
The owner stops 5 s after the last successful renewal, measured when the answer arrives; the server extended the lease when it received the request, earlier. So the key can expire, and another instance acquire, up to the request's latency before the old owner stops. Safety holds (writes compare the ownership), but both credit in memory in the overlap and the design says the ownership "ends at once". Fix: measure from the renewal's send time (monotonic), and/or end it at a bound below the TTL (for example 4 s); say which, and test with a renewal answered late.

### R9-3 - Low: the reread before a refused transaction has no failure rule
design.md:122-128: a refused compare ends the ownership only "once a reread shows another revision or none". If the reread itself fails (outage), or shows the acquisition's revision, the text says nothing for the first case. Say: a failed reread changes nothing (the next tick's renewal decides) and no write is sent until a pass has a result; and test a refused transaction with the key unchanged (another compare lost) leaves the ownership, B0 and T.

### R9-4 - Low: the registration after a transactional TTL put is unspecified when two puts to the same key overlap
design.md:50-54: "a committed TTL put registers its lease under the key with the revision it wrote". The heartbeat and PutTTL (etcd.go:118-146, which stores only the lease, no revision) share `e.leases[key]`. Say that PutTTL stores revision 0 or the put's header revision, so a later Renew(rev) cannot match a PutTTL registration by accident, and that the registration is replaced atomically under `e.mu` with the commit's revision (no registration of a lease from a refused transaction: stated, but give the test: the refused-transaction case leaves the previous registration and revokes the new grant).

## Nits
None.
