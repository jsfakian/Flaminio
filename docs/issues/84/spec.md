# Issue #84: Validate the exported block durability contract — specification

[Intent](intent.md) · [Design](design.md) · [Decisions](decisions.md).
Approved 2026-11-13; not implemented.

## Terms

- **Logical block:** one aligned 4 KiB unit exported by the frontend.
- **Effective ISR:** committed ISR plus a joiner that already counts under protocol §5/§7.
- **Completed write:** every split piece returned durable success before client completion.
- **Cacheless mode:** advertised absence of a volatile write cache, with no acknowledged
  volatile writes hidden behind FLUSH.
- **Hint discard:** success with no promise of reclamation or zero reads.
- **Kernel evidence:** results from actual ublk requests on an identified kernel/device stack,
  distinct from a fake request handler or in-memory persistence model.

## The invariant

No successful frontend write loses an acknowledged logical-block value while its authoritative
ISR survives. No frontend flag or partial-error path bypasses fencing, membership or persistence.
An unsuccessful larger request may have landed partially; there is no multi-block transaction.

## Behaviour

**S1. Advertised geometry and flags.** Every supported queue/buffer mode advertises the same
4 KiB logical block and cacheless contract. Unsupported operations/flags are refused rather
than acknowledged with stronger semantics than the implementation supplies. Geometry admission
uses #80's shared bounds, not a second inconsistent limit.

**S2. Durable completion.** Ordinary and FUA writes complete only after all their pieces meet
the effective-ISR durability requirement, including committed shrink before excluding a peer.
A failure can leave data present; a retry does not depend on that request having rolled back.

**S3. FLUSH and FUA.** Under the cacheless advertisement the kernel completes a FLUSH itself and
clears the flush and FUA bits of writes, so neither reaches the frontend. One that arrives
anyway is still safe: FLUSH completes as a no-op and a FUA write takes the ordinary write path.
Both rest on S2 alone: a flush covers the writes completed before it was issued, and each of
those is already durable. The contract does not turn an arbitrary concurrent write into a
predecessor of every FLUSH, and promises no barrier over outstanding queues.

**S4. Ordering and failures.** Overlapping logical-block operations remain linearizable across
queues. A larger request is not promised atomic across blocks or chunks. Return success only
when every piece succeeds; errors and retries cannot resurrect a stale target or unverified read.

**S5. Discard.** The volume device advertises no discard and no write-zeroes limit, so the kernel
refuses both itself. A discard that arrives anyway is explicitly a hint. Do not advertise
discard, deterministic zero-after-discard or physical reclamation here. A future stronger
operation needs replicated ordering, uncertain-state recovery, snapshot-reference protection and
preserved fence/incarnation evidence.

**S6. Persistence assumptions.** Default durability retains explicit device flush/FUA. Omission
requires independently verified stable-write completion for the entire hardware/controller/VM
stack and a separate reviewed configuration; a PLP label alone is insufficient.

**S7. Evidence.** Tests distinguish handler flags/decisions, modelled durability and actual
kernel behavior. Report kernel, libublk, queue mode/count, write-cache configuration, backing
stack, failure points and observed outcomes. Missing/unsupported environments fail or skip
explicitly, and never count as conformance passes. A kernel check creates the volumes it writes,
accepts no volume or device from its caller, and bypasses no admission check of the node.

**S8. Compatibility and scope.** Keep write-back disabled and checksum extension separate.
Any newly required ordering/safety rule is modelled in flaminio-repl with a mutation caught by
its checker before enabling it. Public docs and device capabilities match measured support.

## Tests

- S1/S5: both request adapters and exported params; invalid sizes/flags/opcodes and discard reads.
  On an actual kernel: no discard limit, the discard ioctl refused, the data unchanged.
- S2/S4: engine/store failure injection across chunk pieces, overlap histories, ISR shrink/join,
  delayed completions, stale epochs, retries and power cuts after acknowledgement.
- S3: the frontend's decision for FLUSH and for FUA writes; the early-acknowledgement mutations
  of S2 stay caught. On each actual kernel/buffer mode measured, with two queues: the cacheless
  limits, and a request trace that shows the check's writes and no flush or FUA at the device.
  A trace that misses writes is a failure.
- S6: persistence fault model and identified disposable-device power-loss evidence before any
  flush-elision option can be enabled; no such option is introduced here. The kernel check
  records that the store's flush reached its disk.
- S7/S8: explicit unsupported/skip reporting, protected lab opt-in, trace metadata completeness,
  declared cacheless flags and no write-back activation. Run test/race/sim/crash-test/contracts
  as applicable; sanitizer/Miri scopes are documented rather than overstated.

## Open questions

The measured matrix starts as one cell: the lab's kernel with the pinned libublk, both buffer
modes, two queues, virtual disks. Is evidence on the minimum supported kernel and on a physical
backing stack required before the issue closes? Should a flag bit the frontend does not know be
refused, or ignored as today? ADR0010 remains Proposed until accepted.
Settled at approval, 2026-11-13: unknown flag bits are refused, as the design's rule 2 states (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): minimum-kernel and physical-backing evidence is required
before closing, gathered by the kernel check as more cells of the matrix (see decisions.md).
