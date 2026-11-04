# Issue #11: Reconcile first the chunks that clients wait on — specification

Approved at the spec gate (2026-10-30), with the wait bound of S6.

Terms:
- a chunk **serves** once it is `Ready`: fenced and reconciled (protocol §6, steps 1–4). Within
  a target a chunk only moves forward (`advance_chunk` is the only place that sets a phase), so
  a chunk that serves never stops;
- a **bulk item** is queued work that holds one of the target's `max_bulk` slots (4 in the data
  plane) until it is done:
  - **reconciliation items:** a digest (one request to each ISR member, done when all have
    answered); a repair (a read from a source, then a write, done when its write quorum, the
    ISR plus any joiner, has acknowledged it);
  - **resync items:** a copy, a discard, a resync digest.

  `Mapped` probes and `BeginResync` are not bulk items: `drive_resync` and `on_mapped` send them
  directly;
- a **source** of a chunk is an ISR member whose removal is not in flight and whose session is
  open (`source`, `may_read_from`);
- an item **can start** when a slot is free and, as in `try_start` today:
  - a digest: its chunk reconciling, every ISR member's session open;
  - a repair: its chunk reconciling, its blocks unlocked, a source;
  - a copy: a source other than its resync's member;
  - a discard: its resync's member's session open;
  - a resync digest: that session open, and a source other than the member.

  A resync item whose round is no longer the chunk's (aborted or finished) is **dropped** when
  tried, without a slot, as today;
- a client operation **waits on** a chunk while it is queued and the chunk does not serve, read
  or write, on any blocks of the chunk. Only client operations make a chunk waited: block I/O
  submitted by the local node (`Target::submit`), not peers or the metadata service. A fast read
  refused for it (`try_fast_read`) and then submitted waits like any other. A chunk is
  **waited** while an operation waits on it;
- the **wait order** is the order in which waited chunks first appear among the queued client
  operations. Client operations sent again go to the front, but only on chunks that serve (S5),
  so this is the submission order of each chunk's first waiting operation;
- **waited work in flight:** a waited chunk's digest or repair in flight;
- **resync items in flight** are counted from what is in flight (resync digests, copies,
  discards), not kept as a separate count;
- the **last start:** whether the last slot given went to a waited chunk's item. It is reset to
  no whenever no chunk is waited;
- a **progress pass** is `progress`'s pass over the chunks; a **pump pass** is `pump`'s pass
  over the queue.

## Behaviour

**S1. The queue order changes in one place.** The digests of chunks whose fence completes in
the same progress pass are queued from the lowest chunk number up, ahead of what was queued
before. Today they are queued from the last chunk down. A chunk fenced in a later progress pass
(other replicas, a late member) is still queued ahead of those, as today. Nothing else is
sorted: that would change the order with no chunk waited (S2).

**S2. With no chunk waited, `pump` tries the queue in its order, as today.** The target then
sends the same requests, in the same order, as today's code for the same queue. A run with one
chunk keeps its schedule exactly: a waited chunk is then the only chunk, and holds every bulk
item, in queue order; a chunk with a resync serves, so it is not waited. A one-chunk seed whose
run changes is a regression, not a hint to update.

**S3. With a chunk waited, each free slot goes to the first that applies:**
1. the first resync item that can start, in queue order, if no resync item is in flight and the
   last start went to a waited chunk's item;
2. the first waited chunk's reconciliation item that can start: chunk by chunk in the wait
   order, within a chunk in queue order;
3. if waited work is in flight: the first resync item that can start, in queue order, if no
   resync item is in flight; failing that, the first item that can start and is not a resync
   item, in queue order;
4. otherwise, the first item that can start, in queue order, as today.

So the cap of one resync item in flight holds only while a waited chunk has a reconciliation
item that can start, or one in flight. A waited chunk stuck in fencing (fewer ISR members fenced
than the minimum), or whose ISR member's session is down at the minimum, caps nothing. Resync
items already in flight when a chunk becomes waited run to their end. With `max_bulk` 1, waited
items and resync items alternate while both can start.

**S4. One pump sees everything.**
- **Within one pump, no chunk changes phase:** replies come in later calls. So the waited
  chunks and the wait order, found once per pump, hold for the whole pump.
- **Every pump tries every queued client operation,** even once every slot is taken, and drops
  every stale resync item (as today). A joiner whose session keeps failing cannot grow the
  queue: each aborted round's items go in the next pump.
