# Issue #64: Erasure coding — intent

[Issue #64](https://github.com/jsfakian/Flaminio/issues/64), README roadmap phase 6,
ARCHITECTURE §1 ("Later") and §20.6, an open question: "When should it come, and only for
'capacity' device classes and cold volumes?". This change is the issue's step 1, the decision,
as a study with a recorded outcome: a Proposed ADR, or a recorded "not yet" with what must exist
first. It writes no code. Steps 2–4 wait for it: issue #64 stays open after this change. Assumes
#45 and #46 as their intents plan them, and reads #50 and #51 for what they assume.

## Why

Three copies cost three times the capacity (the issue). A k+m code cuts a chunk's data into k
data fragments, adds m parity fragments, and puts the k+m on distinct members: any k rebuild the
data. Today's replication rests on two facts that then stop holding: **each member holds the
whole chunk**, and **any in-sync member alone can serve it**. Where the code and the documents
assume them:

1. **Nothing codes, and nothing names who would use it.** A volume has `Replicas` 1–5 and
   `MinISR` (`Spec.Validate`, `meta/domain/domain.go:186`); its `Header` (`domain.go:37`) has no
   coding field; the CSI controller refuses an unknown parameter (`control/csi/controller.go:157`).
   No crate encodes. §20.6's two candidates do not exist either:
   - **Device classes.** ARCHITECTURE §4 and §9 have them. No code does: a placement candidate
     is an id, a replica count, a zone and a rack (`NodeLoad`, `domain.go:303`), a node has one
     disk, and no capacity is accounted (#45, Why 3 and 6).
   - **Cold volumes.** Nothing records when a volume or a chunk was last written.
2. **What it would save** (raw bytes stored per byte written; losses survived; domains needed):

   | | RF 2 | 2+1 | 4+1 | RF 3 | 2+2 | 4+2 |
   |---|---:|---:|---:|---:|---:|---:|
   | Raw per usable | 2.0 | 1.5 | 1.25 | 3.0 | 2.0 | 1.5 |
   | Losses survived | 1 | 1 | 1 | 2 | 2 | 2 |
   | Failure domains | 2 | 3 | 5 | 3 | 4 | 6 |

   2+1 stores a quarter less than RF 2 (a third more usable capacity). The issue's "same
   protection as three copies" needs m = 2: 2+2 stores a third less than RF 3 on four domains,
   4+2 half on six. The README asks for three nodes; the edge profile has one to three.
3. **A write is durable on whole copies.** The target sends the same extent and data to every
   member of the write set (`start_write`, `flaminio-repl/src/target.rs:1400`, `:1415`) and
   completes on all of the ISR (`check_write`, `:1421`; protocol §4). The ISR shrinks to
   `min_isr` (`member_failed`, `:1306`; `meta.rs:163`; `domain.go:238`), down to one member
   (ADR 0006): one member is still all the data. A coded chunk's minimum cannot be below k, and
   at k nothing protects its writes: 2+1 with one member away either blocks or writes
   unprotected.
4. **One member answers a read.** `source` picks one ISR member, the local one else the lowest
   id (`target.rs:1644`), and a client read is one `Read` to it (`:1753`, `:1697`). A failed
   local read goes to one other member (`on_read_failed`, `:1019`). Fast reads need the local
   replica to be that source (`:670`): this is the path measured at 98% of raw at 4K QD32 and
   100% at 1M (`docs/performance-plan.md`). With k+m the local member holds 1/k of the data
   blocks; the rest are remote reads, and a block of a member that is away costs k reads and a
   decode. `dataLocality` (§9) and `PreferNode` (`Place`) mean "a whole copy here".
5. **Leases do not depend on what members hold; their reason does.** Every member of the ISR,
   and a joiner, confirms the epoch (`lease_ok`, `target.rs:1624`; ADR 0005). ARCHITECTURE §8.5
   gives the rationale that a new target "must fence at least one ISR member of every chunk
   anyway, because it needs an up-to-date source". That is not the current takeover rule: protocol §6 requires
   fencing the effective ISR and reconciliation before serving. With k+m one member is not a
   reconstruction source: k coherent fragments are.
6. **Reconciliation takes any member as the authority, so nothing keeps versions.** After a
   failover the target unites the ISR members' uncertain sets (`on_fenced`, `target.rs:1311`),
   asks every ISR member for a CRC per block (`:1764`), calls a block bad when replies differ
   (`digest_mismatch`, `:2739`), reads it from `source` (`:1801`) and writes that value to the
   ISR as a write of the new epoch (`on_read`, `:1582`; protocol §6). It may roll a write
   forward or back: an unacknowledged write may take effect or not, and every member holds one
   complete, checksummed value, old or new.
   - **So the store keeps no version of a block.** A map entry is a location and CRCs (format
     §5.5). A redirected write frees the old cluster at its commit (`state.rs:398`); a small
     write overwrites in place (`DeferredWrite`). A chunk keeps one number, the highest
     sequence applied (`bump_seq`, `state.rs:477`), of sequences that restart in each epoch
     (`target.rs:486`), read by the MVP's `serve` mode only.
   - **The uncertain set is in memory.** After a restart a replica reports every block of its
     chunks (`replica.rs:70`, `:136`).
   - ARCHITECTURE §11.4 still describes sequence numbers compared, and the most advanced
     replica copied from. That is not what is built.
7. **With k+m that reconciliation has nothing to stand on: the write hole.** A write changes
   fragments on several members: all k+m for a whole stripe, 1+m for part of one. After a
   crash they hold fragments of different versions of one stripe.
   - **Digests of different members are of different data.** Comparing them says nothing, and
     a fragment's CRC says only that it is intact, not which version it is.
   - **No member is an authority.** A value exists only where k fragments of one version do. A
     stripe with fewer than k new and fewer than k old decodes to neither: 4+2 split three and
     three; or 2+1 when the target's node dies holding one fragment, with the write on one of
     the other two. A target's node holds a member whenever locality placed one there, so that
     is the ordinary failover.
   - **A partial update loses blocks nobody was writing.** New data with old parity, then a
     member lost: the stripe's other blocks, acknowledged long ago, cannot be rebuilt. §8.1
     ("an acknowledged write is never lost while at least one ISR member survives") breaks,
     and so does the simulator's rule that an unanswered write takes effect or not.
   - **A stripe can only be rolled back or completed,** which needs the old fragments kept
     until the write is known complete, and a version on each: neither exists. The wire has
     the watermark (`prune`, `msg.rs:42`); the replica holds it in memory only.
8. **Resync, moves and rebuild copy from one source.** `drive_resync` picks one (`target.rs:2345`);
   `Mapped`, the digests of source against joiner, and each `Copy` read it (`:2372`, `:1843`,
   `:1819`; protocol §7). A coded member's missing block is rebuilt from k members: k reads per
   block written, and no comparison with a source. #45 (Why 7) finds every byte of a move going
   through the target, 4 pieces of 1 MiB in flight; its step 3 is a copier "pulling from a
   source". When k+m equals the number of domains there is nowhere to rebuild or drain to:
   a move needs a destination without a replica (`meta.rs:207`).
9. **Scrub and repair.** #46's step 1 (each node reads its own blocks against their CRCs)
   holds for fragments as it is. Its step 2 compares digests across replicas, and its step 3
   repairs "from an ISR member whose data verifies" (§12: "from the ISR majority"): each
   becomes a check, and a rebuild, over k members.
10. **Encryption and snapshots are written for identical replicas.** §15.4: "every replica
    stores identical ciphertext. That is required for scrub comparison and resync"; #50 wants
    "the same ciphertext" on every replica, with digests, reconciliation, resync and moves
    "unchanged". #51 wants every in-sync replica to hold every snapshot "with the same
    content". Coded after the target encrypts, members hold different fragments of one
    ciphertext: what each needs is that the fragments are a function of it.
11. **The store's units, and one replica's atomicity.** Blocks are 4 KiB with a CRC each,
    clusters 64 KiB. A write that covers whole clusters is redirected (`store.rs:1754`); a
    smaller one is applied in place after its journal commit, or copies the cluster once
    (`:1790`, `:1799`). Each replica decides that alone, from its own map: "every write is
    all-or-nothing on a replica" (§6), and on no more than one. A stripe needs a unit (k
    blocks, or k clusters) and an update rule: whole stripes only, redirected; or a small
    write that first reads data or parity and then updates 1+m members in place, which is the
    wide hole of item 7. A chunk header has no code and no fragment index (format §5.3), and
    metadata recovery takes each disk's chunk as whole (§11.5).
12. **Placement.** `Place` needs as many distinct domains as members (`resolveDomain`,
    `domain.go:343`). A member's place in `Replicas` is not stable: a move appends the
    destination and removes the source (`validMove`, `domain.go:259`).
13. **The simulator models whole copies.** A replica is a value per block (`replica.rs:292`), a
    digest is that value (`:365`), and the final check wants every replica to hold the same
    value (`sim/mod.rs:1355`). Runs have 3–4 nodes at RF 2–3 (`:130`), and every node returns
    (protocol §8): no member is lost for good. The per-block linearizability check
    (`sim/check.rs`) is about what clients see, and carries over.
14. **The lab can check 2+1 on one node's loss, and little else.** Three data nodes: fl-a and
    fl-c on one host, fl-b on the other, 1 GbE between them; fl-c "adds no failure
    independence" (`docs/lab.md`). 100 GiB disks, on SATA SSD and NVMe.
    - **Can:** a node stopped under a verified load, degraded reads, the returning member's
      resync, space and performance against RF 2 on the same VMs (RF 2 today: 386 IOPS and
      2.44 ms p50 at 4K QD1, 9 458 IOPS at QD32, 106 MiB/s at 1M, the link:
      [results](../../benchmarks/2026-10-17-writes-rf2-w41.md)).
    - **Cannot:** m = 2, so not the issue's comparison with three copies; k above 2; a rebuild
      onto another node, or a drain (item 8); the loss of fl-a's host, which takes two
      members and the metadata quorum; capacity-class disks.

## What we want

One document that answers the questions below, each with the evidence named, and a recorded
outcome. No rule is chosen here.

1. **Who it is for.** Which volumes would be coded, and how they are named when neither a
   device class nor "cold" exists (Why 1). Evidence: the workloads RWO block volumes carry;
   where cold data already goes (backups to object storage, §14.1).
2. **What it saves, at which k+m, on which clusters:** Why 2 against the node counts the
   README and the edge profile allow, thin provisioning, and the rebuild reserve (§9).
3. **The consistency model of a coded chunk,** as rules in the protocol's terms (§4–§7), fit
   to simulate: the in-sync set and its minimum; what happens at k (block, or write
   unprotected: ADR 0006's trade-off); who may read; fencing and the takeover. Whether ADR 0005
   stands as written (Why 5).
4. **How the write hole is closed, and what reconciliation becomes** (Why 6–7): what a member
   keeps and reports, and how a stripe is rolled back or completed. Evidence: each case of
   Why 7 worked through, the target's node lost included; the format and wire changes implied.
5. **The stripe unit and the update rule** (Why 11), with their cost: reads, writes and round
   trips of a small write, a whole-stripe write, a read and a degraded read, against RF 2 and
   RF 3 and the lab's numbers. What the device would advertise to its users.
6. **Whether it can be confined,** and what each confinement leaves untouched: volumes
   written in whole stripes only; a background conversion of cold chunks, which keeps the write
   path as it is and needs a transition under the epoch; or coded volumes written freely.
7. **What #45, #46, #50 and #51 must leave in place** (Why 8–10), so that nothing is built
   twice: the copier's and the repair's source as a set of members, the restated encryption and
   snapshot properties, and the texts of ARCHITECTURE §12, §13 and §15.4.
8. **What the simulator must model and check before any data-plane code** (Why 13; principle
   7): fragments and versions; every k in-sync members decoding each stripe to a value the
   history allows, unwritten neighbours included; a member lost for good after a target crash;
   an injected bug per new rule.
9. **What the lab can prove and what it cannot** (Why 14), and what a check of m = 2 needs.
10. **When.** Phase 6, or not yet. If not yet: what must exist first, and what reopens the
    question. If now: the order of the issue's steps, and its "done when" restated for what
    the lab can check.
11. **The outcome is recorded:** a Proposed ADR in `docs/adr/` (accepting it is a human's), or
    the decision not to build it yet; ARCHITECTURE §20.6 and §11.4 (Why 6), the README's
    roadmap row and the issue's steps say the same.

## Out of scope

- **Steps 2–4 of the issue:** the write and read paths, rebuild and scrub of coded chunks, the
  simulator's model. No code, in the simulator either: the cases are worked on paper.
- **Accepting the ADR,** and any choice it lays out.
- **Device classes, capacity accounting, a measure of "cold":** named as what is missing.
- **A coding library:** a new dependency, named in the ADR, not added.
- **Codes other than k+m** over whole stripes (local reconstruction, regenerating codes).
- **Replicated volumes:** their rules, format and performance stay as they are.
- **New measurements.** The numbers are those in `docs/benchmarks/`; what to measure is named.
- **More lab nodes.**
