# Issue #11: Reconcile first the chunks that clients wait on — design

Everything is in `dataplane/crates/flaminio-repl`: the target (`src/target.rs`), the simulator's
tests (`src/sim/tests.rs`, `src/sim/mod.rs`), and one script. The data plane (`flaminio-node`)
calls `submit`, `tick` and the reply handlers as before: no change there.

## The queue order (S1)

**Today:** `advance_chunk` pushes a newly fenced chunk's digests to the front of the queue, and
`progress` calls it for chunks 0 up, so the last chunk ends first.

**The change:** `advance_chunk` no longer pushes. Where it sets `Phase::Reconciling`, it appends
`(i, pieces)` to a new field, `fenced: Vec<(usize, Vec<Extent>)>`, with the pieces computed as
today (`pending.pieces(self.max_digest)`). A new `queue_fenced()` then pushes them to the front:
the chunks in reverse order, each chunk's pieces in reverse, so the queue starts with the lowest
chunk's first piece.
- **`progress`** calls it after its loop over the chunks, before `send_meta` and `pump`;
- **`on_reply`'s `Scope::Chunk` path** calls it after its single `advance_chunk`, before `pump`.

Between the loop and `queue_fenced`, nothing reads the queue: `advance_chunk` sets phases, sends
`Settle` and `Mapped`, and proposes ISR changes, none of which look at queued work. So the only
difference from today is the order among chunks fenced in the same progress pass. A chunk with
no uncertain blocks still goes to `Ready` in the same call, with nothing queued.

## Which chunks are waited

**New fields of `Target`:**
- `unready: usize`: chunks not `Ready`. `chunks.len()` in `new`; decremented where
  `advance_chunk` sets `Phase::Ready`, the one place that does. Phases only move forward, so it
  only falls;
- `last_waited: bool`: the last start (S3). Set by the waited path of `pump` (below), cleared
  whenever `pump` finds no chunk waited, and by `depose`.

Nothing else is kept: the waited chunks, their order, the resync items in flight and the waited
work in flight are found at each pump.

**`Work::class(&self) -> Class`** (new), the one place that sorts items by kind:
`Client`, `Reconcile(chunk)` (`Digest`, `Repair`), `Resync(chunk, round)` (`Copy`, `Discard`,
`ResyncDigest`). The pump's lists and the counts below use it, and nothing else decides an item's
kind.

**In flight, counted from the maps** (`resync_in_flight`, `waited_in_flight`, new):
- resync items: `digests` with `resync: Some`, `reads` with `ReadFor::Copy`, and session
  requests in flight of kind `ReqKind::Copy` or `ReqKind::Discard`;
- waited work: `digests` with `resync: None`, `reads` with `ReadFor::Repair`, and `writes` with
  `client: None`, on a waited chunk.

They are computed only on the waited path, so a target whose chunks all serve pays nothing.

## `pump` (S2–S5)

```text
pump(now):
  if deposed: return
  if unready == 0 (or the test switch, below): last_waited = false; pump_in_order(now); return
  pass 1: take the queue into a Vec<Option<Work>>, and in queue order:
    - drop a resync item whose round is not its chunk's (try_start's test, as today);
    - a client operation on a chunk not Ready: give the chunk the next wait rank, if it has none;
    - note each bulk item's index in its class list: reconcile (per chunk), resync, other.
  if no chunk is waited: last_waited = false; put the items back; pump_in_order(now); return
  waited list: each waited chunk's reconcile indices, chunk by chunk in rank order.
  others: the reconcile indices of chunks not waited (queue order).
  select (S3), while a slot is free and the last round started something:
    1. resync_in_flight == 0 and last_waited: the first resync item that starts;
    2. the first waited item that starts                     -> last_waited = true;
    3. waited_in_flight > 0: the first resync item that starts, if resync_in_flight == 0;
                             failing that, the first item of others that starts;
    4. otherwise: the first item that starts, of resync and others merged by index.
    Any start other than step 2 sets last_waited = false.
  pass 2: in queue order, skip the items started or dropped, try each client operation as
    today (the `blocked` map), and keep the rest as the new queue.
```

