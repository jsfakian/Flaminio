# Issue #10: 1M sequential writes at about 70% of raw — design

The changes start from the measured prototype (`docs/benchmarks/data/2026-10-29-writes-1m/`,
`prototype.diff`), made sound, with the safety fixes that holding by default needs.

**The shipped rules differ from the prototype:**
- **The count target:** what was outstanding at the landing, not `max(writes + queued,
  ceil(8 MiB / average))`.
- **The count rule:** gated on arrivals until the window passes.
- **Arrivals:** only large writes are counted, and only large writes skip the staging wait. The
  prototype counted and skipped for every write.
- **An expired landing wait:** it is re-armed at the group's first write, instead of starting the
  group at once.
- **The wait's kind:** it comes from the writes (large or not), not from the average size.
- **Which waits hold on arrivals:** only `Wait::Large`. The prototype held any wait once a floor
  was set.
- **`maybe_launch_staged`:** it checks the wait at once after arming. The prototype returned.
- **The gather timer** takes deadlines (below).

At 1M QD8 in steady state these coincide: 16 writes, all large, no group at the landing. So the
849 MiB/s should carry over. The results report the shipped rules, measured, not the prototype.

## Holding by default (S1)

**The default:** `DEFAULT_COMMIT_TARGET_BYTES` (`flaminio-node/src/engine.rs`) becomes 8 MiB.
- **`Engine::new`** applies it through `set_commit_target_bytes`, so the replica service and the
  store hold too (`set_hold_staged`).
  - **Why:** today `Engine::new` sets the field only, and `run_device` calls the setter only when
    `--commit-target-mib` is given. With a non-zero default, the engine would hold while the
    service did not.
  - **The check:** a `debug_assert` in `finish_commit` that the engine and the service agree,
    both read as `commit_target_bytes > 0`.
- **Which flag decides:** `finish_commit`'s branch and the assert use `commit_target_bytes > 0`,
  the service's hold. They do not use `holding()`, which `set_async_commits(false)` clears at
  detach before it settles.
- **When it arms:** arming and re-arming a wait happen only when `async_commits` is set, as today.
  A replica-only node (sync commits) has the floor and the service's hold set by default, but
  `holding()` is false there: it stages nothing, and never arms a wait.
- **Turning the floor to 0:** `set_commit_target_bytes(0)` calls `settle_local` first when a group
  is staged, so the group is not stranded.
- **One setting:** hold and the floor stay one setting. So the three places that mirror a
  landing stay consistent:
  - `Store::poll_write_batch`;
  - `ReplicaService::poll`;
  - `finish_commit`'s held branch.
- **`--commit-target-mib`** keeps `None` as "the engine's default". `0` turns holding off.

**The count rule** (`arm_gather`):
- **With holding, for a large-write wait:** the count target is the writes outstanding at the
  landing. That is the landed commit's writes, plus the group's, plus the queued writes. It
  replaces `ceil(floor / average)`. The window is 2 ms.
- **For `Wait::Large`, while arrivals are above 0, until the window passes:** the count is not
  met (S4). So a
  1 MiB write's second half, still being copied in, joins its first half's group: the two
  arrive microseconds apart.
- **After the window:** the count is met whatever the arrivals, so arrivals unrelated to the
  group cost at most the window. Their sources:
  - reads completing in a mix;
  - the volume's writes to chunks not on this node;
  - pipelined commits.
- **The wait's kind:**
  - **`Wait::Large`:** when the group at the landing holds a large write, or every write of the
    landed commit was large. A wait armed at a group's first write is `Wait::Large` when that
    write is large;
  - **`Wait::Small`:** otherwise.
  - **Where the facts come from:** `committing` entries and the `staged` tuple carry their count
    of large writes, counted as writes join them. Stageable is not large on 16 and 32 KiB
    cluster stores.
- **Small writes:** unchanged (`writes + queued`, 300 µs).
- **A group with no landing:** it uses the wait armed at the last landing while that wait's
  window has not passed. At 1M QD8 every group starts so, with all 16 requests in the commit
  that just landed.
  - **When no wait is armed, or its window has passed:** `maybe_launch_staged` arms a fresh one at
    the group's first write:
    - a window from it;
    - a count target of the larger of the old target and what is outstanding then.
  - **Either way:** it then goes on to check the wait at once, rather than return.

**`finish_commit`:**
- **Re-arming:** its held branch re-arms the gather at each landing, which replaces any wait armed
  while the group was staged during the flight.
- **The order:** the staged bookkeeping comes before `deliver_local`, not after it: holding's
  launch or wait, or without holding the move of the group into `committing`. Delivering replies
  can make a target send `Hello` to its local replica, which settles (S5). The engine's view of
  the group must already match the service's then.

