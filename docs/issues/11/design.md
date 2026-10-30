# Issue #11: Reconcile first the chunks that clients wait on — design

The code is in `dataplane/crates/flaminio-repl`: the target (`src/target.rs`) and the simulator
(`src/sim/tests.rs`, `src/sim/mod.rs`). Two scripts: a new `scripts/dev/mutation-search.sh`, and
`scripts/checks/reconcile.sh` for the lab. The data plane (`flaminio-node`) calls `submit`,
`tick` and the reply handlers as before: no change there. Settled choices are in
[decisions](decisions.md).

## The queue order (S1)

**Today:** `advance_chunk` pushes a newly fenced chunk's digests to the front of the queue, and
`progress` calls it for chunks 0 up, so the last chunk ends first.

**The change:** `advance_chunk` no longer pushes. Where it sets `Phase::Reconciling`, it appends
`(i, pieces)` to a new field, `newly_fenced: Vec<(usize, Vec<Extent>)>`, with the pieces
computed as today (`pending.pieces(self.max_digest)`). A new `queue_newly_fenced()` pushes them
to the front: the chunks in reverse order, each chunk's pieces in reverse, so the queue starts
with the lowest chunk's first piece.
- **`progress`** calls it after its loop over the chunks, before `send_meta` and `pump`;
- **`on_reply`'s `Scope::Chunk` path** calls it after its single `advance_chunk`, before `pump`;
- **`depose`** clears it, and also every chunk's `locks` and the test-only list of fast reads
  (below): a deposed target starts nothing, and `try_fast_read` refuses on it. `pump`
  `debug_assert`s `newly_fenced` is empty on entry.

Between the loop and `queue_newly_fenced`, nothing reads the queue: `advance_chunk` sets phases,
sends `Settle` and `Mapped`, and proposes ISR changes. So the only difference from today is the
order among chunks fenced in the same progress pass. A chunk with no uncertain blocks still goes
to `Ready` in the same call, with nothing queued.

## Which chunks are waited

**New fields of `Target`:**
- `unready: usize`: chunks not `Ready`. `chunks.len()`, set in `new`'s struct literal, before
  its `t.tick(now)`; decremented where
  `advance_chunk` sets `Phase::Ready`, the one place that does. Phases only move forward, so it
  only falls;
- `last_waited: bool`: the last start (S3). Set and cleared by the selection (below), cleared
  whenever `pump` finds no chunk waited, and by `depose`.

The waited chunks, their order, and what is in flight are found at each pump.

**`Work::class(&self) -> Class`** (new), the one place that sorts items by kind: `Client`,
`Reconcile(chunk)` (`Digest`, `Repair`), `Resync(chunk, round)` (`Copy`, `Discard`,
`ResyncDigest`).

**In flight, counted from the maps at the start of the selection** (`resync_in_flight`,
`waited_in_flight`, new), then kept up to date by the selection itself (+1 at each start of its
class):
- resync items: `digests` with `resync: Some`, `reads` with `ReadFor::Copy`, and session requests
  in flight of kind `ReqKind::Copy` or `ReqKind::Discard`;
- waited work: `digests` with `resync: None`, `reads` with `ReadFor::Repair`, and `writes` with
  `client: None`, on a waited chunk.

They are computed only on the waited path, so a target whose chunks all serve pays nothing.

## `pump` (S2–S5)

```text
pump(now):
  if deposed: return
  if unready == 0 (or the test switch, below): last_waited = false; pump_in_order(now); return
  pass 1: take the queue into a Vec<Option<Work>>, and in queue order:
    - drop a resync item whose chunk has no resync, or another round (try_start's test);
    - a client operation on a chunk not Ready: give the chunk the next wait rank, if it has none;
    - note each bulk item's index: reconcile (per chunk), or resync.
  if no chunk is waited: last_waited = false; put the items back; pump_in_order(now); return
  waited: each waited chunk's reconcile indices, chunk by chunk in rank order.
  reconcile: the reconcile indices of chunks not waited, in queue order.
  count resync_in_flight and waited_in_flight from the maps.
  select (S3), while a slot is free and the last round started something:
    1. resync_in_flight == 0 and last_waited: the first resync item that starts
         -> resync_in_flight += 1, last_waited = false;
    2. the first waited item that starts      -> waited_in_flight += 1, last_waited = true;
    3. waited_in_flight > 0:
         the first resync item that starts, if resync_in_flight == 0 -> resync_in_flight += 1;
         failing that, the first reconcile item that starts;          last_waited = false;
    4. otherwise: the first item that starts, of resync and reconcile merged by index
         (+1 to resync_in_flight if it is one), last_waited = false.
  pass 2: debug_assert the queue is empty; in queue order, skip the items started or dropped,
    try each client operation as today (the `blocked` map), and keep the rest as the new queue.
```

