# 0005. Replica-confirmed I/O leases

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

Epoch fencing (ARCHITECTURE §8.4) guarantees that an old volume target can never complete a
write. It cannot stop a partitioned old target from *reading* a replica that the new target
could not reach. That needs a lease.

The first draft had the metadata service grant node leases. That put the metadata service on the
I/O path: if it lost quorum, every volume in the cluster would suspend I/O once leases expired.
Replicating the metadata service more (ADR 0002) makes quorum loss rarer but cannot fix the core
problem. A node that is cut off cannot tell "the metadata service is down" from "I am
partitioned and someone else has taken over".

## Decision

- A target may serve I/O for a chunk only while **every ISR member** of that chunk has confirmed
  the target's epoch within the last `L` (default 5 s). Confirmations ride on I/O replies and on
  one heartbeat per node pair.
- A fenced replica refuses to confirm older epochs. A new target fences at least one ISR member
  of every chunk, serves reads at once, and waits `L × (1 + ρ)` before shrinking the ISR and
  writing.
- Node liveness in the metadata service is used only to decide *when* to fail over, never to
  permit I/O.

## Consequences

- **I/O continues through a total metadata outage.** Only provisioning, attach/failover, and
  chunks that lose a replica during the outage wait for the metadata service to return.
- Writes and leases rest on one invariant ("all ISR members"), which keeps reasoning and
  simulation testing simpler.
- The only timing assumption is a bounded clock rate drift ρ. No synchronized clocks are needed.
- A remote edge witness over a slow WAN never affects I/O latency.
- Cost: per-node-pair heartbeats, and a delay of up to `L` before a new target's first write
  after failover. In practice the delay is already covered by the liveness timeout.
- The lease length `L` defaults to 5 s and is configurable per cluster.
- The protocol must pass the deterministic simulator (ARCHITECTURE §19), with partitions,
  pauses and clock-rate drift injected, before phase 1 ships.
