# Issue #86: Bounded uncertainty with conservative version covers — design

[Spec](spec.md), [intent](intent.md), [proposed ADR](adr-proposal.md),
[decisions](decisions.md). Approved 2026-11-13; not implemented.

The facts it starts from:
- CoreChunk.uncertain is Vec<(Extent,Version)>; report builds an ExtentSet without a cap
  (`flaminio-repl/src/replica.rs:64–79`). Write pushes before applying prune (`:232–238`).
  Restart sets restarted_at=fenced; only prune/Settle from a newer epoch clears it (`:129–143,248`).
- Repair Write has prune=None, and Settle follows chunk reconciliation or a join
  (`target.rs:1346–1363,2262–2271,2578–2588`). Erasing version information during fallback can
  violate that proof. A write prunes only its own chunk, bounded by the volume's oldest write in
  flight (`replica.rs:231–238`, `target.rs:1351`): an idle chunk keeps its last in-flight window.
  No request's epoch is compared with its session's (`replica.rs:224–255`): a prune or Settle
  bound of a higher epoch clears the chunk's records and restart cover (`:234–237,250–253`).
- Fenced encode writes every row/extent then asserts body size; decode caps total extents but
  not rows (`flaminio-net/src/frame.rs:285,342,466`). Any reply matched to a Hello opens the
  session and extends the lease before its body is read (`target.rs:906–909`); on_fenced's
  per-chunk find is quadratic and its bad extents are silently skipped (`:1311–1328`).
