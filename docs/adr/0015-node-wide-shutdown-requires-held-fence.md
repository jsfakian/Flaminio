# 0015. Node-wide shutdown recovery requires a held shutdown fence

- **Status:** Accepted
- **Date:** 2026-11-12. Numbered from [issue #82](https://github.com/jsfakian/Flaminio/issues/82)
  [adr-proposal](../issues/82/adr-proposal.md) on implementation, 2026-11-20.

## Context

[Issue #82](https://github.com/jsfakian/Flaminio/issues/82), its
[specification](../issues/82/spec.md) and [design](../issues/82/design.md) distinguish a live
partition from machine shutdown. The existing controller turns readiness and heartbeat absence
into a node-wide Kubernetes out-of-service taint, affecting all storage drivers. Flaminio epochs
and leases protect its block I/O, not old process side effects. Audit-branch proposed ADR 0007 is
context, not accepted authority for this decision.

## Decision

Node-wide shutdown recovery requires an authenticated authority contract that establishes the
exact machine incarnation cannot run old workloads during recovery. Readiness, elapsed time,
missing heartbeat, Node deletion and Flaminio fencing never supply that evidence. Bind each
conditional Node mutation to incarnation, evidence and a durable operation record. Refuse unknown
outcomes, foreign marks and unsupported authority configurations. Cleanup requires a separate
verified-return policy; Ready plus heartbeat is insufficient. Scoped partition recovery, if chosen,
needs explicit workload opt-in and cannot affect unrelated storage or promise process fencing.

The first production provider is
[manual-lockout-v1](../issues/82/manual-lockout-contract.md): signed trusted physical lockout,
permanent exact-UID binding, authenticated bounded configuration and terminal retirement with
fresh NodeName/PKI and clean OS. Preserve all replica disks/keys and unresolved #13/#14/#61
obligations. Other providers and unattended same-incarnation return are unsupported. Software
verifies attribution and protocol; it does not certify physical isolation.

## Options

- **Disable unsafe automatic mutation first:** smallest fail-closed correction; shutdown recovery
  remains blocked until an authority is integrated. This is a partial issue delivery.
- **Integrate a real shutdown authority:** preserves verified-shutdown recovery with operational
  credentials, mapping and durable fence lifecycle to implement and review.
- **Add scoped partition recovery:** may improve availability for selected workloads, but needs
  separate policy/contracts and does not fence arbitrary application side effects.

## Consequences

Partitions no longer force-detach unrelated volumes. Recovery can remain unavailable even after
long Grace, and old marks require explicit inspection. Controller/API restart cannot release the
fence or erase ownership. Provider outages block recovery rather than weakening its evidence.
Replication, protocol, on-disk format and steady-state I/O remain unchanged.

## Confirmation

Use issue #82's S1–S9 controller/API/evidence matrix, exact-identity concurrency tests and
explicitly authorized #49 live-partition versus power-off tests with mixed storage.
