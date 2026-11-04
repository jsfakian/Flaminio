# Issue #10: 1M sequential writes at about 70% of raw — specification

Terms:
- a **stageable write** covers at least one whole cluster: the store can stage it, writing its
  redirect-on-write data before its commit starts (W4.2), unless it refuses it for room or a
  busy cluster;
- a **large write** is a stageable write of at least 64 KiB. The frontend and the wait use it.
  The store stages any stageable write, as today;
- the **group** is the staged writes waiting for their commit;
- a **small write** is one that is not large;
- **arrivals** are the large writes the frontend is copying in (zero copy) and has not yet handed
  to the engine;
- a **landing** is the moment a commit is durable;
- **holding** is the byte floor's mode (`--commit-target-mib`, W4.5): after a landing, the group
  is held, and grows, before its commit starts.

## Behaviour

**S1. Holding is on by default, with an 8 MiB floor.** After a landing, a held group's commit
starts when the first of these holds:
1. **bytes:** the group and the queued writes hold 8 MiB;
2. **count:** they number as many writes as were outstanding at the landing: the landed
   commit's, the group's, and the queued ones. For a large-write wait, until the window passes,
   the count is met only while no large write is being copied in (S4). So the two 512 KiB
   halves of a client's 1 MiB write join the same group. After the window, the count is met
   whatever the arrivals.
3. **time:** the gather window has passed (and S4):
   - 2 ms for a large-write wait: the group at the landing holds a large write, or every write
     of the landed commit was large;
   - 300 µs otherwise.
4. **behind it:** a request the group cannot take is queued behind it: a small write, a read,
   or a write the store refuses. Kept in order, the group can grow no more. (Found while
   testing: without this, a small write behind a group held it for its whole window.)

**A group started with nothing in flight (S2)** uses the wait armed at the last landing, while
that wait's window has not passed. At 1M QD8, every group starts this way, with all 16 requests
in the commit that just landed.

**A group whose first write comes later,** or with no wait armed, arms a fresh one at that write:
- a fresh window from it;
- a count target of the larger of the old target and what is outstanding then;
- a large-write wait when that write is large.

So a client slow to resend still gathers its writes, and the two halves of a 1 MiB write still
join one group.

**What changes, what does not:**
- **The count rule** replaces the floor's `8 MiB / average write`, which a client at low depth
  could never reach.
- **Small writes** gather as today.
- **`--commit-target-mib 0`** turns holding off. With it, S2–S4 are off too, and the behaviour is
  the one before this change, apart from the gather timer's deadlines and S5's fixes, which
  apply either way.
- **`--flush-coalesce 2`** requires `--commit-target-mib 0`. The node refuses the combination,
  as coalescing never worked while holding.

**S2. While holding, a stageable write is staged with nothing in flight.** A write the store can
stage is staged, and its data written at once, even when no commit is in flight and no group
exists. It starts a group. Before, it waited, unwritten, for the next commit.

**S3. While holding, a large write reaches the engine once its own data is copied in** (zero
copy). Before, the queue waited until every write it was copying in was done. Small writes
still wait for one another, as today.

**S4. While holding, the window of a large-write wait does not end while large writes are
being copied in.** The group goes on waiting, until:
- those writes reach the engine;
- or 8 ms more have passed.

Small-write waits never hold for this.

**S5. A held group is never stranded.**
- **Paths that need the store settled** first start the group's commit, without waiting for
  the gather, and wait for it:
  - opening a session to a peer (`hello`);
  - deleting a volume's replicas;
  - inspecting the local replica;
  - turning asynchronous commits off (detach).
- **A group whose items were all answered without a write:** it is answered with no device
  commit when its commit would start. That covers stale epochs, dropped sessions and store
  refusals.
- **A group whose commit fails to start** (a store error): its writes are answered with that
  error.

