# #73 plan review, round 13 (incremental; design-code and design-security)

Scope: git diff 3b298c6..b7b0b82 -- docs/issues/73/ (minus report.md).

## Earlier finding
- R12-B1: fixed. design.md:183-186 says that with no events appendEvents returns only the tail compare and no tail write. Spec S7-S9 (spec.md:238-244) and design S7-S9 (design.md:243) have each case also assert the other two compares.

## Consistency checks (no defects)
- appendEvents with no events agrees with contracts.md:121 ("one that changes nothing still commits, with no event"), design.md:58-61 (PutAuthorization compares the tail and writes the record and its events, with the overdraft when it grants nothing) and decisions.md (round 12 entry).
- A no-op PutAuthorization still writes the record, so the record compare fires and the tail compare does not.
- Case 1: a cfg prune's first transaction compares the record without writing it (design.md:200-201), and its last transaction deletes the sweep key. So the record's revision is unchanged and the sweep key absent. The tail moves (audit.pruned and pruned).
- Case 3: nothing moves the tail or record after the reads, and the sweep key is present.
- Spec and design list the same assertions per case.

## Findings
None.
