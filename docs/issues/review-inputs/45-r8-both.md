# Plan review: issue #45, round 8 (incremental), design-code and design-security
Reviewed commit: 26b8a543710fbb06e4747235ecfe28ec96fc21d5 (HEAD; it adds only report.md over c9b4bf7, the round-7 fix reviewed as the diff).

## Round-7 findings
- R7-B1 (Low): fixed in intent. Acquisition versus renewal is defined (design.md:98-103, 145-148), "ownership revision" is replaced (design.md:287), and a renewal test is added (design.md:354). Residuals R8-1 to R8-3.
- R7-B2 / R7-N1 (Nit): fixed. "Sighting tick's interval (at most 1 s) plus the 5 s after the sighting" is the same in spec S5, design.md:124 and :169, decisions (round 4 settled note :70, round 6 entry :426, round 7 entry :445) and ADR proposal :39. No leftover "six ticks". (decisions.md:398-401, R5-2, still says "5 s plus the ..."; not rechecked as unchanged.)

## New findings

### R8-1 - Medium: `kv.Renew` is named but not specified or listed
design.md:101 is the only definition: "a new `kv.Renew`: etcd's `KeepAliveOnce` on the key's lease, Mem extending its expiry". The `KV` interface (meta/kv/kv.go:38-53) has no such method; nothing says its signature, and no file list or test row covers it (design.md:354 names only `ListRevs` for `meta/kv`; no Mem/etcd Renew test; the Mem and etcd backends and the kvtest suite are not named as changed). Unspecified behaviours that matter:
- Etcd lease cache (`e.leases`, etcd.go:48, filled only by `PutTTL`, etcd.go:118-146): the observer key is created by a conditional Txn, so the Txn-with-TTL path must register the lease under the key, or Renew finds nothing. The design says TTL txn puts use "the cached lease" for heartbeats but not that the CAS acquisition records it.
- Renew on an expired lease (`KeepAliveOnce` error, lease not found) must return a distinct "lost" result, and must NOT grant a new lease as `PutTTL` does (etcd.go:129-137). Otherwise a lost ownership looks like a renewal.
- Mem (mem.go:91-99, `expires` keyed by key only): Renew extends whatever value is at the key, including a new owner's after takeover. Etcd renews the caller's own lease, so Mem and etcd differ; a Mem test of two observers would see an old owner prolong the new owner's key. Renew must be scoped to the key's revision or the lease (for Mem: only if the entry's Rev equals the acquisition's).
- Cadence is unstated: the TTL is 5 s and the pass is 5 s (design.md:96, 40), so the owner needs a renew several times per TTL, independent of the pass, and a renew must not be blocked by a slow pass. Not stated.
Fix: list `meta/kv/{kv,etcd,mem}.go` and kvtest for `Renew` in the S2-S5 row; give the signature and these semantics (lost = error, no new grant, scoped to the acquisition's revision/lease); name Mem and etcd tests: renew keeps the key and its revision, renew after expiry reports lost, renew after another owner's takeover does not extend it, the acquisition's lease is the one renewed; state the renew cadence.

### R8-2 - Low: the renewal test may not fail if a renewal resets T
design.md:354 says "a renewal changes neither B0, T nor the key's revision". Because Renew puts nothing, the revision assertion cannot fail; the assertion that detects a reset is on T/B0 (or the stored credit) across at least one renewal while a node is Missing and ticks credit between writes. The row does not say the test spans renewals with credit accrued on both sides, nor that a regression (reset on renewal, or reread of B0 and the "no write before the B0 read" stall) shows as the stored credit falling or a write gap. Fix: state the test as: owner holds, credits x, a renewal, credits y, next write stores B0 + x + y (not B0 + y), and writes continue without a B0 reread (count of B0 reads is 1). Add that it fails when the implementation resets T on a lease renewal or on any new key revision.

### R8-3 - Low: a lost lease is only found at the next compare
With a renewal that writes nothing, an owner whose lease expired (a paused process, a partition) keeps crediting T in memory until a clock write's ownership compare fails. Safety holds (writes compare the ownership, design.md:144-147; timeout Failed compares the stored clock), so this delays only. Say that a Renew reporting lost, or a refused compare, ends the ownership at once (stops the step per design.md:284-287), and test it.

## Nits
- design.md:101-103: the paragraph's last sentence runs on past the line ("That revision and nonce fence late old observers. The first owner creates...") with the diff's line wrap left unbroken; reflow.