**S6. Unchanged:**
- **Crash safety:**
  - early data goes to clusters that no durable batch references until its batch is durable;
  - batches become durable one at a time, in order;
  - checkpoints wait while a group is staged, as with W4.2.
- **Small writes,** and the order of a node's writes: a small write behind a group starts the
  group (S1.4), and goes into the commit after it.
- **Copy mode:** S1, S2 and S5 apply. It has no copy-in, so S3 and S4 do not arise.
- **Where holding applies:** a node with no volume attached commits synchronously and never
  holds. A node with a volume attached holds peers' large writes too: up to 2 ms more per
  commit on an RF 2 replica (10 ms while the node's own large writes are being copied in), when
  its client's writes do not come back sooner.

**S7. The bound on a wait.**
- **A large-write wait:** its window plus 8 ms, so 10 ms. The limit is checked at pumps, with the
  10 ms ticker as the backstop.
- **A small-write wait:** its window.

## Tests

- **Store** (`flaminio-store`), with `set_hold_staged(true)`:
  - a write staged with nothing in flight has its data written at once, and the group's commit
    makes it durable;
  - a crash with such a group staged keeps exactly the commits that landed (the staging tests'
    crash images, for this case);
  - a stream through a full journal while holding:
    - checkpoints still run (`checkpoint_seq` advances);
    - the data survives a reopen.
- **Engine** (`flaminio-node`, its harness, real time):
  - **a default-built engine:** with asynchronous commits, it streams large writes. The engine
    and the service agree on holding;
  - **staging:** a large write arriving with nothing in flight is staged at once;
  - **the count target:**
    - a group waits after a landing until the outstanding writes are back;
    - a single large write at depth 1 commits without waiting for the window;
    - two 512 KiB halves, the second still being copied in when the first reaches the engine,
      give one commit, with no window wait. The test starts from a wait whose target is 1: cold,
      or after a landing of a single write;
  - **arrivals:** with arrivals reported, a large-write wait does not end at its window, and
    ends once they reach zero, or 8 ms later;
  - **small writes:**
    - a small-write wait never holds for arrivals;
    - small writes queued behind a group commit after it, in order;
  - **S5:**
    - each settle path, with a group held and a gather window of 10 s. Each returns, with the
      group durable:
      - inspect;
      - detach;
      - a peer session opening (an injected `NetEvent::Opened`);
      - the target reopening its local session (the attach path's `hello`);
      - deleting a second volume's replicas;
    - a group whose only write is from a stale session, at the replica service, is answered;
    - a store failure at launch answers the group's writes with the error (`fail_commit` on the
      commit ahead of the held group);
    - the byte-floor test is rewritten for the count rule:
      - after a commit of four 1 MiB writes, four more start at once;
      - the 8 MiB rule is tested on its own (a landing with 8 MiB already staged);
      - its 4K tail is kept.
- **Node** (`flaminio-io`):
  - `--flush-coalesce 2` without `--commit-target-mib 0` is refused;
  - the arrivals guard gives its count back when dropped, including on a failed copy.
- **Lab,** fl-b, RF 1, zero copy, one queue, from the empty state:
  - **1M QD1:** requests per commit (expect 2: both halves of the write);
  - **the profile's traced run:** commits carry 7 MiB or more on average (today 4.0), and 1M is
    at 80% or more of raw across the disk, steady;
  - **`make bench-writes`, `VOL=wbench`:**
    - **1M:** steady at 80% or more of raw across the disk, from the profile's raw run the same
      day. The campaign's own raw 1M rewrites one 16 GiB span, which the drive keeps in its
      cache (1 655–1 967 MiB/s with W4.2). It is reported, as in W4.1 and W4.2, not judged
      against.
    - **4K QD1, 4K QD32 and the 70/30 mix:** within 10% of the W4.2 full campaign.
    - **Integrity:** every check clean.
  - **Large writes at low depth:** 64 KiB QD1 and 1M QD1, Flaminio before and after, in the
    profile's way: no worse.
