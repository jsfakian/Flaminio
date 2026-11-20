# Proposal: Publish cuts atomically and catch up every retained generation

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12

## Context

[Issue51](https://github.com/jsfakian/Flaminio/issues/51), [spec](spec.md), [design](design.md).
Existing LayerCreate/top-only writes do not by themselves establish consistent multi-replica
snapshots. Audit proposed0011 requires durable operation identity/commit and lazy untouched-
chunk semantics; proposed0009/#50 constrains future encrypted references. Neither is accepted.

## Decision proposed

Pending snapshot request is durable prepare with immutable operation/incarnation identity;
only current target at expected epoch/generation can atomically commit catalog+generation.
Target first reconciles all chunks and drains ISR writes, holds subsequent client writes,
then resumes only after authoritative outcome. Lost replies never cause speculative abort.
A committed cut determines views through layers below its generation even when a chunk has
not materialized that generation. O(1) describes metadata commit, not drain latency.

New version2 wire carries write/copy generations and snapshot views. Replicas push existing
LayerCreate before newer payloads; no write below durable top. Catch-up verifies each stage
oldest-first, suppresses newer live writes until lower history complete and joins only with
all committed cuts. A lower mismatch under a newer layer is blocked; do not erase evidence
or invent a new record. If model/runtime cannot prove progress under supported failures,
revisit the no-format-change decision before implementation.
Settled at approval, 2026-11-13: Q3's order, revisited only if the model cannot prove progress (see decisions.md, Settled at approval).

This slice caps63 lifetime snapshots, no individual deletion/GC, CSI clone/group/quiesce or
external retention. Live-volume deletion removes its private cuts. Encrypted snapshots are
not admitted until original authenticated identity/key reference policy is settled. Future
retained readers/clones/backups need durable pins and conditional GC; no retained key can be
released by ordinary PVC deletion. Group publication and idempotent thaw remain later steps.

## Consequences and open choices

Cuts can block writes for metadata outage/reconciliation; resync/moves must transfer historical
content and can be far more expensive than current-only copies. Catalog increases reply budget;
snapshot geometry eligibility (#13's bound plus the full catalog, also at growth) is a
compatibility condition. Retained layers are charged: a volume with k snapshots is charged
(1 + k) times by #72's ledger, in bytes and slots, and #73's quota; a layer's slot bounds the
page pool by the format's sizing (plan review round 2). Existing nodes need a coordinated wire
upgrade; no rolling compatibility is claimed.
Disk-replacement identity and historical-corruption repair remain prerequisites/limits, not
solved by top generation.
Q1–Q8 and the exact stage invariant are settled with the plan (decisions.md); the bytes shared
with #52 and #58 wait for #58's coordinated review (contracts).
Settled at approval, 2026-11-13: Q1–Q8, the stage invariant and budget eligibility as proposed; accepted with the plan (see decisions.md, Settled at approval).

## Confirmation

Metadata crash/CAS/idempotency vectors, layered store/replica replay tests, untouched-chunk
failover scenarios, bounded wire fuzzing and all eleven spec mutations, early publication's
against the publication check, each caught on a seed that passes clean. Later authorized lab
compares per-replica snapshots against recorded cut histories.
No tests/results or global ADR numbering are claimed here.