- Node Hello materializes its first report before creating missing advertised chunks
  (`flaminio-node/src/replica/mod.rs:221–264`). It creates every advertised chunk it lacks, blank,
  whatever the reason it is missing (`:239–257`); no `NotHeld` or bootstrap refusal exists in
  this tree (#13 and #61 are drafts). Held source/release leftovers must remain acceptable.
- `Request::Hello` is a unit variant (`flaminio-repl/src/msg.rs:32`); the engine recomputes the
  chunk list from its own view (`flaminio-node/src/engine.rs:1926–1934`); the simulator delivers
  packets later, when the sending target may be gone (`sim/mod.rs:696,740`).
- `ReplicaService::new` is infallible and narrows with `as` (`mod.rs:127–146`); the
  earlier-incarnation delete runs before the core is consulted (`:194–212`).
- `Frame::encode_checked`, `EncodeError` and `SessionError::Encode` do not exist; `Conn::send`
  calls `encode` (`flaminio-net/src/session.rs:30–39,60`). `try_seed` catches only `not
  linearizable` and `replicas diverge` (`sim/tests.rs:238`); `ReplicaCore` derives `Default`.
- Safe probes of audit 7b94e2d confirm both failure paths on 1 and 2 legal 1 GiB chunks; #80 is
  independent.

## One bounded arena, per-chunk coarse cover (S1, S2, S4, S8)

Expose `flaminio_repl::replica::UNCERTAINTY_BUDGET: u32 = 65_536`; `flaminio-net/src/frame.rs`
asserts at compile time that `MAX_UNCERTAIN` and `MAX_CHUNKS` equal it. The budget is a
`ReplicaCore` field set by `ReplicaCore::with_budget(b)`; `Default` is written out as
`with_budget(UNCERTAINTY_BUDGET)`, no longer derived (a derived one would give 0).
CoreVol tracks N held chunks, R live precise records, one precise-record arena/free list and the
set of chunk ids holding records (≤N entries). Reserve one report slot per held chunk: N≤B and
N+R≤B. Per-chunk linked heads reference arena slots containing extent/Version/next; no per-chunk
Vec capacity can accumulate after prune. Arena grows in fixed bounded pages with allocated
slots≤B; freed slots are reused across chunks.

A chunk is precise (a record list, possibly empty) or coarse (`cover: Some(M)`, no records).
There is no volume-wide mode. Process current-chunk prune first, then retain the new write iff
Version≥prune (or no prune), equivalent to existing push-then-retain without transient B+1
allocation. Retaining on a coarse chunk sets M=max(M,Version). On a precise chunk it allocates a
record if N+R<B; otherwise it coarsens that chunk only: its records are freed and
`cover=Some(max(their Versions, the write's))`. No other chunk changes. Cache R as records
allocate/free, not by scanning the volume each write. Preserve record-before-Action::Write,
so failed/unanswered writes stay conservatively covered. Do not interpret a copy as completion.

`add_chunk`, `recover_chunk` and `can_add_chunk` return `Result<(), ChunkError>`: `TooManyChunks`
at N=B, `Exists` for a held id, so a duplicate cannot overwrite coverage. Node and simulator
adapters propagate it and call `can_add_chunk` before a store create, in `hello()` and in
`create_moved_chunk` (`mod.rs:268–292`): the store never holds a chunk the core refused, which
the next restart would quarantine. An add at N+R=B first coarsens the
lowest-numbered chunk of the record set (O(log N + its records)), then inserts the new chunk
precise and empty. Recovered chunks get restart cover. `restart()` frees every record, clears
every cover and sets restart cover on every chunk, as `uncertain.clear()` does today: the core
then equals the one a new process builds (`mod.rs:127`). Remove/unlink a chunk updates N/R but
never removes another chunk/volume fence.

`hello()` refuses a Hello whose missing advertised chunks would make N>B:
`Err("volume would hold more than <B> chunks on this node")`, B the service's budget;
`create_moved_chunk` refuses with the same text. The check sits after the
quarantine check, the earlier-incarnation block and the geometry-mismatch check
(`mod.rs:194–220`), and before `vol_id` (`:221`), `core.handle`, any create and the fence. N
counts what that delete left: an earlier incarnation's chunks never count against a new volume.
The chunk-count check in `Engine::start_target` is #80's (its "Target, session and store
checks"); this issue adds none: its defence is the replica's refusal and the Fenced bounds.

**Unsupported volume.** `ReplicaService::new` stays infallible and groups `store.chunks()` by
volume first. A volume with more than B chunks, or a chunk index that does not fit `u32`, goes to
`unsupported: BTreeMap<Uuid, &'static str>` (`too many chunks`, `chunk index out of
range`), is logged once, and gets no `vols` entry, `VolumeId` or core state. `chunk_size/BLOCK`
needs no check: the store refuses a chunk above 1 GiB at replay (`flaminio-store/src/state.rs:300`).
- **Hello:** after the geometry checks and before the earlier-incarnation delete (`mod.rs:201`),
  such a uuid returns `Err("volume unsupported on this node: <reason>")`, whatever its
  `first_epoch`: no store or core change, no session. The engine closes the connection with that
  text (`engine.rs:799–806`) or logs it for the local replica (`:1955–1968`).
- **Fence and bytes:** chunks and fenced epochs stay as found; no session can touch them.
- **Leaving:** `ReplicaService::delete_volume` (`mod.rs:813`, for a volume deleted from the
  metadata service) works as today and removes the mark. Otherwise an operator removes chunks
  and restarts; `new` evaluates the inventory again.

## Prune and restart proof (S3, S4)

`ReplicaCore::handle` returns `Action::Drop` for a Write whose `version.epoch` or `prune.epoch`,
or a Settle whose `below.epoch`, differs from `m.epoch`; honest targets always send their own
(`target.rs:1356–1363,2486–2489`). This is a safety rule: every version and bound a replica
applies then carries the fenced epoch. It closes what a forged epoch does: a higher bound clears
restart cover set in the fenced epoch, which no bound of that epoch can clear; a lower version
is pruned by any bound of the session's epoch; a higher version keeps M above every later bound.
So a Settle of any newer epoch clears a cover, and restart cover outlasts any cover it replaces.
It does not check a bound of the session's own epoch: that is its holder's claim that the writes
below it completed. A Write whose version is below its own prune, or a Settle with the largest
`seq`, still clears the chunk's records and coarse cover, as today. The replica cannot know
which writes completed and the sender is the authenticated target of the fenced epoch, so this
plan adds no rule for it; the existing `early_settle` and `prune_reconcile` mutations check that
the honest target makes no such claim early.

Simulator check: `Config.forge_epoch_pct`, 0 in `Config::random`. In `Sim::send`
(`sim/mod.rs:648`), only while `self.chaos` and drawing from the rng only when the percentage is
above 0 (as `local_read_fail_pct`, `:746–757`: pinned seeds do not move, and nothing is forged
while settling), it rewrites that share of outgoing Write prunes and Settle bounds to the
session's epoch + 1, whatever the destination, the target's own replica included. The replica
drops the request; it times out, and the session fails and reopens (`target.rs:811–830`). The
shadow takes a bound only at its session's epoch. Mutation `accept_foreign_epoch` skips the drop.

For coarse cover M and bound p, clear the cover only if M<p. Otherwise keep
whole-chunk cover: partial pruning cannot recover lost spatial/version detail. New writes update
M=max(M,Version), including same-epoch repairs with prune=None. Coarse maxima use full tuple
ordering, never just epoch or the newest incoming request's version. A current Settle after all
repair writes have completed can exceed M and clear them; no new epoch is artificially required
for version-based coarse cover. A cleared chunk is precise and empty again: the cover stood only
for writes below p, the state of a chunk whose last record was pruned. No extra proof is needed.

Restart cover is independent: exactly existing rule restarted_at<p.epoch. A same-epoch prune
can clear coarse records yet the report stays whole under restart cover. Restart clears old
volatile records only while installing restarted_at=fenced for every chunk; same/new epoch
Hello does not itself clear either cover. Late/old session requests retain existing drop rules.
No pruning proof is derived from frame size, node liveness, acknowledgement arrival or a timer.

## Report construction and chunk-set compatibility (S5, S7, S8)

A row is emitted for every held chunk, sorted by chunk ID. Restart/coarse-covered chunk gives
one full extent [0,blocks); precise row unions its arena extents; known-clean row is empty.
Thus T≤(covered chunks)+R≤N+R≤B. Held includes released/moved copies still in the store; count
unique chunk identities, not only current placement.

Node Hello retains its existing stale-epoch check and durable create/fence ordering. After all
successful missing-chunk creates, discard the preliminary reply and rebuild the final report
with `ReplicaCore::report(vol)` before sending; chunks the Hello created now have explicit empty
rows. An empty row carries exactly the trust a missing row carries today: create-on-Hello is
unchanged, so a node that lost its disk still creates blank chunks and reports them empty.
Telling a fresh allocation from a lost one is #13/#61's work; when they land they must define
how `NotHeld` or a bootstrap refusal meets the required-row rule. This also covers a coarsening
caused by adding chunks without stitching an obsolete report to new state. A failure sends no
success; never undo a durable fence to make a retry appear available. At most two bounded report
builds per Hello; no unbounded preliminary snapshot is constructed.

`Request::Hello` becomes `Request::Hello { chunks: Vec<ChunkIdx> }`: the ids, ascending, of the
chunks whose `replicas` hold the peer, computed in `Target::open_session` (`target.rs:1168–1178`)
and kept in `Session.hello_chunks`. The engine copies it into `Accepted.chunks` and
`Frame::Hello.chunks` in place of its own recomputation; the node builds the core's message with
it; the core ignores it. The simulator's `Replica::handle` creates each listed chunk it lacks
after the core's stale check, as the node does, then replies with `ReplicaCore::report(vol)`. The
set travels with the packet, so a late delivery needs no Target. No wire change: `Frame::Hello`
already carries the list. All required IDs must occur once in Fenced. Extra held rows are legal
when unique and within the volume geometry; validate then ignore rows not relevant to current
peer placement. Do not require an exact assigned-only set. Fixtures previously using omitted
empty rows must change, not silently relax the validator.

In `on_reply_inner`, a reply matched to a Hello is handled before `session.open` and
`confirmed_until` (`target.rs:906–909`). Stale deposes first, as today. Otherwise the body must
be `Fenced` and pass the whole check: row count, total extents, increasing chunk IDs, all
required IDs present, nonempty checked ranges, ordered/nonoverlapping extents and
end≤blocks_per_chunk. Any other body, or a failed check, takes the existing failure arm
(`:990–999`) with session.open, confirmed_until, first_fence, c.fenced and reconciliation
untouched; the new reason string is `an invalid Fenced reply`.
No parser can detect a malicious replica lying with a present empty row; that is not claimed.
Use a sorted merge of captured IDs, report and chunk table; remove repeated `.find` searches.

## Checked codec shared with #80 (S5, S6)

**One** `Frame::encode_checked(&self)->Result<Vec<u8>,EncodeError>` serves #80 (Hello) and #86
(Fenced). Whichever issue lands first adds the entry point: `EncodeError` (`Invalid(&'static
str)`, `TooLarge(usize)`, mirroring `DecodeError`; #80 names no variants),
`SessionError::Encode(#[from] EncodeError)`, and `Conn::send` calling it and writing zero bytes
on error. The other adds only its frame's preflight arm. #86 is expected first (#80's design:
"#86 must land before this transport guarantee is complete"); until #80 lands, every frame but
Fenced passes through `encode` unchanged. Preflight before buffer allocation: N ≤ 65,536, T =
the checked sum of the rows' extent counts ≤ 65,536, ordered unique row IDs and valid extent
shape. Extent length is not limited
to IO MAX_BLOCKS: a whole 1 GiB chunk is 262,144 blocks and must encode. Geometry-relative range
checks belong to target.

Existing wire is unchanged: prefix:u32le, tag (u8, 64), req:u64le, N:u32le, then chunk:u32le,
m:u32le and m×(start:u32le,len:u32le). Body = **13+8N+8T**, prefix excluded.
N=T=65,536 gives 1,048,589 ≤ MAX_BODY 1,048,704 (115 bytes spare). Exact-size/preallocation
uses checked arithmetic; normal Fenced sends never invoke the old assertion path. Keep an
infallible helper only for explicitly trusted existing callers/tests, not network/report dispatch.
Do not increase limits or add pagination. Coordinate encoded-length helper with #62's counters.

Decoder checks N before reserve/loop, checked aggregate m before extents allocation, remaining
body before read, strictly increasing IDs, valid/nonoverlap ranges and no trailing bytes. Bounded
fallible output allocation returns a controlled framing error; no partial response is accepted.
Local reply conversion must reach the same Target validation as remote frames. Future #51/#61/
#72 envelopes need their own exact body proof; this proof is for existing Fenced v1 only.

## State/work bounds and compatibility (S8, S9)

Uncertainty storage: ≤B arena slots, ≤B chunk markers and ≤N record-set ids, never historical
per-chunk spare Vecs. Precise append O(1); prune O(r_chunk); coarse update/prune O(1). Coarsening
a chunk O(r_chunk+log N), each record freed once. Report O(N+R log R), allocated rows/extents
O(B); node final rebuild costs at most twice that. Decode/consumer validation/merge
O(N+T+V×RF), V the chunk table the target already holds. Fixed test operation counters verify
loops, not timing claims. These bound uncertainty, not every volume/resync filter (#75).

Same wire/disk/token/metadata layout, larger conservative reconciliation cost. Upgrade producers
and consumers together before new attach/provisioning: old producers omit just-created rows,
so strict new consumers can refuse them. Existing old/new rolling support is not certified;
no unsafe fallback to missing required rows. Runtime rejects legacy unsupported held geometry
even if #80 admission was bypassed. Operator cleanup/migration is explicit, never auto-erase.

## Files and complete rule-to-test mapping

| Rules | Files and concrete proof |
|---|---|
| S1, S2, S4 | `repl/src/replica.rs` unit/reference tests B−1/B/B+1, N=B/N=B+1, fragmented and duplicate records, rotating fill/prune across chunks, arena capacity/free-slot reuse. At N+R=B only the written chunk coarsens and every other row is unchanged. An add at N+R=B coarsens one chunk and the new chunk's row is empty. After `restart()` no cover or record is left and the next writes are precise. Coarsen, Settle above M, one more write: the row is that write's extent. Sequential fill, 512 chunks × 128-write window (the defaults): at most 8 chunks whole (arithmetic gives 4), the others ≤128 extents. Add/remove/recover refusal before overwrite. |
| S1, S4 | `node/src/replica/tests.rs`, on `ReplicaService::with_budget(store, 4)` (`pub(crate)`; `new` passes `UNCERTAINTY_BUDGET`). The fixture `accepted()` describes a 4-chunk volume (`tests.rs:68–86`), so these tests use a new helper `accepted_in(volume, count, epoch, session, chunks)` with `size: count * CHUNK`, on 6-chunk volumes, and assert each refusal by its text: `out of range` (`mod.rs:277`) or any other refusal must not pass for it. `unsupported_volume_is_refused_and_kept`: a store given 5 chunks of A and 2 of B through `new`, then reopened with budget 4; Hello for A is refused with `volume unsupported on this node: too many chunks`, also with `first_epoch` above A's fence; `store.chunks()` (keys, fenced epochs) is identical after both; B's Hello returns Fenced and its write commits; `delete_volume("A")` returns 5 and a later Hello for A succeeds. Same refusal, with `chunk index out of range`, for a chunk index 2^32 created through the store. `hello_over_budget_is_refused_before_create`, each refusal with `more than 4 chunks`: (a) with {0,1,2} held, a Hello of the same incarnation advertising {3,4} is refused, `store.chunks()` and the fence unchanged; (b) an earlier incarnation holds {0,1,2}, a Hello with a higher `first_epoch` advertising {3,4} succeeds and `store.chunks()` holds only {3,4} (a check placed before the delete refuses it); (c) the same earlier incarnation, a Hello with a higher `first_epoch` advertising {0,1,2,3,4} is refused, the earlier chunks are gone and no chunk of the new volume is held. `moved_chunk_over_budget_is_refused_before_create`: with {0,1,2,3} held, a `BeginResync` for chunk 4 returns `Reply::Close` with `more than 4 chunks`, `store.chunks()` is unchanged, and a restart on that store does not quarantine the volume. |
| S3, S4 | Mixed-version oracle: max versus lesser bound, same/new epoch, lower/equal/greater p, restart+coarse independent clearing, prune=None repairs then actual Target Settle, write/store failure/reconnect and partial reconciliation. Epoch rule, unit cases: `version`, `prune` and `below`, each lower and each higher than the session's epoch, are dropped and change no row; a higher `prune` and a higher `below` against a restart-covered chunk leave its row whole. The node test helper `write()` (`node/src/replica/tests.rs:88–100`, epoch 1 on sessions at epochs 5 and 2) takes the session's epoch; the check is not relaxed for it. Never omit oracle uncertainty. |
| S5, S6 | `net/src/frame.rs`, `tests/session.rs`: retained F09 real-core probes now encode/decode; one/two full chunks, max N=T=B exact body, empty rows, T=B+1/N=B+1/body overflow and arithmetic extrema; zero bytes on checked refusal, no panic; other frames encode as before. The constants' equality is the compile-time assertion. |
| S7 | Target/node/engine tests reject missing required/duplicate/unknown/out-of-range/wrapping rows before authority. `hello_answered_ok_grants_nothing`: an `Ok` reply to Hello leaves the session closed and `confirmed_until` unchanged; Stale still deposes. Chunks a Hello creates have explicit empty rows (the refusal for a lost holder is #13/#61's test, not written here). Valid extra released/source rows, the Hello's set versus later placement, the engine's `Accepted`/`Frame::Hello` list equal to `Request::Hello.chunks`, local/remote equivalent paths and linear scan counters. |
| S8 | Capacity/work counters include retained arena allocation after repeated pruning, report union/preliminary rebuild and max-set decoder/target loops; no hidden quadratic path or completed-operation history. |
| Invariant | `sim/mod.rs/check.rs/tests.rs`: `Config.uncertainty_budget` (default `UNCERTAINTY_BUDGET`; these tests 8/16; setup asserts chunks≤budget) triggers coarsening during writes/repairs/partitions/restart; independent exact-version/restart shadow checks every report covers required blocks (`uncertainty not covered`) and N+R/capacity limits (`uncertainty budget exceeded`). Keep per-block linearizability/agreement. `forged_epochs_leave_runs_clean`: 200 seeds of `forged` (next row) pass with no bug on, liveness included. |
| Mutations | `drop_on_coarsen` omits cover, `coarse_prune_early` clears M≥p, `bypass_uncertainty_budget` exceeds allocation/demand, `accept_foreign_epoch` skips the epoch drop: `mutation!(catches_accept_foreign_epoch, replica(\|b\| b.accept_foreign_epoch = true), <budget>, forged)` with `fn forged(s: u64) -> Config { Config { forge_epoch_pct: 20, ..Config::random(s) } }` (`Configure` is `fn(u64) -> Config`, `sim/tests.rs:213`), caught as `uncertainty not covered`, its budget recorded after a real search; `accept_invalid_fenced` installs authority from malformed report (`invalid Fenced accepted`): each caught by independent checker on a clean-passing seed, recorded after actual search. `try_seed` adds the three quoted strings to its catch classes. Existing early_settle/prune_reconcile/no_reconcile mutations remain caught; their pinned seeds may move with the simulator's Hello change and are searched again (`scripts/dev/mutation-search.sh`). |
| S6, S9 | Extend frame fuzz target/regressions with count/body/shape seeds, invalid decoded replies and canonical roundtrip; #80 geometry and real Rust/Go interop remain unchanged. |

The shadow is test-only and intentionally unbounded only for bounded test input: it stores each
accepted write's full extent/Version and applies the original prune/restart rules independently,
not the new max-marker algorithm. Invalid-reply injection records pre/post authority to catch
validation bypass; no fabricated first-catching seed or passed campaign is reported here.

Touched: repl msg/replica/target/sim and tests; node replica (and tests) and the engine's Hello
adapters (`engine.rs:1926–1934,2364`); net frame/session and fuzz corpus; docs/protocol/limits.
`make test sim lint race contracts`, sanitizer quick run if arena/lifetime changes warrant it.
No new ABI/proto/disk record and no new dependency.

## Lab, docs, risks and ADRs

No lab required/authorized: real memory-store/core+frame reproductions suffice for boundary
verification. Document budget/arena/coarse maxima, unchanged restart proof, explicit empty rows,
producer/consumer rollout, unsupported legacy volumes, the two coarsening triggers and their
reconciliation cost in protocol §3/§6/§9, dataplane README and followups; no benchmark unless asked.
[Proposed ADR](adr-proposal.md) records this recovery representation choice, not acceptance.
Settled at approval, 2026-11-13: that ADR is accepted with the plan (see decisions.md, Settled at approval).
Accepted Go/Rust/etcd/fencing/ISR ADRs remain; the proposed ADR 0007 of #83 (clocks) is unchanged.

Risks, none accepted:
Settled at approval, 2026-11-13: these risks are accepted with the plan (see decisions.md, Settled at approval).
- **Coarsening is routine, not an attack residue.** Idle chunks keep their last in-flight window,
  so R≈chunks touched×queue depth: at the defaults (256 MiB chunks, `control/csi/controller.go:71`;
  depth 128, `flaminio-io/src/node.rs:134`) a sequential fill of a 128 GiB volume (N=512) reaches
  B−N=65,024 after 508 chunks. One write stalled on a slow ISR member holds the prune bound while
  B−N writes elsewhere complete: about a second at 65k IOPS. Each chunk written while N+R=B is
  then digested whole at the next takeover, unless a prune or Settle above M clears it first: 4
  of 512 chunks in the fill, every written chunk at N=B, as after a restart. Never weaker safety.
- **The idle residue itself stays.** Pruning idle chunks would be a new replica rule with its
  own proof; bounding all background work needs another design (#75).
- **A quarantined earlier incarnation blocks its name on that node** until the replica delete
  or an operator removes it. Proposed trade: no automatic erase of data this node cannot serve.
  Settled at approval, 2026-11-13: the trade is accepted (see decisions.md, Settled at approval).
- **Empty rows prove nothing about a lost disk** until #13/#61 land (same trust as today).
- **A bound of the session's own epoch is not checked.** A faulty target of the fenced epoch can
  still clear a chunk's records with its own prune or Settle, as today. The epoch drop defends
  only against a forged epoch; nothing in this plan covers the rest.

Core/codec can parallelize only against this exact budget/encoding; node/simulator expected-set
adapters and target authority validation must integrate together before claiming normal recovery.
