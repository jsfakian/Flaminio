# Issue #78: Placement admitted after drain marking — intent

[Issue #78](https://github.com/jsfakian/Flaminio/issues/78), audit F03 at `8aaa7a3`.
Approved 2026-11-13; not implemented.

## Why

1. **Admission uses stale lifecycle state.** `createVolume` lists nodes and load before its
   claim (`meta/service/service.go:382–415`). The claim compares only the volume's absence and
   tombstone's absence. `DrainNode` writes an independent drain key (`:827–846`). The audit's
   `TestAuditCreateDoesNotPlaceOnConcurrentDrain` pauses after the node listing, marks its only
   node draining, and resumes: RF=1 creation succeeds there. Evidence is preserved in the main
   checkout's `docs/audits/2026-11-12/repro/metadata_test.go.txt` and audit README, F03.
2. **Publication spans transactions.** A creating header claims the incarnation, chunks are
   written in bounded batches, then the header becomes visible (`:410–475`). `ListVolumes`
   excludes creating headers (`:848–862`), so merely validating drain state at claim time leaves
   admitted but unpublished work invisible to a drain's completion scan.
3. **Direct moves bypass eligibility.** `MoveReplica` checks that its destination is registered,
   then compares only header and chunk (`:866–925`). It can admit a move to a draining node even
   when the controller's earlier candidate listing excluded it.
4. **Completion is a client observation.** `control/drain/drain.go:172–185` reports success when
   its visible-volume scan finds no replicas. It neither counts hidden creators nor validates
   the drain generation, and it ignores a move into the node until its replica is added.

## What we want

1. Placement admission and drain marking have explicit, mutually ordered linearization points.
   Completed marking excludes later create and move admissions to that node.
2. Earlier admitted creations remain durably represented through publication or fenced cleanup.
   Drain completion accounts for them, visible replicas and incoming recorded moves.
3. Cancellation/restart cannot validate an old drain scan against a new drain. Concurrent
   creators, delayed publication and process restart retain the same guarantees.
4. Deterministic Go/etcd races and an executable admission model expose missing commit compares
   or reservation accounting. Keep every transaction within existing storage bounds.

## Out of scope

- Capacity reservation and balancing policy (#72); share this lifecycle admission contract later.
- Executing moves for unattached volumes (#17), failure-domain choice (#18), reclaiming stale
  physical copies (#13), and provisioning retry recovery (#76). Hidden interrupted work must
  block completion until existing cleanup or #76 resolves it; this issue never guesses it gone.
- Device evacuation certification, host shutdown proof, new epochs, I/O leases, format changes,
  lab runs or performance claims. Completion concerns admitted metadata work; tombstones still
  track physical deletion separately.
