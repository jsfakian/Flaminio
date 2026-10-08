# 0002. Metadata service backed by a dedicated etcd cluster

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

Flaminio needs a linearizable, replicated system of record for placement, epochs, ISR sets,
node liveness and catalogs. That state changes too often to live in Kubernetes CRDs. The options
were a dedicated etcd cluster or a custom Raft implementation.

## Decision

- The metadata service is a Go domain service (placement, ISR transitions, epoch issuance, token
  signing) on top of a **dedicated etcd cluster**. It is never the Kubernetes etcd.
- etcd is used only through a narrow storage interface inside the metadata service. No other
  component talks to etcd directly.
- 3 members by default, 5 across ≥ 3 failure domains when available. The operator replaces
  failed members automatically, one at a time, without ever dropping below quorum. 2-node edge
  sites use a third, data-less **witness** member.

## Consequences

- A proven consensus implementation with leases, watches and transactions, available from day
  one.
- The practical ~8 GB size limit is several PiB of logical data at 256 MiB chunks. If that
  becomes a constraint, the storage interface lets us move to custom multi-Raft without changing
  clients.
- ISR changes take one extra hop (service → etcd). They happen only on failures, so this is
  acceptable.
- Replicating the metadata service does not make it safe for a partitioned node to act alone.
  Keeping the metadata service off the I/O path is handled separately by ADR 0005.
