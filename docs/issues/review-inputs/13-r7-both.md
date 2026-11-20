# #13 plan review, round 7 (incremental), design-code and design-security
Reviewed: git diff b7f686c..8586a66 (docs/issues/13/), against meta/service/service.go and service_test.go.

## Round-6 findings
- R6-L1 fixed in intent (tests stated); see R7-L1 for one counting problem.
- R6-L2 fixed: re-read order (deleting branch, then either key present gives ErrExists, both absent retries) matches service.go:424 and loses nothing: today both-absent also gave ErrExists, which was only reachable through a race. A fresh Get of the volume key is needed (readHeader returns NotFound for absent and for Creating/Deleting alike), and the design says so.
- R6-N1 fixed (step 2 now reads before its Txn). Remaining wording gap in R7-N1.

## New findings
### R7-L1 - Low: "exactly two claims" cannot be counted with interleaveKV.txns
design.md TestForgetRacesAPlacement. interleaveKV.txns counts every non-hook write op on `key` (service_test.go:662-666), counted before the Txn result. If key is the volume key, a successful creation counts failed claim (1), second claim (2), and the final header write (3, createVolume's Start/Creating=false txn). So "exactly two" fails or the test is vacuous. Fix: say the claims are counted as TxnRev calls whose cmps hold the volume key at Rev 0 (as TestCreateVolumeRetries' wrapper must anyway, for "32 claims" and "after one claim").

## Checks asked for
- TestCreateVolumeRetries' five cases are buildable: the tombstone check is a Get of tombstoneKey (hook on it fires after the read, before the claim, so the claim's tombstone compare fails and the re-read finds it); a rewriting wrapper is needed since interleaveKV's hook runs once (design says a new test KV); a taken id or a deleting header (no tombstone, since the check precedes the claim) fail the claim at volumeKey Rev 0. Exhaustion costs about 1 s of backoff (service.go:53), fine. The fresh forgotten node must not be a candidate (else Place fails differently); "fresh" covers it.
- Step 2 reads vs compares: reads match for the mark and drain/<node>.

### R7-N1 - Nit
design.md step 2 says it reads the gate with #78 but its Txn text lists only the mark and drain/<node> compares; add "(with #78, the gate)" to the compares as the round-5 fix did elsewhere.
