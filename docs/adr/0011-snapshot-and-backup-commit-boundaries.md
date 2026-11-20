# 0011. Snapshots and backups have durable commit boundaries

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

Architecture §13 describes a lazy O(1) generation bump and a group barrier; §14 equates RPO with the snapshot interval. A target crash during the bump, concurrent deletion/GC and interrupted transfer need durable state before these guarantees can be made. This proposal supplies the contract for #51–#54 and #58–#59.

## Decision

- Give snapshot and group operations durable IDs and recoverable prepare/commit state. Establish a barrier ordering all earlier I/O before the snapshot and later I/O after it. The commit is conditional on the target epoch and expected generation, so an old target cannot complete it after takeover.
- A lazily materialized generation is valid only when the durable metadata/barrier record determines reads of untouched chunks, including after restart, failover, move and resync. Mark the O(1) claim as metadata-operation complexity; it does not bound I/O-drain or application-quiesce time.
- For group snapshots, no member snapshot is exposed as the completed group until the durable coordinated decision is committed. Recover an interrupted prepare/commit with a specified completion/abort policy and idempotent quiesce/thaw cleanup. Failure to quiesce must not silently produce an application-consistent label.
- Snapshot/clone deletion, hydration and backup GC use durable references/pins and conditional transitions. In-progress readers keep required layers/keys alive. Publish a backup manifest only after its objects and key references are durable and validated. Resume uses immutable object identities and verifies content, not just names.
- A backup/DR recovery point is complete only when it is restorable at the destination. Actual RPO is the age of the latest completed recovery point, including scheduling, transfer, retries and outages; the configured interval is a target, not a bound. Report backlog, oldest unprotected data and failed restore verification.
- Retention and Object Lock are coordinated with reference GC and key lifetime (proposed ADR 0009). A manifest must identify immutable object versions where versioned storage requires them. A standby promotion records its recovery point and forks the relationship; it does not silently overwrite divergent history on failback.

## Consequences

A coordinated group barrier and reference GC are more complex than a generation counter. Outages make RPO unbounded unless the product deliberately blocks writes under a separate policy. Retained backups consume capacity and keys even after live volume deletion.

## Confirmation

Model barriers, epoch changes and reference lifetime. Inject crashes before/after each prepare/commit, partial group completion, backup manifest publication, concurrent deletion/GC, failed thaw, transfer restart and promotion/failback. Verify restored data and keys in a second empty cluster; include caught mutations for early publication and premature collection.
