# Issue #81: Bounded ISR updates — design

How the [spec](spec.md)'s rules are built. Settled choices: [decisions](decisions.md).
Approved 2026-11-13; not implemented.

The facts it starts from:
- `Target::send_meta` collects all pending chunks; `meta_inflight` permits one change request
  at a time (`dataplane/crates/flaminio-repl/src/target.rs:2389`). Its timeout clears only
  that slot (`:850`); `on_meta_inner` rejects an expired request ID (`:1087`).
- `Chunk::required` includes `joining` before commit (`target.rs:214`). `apply_view` adopts
  only greater ISR versions, preserves deferred membership and rechecks writes (`:2510`).
- The engine forwards one vector and keeps its ordered chunk indices in `MetaWait::Isr`
  (`dataplane/crates/flaminio-node/src/engine.rs:1790`). Its answer path updates `rt.view`
  before the core can ignore an expired request (`:1504`); this cache also needs protection.
- `ChangeIsr` refuses over 64 before accessing storage; otherwise it compares the header and
  each named chunk revision in one transaction (`meta/service/service.go:755`). The bound is
  `domain.MaxChangesPerRequest` (`meta/domain/domain.go:16`). The Rust metadata model and
  engine fixture currently omit that bound (`flaminio-repl/src/meta.rs:150`, engine test `:85`).
- Audit F06's 65-chunk counterexample and captured failure are under
  `docs/audits/2026-11-12/` in the audit checkout. The real client interop fixture currently
  creates only two chunks (`scripts/dev/meta-interop.sh:43`); existing interop cannot catch F06.

## Select bounded work in the core (S1–S3, S6, S7)

- Add public `meta::MAX_CHANGES_PER_REQUEST: usize = 64` in `flaminio-repl`. Use it in the
  producer, simulator boundary and engine fixture. Keep Go's existing constant; interop tests
  check that both implementations accept exactly this boundary and refuse boundary + 1.
- Add `Target::next_meta_chunk: usize`, initially zero. Retain the move eligibility pass in
  `send_meta`, then scan chunks cyclically from the cursor, at most one complete traversal.
  Select eligible `change_pending()` chunks until 64 are collected. Build each change from
  that chunk's current ISR, expected version, pending join/removal and move state as today.
- After emitting, set the cursor immediately after the last selected chunk, modulo chunk
  count. Advance on emission, including a proposal later refused or timed out: a hot or
  contended early chunk cannot monopolize retries. Empty selection emits no call and changes
  no membership. A failed batch does not block a later eligible chunk forever.
- Keep one in-flight change proposal and the independent refresh slot. A matching answer
  invokes existing `apply_view` and `progress`, which emits the next batch without waiting
  for a periodic refresh. Timeout retries rebuild a bounded proposal from current state;
  request IDs remain monotonically increasing within the target instance.
- Selection never clears `joining`, `removing`, dirty sets, uncertain extents, pending moves
  or write tracking. In particular, a completed resync's `joining` member counts immediately
  in `required()` and leases even if its chunk waits behind another batch. A removal affects
  write completion only after a newer authoritative view commits it. Preserve minimum-ISR,
  reconciliation and move-destination-before-source checks without alteration.

## Correlate and merge replies (S3–S5)

- Add a read-only `Target::accepts_meta_reply(req: u64) -> bool` for the two live metadata
  request slots. Use it before the engine mutates its cached view, and keep the check inside
  `on_meta_inner` as well. A timed-out reply cannot clear or answer the next batch's slot.
- Give `TargetRt` a `generation: u64` from its existing attach `attempt`; carry it in
  `MetaWait::Isr` and `MetaWait::Refresh`. Ignore an answer for a removed/replaced generation
  before converting node IDs, mutating cache or invoking the new target. Core request IDs
  alone are insufficient because a newly attached core starts a new request sequence.
- After generation and live-request association, inspect the authenticated answer's epoch before
  chunk-count, geometry or membership conversion. A higher authoritative epoch immediately
  reaches the existing core deposing path using a clone of the known safe current view with
  only its epoch raised; never convert or install its malformed chunk payload. Apply this
  ordering to both ISR and refresh answers, consistently with #80's validation contract.
