# #13 plan review, round 8 (incremental), design-code and design-security
Reviewed: git diff 37808a0..e566ab1 (docs/issues/13/), against meta/service/service.go, service_test.go.

## Round-7 findings
- R7-L1 fixed. claimCounter (TxnRev calls with the volume key at Rev 0 in the compares and a put of it) counts exactly the claims. Checked against service.go: the only such call is createVolume's claim (:415). Not counted: deleteChunks (:481; Txn, chunk-key deletes only; compares at :508 and :561 are on the volume key at 0 but put nothing of it), DeleteVolume's mark (:542, rev e.Rev) and final txn (:561, delete, rev nonzero), the chunk batches and last header write (compare at the claim revision). The claim reaches the wrapper because createVolume calls s.kv.TxnRev; Txn calls on a Mem-backed wrapper do not (Mem.Txn calls its own TxnRev), but no claim uses Txn. interleaveKV over claimCounter works (interleaveKV.TxnRev calls k.KV.TxnRev). Counts per case: 2 claims (race test), 32 (retries = 32, service.go:45), 1 (taken id; tombstone written by hook), 0 (tombstone present), 0 claims in the finish-deletion case beyond the one failed claim.
- R7-N1 fixed: step 2 compares now list the gate with #78.

## New findings
None above Nit.

### Nit
- design.md step 2 (line ~163-167): the added "(with #78, the gate)" sits after "(0 if absent)", and the reflow leaves "`drain/<node>` is" alone on a line; "its (with #78, the gate)" reads as if the gate's revision is compared as 0-if-absent, which is the intent, but "and, with #78, the gate's" would read better.