**`pump_in_order`** is today's `pump`, unchanged. With all chunks serving, or none waited, it is
the only pass that starts anything, so the target sends what today's code sends, in the same
order (S2). With one chunk, a waited chunk is the only chunk: no resync item exists (a resync
runs only on a `Ready` chunk), no client operation can start, and the waited list is the queue's
bulk items in queue order. Today's pass tries them in the same order, and stops starting when the
slots are full: the same sends.

**Each bulk item is tried at most once per pump.** Lists have cursors. An item that does not
start is skipped for the rest of the pump. That is sound because nothing it depends on changes
within a pump (S4): phases change only on replies; sessions only close (a failed send); locks
are taken only by starts, on reconciling chunks for repairs, and client operations start only on
serving chunks. A slot is free whenever an item is tried, so "does not start" means its own
conditions fail.

**Whether an item started, and took a slot:** `try_start` is unchanged and still returns whether
the item leaves the queue. The pump compares `self.bulk` before and after. A stale resync item
leaves without a slot, as today, but pass 1 has already dropped those.

**Client operations** are tried in pass 2, all of them, in queue order, with today's `blocked`
map: the same operations start as in `pump_in_order`. Bulk starts (on reconciling chunks, or
copies without locks) never take a lock a client operation could need, so starting them first
changes the order of the sends within the pump, not what starts. `try_fast_read` keeps its scan
of the queued client operations.

**Cost:** two passes over the queue, plus one try of each bulk item, and the in-flight counts
over the maps, only while a chunk does not serve. During reconciliation every client operation's
`submit` pumps: with 4 096 queued digests (1 TiB), two passes instead of one.

## What a client waits for (S6)

It follows from the selection. Each time the waited chunk has an item that can start, the next
slot that frees goes to it (step 2), unless no resync item is in flight and the last start was
waited work, when one resync item goes first (step 1). Step 1 needs a waited start since, so it
gives at most one resync item between two waited starts. Chunks earlier in the wait order come
first (rank order).

## Resyncs (S7) and work sent again (S8)

No change to how work is sent again: `fail_peer`, `on_read`, `on_read_failed` and `on_digest`
push to the front, as today. Pass 1 classes items wherever they are, so a waited chunk's item
sent again is in the waited list, and one of a chunk not waited goes back to the queue's front,
where steps 3 and 4 find it first among its own.

A resync is capped (steps 1 and 3) only while waited work is in flight or can start (step 2 was
tried first and failed, so at step 3 it cannot start; then `waited_in_flight` decides). A chunk
still fencing has no reconciliation items, so it caps nothing.

## Test-only parts

- **Today's pump, as a switch:** `Bugs::today_pump` (`target.rs`, `cfg(test)`): `pump` always
  calls `pump_in_order`. Not a bug: it is there to compare runs (S2). It leaves S1 as is.
- **The accounting check:** `check_accounting(&self)` (`cfg(test)`), at the end of every `pub`
  method that changes state (`new`, `tick`, `submit`, `on_reply`, `on_read_failed`, `on_meta`,
  `try_fast_read`, `finish_fast_read`). It asserts that `bulk` equals the bulk items in flight:
  `digests`, `reads` that are not a client's, `writes` with `client: None`, and session requests
  of kind `Copy` or `Discard`. On a deposed target, 0. It runs in every target test, and in the
  simulator under `cargo test`, debug and release. `release_bulk`'s `debug_assert` stays.
- **Stats** (`cfg(test)`): pumps that took the waited path, and those with a resync item queued
  then. The simulator adds a target's stats to the run's `Report` (two new fields, 0 outside
  tests) when it replaces or drops the target (`sim/mod.rs`, `n.target = …`), and at the end.

## The simulator

**`find_failure`** (`sim/tests.rs`) takes the configuration as a function of the seed, and a
range. For each seed:
- run with the switch on. `Ok`: next seed;
- an error with `not linearizable` or `replicas diverge`: run the same seed and configuration
  with no switch on. `Ok`: a catch. An error: panic, naming the seed and both errors (a bug to
  fix, not a catch);