**`pump_in_order`** is today's `pump`, unchanged. With all chunks serving, or none waited, it is
the only pass that starts anything, so the target sends what today's code sends, in the same
order (S2). With one chunk, a waited chunk is the only chunk: no resync item exists (a resync
runs only on a `Ready` chunk), no client operation can start, and the waited list is the
queue's bulk items in queue order. Today's pass tries them in the same order, and starts
nothing once the slots are full: the same sends.

**Each bulk item is tried at most once per pump.** Lists have cursors, and an item that does not
start is skipped for the rest of the pump. Nothing it depends on changes within a pump (S4):
phases change only on replies; sessions open and fail only on replies and ticks (`send` never
closes one); locks are taken only by starts, repairs on reconciling chunks, and client
operations start only on serving chunks. A slot is free whenever an item is tried, so "does not
start" means its own conditions fail. `try_start` is unchanged: in the selection its `true`
means started with a slot, as pass 1 has dropped the stale resync items, the only bulk items it
drops.

**Client operations** are tried in pass 2, all of them, in queue order, with today's `blocked`
map: the same operations start as in `pump_in_order`. Bulk starts (on reconciling chunks, or
copies without locks) take no lock a client operation could need. So starting them first changes
the order of the sends within the pump, not what starts. A `cfg(test)` check in `pump` asserts
it: the selection leaves the locks of every `Ready` chunk, and the set of open sessions, as they
were. `try_fast_read` keeps its scan of the queued client operations.

**Cost:** two passes over the queue whenever a chunk does not serve (`unready > 0`), waited or
not; the tries of the bulk items and the in-flight counts over the maps only when one is
waited. During reconciliation every `submit` pumps: with 4 096 queued digests (1 TiB), two passes
instead of one.

## What a client waits for (S6)

It follows from the selection. Each time the waited chunk has an item that can start, the next
slot that frees goes to it (step 2), unless no resync item is in flight and the last start was
waited work, when one resync item goes first (step 1). Step 1 needs a waited start since, and
counts the resync item it starts, so it gives at most one resync item between two waited
starts, even with several slots free in one pump. Chunks earlier in the wait order come first.

## Resyncs (S7) and work sent again (S8)

No change to how work is sent again: `fail_peer`, `on_read`, `on_read_failed` and `on_digest`
push to the front, as today. Pass 1 classes items wherever they are, so a waited chunk's item
sent again is in the waited list, and one of a chunk not waited goes back to the queue's front,
where steps 3 and 4 find it first among its class.

A resync is capped (steps 1 and 3) only while waited work is in flight or can start: step 2 is
tried first, so at step 3 nothing waited can start, and `waited_in_flight` decides. A chunk still
fencing has no reconciliation items, so it caps nothing.

## A reply that does not match (fixed here)

**Today:** `on_reply` removes the request from the session's in-flight map before it matches the
reply. On a reply of the wrong type, or `on_digest`'s wrong length, it calls `fail_peer`, which
re-queues what is still in flight on that session: not the removed request. So its digest or read
stays in `digests` or `reads`, holding a slot, and never completes. With this issue, a dead digest
would keep its chunk waited, and a resync capped, for the target's life.

**The change:** the failure paths put the removed request back into the session's in-flight map,
under its id and send time, before they call `fail_peer`, which then handles it with the others:
in id order, re-queued at the front as any work sent again (S8). `on_digest` gains the request's
id and send time for this. `fail_peer` is unchanged.

## Test-only parts

- **Today's pump, as a switch:** `Bugs::today_pump` (`target.rs`, `cfg(test)`): `pump` always
  calls `pump_in_order`. Not a bug: it is there to compare runs (S2). It leaves S1 as is.
- **The accounting check:** `check_accounting(&self)` (`cfg(test)`). Each state-changing `pub`
  method (`tick`, `submit`, `on_reply`, `on_read_failed`, `on_meta`, `try_fast_read`,
  `finish_fast_read`, and `new`) becomes a wrapper: it calls its body (`*_inner`, the code of
  today), then the check, so early returns are covered. Tests that depose a target directly
  (`deposition_ends_reads_waiting_to_go_again`) call `depose_checked`, a `cfg(test)` wrapper of
  `depose`. It asserts:
  - `bulk` equals the bulk items in flight: `digests`, `reads` that are not a client's, `writes`
    with `client: None`, and session requests of kind `Copy` or `Discard`;
  - each `digests` entry has a `Digest` request in flight to each member it still needs, and each
    `reads` entry a `Read` request;
  - each chunk's `locks` are exactly the extents of its locked `reads`, its `writes`, and its
    fast reads not yet finished (a `cfg(test)` list, added to in `try_fast_read` and taken from
    in `finish_fast_read`);
  - on a deposed target, all of it empty, every chunk's `locks` included, and `bulk` 0.

  It runs in every target test, and in the simulator under `cargo test`, debug and release.
