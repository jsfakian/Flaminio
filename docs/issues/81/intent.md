# Issue #81: Bounded ISR updates — intent

[Issue #81](https://github.com/jsfakian/Flaminio/issues/81), correctness audit finding F06.
This plan covers the whole issue; it changes recovery batching, not replication policy.

## Why

1. **The producer has no request bound.** `Target::send_meta`
   (`dataplane/crates/flaminio-repl/src/target.rs:2389`) collects every chunk with a pending
   ISR or replica-set change into one `ChangeIsr`. Its single metadata slot prevents another
   proposal until a reply or timeout, but does not limit the proposal's length.
2. **The adapter forwards that length unchanged.** `Engine::drain_one`
   (`dataplane/crates/flaminio-node/src/engine.rs:1790`) builds one protobuf request from the
   complete change vector and records the chunk indices for its reply.
3. **The real service refuses more than 64.** `domain.MaxChangesPerRequest` is 64
   (`meta/domain/domain.go:15`); `Service.ChangeIsr` rejects larger vectors before reading or
   writing metadata (`meta/service/service.go:755`). An accepted batch checks the header and
   named chunks conditionally, including the epoch and expected per-chunk ISR versions.
4. **Retries do not repair the size mismatch.** The engine ignores a failed ISR answer
   (`engine.rs:1504`); the core later times out and proposes the still-pending vector again.
   A peer loss affecting 65 chunks can therefore block recovery indefinitely. At the default
   chunk size this is a 16.25 GiB volume, not an exceptional scale.
5. **The audit reproduces the boundary.** Its
   `shrink_after_losing_peer_fits_metadata_rpc_limit` fences one RF=2 peer for a 65-chunk,
   minISR=1 target and advances the takeover wait: the real core emits 65 changes.
   The engine's test `Model` (`dataplane/crates/flaminio-node/tests/engine.rs:85`) accepts
   that size, so it does not reproduce the production service's refusal.

## What we want

1. Every emitted and forwarded ISR-update request fits the service's accepted batch size.
2. Finite pending changes on 65+ chunks make progress through bounded transactions, including
   shrink, rejoin and move changes; omitted chunks remain pending rather than being forgotten.
3. Every chunk retains the existing fencing, minimum ISR, durable acknowledgement and
   pending-joiner rules through partial success, lost replies, retries and epoch changes.
4. Simulator, engine fixtures and real Rust-client/Go-service tests share the production
   boundary and expose a future mismatch. Preserve the fixed counterexample as a regression.

## Out of scope

- Raising etcd transaction limits, changing the protobuf wire format, or adding a new RPC.
- Changing minimum ISR, failure domains, takeover waits, read admission or replica authority.
- New rebuild policy, arbitrary volume-scale support, or geometry limits: #45, #72 and #80.
- Lab runs or performance claims; no shared VM or disk needs changing to prove this defect.
- Accepting proposed ADR 0007 or altering the accepted lease decisions. This work preserves
  the stricter implemented protocol while its historical ADR wording is reconciled separately.