**`--flush-coalesce 2`** (`flaminio-io/src/node.rs`): refused unless `--commit-target-mib 0`.
While holding, `maybe_coalesce_partner` refuses to promote. A coalescing commit's flush would
then be held back for as long as writes keep being staged.

## Staging with nothing in flight (S2)

Three gates allow staging only behind a commit in flight, or into an existing held group:
- `Store::stage_check` (`flaminio-store/src/store.rs`);
- `ReplicaService::stage` (`flaminio-node/src/replica/mod.rs`);
- `Engine::stage_local`.

**The change:** while holding, all three also take a write with nothing in flight and no group;
it starts one.
- **`Engine::flush_local`:** its held-group path calls `stage_local` before it looks for a
  group. So a large write at the head of the queue is staged, and its data written, at once.
  - **When something is left queued behind the group** (a request it could not take), it starts
    the group at once (`launch_held`): in order, the group can grow no more (S1.4).
  - **Otherwise** the group's wait decides (`maybe_launch_staged`).
- **`Store::launch_staged`:** it already starts a held group with nothing in flight.

**Why it is safe:** it is W4.2's argument, with nothing in flight instead of one commit.
- **The early data's clusters:** they are reserved for the group. No durable batch references
  them until the group's batch is durable, so a crash leaves them free.
- **The device:** it tags early writes with the next commit's generation, whose flush waits for
  them.
- **The batches:** they still become durable one at a time, in order.
- **Checkpoints:** they wait while a group is staged (`execute_finish`), as with W4.2.
  - **Under a stream:** groups follow one another, so a checkpoint runs when `stage_check`
    refuses a write for lack of journal room. The group then starts by its rule, and the
    checkpoint follows its landing, as W4.2 does when the journal fills. A store test covers
    this case while holding.

**"Stageable" and "large", in one place:**
- **`flaminio_store::covers_cluster(cluster_size, offset, len)`:** stage_check's shape test today,
  that the range covers a whole cluster. `stage_check` uses it unchanged, so the store stages
  what it stages today, on every cluster size.
- **`flaminio_store::large(cluster_size, offset, len)`:** `covers_cluster` and `len` at least
  64 KiB. The wait's kind and the frontend use it.
- **A write the frontend counts as large may still be refused by the store** (room, a busy
  cluster). That costs only its copy's wait.
- **A client write that crosses a chunk boundary** is split by `Engine::submit`. The frontend
  tests the whole write, the store each piece. A mismatch costs at most one wait.
- **The cluster size:** `Store::cluster_size()` (new), which the engine passes on as
  `Engine::cluster_size()`.

## Writes reach the engine one by one (S3)

**The test:** while holding, the zero-copy write path (`engine_io_task_zc`, `flaminio-io/src/ublk.rs`)
tests each write with `large` at the volume offset, before its copy (the request's offset and
length are known then). That is the store's shape test, since chunks are a whole number of
clusters (`validate_chunk_size`).

**Large writes:**
- **The counter:** they are counted in arrivals (S4), not in `EngineCtx::staging`.
- **The pump:** it waits for `staging == 0`, so it does not wait for them.

So a large write reaches the engine when its own copy is done. Small writes keep today's wait,
which gathers them. Letting small writes skip gathering cost 4K QD32 dearly in W4.2: 6 700 IOPS,
when they were staged at once.

**The setting:**
- **What the frontend reads:** `Engine::holding()` and `Engine::cluster_size()`, in
  `run_device`, after every engine setting: `set_async_commits`, `set_commit_gather`,
  `set_commit_target_bytes`.
- **`holding()`:** `async_commits && commit_target_bytes > 0`. Under `--sync-commits` nothing is
  held, and S3 does not apply.

## Arrivals (S4)

**The counter:**
- **Per device:** `run_device` creates an `Arc<AtomicUsize>`, gives it to the engine
  (`Engine::set_arrivals`), and to every queue's frontend.
- **No resets:** a new device gets a new counter, so a reset can never underflow one.
- **The guard:** for each large write, the frontend takes an RAII guard just before the copy. The
  guard adds 1, and gives it back when dropped. It is dropped just after `ctx.submit`, or when
  the copy fails, not when the request completes.
- **The failure path** sets `due`, so the engine looks again.

**The wait** (`gather: Option<(usize, Instant, Wait)>`):
- **Its kind:** `Wait::Large` or `Wait::Small`, set by `arm_gather`.
- **`gather_ready`:** for `Wait::Large`, when the window has passed but arrivals are above 0, it
  is not ready until either:
  - they reach 0;
  - or 8 ms more have passed.
- **`Wait::Small`** never holds for arrivals.

**Waking:**
- **When arrivals reach 0:** the submit's pump.
- **The 8 ms limit:** checked at every pump. The engine's 10 ms ticker on queue 0 is the
  backstop.
