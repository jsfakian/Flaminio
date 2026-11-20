# #72 round 7 (incremental, design-code and design-security): diff 8c718f4..4eadb5f

## Round-6 findings
- R6-B1 (raced blocked node line): fixed. Contracts :142-145 key on `had_ledger` (which counts a
  mark's prev, :88); S10 test lists the raced blocked node and the one without prev.
- R6-B2 (blocked_by scope): fixed in text (contracts :92-93,:137-139; spec S6; S10 CLI test exits 0
  for an untouched pending node). The service-side rule is not tested: see R7-1.
- R6-B3 (growth with no design): fixed. Design bullet (:85-89), Files touched (:143), S3 row,
  contracts writer list and transaction row, intent item 5, decisions. They agree with #70's
  "Charges" (soft watermark, Place with room map, each used node's ledger compared and written in
  the step's transaction; ledger lander removes step 1's refusal; 120 bound 1+k+D, k shrinks).
  Release works: the tombstone's charges are taken from the header's chunks at delete, so grown
  chunks are released by the same rule.
- Nits 1-3: fixed (lab.sh and lab.md wording on exit 2; TestInit121Nodes both stores, etcd 120;
  the no-prev case in the race test).

## Findings
### R7-1 Low: "a node the call did not target carries no blocker" has no service-level test
spec.md S2/S6 test line says it; design.md S6 row (`TestInitCapacity`, :175) does not list it. The
only test is the CLI's (S10 row, :179), which uses a fake server and cannot fail if the service
fills `blocked_by` for untargeted rows. Fix: add to `TestInitCapacity` a plain (non-`all`) call
with a readable node pending on a legacy tombstone: its row has empty `blocked_by`, `marked`
false, and its ledger is unchanged.

## Nits
1. design.md:85-89 gives growth's 120 bound only by reference to #70; one clause ("k shrinks to
   the longest prefix that fits, as #70") would let #72's implementer, if second, build it without
   opening #70.