- **Stats** (`cfg(test)`, on the target): pumps that took the waited path; those with a resync
  item queued then; and, counted in `submit`, independently of the pump, client operations
  submitted while their chunk was not `Ready`. A chunk that serves never stops, so an operation
  can only ever wait if it was submitted so. The simulator adds a target's stats to three new
  `Report` fields (0 outside tests) when it replaces or drops the target (`sim/mod.rs`,
  `n.target = …`), and at the end.
- **The history:** `Sim::run_with_history(self) -> (Result<Report, String>, String)`
  (`cfg(test)`): the run, and its history's `Debug` text.

## The simulator

**`find_failure`** (`sim/tests.rs`) takes the configuration as a function of the seed, and a
range. For each seed, it runs with the switch on. On an error:
- the same seed and configuration run with no switch on. An error there panics, naming the seed
  and both errors: a bug to fix, not a catch;
- then, with `not linearizable` or `replicas diverge`: a catch;
- with another class (`liveness:`, `event storm`): counted, printed with the result, and the
  search goes on. On the campaign with more chunks, `move_without_resync` fails so on 490 of the
  first 2 000 seeds.

**The modes**, by environment:
- **`FLAMINIO_MUTATION_FIRST` unset** (`make sim`): from seed 0 over the check's budget. No
  catch panics, as today.
- **`FLAMINIO_MUTATION_FIRST` set** (a search): from it over `FLAMINIO_MUTATION_SEEDS` (default
  the budget). It prints `caught at seed N` or `searched [a, b): no catch`, and passes either way.
- **`catches_fast_reads_across_a_fence`:** its hint goes through `find_failure`'s checks (class,
  clean run) like any seed. Without `FLAMINIO_MUTATION_FIRST`, the hint only: one that does not
  catch panics, "the hint no longer catches: search again (docs/issues/11, the search)". With
  it, the hint first, then the range, unless `FLAMINIO_MUTATION_NO_HINT=1` skips the hint. The
  4 000 000-seed fall-through goes.

**The campaign with more chunks:** `fn more_chunks(seed) -> Config`, `Config::random(seed)` with
`chunks = 3 + seed % 6`.
- `campaign_with_more_chunks` runs it over `seeds()` (200 by default), and prints the runs with a
  resync item queued while a chunk was waited;
- `campaign_with_more_chunks_in_release`, ignored (`make sim`): 2 000 seeds, about 6 s.

**Six checks on it,** ignored like the others (`make sim`), budget 20 000:
`catches_no_reconciliation_more_chunks`, `catches_trust_single_digest_more_chunks`,
`catches_early_settle_more_chunks`, `catches_dirty_set_missing_inflight_more_chunks`,
`catches_move_without_resync_more_chunks`, `catches_copies_over_live_writes_more_chunks`. The
`mutation!` macro gets a form that takes the configuration.