- For a same/lower-epoch live ISR answer validate returned count against saved request order
  before merging. `ChunkMeta` has no chunk ID: associate positions only through that request's
  saved indices; do not invent an on-wire index check. A wrong count leaves the slot to timeout
  and authoritative refresh; never zip a partial response. Interop verifies service order.
- Same-epoch cache merges replace ISR/replicas only at greater `isr_ver`; at equal versions
  retain the existing requested-move refresh behavior. Lower epochs/versions cannot overwrite
  the cache. Apply the same epoch/version policy and #80 payload validation to refresh replies.
- A failed transport/RPC answer changes no membership and leaves existing timeout/refresh
  recovery in place. A commit whose answer was lost is learned from a version-refused retry
  or refresh. Both responses use authoritative versions; no speculative success is fabricated.
  An old owner cannot commit a subsequent batch after `Attach` advances the epoch.

## Enforce the boundary in models (S8, S9)

- At the simulator's `Body::MetaReq` boundary, reject oversized `ChangeIsr` before calling
  `Metadata::handle`, make no metadata change and provide no successful reply. Let the core
  timeout as it does on the real adapter's `Answer::Failed`. Keep domain transition vectors
  focused on valid-size requests: Go's domain checker itself is not the RPC size validator.
- In `FakeMeta::call`, return `Answer::Failed { code: InvalidArgument, message: ... }` above
  64 before indexing or mutating volume chunks. Match production ordered, all-or-nothing
  responses for valid batches, including moves and duplicate/version/minimum checks needed
  by the new recovery cases. Record proposed batch sizes for assertions.
- Add test-only `Bugs::unbounded_meta_batch`, bypassing selection's cap; the fixed F06
  scenario must catch it by an explicit emitted-size/progress check, rather than accepting
  a silently dropped oversized request. Exercise existing `ack_before_shrink`, `late_join`
  and `drop_before_join` mutations with deferred chunks so batching cannot hide those bugs.
  No safety rule is relaxed; any extra safety rule discovered during implementation requires
  its own simulator assertion and caught mutation before shipping.

## Files touched

- `dataplane/crates/flaminio-repl/src/meta.rs`: exported existing API bound (S1, S8).
- `dataplane/crates/flaminio-repl/src/target.rs`: bounded cursor, request guard, unit tests
  and deliberate batching bug (S1–S7).
- `dataplane/crates/flaminio-repl/src/sim/{mod.rs,tests.rs}`: production refusal boundary,
  large-chunk recovery and caught mutations (S2–S8).
- `dataplane/crates/flaminio-node/src/engine.rs`: generation/order/version guards (S3–S5).
- `dataplane/crates/flaminio-node/tests/engine.rs`: bounded fixture and recovery (S1–S8).
- `meta/service/service_test.go`, `meta/service/auth_test.go`: batch boundary and existing
  transaction/authorization regression coverage (S1, S4, S5, S9).
- `dataplane/crates/flaminio-metaclient/tests/interop.rs`, `scripts/dev/meta-interop.sh`:
  real mTLS client/service boundary with a separate 129-chunk fixture (S1, S3–S5, S7–S9).
- `docs/replication-protocol.md`, `docs/followups.md`: bounded-transaction wording and closure.

## Contracts

Existing `ChangeIsrRequest { volume, epoch, repeated IsrChange changes }` is unchanged.
Each target emission carries 1–64 unique chunks, each with its own `expected_ver`, `new_isr`,
`change_replicas` and `new_replicas`. The service atomically commits or refuses that batch;
other chunks are outside the transaction. Over 64 returns `InvalidArgument` with no writes.
`ChangeIsrResponse { applied, epoch, chunks }` retains request order, one current `ChunkMeta`
per named chunk even on a version refusal. No wire fields, methods, token or disk formats
change; `api/proto`, generated code and per-method authorization require no modification.
The engine does not split a core request into several independently acknowledged calls.

## Tests

