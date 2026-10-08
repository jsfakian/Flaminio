# Replication in the MVP (step 5)

What the MVP implements, how it behaves under failure, and how it falls short of the full design
([ARCHITECTURE §8](ARCHITECTURE.md#8-replication-and-consistency-model)). The code is
`dataplane/crates/flaminio-io/src/{proto,remote,replica,volume}.rs`.

## Shape

```
 fl-a: flaminio-io serve --replica 147.52.71.226:7410        fl-b: flaminio-io replica --listen :7410
   /dev/ublkb0 ─▶ volume ─┬─▶ local store (/dev/vdb)            ┌─▶ store (/dev/vdb)
                          └─▶ remote ── TCP (one connection) ───┘
```

- **RF = 2, with a fixed role for each side.** fl-a is the volume target and the source of
  truth. fl-b holds a replica. There is no metadata service: placement, epoch and membership
  come from the command line.
- **Writes:** sent to the replica and committed locally at the same time; acknowledged when both
  are durable. Each side runs the batch as one group commit.
- **Reads:** always served by the local replica (data locality). Replication does not touch the
  read path.
- **Discards:** forwarded, so both stores apply the same whole-cluster semantics and stay
  identical.

## Protocol

Length-prefixed frames over one TCP connection (`proto.rs`): `Hello`, `Write`, `Discard`,
`Digest`, and replies `Ok`, `WriteResult`, `Digests`, `Error`. The target sends one request
and reads its reply before sending the next, so the replica applies requests in order. Frames
are capped at 64 MiB, and every decoder rejects malformed input without panicking (unit tests
cover every truncation).

- **`Hello {volume, epoch, layout}`:** the replica creates missing chunks and **raises the
  fence** to `epoch`. A target with an older epoch is refused from then on (tested).
- **`Digest`:** CRC-32C of each 64 KiB unit of a range. Unmapped data reads as zeros, so two
  replicas agree exactly when their contents do.

## When the replica fails

Following the 2-node rule ([ADR 0006](adr/0006-edge-two-node-degraded-writes.md)), the volume
**keeps serving** from the local replica:

1. A send, receive or timeout error (5 s by default) drops the connection. The writes of that
   batch are already durable locally, and are recorded as **dirty** 64 KiB units.
2. Writes while the replica is down are acknowledged after the local commit, and their units
   are recorded as dirty.
3. Every 50 ms (an io_uring timeout on the ublk ring), up to 10 ms of background work runs:
   reconnect (every 2 s while down), then copy dirty units, 1 MiB per request.
4. When nothing is dirty, the replica is in sync again (logged as `InSync`).

**After a target restart** the dirty set is gone (it is not persisted). On its first connection
the target therefore compares every unit's digest with the replica, 16 MiB per step, and copies
only the units that differ. That is also what makes a replica that missed writes while the
target was down safe.

**Ordering:** all of this runs on the target's single thread, between write batches, over one
ordered connection. A resync copy reads the local unit and sends it in one step, so it can never
overwrite a newer write on the replica.

## Verifying it

- Unit tests (`cargo test -p flaminio-io`, run on loopback): writes are mirrored; writes
  continue while the replica is down and are resynced; data written before the replica was
  attached is found by the scan and copied; an older epoch is refused. Two mutation checks
  confirm the tests fail without dirty tracking and without the initial scan.
- `make repl-verify` stops both sides and compares per-chunk checksums of the two disks
  (`flaminio-io digest`).
- `make repl-fault`: fio writes 4 GiB with its own checksums on fl-a; fl-b's replica is stopped
  10 s in and restarted 15 s later. The writes must not fail, fio's read-back must verify, the
  replica must reach `InSync`, and both disks must then be identical.

## Limitations (all in the full design, none in the MVP)

| Missing | Consequence | Where it comes in |
|---|---|---|
| TLS, authentication | Anyone who can reach the port can write. The lab allows only rhythm's address (`make net-setup`) | Architecture §15.2: mTLS and capability tokens |
| Metadata service, ISR, epochs from Raft | Roles and epoch are static. No automatic failover to fl-b if fl-a dies | Architecture §5.4, §8 |
| Persistent dirty tracking | A target restart means a full digest comparison (reads all mapped data on both sides) | Per-chunk dirty bitmaps (§8.3) |
| Replica-confirmed leases | Not needed while the target is fixed | ADR 0005 |
| More than one replica, `minInSyncReplicas` > 1 | RF = 2 and degraded writes only | Placement controller (§9) |
| Zero-copy, pipelining across batches | Each write batch waits for both sides before the next | Later performance work |

**Performance note:** fl-a and fl-b are joined by 1 GbE, so replicated writes are capped at
about 110 MB/s. Following the reads-only rule for benchmarks, replication is judged on
**correctness**, not on write throughput.
