# Issue #80: One supported volume geometry — intent

[Issue #80](https://github.com/jsfakian/Flaminio/issues/80); [follow-ups](../../followups.md).
Approved 2026-11-13; not implemented.

## Why

1. **Metadata accepts unattainable layouts.** `Spec.Validate` permits any positive 4 KiB size
   and only checks a floor-divided 2^20 chunk ceiling (`meta/domain/domain.go:176`).
   `Header.ChunkCount` uses an overflowing ceiling addition (`:57`). A 1 MiB volume with
   256 MiB chunks passes, but engine attach and replica Hello require complete chunks
   (`flaminio-node/src/engine.rs:1603`, `replica/mod.rs:179`).
2. **An accepted create can panic transport.** Hello encoding asserts at 65,536 chunks per
   replica node (`flaminio-net/src/frame.rs:196`); RF=1 can place every chunk on one node.
   The audit reproduces rejection and panic using real code and memory stores, not the lab
   (`docs/audits/2026-11-12/repro/geometry.rs.txt` in the audit checkout).
3. **CSI and the store disagree too.** CSI rounds to 1 MiB, casts signed capacities before
   checking them and multiplies quantity suffixes unchecked (`control/csi/controller.go:193–229`).
   The store permits at most 1 GiB chunks and one page table per chunk (`flaminio-store/src/state.rs:210`).
   Its 64 MiB format sizing constant is not a minimum enforced by chunk creation; 1 MiB chunks
   work but can exhaust the finite chunk-slot table earlier (`flaminio-format/src/layout.rs:112`).
4. **Hello is not the only transport bound.** Full volume protobuf replies repeat replica/ISR
   names, versions and moving endpoints (`api/proto/flaminio/meta/v1/meta.proto:105–126`).
   CSI and the Rust metadata client use their normal 4 MiB receive budgets. A 65,536-chunk
   Hello ceiling alone cannot guarantee the full metadata view fits, especially at RF=5.

## What we want

1. One proposed contract: full chunks, powers of two from 1 MiB through 1 GiB, checked counts,
   a 65,536 Hello ceiling and a stricter worst-case full-metadata-reply budget where necessary.
2. CSI chooses a supported capacity within the caller's signed range and returns that capacity;
   direct metadata callers receive validation errors before any provisioning write.
3. Engine, authenticated replica sessions and wire sending reject unsupported geometry without
   panic or integer truncation. Local store table exhaustion remains an explicit resource error.
4. Boundary vectors shared across Go/Rust and real frame/store paths prove each layer agrees.

## Out of scope

- Partial final chunks, frame/RPC pagination and wire/on-disk changes.
- Expansion and XFS (#70), distributed capacity/slot reservations (#72), queue/scale acceptance
  (#75), and the separate ISR transaction splitting defect (#81).
- A guarantee of physical free space after creation: thin provisioning, concurrent consumption
  and transient failures can still prevent attachment; geometry must not cause that failure.
- Runtime implementation, shared-lab operations and adoption of proposed ADR 0010.