| Rules | Concrete tests and level |
|---|---|
| S1–S3 | `target.rs`: preserve `shrink_after_losing_peer_fits_metadata_rpc_limit`; parameterize 1/64/65/128/129 chunks, assert distinct IDs, <=64, all deferred state retained, independent batch adoption. |
| S2, S6 | `target.rs`: `deferred_shrink_keeps_write_waiting`, `deferred_joiner_counts_for_write_and_lease`, `deferred_move_drop_waits_for_join`; withhold required acknowledgements and commit another chunk's batch first. |
| S3–S5 | `target.rs`: `lost_batch_reply_is_learned_by_refresh`, `takeover_between_batches_fences_old_owner`; compare refusal, timeout, lost applied reply and delayed old reply with no rollback or early completion. |
| S3–S5 | `engine.rs` unit tests: `stale_isr_reply_does_not_regress_cache`, `reattach_ignores_previous_generation`, `isr_reply_requires_complete_count`; test cache and core, including interleaved refresh/ISR responses and malformed counts. Both ISR/refresh higher-epoch malformed payloads must depose without converting/installing them; same/lower-epoch malformed payloads retain safe state and retry. |
| S1–S7 | `tests/engine.rs`: `large_volume_recovers_with_production_batch_limit`; 129 small chunks through peer loss/shrink/rejoin, captured RPCs, held replies and writes; a separate mixed move case asserts final replica sets and minimum ISR. |
| S7 | `target.rs`: `round_robin_batches_do_not_starve_tail`; recurring transitions in the first 64 chunks plus a continuously eligible last chunk, then heal faults and require every finite pending transition to commit. |
| S2–S8 | `sim/tests.rs`: 65/129-chunk shrink/rejoin/move campaigns with small block count; lost replies, partitions and takeover; assert request bound, minimum ISR, durable effective-quorum writes, convergence and final history. Catch `unbounded_meta_batch` and existing membership mutations on deferred chunks. |
| S1, S4, S5, S9 | Go `TestChangeIsrBatchBoundary`: 64 succeeds, 65 returns `InvalidArgument`, every original chunk unchanged after refusal; stale epoch, one wrong version and duplicate ID refuse the entire valid-size batch. Existing auth cases plus a multi-batch non-target attempt remain refused. |
| S1, S3–S5, S7–S9 | `interop.rs`: `bounded_isr_recovery_with_go_service`, using real target emissions via the Rust client against the Go service for 129 chunks; accept 64, reject 65 without writes, shrink then rejoin through multiple batches, replay after a hidden committed answer, then advance epoch and refuse the old owner. Compare every final chunk/version. |

Run `make test race contracts sim` and `make lint hygiene` before a pull request.
`contracts` runs real mTLS interop; fixture setup uses an isolated in-memory Go service, not
shared lab resources. Existing vectors remain unchanged unless transition semantics change.

## Lab

None. The counterexample, state-machine checks, engine memory devices and real Go-service
interop prove this defect without reformatting disks or changing shared VMs.

## Docs

Document the 64-change request bound and per-chunk progress in protocol §2 without weakening
membership or lease rules. Close #81 in follow-ups only after the mapped checks pass. No
performance or lab command changes; README, lab guide and performance plan need no new claims.

## Risks

- **Partial progress:** different chunks may become available at different times; that already
  follows per-chunk readiness. No volume-wide membership atomicity is introduced.
- **Conservative joiners:** waiting behind a batch can extend write/lease dependence on a
  joiner. Keep this existing safety gate; measure tuning separately if needed.
- **Cached state:** adding batching exposes old reply/replacement races more often. Generation,
  request and monotonic-version checks must precede every cache mutation on these paths.
- **Compatibility:** old targets retain the defect with new servers. Deploying the core fix
  is required; raising server limits or retrying an invalid vector cannot substitute for it.
- **Scale:** simulator histories are bounded; vary many chunks with few blocks and a bounded
  client history, then validate large engine workloads separately. No benchmark claim is made.
- **Open choice:** no additional architectural choice is required. Internal cursor and guard
  names can change together during implementation if their stated contracts are preserved.

## ADRs

ADR 0001's Rust/Go split, 0002's dedicated etcd and 0006's configured minimum ISR are preserved.
ADR 0005's all-member lease requirement is preserved. Its historical early-read wording
conflicts with the stricter implemented protocol; this issue makes no readiness change and
must not restore early reads. Proposed ADRs 0007–0011 remain unaccepted and are not dependencies.
No new dependency, disk/wire rule or superseding ADR is needed for bounded existing requests.

## Parallelizable?

Yes. Rust batching/engine work and Go boundary tests can proceed against the unchanged
contracts above; combine them in the shared interop fixture before completion.
