# Issue #86: Bound conservative Fenced uncertainty — intent

[Issue #86](https://github.com/jsfakian/Flaminio/issues/86), audit follow-up F09 from the
independent correctness/security review of #80. This is a separate runtime defect, not a
larger geometry-admission example. Approved 2026-11-13; not implemented.

## Why

1. `CoreChunk.uncertain` retains every unpruned (extent,Version); duplicate writes grow it too.
   `report` unions that vector only when Hello arrives (`flaminio-repl/src/replica.rs:64–79`).
   A valid token and bounded individual frame therefore do not bound retained uncertainty.
2. Safe in-memory probes use actual ReplicaCore/Frame code. One legal 1 GiB chunk with 65,537
   alternating 4 KiB writes produces a 524,317-byte reply the decoder rejects. Two legal 1 GiB
   chunks with 65,543 each produce a 1,048,717-byte body and encoder assertion panic.
   Evidence: audit commit 7b94e2d, `docs/audits/2026-11-12/repro/fenced.rs.txt` and
   `fenced-repro-results.txt` in the audit checkout. No shared disk/lab was used.
3. These volumes have 1 and 2 chunks, below #80's proposed admission limits. Fenced decoder caps
   total extents at 65,536, but encoder has only its final body assertion
   (`flaminio-net/src/frame.rs:285,342,466`). Its chunk-row count is not bounded explicitly.
4. Restart already safely reports a whole chunk until a newer epoch prunes it
   (`replica.rs:129–143,248`). Repair writes use prune=None; Settle follows reconciliation
   (`target.rs:1347,2480`). A new fallback must preserve both version and restart barriers.
5. Target on_fenced currently searches the report once per placed chunk and ignores bad
   extents after marking fencing (`target.rs:1311`). Bounds alone must not turn malformed
   uncertainty into an empty report or quadratic recovery work.

## What we want

1. A fixed per-volume uncertainty budget, with conservative whole-chunk fallback rather than
   truncation or unlimited records, including repeated unpruned/reconciliation writes.
2. Coarsening that never omits an unresolved block: retain an upper bound on represented
   versions and preserve restart's separate epoch rule; ordinary Settle remains the proof path.
3. Generated Fenced reports always fit existing count/body bounds for supported held chunk
   sets; checked encoder/decoder/target validation refuses unsupported states without panic.
4. Real retained-probe/frame regression tests plus simulator coverage and caught mutations for
   the new coarsening/prune/validation rules; bounded state, allocation and work are asserted.

## Out of scope

- #80's CSI/create admission fix, #75's global scheduling/resource policy and #51's versioned
  snapshot protocol. Coordinate limits; do not pretend these plans are implemented.
- Frame pagination, limit increases, new wire fields, on-disk uncertainty or epoch changes.
- Earlier acknowledgements, weaker all-ISR durability, skipped fencing/reconciliation or
  automatic destructive cleanup of unsupported legacy replica data.
- Running the lab or claiming new runtime/model tests passed while drafting.
