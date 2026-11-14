# Volumes grow offline, in committed steps served from the next attach

- **Status:** Accepted with the plan, 2026-11-13 ([decisions](decisions.md)); binding for this
  plan's implementation from the approval on; numbered and moved into `docs/adr/` only with the
  implementation.
- **Date:** 2026-11-12 (revised 2026-11-13, plan review round 1; after acceptance, rounds 2 and 3
  edited wording and added the marked note in Consequences; rounds 4 to 6 extended it)
- **Issue:** [#70](https://github.com/jsfakian/Flaminio/issues/70)

## Context

The [intent](intent.md) and [specification](spec.md) require PVC expansion. Metadata, targets,
replica sessions and the ublk device all assume a size fixed at attach. A replica creates chunks
only on Hello, and a new Hello replaces the target's session, which the target treats as a member
failure for every chunk on that node: a running target cannot take new chunks without dropping
its prefix replicas from their ISRs. A growth larger than one etcd transaction needs several.
Accepted ADR 0003 keeps Linux 6.6 supported, which has no ublk `UPDATE_SIZE`.

## Decision proposed

Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).

- **Growth is a sequence of self-contained committed steps.** Each step is one conditional
  transaction that compares the volume header and #78's placement gate, appends at most 64 whole
  chunks placed by the volume's recorded failure domain and policy with its RF, and raises the
  header's size. There is no pending operation, growth record or generation: every committed size
  is a complete volume, and a failed request leaves a smaller complete one.
- **A target serves the size it attached with.** It applies metadata views to its own chunks and
  ignores chunks appended later; they are served by the next attach, through the normal fencing,
  ISR and reconciliation. Expansion is therefore offline: the ublk device takes the new size when
  an attach creates it, and no live device is resized.
- **A replica's known size only grows,** and only from a Hello its fence accepts.
- **Chunk descriptors keep `volume_size` from creation.** The format's bytes are unchanged; the
  field's meaning becomes "size when this chunk was created". Authoritative size is metadata's.
- **Growth is off by default:** the metadata service grows volumes only with `--volume-expansion`,
  set after every data plane node runs a version that follows the two rules above.
- **ext4 grows with `resize2fs` on the device pinned under #15 and #79;** raw block needs no tool.

## Options

- **Pending growth operation with live append:** needs a same-epoch session
  reopen the core does not have, and an abort path for a stuck operation that blocks attach.
- **Replace the volume and copy data:** not an in-place expansion; a separate migration design.
- **Change only the header and the ublk size:** exposes chunks no replica holds and breaks the
  target's chunk vector.
- **Committed steps served from the next attach:** small changes to existing paths, offline only.

## Consequences

Controller success commits metadata only; the device and filesystem grow at the next stage. Online
resize and XFS need later decisions. A node out of chunk slots at that attach refuses the session,
like any failed replica, until space is freed or #72 reserves it. A data plane rollback after a
growth is unsupported. Descriptor sizes alone cannot recover a volume's current size (#54).

*Added after acceptance (plan review rounds 2 to 6). Settled 2026-11-13 (user's instruction):
accepted (see decisions.md):* each growth step charges #72's capacity ledger and #73's quota in
its transaction; this plan lands with or after #72 and adds the ledger charge, and whichever of
#70 and #73 lands second adds the quota charge. What is left: when a node's physical slots run out
under its ledger, a chunk none of whose replicas accepts the session waits without a deadline, and
for ext4 the node plugin's steps on the device wait with it, holding its lock: the design's
node-wide wait (Risks), Settled 2026-11-13 (user's instruction): accepted as stated (see
decisions.md).

## Decisions settled at approval

Settled with the recommendations on 2026-11-13 ([decisions](decisions.md)): expansion does not
wait for #72, accepting the chunk-slot availability risk above; 64 chunks per step; suffix chunks
prefer the volume's last target; the rollout order is the gate, with no automatic version check.
*Changed after acceptance (rounds 5, 6). Settled 2026-11-13 (user's instruction): accepted (see
decisions.md):* this plan lands with or after #72, so growth always charges #72's ledger.

## Confirmation

The [design](design.md) maps S1–S9 to service tests on memory and etcd, simulator runs that
catch `drop_grown_view`, replica and engine memory-store tests, CSI tables and the lab check
`make csi-expand`. None of those checks ran here.