- **No limit timer.** It is not armed separately.

**What the counter cannot see:** requests the kernel has posted but the queue thread has not
reaped yet. The count rule, not the window, is what waits for those.

**The gather timer** (`ublk.rs`, each queue's timer task) gets deadlines, not durations.
- **Today:** every wait queues a timeout, and the task runs them one at a time. On a device whose
  cycle is shorter than the window, they pile up, and a 300 µs wait can end up to 2 ms late.
  With holding by default, every large-write landing arms 2 ms.
- **Now:** the task races `recv` against its sleep.
  - **A new, earlier deadline:** it sleeps again to that one.
  - **At each expiry:** it pumps, and drops the deadlines that pump covered.

## A held group is never stranded (S5)

**`Engine::launch_held(now)`:** if a group is staged and no commit is in flight, it starts the
group's commit (`service.launch`), without regard to the gather, and moves it to `committing`.

**`settle_local(now)`:**
- **Its loop:** `launch_held`, `finish_commit(wait)`, then `flush_local` with `settling` set.
  It runs while `committing`, `local` or `staged` is non-empty.
- **Progress** is a counter of landings, launches and requests started. Sizes are not enough:
  one iteration can land a commit and start the next with the same sizes.
- **`settling`:** the gather counts as ready, so a queued small write is committed, not left
  waiting for a window that the same `now` never ends.
- **No progress:** it stops when an iteration changes nothing, with an `eprintln` and a
  `debug_assert`. With `finish_commit`'s order fixed and failed groups answered, nothing should
  cause that.
- **Its callers:**
  - `InspectLocal`, `DeleteReplicas` and `NetEvent::Closed` (already; a peer's close now also
    starts a held group, which is intended: the store settles then today);
  - before both direct `service.hello` calls (`on_net` Opened, and the attach path);
  - `set_async_commits(false)`, before it clears the gather.

**The invariant:** `service.start`, and any store operation that settles, is never called while
the store holds a group the engine has not launched. `flush_local`'s held path returns before
`service.start` while a group is staged, as today.

**`ReplicaService::launch`:**
- **No writes:** a group with no writes (every item answered by the core, or refused) moves to
  `flying` with no device commit when nothing is in flight. `launch` returns true, and `poll`
  answers it. Today, while holding, such a group is stranded.
- **A store error:** when `store.launch_staged` fails, the store is poisoned. Either it already
  was, or `promote_staged` failed and `poison_on` caught it. So nothing retries.
  - **What `launch` does:** it fills the group's write replies with the error, empties its
    `writes`, and moves it to `flying` with no device commit. `poll` then answers it from its
    `writes.is_empty()` branch, without the store, and `launch` returns true.
  - **Before:** it waited for a poll that, with nothing in flight, never came.
- **The invariant, checked:** a `debug_assert` in `launch` that the store holds a group whenever
  the service's group has writes. A settle behind the engine's back would otherwise strand it
  without a sign, as `launch_staged` would return `Ok(false)`.

## Stats

The local commit report (at detach) adds two counts: how many waits held on arrivals, and how
many reached the 8 ms limit.

## Tests

As the spec lists, in:
- `flaminio-store/tests/async_commit.rs`: the staging tests with `set_hold_staged(true)`, and a
  hold variant of `staging_through_full_journals_keeps_the_data`;
- `flaminio-node/tests/engine.rs`: the harness runs on real time; peer sessions are injected as
  `NetEvent`s;
- `flaminio-node/src/replica/tests.rs`: the stale-session group, the launch failure
  (`fail_commit` on the commit ahead);
- `flaminio-io`'s unit tests: the guard, and the option check.

**The engine's byte-floor test** (`tests/engine.rs`) assumed `ceil(8 MiB / average)`. It is
rewritten:
- **The count rule:** after a commit of four 1 MiB writes, four more start at once.
- **The 8 MiB rule, on its own:** a landing with 8 MiB already staged starts at once.
- **Its 4K tail:** kept.

## Measurement

1. **The profile's traced runs,** on fl-b, from the empty state:
   - 1M QD8: MiB per commit, steady MiB/s against the raw run's ~984;
   - 64 KiB QD1 and 1M QD1, before and after.

   The scripts are in the data directory.
2. **`make bench-writes`,** RF 1, zero copy (`ZERO_COPY=1`), one queue, `VOL=wbench`, so that the
   harness does not delete vol1. Every integrity check must pass.
3. **The results document,** `docs/benchmarks/2026-10-29-writes-1m-floor.md`, with:
   - the profile's tables;
   - the prototypes' steps;
   - the campaign;
   - the performance plan's W4 item 5 and Status updated.

**Then the lab gets volb again:** fl-b's store is reformatted by the campaign. Create volb (RF 2,
a replica on fl-b), fill it, and check that a read bench runs.