**Today's pump, side by side:** `todays_pump_gives_the_same_runs`, over `seeds()`, on both
configurations: `Config::random` (1 or 2 chunks) and `more_chunks`. Each seed runs with the new
pump, then with `today_pump`.
- **Which runs must match:** one chunk, or no client operation submitted while its chunk was not
  `Ready` (the `submit` count, not the pump's). For those, the histories' text must be equal, and
  the results: both `Ok` with the reports' `Debug` text equal once the stats fields are zeroed
  (`Report` has no `PartialEq`), or both `Err` with the same text.
- **The rest:** waited pumps occur only in runs with such a submission; asserted on every run.

## The search

`scripts/dev/mutation-search.sh CHECK END [JOBS]` (JOBS defaults to `nproc`):
1. builds once, and takes the test binary's path from cargo's output (`cargo test --release
   --no-run -p flaminio-repl --lib --message-format=json`, its `executable`);
2. splits `[0, END)` into JOBS ranges, and runs the binary once per range, in parallel, each
   logging to `target/mutation-search/CHECK/<a>.log`: `FLAMINIO_MUTATION_NO_HINT=1
   FLAMINIO_MUTATION_FIRST=<a> FLAMINIO_MUTATION_SEEDS=<n> <binary> sim::tests::CHECK
   --include-ignored --exact --nocapture --test-threads=1`;
3. fails a range whose log does not say `running 1 test` (a wrong name runs nothing, and passes);
4. merges, in range order: a range that caught gives the answer, `first catch of CHECK: N`; a
   range with `no catch` goes on; any other end (a panic: a seed that fails with no switch on)
   stops, printing `searched [0, a) in full; [a, b) failed: <log>`. No catch at all: `no catch in
   [0, END)`.

**After the change:** every check with its budget as END, the fencing check with 4 000 000, and
the six on the campaign with more chunks. The results go into `sim/tests.rs` (the hint, the
18 962 comment) and §9's table. The fencing check's hint, 3 626 767, is a one-chunk run: if it
stops catching, or the search finds nothing below 4 000 000, S2 is broken, and the pump is fixed
before merging. There is no other fallback.

## Tests

In `target.rs` (`mod tests`). The bench gets:
- `Bench::with_chunks(placement: &[&[NodeId]], min_isr)`: one replica list per chunk
  (`create_volume` takes per-chunk lists already; today the bench passes one). Chunks on
  different nodes are what let a chunk be fenced later than another, or stay fencing: a
  `Fenced` reply fences its node for every chunk it holds;
- `restart(node)`: `Replica::restart` on that node's replica, then a new target attached at the
  next epoch (`MetaRequest::Attach`), so that its `Hello` replies carry whole chunks as
  uncertain. Tests that need a chunk still fencing hold those replies;
- `hold: BTreeMap<NodeId, Pred>`: replies held per destination. `Pred` stays a plain `fn`
  pointer, so the predicates are named functions (`is_hello`, `is_digest`, `is_read`…), and a
  hold per chunk comes from placement, not from the predicate. `fail_held` takes the node (today
  it assumes `TARGET`);
- `diverge(node, chunk, block, value)`: sets a block on one replica only (a `cfg(test)` setter on
  `Replica`'s data), before `restart`, so that digests disagree and repairs run. A restart alone
  leaves the replicas equal;
- a teardown (`Drop`, unless the thread is panicking). It drains first: clears `down`, `hold` and
  `hold_meta`, delivers what is held, then ticks until `is_settled()`, which it asserts within a
  bound (60 s of bench time). Opted out, by setting `deposed_ok`: the deposition tests,
  `deposition_ends_reads_waiting_to_go_again` and `deposition_fails_waiting_operations`. The
  other existing tests end with a node down, a removal pending or `hold_meta` set
  (`a_read_sent_elsewhere_may_come_back_to_the_local_replica`,
  `a_member_being_removed_does_not_count`,
  `a_waiting_read_goes_back_once_its_other_member_is_being_removed`): the drain settles them.

Each test of S1, S3 and S6 is built so that it fails on today's code: the waited chunk's work
sits behind other work in queue order.

| Spec | Test |
|---|---|
| S1 | `digests_of_chunks_fenced_together_go_from_chunk_0` |
| S2 | `with_nothing_waited_requests_go_in_queue_order` (a hand-written list: digests from chunk 0, a later-fenced chunk's ahead, a copy at the front) |
| S3.2 | `a_read_on_a_middle_chunk_reconciles_it_first` (chunk 2 of 5, queued before the fence completes); `a_write_moves_its_chunk_ahead`; `waited_chunks_go_in_wait_order` (and `max_bulk` 2: both under way); `two_pieces_go_in_submission_order`; `repairs_of_a_chunk_not_waited_wait` |
| S3.1, S3.3 | `waited_work_and_a_resync_alternate` (`max_bulk` 1, the waited chunk serves); `one_resync_item_in_flight_while_a_chunk_is_waited` (`max_bulk` 4, all four slots free in one pump) |
| S3.4 | `a_chunk_stuck_fencing_caps_no_resync` (chunk 0 on [1, 2, 3] resyncing 3; chunk 1 on [1, 4], `min_isr` 2. `with_chunks` waits until the target serves, so node 4 goes down after that, and the target restarts) |
| S3, S7 | `resyncs_progress_while_a_member_flaps` (many chunks waited, periodic session failures) |
| last start | `the_last_start_resets_when_nothing_is_waited` |
| S4 | `aborted_rounds_leave_the_queue`; `every_client_operation_is_tried_with_every_slot_taken` |
| S5 | `a_waited_read_returns_the_repaired_value`; `a_write_then_a_read_keep_their_order_while_waited` (and across a session failure); `fast_reads_wait_for_the_chunk_and_queued_operations`; `client_starts_match_todays_pump`; `overlapping_client_operations_start_in_queue_order` |
| S6 | `a_waited_chunk_takes_the_next_free_slot` (`max_bulk` 4); `a_chunk_fenced_first_and_waited_goes_before_one_fenced_later` (chunks on [1, 2] and [1, 3], node 3's `Hello` held, `max_bulk` 1); `a_chunk_fenced_late_takes_the_next_free_slot` (the same placement, `max_bulk` 1, the other chunk's digests differing: its repairs, queued at the front, start after the late chunk's digest; today, before it); `a_waited_chunk_goes_ahead_of_repairs_queued_before_it`; `a_waited_digest_that_cannot_start_holds_nothing` |
| S8 | `a_digest_sent_again_does_not_pass_a_waited_chunk` (another chunk's digest fails in flight and goes to the front; `min_isr` = ISR size) |
| S9 | `deposition_fails_waiting_operations`; the existing tests, unchanged |
| fix | `a_digest_reply_of_the_wrong_length_frees_its_slot`; `a_reply_of_the_wrong_type_requeues_its_request` (a digest; a client read with an overlapping write queued behind it, which still starts after it); `a_copy_reply_of_the_wrong_type_frees_its_slot_once` (a copy, then a discard: released once, nothing re-queued, the resync aborted); `a_mapped_reply_of_the_wrong_type_aborts_the_round` |
| all | `check_accounting` after every call; the teardown |

**`client_starts_match_todays_pump`:** 1 000 cases from the simulator's `Rng`. Two benches built
the same way, one with `today_pump`, several chunks, some restarted so that they reconcile, every
reconciliation reply held so that no phase changes. In half the cases chunk 0 serves and
resyncs, `max_bulk` is the reconciliation items plus 2, and its copy, discard and resync-digest
replies are delivered, so slots free and the selection keeps acting. The holds are by named
predicates that test the chunk (a `Digest` or `Read` of a chunk other than 0): a resync digest
and a reconciliation digest are the same request to the same node. In the other half `max_bulk`
is the queue's length. The same random calls on both:
client reads and writes on random chunks and extents, overlapping, and fast reads. The client
operations sent (reads and writes with a client, in order) and every `try_fast_read` answer must
be equal.

**`overlapping_client_operations_start_in_queue_order`:** 1 000 cases, one bench, replies
delivered in random order and at random times, so chunks become `Ready` along the way, with
session failures and replies of the wrong type injected. Random client operations, overlapping.
Invariant: of two operations on overlapping blocks, the one
submitted first starts first (a read sent again counts from its first submission).

In `sim/tests.rs`: `campaign_with_more_chunks` and its release form,
`todays_pump_gives_the_same_runs`, the six `_more_chunks` checks, the changed `find_failure`
and fencing check. `make test` and `make sim` run them; the scenarios and the existing checks
are unchanged.

## Measurement

The lab step of the spec, once the lab is free:
1. `scripts/checks/reconcile.sh` gains `FIRST_CHUNK` (default 0): the first read is block
   `FIRST_CHUNK × chunk blocks`; the loop then reads one block of every other chunk, from 0 up.
   Changed and reviewed before any run.
2. On the same day, without zero copy, `VOL=vol1 NODE=fl-a`: three runs each of `FIRST_CHUNK=0`
   and `FIRST_CHUNK=32`, first with main's binaries deployed, then with the branch's. Both use
   the branch's `reconcile.sh`: main's has no `FIRST_CHUNK`.
3. Recorded in `docs/benchmarks/` ("device ready", "first read", per run), with the follow-ups
   §2 update.

## Documents

- `docs/replication-protocol.md` §6 step 3: the order (S1, S3) in three sentences; §9: the table
  from the search, the six checks on the campaign with more chunks, and the search script.
- `docs/followups.md` §2: the "Left" item done, with the lab numbers.

## Risks

- **A one-chunk run that changes.** The hints depend on S2 holding exactly. The side-by-side test
  catches a drift on 200 seeds; the hint runs catch it on the two hint seeds.
- **The accounting check may find another existing leak** (`release_bulk` only `debug_assert`s
  today, which release builds skip). Then the leak is fixed here.
- **Pump cost while reconciling:** two passes, and the in-flight counts, per pump. Bounded by the
  queue's length; only while a chunk does not serve.
- **Redundancy:** a resync slows to one item at a time while waited work exists (S7). Accepted.
