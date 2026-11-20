# 0010. The block interface promises durable per-block writes

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

Architecture §6 says every write is all-or-nothing, §7 calls FLUSH a cheap barrier, and mentions optional write-back. The engine splits requests at chunk boundaries and the simulator checks histories per block. The current ublk path advertises no volatile write cache, acknowledges writes only after durable replication and treats client discard as a hint. This proposal makes the actual contract distinct from future features.

## Decision

- The exported logical block is 4 KiB. The replication contract is linearizable per logical block; it does not promise atomic multi-block, cross-chunk or multi-volume transactions. A failed or unanswered larger request may have partially landed and may be retried. Geometry and request bounds must be validated consistently by CSI, metadata, transport, engine and store before allocation.
- Success requires every piece to be durable on the effective ISR, and ISR shrink committed before excluding a member. Failed/unanswered writes may be present after recovery, but replicas must converge before serving the affected chunk.
- With no volatile write cache advertised, each completed write is durable; FUA does not weaken that rule. A no-op FLUSH is valid only under that advertised mode and the kernel's request-ordering contract. If a frontend supports a cache or an ordering barrier over pending requests, it must implement a cross-queue drain explicitly and test it.
- Do not add write-back in this phase. It requires a separate ADR, a recovery model for acknowledged volatile writes and frontend cache/FLUSH/FUA conformance tests.
- Client discard is currently a successful hint, with no reclamation or read-zero promise. A future deterministic zero/unmap operation is replicated, fenced and reconciled like a write; it cannot free data referenced by snapshots/clones or erase fencing/incarnation evidence.
- Persist the journal and dependent data before acknowledgement. Power-loss-protection branding alone does not justify skipping flushes: require a documented, verified stable-write completion contract for the entire device/controller/virtualization stack. Default to explicit flush/FUA.
- End-to-end checksum generation at the target is a future wire-format requirement, distinct from current receiver-generated storage checksums and TLS. Do not describe the current frames as carrying target checksums.

## Consequences

Applications must not infer transaction atomicity from block-request size. Conservative flushes can cost throughput. Exported discard may reclaim no space until the replicated implementation exists. Uncertain device durability is an unsupported deployment, not an optimization opportunity.

## Confirmation

Test overlap and partial failure across chunk boundaries, torn/power-loss writes, checksum failures, FUA/cache flags and FLUSH ordering across queues. Cross-layer geometry limits need boundary tests. Preserve crash-consistency campaigns and simulator mutations; add frontend conformance on disposable local devices or an explicitly authorized lab.

Implementation follow-up: [#84](https://github.com/jsfakian/Flaminio/issues/84).