- **`pump` makes at most two pump passes,** and tries each bulk item at most once: one pass
  finds the waited chunks and the items of each class, one starts work. A 1 TiB volume queues
  about 4 096 digests.

**S5. Client operations keep one order.** S3 orders bulk items only.
- **One sequence, in submission order,** which both `pump` and `try_fast_read` see. Waited chunks
  never reorder it. On overlapping blocks, operations start in that order. A fast read is
  refused on a chunk that does not serve, and on blocks a queued client operation overlaps.
- **A client read sent again** goes to the front, as today: after its session failed, after it
  failed on the local replica (issue #5), or when the lease lapsed. All three happen only on
  chunks that serve.

**S6. What a client operation waits for.** The issue's "only for its own chunk" becomes (approved):
- its chunk's fence (S9);
- its chunk's reconciliation items;
- the reconciliation items of chunks earlier in the wait order;
- **each time its chunk has an item that can start:** the items in flight when it became
  startable, plus at most one resync item started after them (S3.1).

While its chunk has nothing that can start (still fencing, an ISR member's session down, its one
digest in flight), freed slots go to other work, which may then hold them. So none of this is
bounded in time, here or today: a repair holds its slot until its write quorum acknowledges, a
write to a failed member waits for its removal or, at the ISR minimum, for the member
(`member_failed`), and a chunk with fewer ISR members fenced than the minimum waits until enough
answer.

A ublk request is at most 1 MiB (`--io-buf`), and chunks are at least 64 MiB
(ARCHITECTURE), so one request covers one or two chunks. Many requests in flight (ublk's queue
depth, per queue) can wait on many chunks at once.

**S7. Resyncs.** S3 keeps them going while clients wait, at one item at a time.
- **What it costs:** a serving chunk with a replica out of its ISR stays a copy short longer
  while waited work exists. A member that fails and reconnects again and again, at the ISR
  minimum, is never removed, and sends a waited chunk's digest or repair read again each time
  (`fail_peer`): resyncs then go at the rate of S3 for as long. Accepted: clients are waiting.
- **With no chunk waited,** resync items keep today's place: a copy at the front.

**S8. Bulk work sent again keeps today's place in the queue:** the front. S3 then puts a waited
chunk's items first. What is sent again is unchanged:
- a digest or resync digest whose session failed;
- a repair only in its read phase. Its write is sent again on reconnect (`retransmit`), and
  keeps its slot;
- a copy only when its source's session failed. When the joiner's fails, the resync is aborted;
- a discard never: its resync is aborted.

**S9. Unchanged:**
- **Fencing:** a chunk serves only once every ISR member is fenced. Members still silent after
  the takeover wait `L × (1 + ρ)` are removed by `ChangeIsr`, and the chunk goes on once that
  commits (§6 step 2). Being waited shortens none of it.
- **What is reconciled:** the union of the ISR members' uncertain sets. Digests are compared
  across the whole ISR, one authority's value is written to the ISR as a write of the new
  epoch, and the chunk settles once its repairs have landed.
- **ISR changes during reconciliation:** a member removed while its chunk reconciles has the
  whole chunk as its dirty set.
- **Epochs:** the order belongs to one target. A deposed target fails its waiting operations.
- **The data plane's sizes:** one digest per default chunk (65 536 blocks), repairs of 256
  blocks, 4 bulk items in flight.

**S10. Recorded.** The protocol, §6 step 3: the order of S1 and S3; §9: the first catching
seeds after the search below. The follow-ups, §2: the "Left" item done; the uncertain set's
persistence stays item 2.

**S11. A dropped replica's acknowledgements are forgotten.** When a chunk's replica list drops a
node (a move or a removal commits), the chunk's writes in flight forget that node's
acknowledgements and sends. If the node comes back as a new replica, a write counts as on it
only once the new replica holds it: acknowledged by it, or copied by its resync. A write is
never completed on an acknowledgement from a replica that no longer exists. Already broken on
main; the new order reaches it (simulator seed 1 183 196: `replicas diverge`). Added at
implementation, approved 2026-11-04.

## Tests

- **S11:** a target test where a move drops a replica that has acknowledged a write still
  waiting on another member, and the node rejoins before the write completes: the new replica
  gets the block, and the write completes only once it holds it. A simulator check with an
  injected bug that keeps the dropped replica's acknowledgements; seed 1 183 196 passes with no
  bug injected.
- **Target** (`flaminio-repl`, `target.rs` tests). The bench, today one settled chunk with only
  the target's own replica's replies held, gains: several chunks; a replica restart, so that
  whole chunks are uncertain; replies held back from any node. With `max_bulk` 1 unless said:
  - **no chunk waited** (S1, S2): with resync and reconciliation items of several chunks queued,
    the requests sent are a hand-written list, in order: digests from chunk 0 up for chunks
    fenced together (today's code: from the last down), a later-fenced chunk's ahead, a copy at
    the front;
  - **the waited order** (S3):
    - a read queued on the last chunk before the fence completes: its digest goes first;
    - `max_bulk` 4, four other chunks' digests in flight, a read on a chunk still queued: its
      digest is the next sent, once the first digest in flight completes;
    - a write moves its chunk ahead as a read does;
    - two waited chunks: the first in the wait order goes first; with `max_bulk` 2, both under
      way at once;
    - two operations submitted one after the other, on two chunks (as the engine submits a
      request's two pieces): the first's chunk first;
    - a chunk not waited, with differing digests, does not start its repairs while a waited
      chunk has work that can start;
  - **resyncs** (S3, S7):
    - `max_bulk` 1, a waited chunk with several repairs and a serving chunk resyncing: the
      starts alternate, waited and resync, and the waited chunk serves;
    - `max_bulk` 4: never more than one resync item in flight once a chunk is waited, those
      already in flight excepted;
    - a waited chunk stuck fencing (fewer ISR members fenced than the minimum): a resync on
      another chunk uses every slot;
    - operations on many chunks queued at once, and a member of their ISRs failing and
      reconnecting periodically: the resync progresses and ends, and so does every chunk;
    - a joiner whose session keeps failing: its aborted rounds' items leave the queue in the
      next pump (S4);
  - **the last start** resets once no chunk is waited: a new waited chunk's first slot goes to
    it (S3.2), not to a resync item;
  - **the bound** (S6): the waited chunk fenced after the others; and its one digest in flight
    while another chunk's repairs are queued. Freed slots go to that work meanwhile; then the
    waited chunk takes the next free slot;
  - **no idle slot:** a waited chunk whose digest cannot start (an ISR member's session down)
    does not stop other chunks' digests;
  - **every client operation tried** (S4): every slot taken, a client read queued on a serving
    chunk starts in the same pump;
  - **sent again** (S8), with `min_isr` equal to the ISR's size, so that a failed session starts
    no removal: another chunk's digest fails in flight and goes back to the front; a waited
    chunk's digest still goes first;
  - **integrity** (S5):
    - a waited chunk whose digests differ: its queued read is sent only once the repairs are
      acknowledged, and returns the authority's value;
    - a write, then a read of the same block, queued while the chunk is waited: in that order,
      the read returns the write's value; the same with a session failing between them;
    - a fast read on a waited chunk is refused until it serves, then while an overlapping write
      is queued, and while an overlapping read sent again is queued (after a failed local read,
      after a lapsed lease);
    - `a_failed_read_goes_again_before_later_overlapping_reads` still passes;
    - **client starts unchanged,** a property test: on random queues (several chunks, some
      serving, overlapping client operations, bulk items, chunks waited), the new `pump` starts
      the same client operations, in the same order, as today's, and `try_fast_read` admits the
      same reads. The simulator never queues overlapping operations;
  - **deposition** while chunks are waited: every waiting operation fails;
  - **in every test but deposition,** every chunk serves and the target ends settled;
  - **after every call, in every test** (a `cfg(test)` check, also run by the simulator's
    campaigns and mutation checks under `cargo test`, in debug and release): `bulk` equals the
    bulk items in flight.
- **Simulator:**
  - **as today:** `make test` runs the campaigns (200 seeds) and the scenarios,
    `scenario_split_brain_catches_missing_lease_and_wait` included; `make sim` runs the example
    binary over 20 000 seeds, then every mutation check, in release;
  - **today's pump, side by side:** a test-only switch makes `pump` today's. On one-chunk seeds,
    and on runs in which no chunk is ever waited, the runs with and without it are identical
    (same history, same report);
  - **every catch is checked** (`sim/tests.rs`, under `cargo test`; the example binary runs no
    checks). Every mutation check is a safety switch, so a catch is a failure with `not
    linearizable` or `replicas diverge`. A failure of another class is not a catch: it is
    counted, printed, and the search goes on. Every failing seed, caught or not, must pass with
    no switch on; if it does not, the check fails at once, naming the seed: that is a bug, not a
    catch. Today
    (measured on centaur, release, 47 s for all):

    | Check | First seed | Error |
    |---|---|---|
    | `catches_no_reconciliation`, `catches_trust_single_digest` | 1, 1 | not linearizable; diverge |
    | `catches_move_without_resync`, `catches_drop_before_join` | 2, 3 | not linearizable; diverge |
    | `catches_dirty_set_missing_inflight`, `catches_old_sessions_accepted` | 7, 16 | diverge |
    | `catches_ack_before_shrink`, `catches_late_join` | 49, 49 | diverge; not linearizable |
    | `catches_no_takeover_wait`, `catches_reconciliation_pruning` | 75, 104 | not linearizable |
    | `catches_copies_over_live_writes`, `catches_early_settle` | 112, 145 | not linearizable |
    | `catches_reads_without_lease`, `catches_fast_reads_without_lease` | 276, 276 | not linearizable |
    | `catches_fast_reads_from_a_replaced_replica` | 18 962 | not linearizable |
    | `catches_fast_reads_across_a_fence` | 3 626 767 (hint) | not linearizable |

  - **a campaign with more chunks** in `make test`: `Config::random(seed)`, then
    `chunks = 3 + seed % 6`, other fields unchanged, 200 seeds. On today's code, one thread on
    centaur: 5.8 s in a debug build (the existing campaign: 4.7 s); 2 000 seeds in 6.2 s in
    release. It reports how many runs had a resync item queued while a chunk was waited. `make
    sim` runs it over 2 000 seeds (about 6 s), not 20 000: about a minute more, for runs the
    unit tests already pin down;
  - **six checks on it too,** in `make sim`, with a budget of 20 000 seeds. On today's code,
    first safety catches: `no_reconcile` 1, `trust_single_digest` 1, `early_settle` 85,
    `no_dirty_inflight` 35, `move_without_resync` 2, `copies_over_live_writes` 116. (On this
    campaign `move_without_resync` also fails 490 of the first 2 000 seeds with `liveness:`,
    which do not count.)
- **The checks searched again** (issue step 2), after the change:
  - **test code:** `catches_fast_reads_across_a_fence` honours `FLAMINIO_MUTATION_FIRST`, and
    `FLAMINIO_MUTATION_NO_HINT=1` skips its hint. A search that finds nothing prints "searched
    [a, b): no catch";
  - **the search:** every check from seed 0, split over centaur's 24 cores, one exact test per
    process: `FLAMINIO_MUTATION_NO_HINT=1 FLAMINIO_MUTATION_FIRST=<a> FLAMINIO_MUTATION_SEEDS=<n>
    cargo test --release -p flaminio-repl sim::tests::<check> -- --include-ignored --exact
    --nocapture`. About 3.6 M seeds for the fencing check: minutes over 24 cores;
  - **merging:** a check's first seed is the lowest caught in a range all of whose lower ranges
    were searched in full without a catch. A range that stops on a seed failing with no switch
    on is recorded as searched up to that seed, and the search stops: that seed is a bug to fix
    first;
  - **updated:** the hint, if the fencing check's first seed is no longer 3 626 767; the
    comment for 18 962 if that moved; §9's table. The budgets stay. A one-chunk seed that moved
    is a regression (S2);
  - **in `make sim`,** the fencing check tries its hint only. If the hint does not catch, the
    check fails at once, with a message that names this procedure. It no longer falls through
    to a 4 000 000-seed search.
- **Lab** (a later step: the lab is busy). `vol1` (replicas on fl-a and fl-c), the `reconcile`
  check, without zero copy, three runs before the change and three after, the same day:
  - **the script first:** `scripts/checks/reconcile.sh` reads block 0 first, then one block of
    every chunk from 1 up. It gains `FIRST_CHUNK` (default 0): the first read is of that chunk's
    first block, and the loop then reads every other chunk once, from 0 up;
  - **recorded,** with `FIRST_CHUNK` 0 and 32: "device ready" and "first read". Today block 0
    comes 0.21–0.25 s after the attach request, behind all 64 chunks' digests. Expected: well
    under that. The 0.10–0.12 s of an attach with nothing to reconcile (issue #6) was measured
    with zero copy, so it only indicates;
  - **chunk 32 first:** today it waits for about half the chunks. Expected: its own digests, up
    to 4 items in flight when it came, and the chunks waited on before it. Whether the kernel's
    partition scan and udev's probes (block 0, the device's end) are queued before `dd` is a
    race: on Linux 7.0 the scan runs after `START_DEV` (issue #6). So chunk 0 and the last chunk
    may come first;
  - the check's limits stay. The comparison is recorded in the results, not enforced.
