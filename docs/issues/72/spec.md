# Issue #72: Capacity safety, balancing and locality — specification

[Intent](intent.md). [Design](design.md). [Decisions](decisions.md). Approved 2026-11-13; not
implemented. This step: a per-node capacity ledger, and admission of creates and moves.

## Terms

- **Node store:** the one replica store of a data node (one disk per node, `agent.Pick`).
- **Capacity T:** usable data bytes, data clusters × cluster size. Raw disk bytes, metadata
  pages and the journal are not data.
- **Physical use U:** used plus quarantined data bytes, from the node's latest report. Used
  counts clusters freed since the last checkpoint too.
- **Slots S:** chunk replica slots of the store; **slots used:** the slots taken (chunk replicas,
  and #13's fence-only slots once its store has them).
- **Charge:** one copy charges its volume's chunk size in bytes and one slot, written or not; a
  fence holder (#13: a node keeping a volume's fence) one slot and no bytes.
- **Released copy:** #13's: the copy a completed move leaves on its source, named in the chunk's
  `released` until the node confirms it deleted it (#13's `ConfirmRelease`).
- **Named copies** of a node: its replicas, its pending move destinations, its released copies,
  its fence holds, and the unwritten placements of a creation in progress.
- **Ledger L:** a node's committed bytes and slots, kept by the metadata service. It is
  readable, being initialized, unreadable (does not parse) or absent.
- **Budget at w:** `floor(T × w / 1,000,000)` bytes and `floor(S × w / 1,000,000)` slots, for a
  watermark w in parts per million. **Soft** is 800,000, **hard** 950,000 (ARCHITECTURE §11.6).
- **Room:** how many copies of chunk size c a node may still take at w (S5).

Metadata pages are not a resource in this step: the page pool is sized for every data cluster
mapped twice plus every slot's page tables (`docs/format/on-disk-format-v1.md:62`).

## The invariant

A readable ledger is at least the charge of its node's named copies, and more only where S4
says. Every copy a current metadata service names on a node is charged in the transaction that
names it, and no two placements are admitted from the same ledger revision. A charge leaves only
when the node confirms it deleted the copies: a tombstone's confirmation, or #13's
`ConfirmRelease`. So for a store holding only named copies, L bounds its physical use plus its
unwritten commitments, and no placement takes L or U, plus the new charge, beyond the budget.
Replication, ISR decisions, epochs and acknowledged durability do not change.

## Behaviour

**S1. Capacity report.** With each heartbeat the node's data plane reports T, used bytes,
quarantined bytes, S and slots used. The service keeps the report with the node's liveness
record, so it expires with liveness; no clocks are compared. A report is valid if T and S are
positive, used plus quarantined is at most T, and slots used is at most S; an invalid one is
kept as none, liveness is renewed anyway, and a warning is logged each time a node's report
turns invalid. A heartbeat without a report stores none. A report from any identity but the
node's own data plane is refused (S10). A node without a valid current report takes no new copy.

**S2. Ledger.** A node has at most one ledger. Registering a node that has no record creates its
ledger at zero; a node registered before this change has none until S6. Only a node with a
readable ledger takes a new copy.

**S3. Charging.** Outside initialization (S6), exactly these transactions add charges,
conditional on the ledger revisions they read:
- `CreateVolume`'s claim charges every copy it places, and records the claim's charge per node
  until the creation completes.
- `MoveReplica` charges the destination. A destination the chunk releases is refused, by #13's
  S2, before any charge.
- A volume's growth (#70) charges its new chunks' copies, at soft as a create, in the transaction
  that exposes them. #70 lands with or after #72 and adds that charge and its test.

The move's completion makes the source a released copy (#13's S1), still charged. This plan
changes nothing in `ChangeIsr`.

**S4. Release.** `DeleteVolume` writes in the tombstone the charge per node of the volume's
named copies: for a volume being created, the claim's charges; otherwise its replicas, pending
destinations, released copies and fence holders. The tombstone names every such node (#13's S5).
The `ConfirmDeletion` that newly confirms a node, by an identity `auth.go` allows for it (the
node's agent today, its data plane after #13's S12) or an administrator,
subtracts that node's charge in the same transaction, once; so does #13's `ConfirmRelease` for
the copies it confirms, keeping one slot when it makes the node a fence holder (S11). A release
subtracts from a readable ledger, or from the value a mark keeps (S6); a release above it removes
that ledger or value, logged as an error. A volume whose header does not parse gets no charges:
its copies stay charged until S6.

**S5. Admission.** A node may take `n` more copies of chunk size `c` at watermark w if
`L.bytes + n × c ≤ budget bytes` and `U + n × c ≤ budget bytes`, and `L.slots + n ≤ budget
slots` and `slots used + n ≤ budget slots`. Equality is admitted. Room is the largest such `n`.
L is the bound that matters; U guards what L cannot see (S9's transient use, copies no record
names).
- **Example:** T 100 GiB, at soft 80 GiB. L 79.5 GiB, U 20 GiB, c 256 MiB: room 2 by bytes.
  S 3,200, at soft 2,560 slots; L 318 slots, 300 used: 2,242. Room is 2: a third is refused.
- **Drain, then undrain:** T 100 GiB; 240 copies of 256 MiB, written, are moved off: released,
  so L and U stay 60 GiB until the node confirms their deletion. The node then takes 80 copies
  (room (80 − 60) / 0.25); L is 80 GiB, room 0. Had the moves released the charges (L 0), L would
  be 20 GiB after those 80 while U stayed 60, so room would be 80 again: 140 GiB promised on a
  100 GiB store.
- **CreateVolume** uses soft. Candidates are today's (alive, not draining, not a witness) with a
  valid report and a readable ledger; others are left out as a dead node is today. Placement
  fills no node beyond its room. A node without room stays a candidate for failure domains, so a
  full zone or rack refuses the volume rather than letting `bestEffort` fall back. A preferred
  node is used only if its room holds a copy of every chunk; otherwise it is ignored, as an
  ineligible one is today. More than 116 distinct nodes (a margin under the 118 a claim fits):
  `FAILED_PRECONDITION`, before any write.
- **MoveReplica** uses hard: drain, rebuild and administrator moves may use the space between
  the watermarks. A destination without a valid report or a readable ledger:
  `FAILED_PRECONDITION`.
- **Refusal for room:** `RESOURCE_EXHAUSTED`, naming each node refused with L, U, its budget and
  the charge; nothing is written. A placement that fails while some node had no room left is a
  refusal for room; nodes left out are named in either error.
- **Contention:** a transaction that fails on a ledger revision rereads and plans again, up to
  32 attempts with the service's backoff, then `ABORTED`.

**S6. Initialization.** `InitCapacity`, by an administrator, recomputes ledgers from the records:
those absent, unreadable or being initialized, or with `all`, every registered node's. It marks
each such ledger as being initialized; the mark keeps the readable value it replaces, or the one
an earlier mark kept, and releases while marked subtract from it (S4). It reads the records in
one snapshot and writes each node's charge of named copies (S3, S4 applied to the snapshot).
A target pending on a tombstone without charges (written before this change or rewritten by an
older binary), or named by the chunks of a volume whose header does not parse, is blocked by it:
it gets back the value its mark keeps, or stays marked if the mark keeps none. Every write obeys
the contracts' one rule (Initialization and releases), so no release is lost: a release during
the call leaves the node marked, to be run again. It reports per node the value before the call
(a mark's kept value included), the recorded charge (a recorded charge above the earlier value
names a placement the ledger did not charge), whether it is still marked, and, for a target,
its blocker; and the volumes holding a claim. The CLI exits 2 when a target is still marked or
blocked, with one line per node saying what to do (the contracts' CLI lines); a node the call did
not target never makes it exit 2.

**S7. Visibility.** `ListNodes` shows each node's current report and ledger.

**S8. Drain.** A move refused with `RESOURCE_EXHAUSTED` or `FAILED_PRECONDITION` takes that
destination out of the drain step's candidates; the chunk goes to the next least-loaded one.
With none left, the step fails, naming the last refusal.

**S9. Unchanged.** Replicas, ISRs and minimum ISR are node lists counted as today; `ChangeIsr`,
`Attach`, epochs, fencing, tokens, the I/O path, out-of-space handling (a full replica fails its
session), the on-disk format and the replication wire, beyond what #13 changes. Transient use,
clusters held in quarantine or freed before the next checkpoint, is not charged: the space above
the watermark absorbs it.

**S10. Authority.** `InitCapacity`: administrators only. A heartbeat carrying a report: only the
node's own data plane identity; from its agent or an administrator, `PERMISSION_DENIED`. No RPC
writes a ledger directly.

**S11. Dependency on #13.** This plan builds on #13's metadata rules (#13's S1–S5: `released`,
`Fenced`, the refused move to a released node, `ConfirmRelease`, the tombstone naming released
nodes and fence holders). It also needs one change to #13's approved plan, a prerequisite: #72
lands only with or after it. The change, all of it:
- `ConfirmRelease` takes 1 to 118 chunks, and #13's sweep confirms in batches of at most 118.
- Its transaction also compares the node's ledger (revision 0 if absent) and writes it by S4's
  release rule: n chunk sizes and n slots, less one slot when it adds the node to `Fenced`.
  Header, 118 chunks and the ledger make 120 compares and 120 writes.
- A failed compare with every chunk still at its listed revision (the header or the ledger
  moved) is retried as a moved header is.
- The ledger read is part of the service's handling of the sweep's call; #13's bound on a
  node's reads (its S17) does not change.
- In #13's text: `design.md:72, 76–81, 142, 628, 674` and `decisions.md:248`; its engine tests
  gain a volume with 119 released chunks confirmed in two batches. #13's spec is unchanged.

Until #13's sweep confirms releases (its S6, with release deletion on, which is off by default),
nothing but its volume's deletion releases a released copy's charge. A copy deleted by hand or
lost with a replaced disk stays charged meanwhile: hand deletion frees no capacity.

## Tests

- **S1, S7:** a valid report shows in `ListNodes`; each invalid one is stored as none with
  liveness renewed and one warning; a report expires with liveness; the data plane's heartbeat
  carries its store's figures; the Rust client's report reads back through the Go service.
- **S2, S6:** a new node gets a zero ledger, a re-registration keeps it, a legacy node gets none;
  initialization of a legacy fixture, fence holders included, gives exact values; a
  `ConfirmDeletion` or `ConfirmRelease` racing it, on a node with a ledger, without one, or
  blocked, leaves the node marked (the release in its kept value where it has one, the mark
  rewritten where it has none), and the rerun is exact; a node the call did not target carries no
  blocker; two
  concurrent calls; 32 failed mark batches give `ABORTED`; unreadable and marked ledgers are
  replaced; the CLI exits 2 with each still-marked or blocked node's line; a second call changes
  nothing.
- **S3–S5:** the room arithmetic at every boundary and both examples, the second as a drain,
  undrain and creates; charges after create and move, and after growth once #70 has landed; a
  completed move keeps the source charged; a move to a released node writes no charge; deleting
  a volume mid-creation releases its whole claim; release once per confirmation and per
  `ConfirmRelease`, by ledger state, an administrator's confirmation included, a fence holder
  keeping one slot; a stale, replayed or non-releasing `ConfirmRelease` leaving the ledger
  unchanged; interleaved creates, moves and confirmations never over-admit, on both stores, the
  etcd one enforcing 120 operations; a full zone refuses; 116 nodes succeed and 117 are refused;
  refusals write nothing; a create succeeds after one ledger conflict and is `ABORTED` after 32.
- **S8:** a drain skips a full or unreported destination, and fails when every one is.
- **S10:** the authorization matrix for `InitCapacity` and for reports by every identity.

## Open questions

- **Q1:** the watermarks are fixed in this step; operators cannot tune them before the
  configurable policy lands. Recommended: keep them fixed until that step.
  Settled at approval, 2026-11-13: kept fixed at 80 % and 95 % in this step (see decisions.md,
  Settled at approval).
- **Q3:** a metadata-service instance on the old binary charges nothing, and when it rewrites a
  tombstone it drops its charges (and, per #13's S14, `released` in a chunk). Recommended: an
  upgrade runs every instance on the new binary, then `capacity-init --all`, whose rows name the
  placements the ledger did not charge; nodes still pending on a tombstone an older binary
  rewrote stay blocked, so run `--all` again once those tombstones are gone. There is no
  automatic exclusion in this step.
  Settled at approval, 2026-11-13: that upgrade procedure, with the second `--all` added in
  round 3 (see decisions.md, Settled at approval).
- **Q4:** a create may touch at most 116 nodes. Recommended: accept in this step.
  Settled at approval, 2026-11-13: accepted (see decisions.md, Settled at approval).
- **Q5:** `ListNodes` shows capacity to every authenticated identity, as it shows addresses.
  Recommended: accept.
  Settled at approval, 2026-11-13: accepted (see decisions.md, Settled at approval).