- an error of another class (`liveness:`, `event storm`): counted, and the search goes on. On
  the campaign with more chunks, `move_without_resync` fails so on 490 of the first 2 000 seeds.

The count of skipped failures is printed with the result.

**The modes**, by environment:
- **`FLAMINIO_MUTATION_FIRST` unset** (`make sim`): from seed 0 over the check's budget. No catch
  panics, as today.
- **`FLAMINIO_MUTATION_FIRST` set** (a search): from it over `FLAMINIO_MUTATION_SEEDS` (default
  the budget). It prints `caught at seed N` or `searched [a, b): no catch`, and passes either way.
- **`catches_fast_reads_across_a_fence`:** without `FLAMINIO_MUTATION_FIRST`, its hint only. A
  hint that does not catch panics: "the hint no longer catches: search again (docs/issues/11,
  the search)". `FLAMINIO_MUTATION_NO_HINT=1` skips the hint; with `FLAMINIO_MUTATION_FIRST` it
  searches its range like the others. The 4 000 000-seed fall-through goes.

**The campaign with more chunks:** `fn more_chunks(seed) -> Config`, `Config::random(seed)` with
`chunks = 3 + seed % 6`. A test, `campaign_with_more_chunks`, runs it over `seeds()` (200 by
default), and prints the runs with a resync item queued while a chunk was waited.

**Six checks on it,** ignored like the others (`make sim`), budget 20 000:
`catches_no_reconciliation_more_chunks`, `catches_trust_single_digest_more_chunks`,
`catches_early_settle_more_chunks`, `catches_dirty_set_missing_inflight_more_chunks`,
`catches_move_without_resync_more_chunks`, `catches_copies_over_live_writes_more_chunks`. The
`mutation!` macro gets a form that takes the configuration.

**Today's pump, side by side:** `todays_pump_gives_the_same_runs`, over `seeds()`: each seed run
twice, with and without `today_pump`. When the run has one chunk, or its stats show no waited
pump, the two histories (their `Debug` text) and reports must be equal.

## The search

`scripts/dev/mutation-search.sh CHECK END [JOBS]` (new; JOBS defaults to `nproc`):
1. builds once (`cargo test --release --no-run -p flaminio-repl`);
2. splits `[0, END)` into JOBS ranges, and runs one process per range, in parallel, each logging
   to `target/mutation-search/CHECK/<a>.log`:
   `FLAMINIO_MUTATION_NO_HINT=1 FLAMINIO_MUTATION_FIRST=<a> FLAMINIO_MUTATION_SEEDS=<n> cargo test
   --release -q -p flaminio-repl sim::tests::CHECK -- --include-ignored --exact --nocapture
   --test-threads=1`;
3. fails a range whose log does not say `running 1 test` (a wrong name runs nothing, and passes);
4. merges, in range order: a range that caught gives the answer, `first catch of CHECK: N`; a
   range with `no catch` goes on; any other end (a panic: a seed that fails with no switch on)
   stops, printing `searched [0, a) in full; [a, b) failed: <log>`. No catch at all: `no catch in
   [0, END)`.

**After the change:** every check with its budget as END, the fencing check with 4 000 000, and
the six on the campaign with more chunks. The results go into `sim/tests.rs` (the hint, the
18 962 comment) and §9's table. A one-chunk seed that moved is a regression of S2, to fix in the
pump, not a hint to update.

## Tests

In `target.rs` (`mod tests`). The bench gets `Bench::with_chunks(n, replicas, min_isr)`, a
`restart(node)` that gives a replica's chunks whole uncertain sets (a fresh `Replica` fenced at
the old epoch), and `hold: Option<Pred>` for any node, keyed by destination.

