# Issue #11: Reconcile first the chunks that clients wait on — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-10-30.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-04.

Settled during the spec and design reviews. Each line: the choice, and why.

## Scope

- **Persisting the uncertain set (the issue's step 3) is out:** a store and on-disk format
  change with its own crash tests; it saves digests, not waits.
- **The re-search of the mutation checks (step 2) is in:** the issue asks for it; a script makes
  it minutes over 24 cores.
- **Cut:** an order invariant in the simulator, mutations of the new order, catch rates, a
  scripted fencing scenario, a 50 M-seed cap, a 20 000-seed sweep of the campaign with more
  chunks. The unit tests pin the order down; the rest cost more than they check.
- **Serving reconciled blocks of a chunk early, preempting work in flight, the bulk limit and
  the digest and repair sizes stay as they are:** not needed for the issue.
- **The mismatched-reply leak in `on_reply` and `on_digest` is fixed here:** pre-existing, but a
  dead digest would keep a chunk waited, and resyncs capped, for the target's life.

## Order

- **A chunk is waited while a queued client operation waits on it; only local client I/O makes
  one waited:** peers and the metadata service cannot reorder a target's work.
- **Wait order = first appearance among queued client operations:** client operations sent again
  are only on serving chunks, so this is submission order, with no new state.
- **Only the digests of chunks fenced in the same progress pass are reordered, from chunk 0 up;
  nothing else is sorted:** sorting would change the order with no chunk waited (S2).
- **With no chunk waited, `pump` is today's, unchanged:** one-chunk runs keep their schedule, so
  the hints hold; a one-chunk seed that moves is a regression, not a hint to update.
- **No preemption; no idle slot:** work in flight cannot be recalled usefully; work that cannot
  start holds nothing back.
- **Bulk work sent again keeps today's front place:** a waited chunk's items go first anyway.
- **Client operations keep one sequence, in submission order, for `pump` and `try_fast_read`:**
  the reorder applies to bulk items only.

## Resyncs (gate fix 1, H1)

- **While waited work can start or is in flight, at most one resync item is in flight, and with
  `max_bulk` 1 they alternate with waited work:** resyncs always progress, and the wait bound
  holds.
- **A waited chunk stuck in fencing, or with a session down at the ISR minimum, caps nothing:**
  its clients wait for the fence anyway.
- **Resync items already in flight when a chunk becomes waited run to their end:** no
  preemption.

## The bound

- **"Waits only for its own chunk" becomes: its fence, its reconciliation, earlier-waited chunks,
  and each time it can start, the items in flight then plus at most one resync item:** approved
  at the spec gate (2026-10-30); no order can do better while other work holds the slots.
- **No bound in time:** items in flight can last (a repair at the ISR minimum), as today.

## Implementation

- **New state is `unready`, `last_waited` and `newly_fenced` only; in-flight counts come from the
  maps:** nothing to keep in step; computed only when a chunk is waited.
- **Two passes over the queue per pump while a chunk is waited, each bulk item tried once:**
  linear; nothing a try depends on changes within a pump.
- **The accounting check is `cfg(test)`, in wrappers of the `pub` methods:** covers early
  returns; runs in the target tests and the simulator under `cargo test`, debug and release.
- **The leak fix puts the removed request back in the session's map before `fail_peer`:** no new
  path; it is re-queued with the others, in id order (S8).
- **The bench's teardown drains, then asserts `is_settled()`; only the deposition tests opt
  out:** the spec's "every chunk serves" without breaking the existing tests.
- **Client order is checked by two randomized target tests** (against today's pump; and, with
  replies and faults, that overlapping operations start in submission order), **and a
  `cfg(test)` check that the selection leaves serving chunks' locks and open sessions as they
  were:** the simulator never queues overlapping operations.
- **The accounting check covers locks too:** the new pump starts work out of queue order.
- **`depose` clears every chunk's locks and the fast-read list:** the lock check holds once
  deposed; a deposed target starts nothing.
- **The bench places chunks per chunk:** chunks fenced at different times need different nodes.
- **The side-by-side test picks its runs by the `submit` count, not the pump's stats:** the
  pump under test does not decide which runs it is compared on.

## Simulator

- **Every failing seed must pass with no switch on; only `not linearizable` or `replicas
  diverge` counts as a catch; other classes are skipped and counted:** `move_without_resync`
  fails with `liveness:` on 490 of the first 2 000 seeds with more chunks.
- **The campaign with more chunks is `chunks = 3 + seed % 6`, 200 seeds in `make test` and
  2 000 in `make sim`, with six checks on it at a budget of 20 000:** reproducible and cheap
  (5.8 s debug, 6.2 s release).
- **In `make sim` the fencing check tries its hint only, and fails fast:** a 4 M-seed search
  does not fit CI's 40 minutes.
- **No fallback if the fencing check finds nothing below 4 M:** its hint is a one-chunk run, so
  that means S2 is broken, and the pump is fixed.
- **The search merges by the lowest catch with every lower range searched in full; a range that
  ends otherwise stops the search:** a seed that fails clean is a bug to fix first.
- **The fencing check's hint becomes 103 223, a two-chunk run (settled during implementation,
  2026-11-04):** the search from seed 0 found it first; main does not catch it, the new order
  does. 3 626 767 still catches, and so does the one-chunk seed 1 001 529, on main too: the
  earlier search had not covered every lower seed. So S2 holds; the hint is simply no longer
  pinned by it, and a pump change may move it (then search again).

## Lab

- **Later, when the lab is free; recorded, not enforced:** the lab is busy, and 16 GiB shows
  the effect only on a mid-volume first read.
- **Without zero copy, `FIRST_CHUNK` 0 and 32, main's binaries then the branch's, both with the
  branch's `reconcile.sh`:** one script, one mode, so the runs compare.

## Accepted risks

- **Resyncs slow to one item at a time while clients wait:** a serving chunk stays a copy short
  longer; clients come first.
- **Two passes per pump while reconciling:** bounded by the queue's length, only then.
- **The accounting check may expose another existing leak:** it is fixed here if so.
- **Whether the partition scan and udev's probes queue before the lab's first read is a race:**
  the lab expectation says so.
