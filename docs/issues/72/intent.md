# Issue #72: Capacity safety, balancing and locality — intent

[Issue #72](https://github.com/jsfakian/Flaminio/issues/72), ARCHITECTURE §9–11.

**This plan is one step of the issue:** its step 1, with the admission half of step 2, on the
code as it is: one store per node, the placement of `CreateVolume`, and the moves of
`MoveReplica`. Each node gets a capacity ledger; creation is refused from the soft watermark,
moves up to the hard one. The issue stays open; what stays open is listed below.

## Why

1. **Placement counts replicas, not space.** `domain.NodeLoad` holds a replica count; `Place`
   scores that count and topology (`meta/domain/domain.go:302–422`). Creation scans every chunk
   record for that count, then claims the id with no capacity check (`meta/service/service.go:
   384–417`). `MoveReplica` checks only that the destination is registered (`service.go:870`).
   Two creators can each place on the same free space; a 3 × 100 GiB lab can be promised any size.
2. **Nothing reports space.** Store `Stats` has data clusters, used and quarantined clusters,
   and chunk replicas (`flaminio-store/src/store.rs:99–118,2159–2171`); the superblock has the
   chunk slots. None of it leaves the node: the heartbeat carries only the node id
   (`flaminio-node/src/engine.rs:726–728`).
3. **Running out is a replica failure.** A store error closes the session (`fill`,
   `flaminio-node/src/replica/mod.rs:848–856`); the target removes the member if the ISR stays at
   or above the minimum, else writes wait (`flaminio-repl/src/target.rs:1296–1298`). That is safe,
   but a full node fails replicas that admission could have kept off it.
4. **The policy is unwritten.** ARCHITECTURE §11.6 gives overcommit 1.0 and 80/90/95 %
   watermarks, with rebuild and drain allowed up to the hard one. No code applies them.

## What we want

1. Each data node reports its store's capacity and use with its heartbeat.
2. A per-node ledger of committed bytes and chunk slots. Every placement that adds a replica to
   a node charges it in the same conditional transaction, so concurrent creates and moves cannot
   promise the same space. A copy a move leaves on its source stays charged while #13 names it
   released; a charge is released when the node confirms it deleted the copies (a tombstone's
   confirmation, or #13's `ConfirmRelease`). This step builds on #13's metadata rules and needs
   one change to #13's plan first (spec S11).
3. Creation refused from the soft watermark (80 %), moves (drain, rebuild, administrator) up to
   the hard one (95 %), with an exact error. Existing clusters initialized once by an
   administrator; the state visible in `ListNodes`.
4. Nothing changes on the I/O path, in replication, ISR or epochs, on disk or on the wire.
5. Growth, which is #70's, charges the same ledger (spec S3): #70 lands with or after #72 and
   adds the charge.

## What stays open in #72

- **Configurable, per-device-class policy** and overcommit above 1.0: this step fixes the
  defaults (overcommit 1.0, soft 80 %, hard 95 %) for one implicit class. It adds no device class
  to `CreateVolumeRequest`, `Volume` or `Node`. The high watermark (90 %) applies to
  discretionary moves, which come with the rebalancer.
- **Backpressure:** a typed ENOSPC and a defined target behaviour when space runs out.
- **The largest-failure-domain reserve,** with #45's step 5.
- **Rebalancing** (step 3), **locality** (step 4), **alerts and metrics** (step 5).
- **Several disks per node,** after #61's store identity.
- **Carrying the claim's charges into #76's recoverable creation.**
- **Measured operating limits** (the 2026-11-12 audit follow-up).

## Out of scope

- An operator or CRD redesign, cloud scale decisions (#63), snapshot or clone accounting.
- Any metadata call or lock on the I/O path.