| Spec | Test |
|---|---|
| S1 | `digests_of_chunks_fenced_together_go_from_chunk_0` |
| S2 | `with_nothing_waited_requests_go_in_queue_order` (a hand-written list: digests from chunk 0, a later-fenced chunk's ahead, a copy at the front) |
| S3.2 | `a_read_on_the_last_chunk_reconciles_it_first`; `a_write_moves_its_chunk_ahead`; `waited_chunks_go_in_wait_order` (and `max_bulk` 2: both under way); `two_pieces_go_in_submission_order`; `repairs_of_a_chunk_not_waited_wait` |
| S3.1, S3.3 | `waited_work_and_a_resync_alternate` (`max_bulk` 1, the waited chunk serves); `one_resync_item_in_flight_while_a_chunk_is_waited` (`max_bulk` 4) |
| S3.4 | `a_chunk_stuck_fencing_caps_no_resync` |
| S3, S7 | `resyncs_progress_while_a_member_flaps` (many chunks waited, periodic session failures) |
| last start | `the_last_start_resets_when_nothing_is_waited` |
| S4 | `aborted_rounds_leave_the_queue`; `every_client_operation_is_tried_with_every_slot_taken` |
| S5 | `a_waited_read_returns_the_repaired_value`; `a_write_then_a_read_keep_their_order_while_waited` (and across a session failure); `fast_reads_wait_for_the_chunk_and_queued_operations`; `client_starts_match_todays_pump` |
| S6 | `a_waited_chunk_takes_the_next_free_slot` (`max_bulk` 4); `a_chunk_fenced_late_takes_the_next_slot`; `a_waited_digest_in_flight_lets_others_go`; `a_waited_digest_that_cannot_start_holds_nothing` |
| S8 | `a_waited_digest_sent_again_goes_first` (`min_isr` = ISR size) |
| S9 | `deposition_fails_waiting_operations`; the existing tests, unchanged |
| all | `check_accounting` after every call |

**`client_starts_match_todays_pump`:** 1 000 cases from the simulator's `Rng`. Two benches built
the same way, one with `today_pump`, several chunks, some restarted so that they reconcile, every
replica reply held so that no phase changes. The same random calls on both: client reads and
writes on random chunks and extents, overlapping, and fast reads. The client operations sent
(reads and writes with a client, in order) and every `try_fast_read` answer must be equal.

In `sim/tests.rs`: `campaign_with_more_chunks`, `todays_pump_gives_the_same_runs`, the six
`_more_chunks` checks, the changed `find_failure` and fencing check. `make test` and `make sim`
run them; the scenarios and the existing checks are unchanged.

## Measurement

The lab step of the spec, once the lab is free:
1. `scripts/checks/reconcile.sh` gains `FIRST_CHUNK` (default 0): the first read is block
   `FIRST_CHUNK × chunk blocks`; the loop then reads one block of every other chunk, from 0 up.
   Changed and reviewed before any run.
2. On the same day, without zero copy, `VOL=vol1 NODE=fl-a`: three runs each of `FIRST_CHUNK=0`
   and `FIRST_CHUNK=32` on main, then on the branch.
3. Recorded in `docs/benchmarks/` ("device ready", "first read", per run), with the follow-ups
   §2 update.

## Documents

- `docs/replication-protocol.md` §6 step 3: the order (S1, S3) in three sentences; §9: the table
  from the search, the six checks on the campaign with more chunks, and the search script.
- `docs/followups.md` §2: the "Left" item done, with the lab numbers.

## Risks

- **A one-chunk run that changes.** The hints depend on S2 holding exactly. The side-by-side test
  catches a drift on 200 seeds; the hint runs catch it on the two hint seeds.
- **The accounting check may find an existing leak** (`release_bulk` only `debug_assert`s
  today, which release builds skip). Then the leak is fixed here.
- **Pump cost while reconciling:** two passes, and the in-flight counts, per pump. Bounded by the
  queue's length; only while a chunk does not serve.
- **Redundancy:** a resync slows to one item at a time while waited work exists (S7). Accepted.
- **The search's wall time:** about 3.6 M seeds for the fencing check, in minutes over 24 cores,
  if the hint stays where it is.
