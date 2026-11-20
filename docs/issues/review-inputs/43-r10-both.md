# Plan review #43, round 10 (a78216f..13c1e90), design-code + design-security

## Round-9 findings
- R9-1 fixed: TestBlockedTable covers refusal+drift and refusal+later dry-run error.
- R9-2 fixed in principle (512-byte cut, 32768 limit stated, 100 kB row); see L1 for an inconsistency.
- Nit 1 (refusal text) fixed: "the API server's refusal".
- Nit 2 fixed: StorageClass exempt from step 4; TestAdoptionRefused covers it.

## Consistency
- TestBlockedTable count checks out: subsets of {owner, drift, dry-run refusal, dry-run error, apply error}, no error pair, apply error only without a refusal: 15 without apply error + 2 (A, A+D) = 17, plus InvalidInput and failed read = 19. Every row is reachable (needs the owner, refusal and error on three distinct non-StorageClass objects, drift on the DaemonSet, refusal before the erroring dry run). No reachable combination missing.
- Converged order (Blocked, AwaitingRollout, AwaitingNodeUpgrade, True) agrees with S4 and TestStatus; deploy's wait is consistent (a Blocked cluster just polls 300 s, then dies with the messages).
- 6 entries x (512 + prefix) stays under 4 kB, far under 32768.

## Findings
### L1 Low - cut is stated on the text, tested on the entry
Contracts: "A text over 512 bytes is cut there and ends in `...`" (design.md ~l.224). TestBlockedTable: "its entry is 512 bytes ending in `...`". The entry has a `<Kind>/<name>: ` prefix, so it cannot be 512 bytes if the text is. Pick one (say the text, `...` included, is at most 512 bytes) and make the test assert that. Also say the cut is on a rune boundary (a byte cut can leave invalid UTF-8 that the JSON patch turns into U+FFFD, so the status differs from itself on each pass only if the cut varies; keep it deterministic).

### L2 Low - step 4's "leaves the pass" is ambiguous against the table
Step 4 reads "a live object ... is a refusal, and leaves the pass: no dry run". Taken literally the pass ends there, and rows with an owner refusal plus a dry-run refusal/error/drift (8 of the 17) are unreachable; the table assumes only that object skips its dry run. Reword: "is a refusal; that object gets no dry run, the others go on".

## Nits
- Step 7 says "naming the object"; step 9 now lists every refused object. Say "naming each".
- `InvalidInput` entry `spec: <failure>` is not an object, but step 9 and the table say "the message has exactly the objects found"; say "or `spec`".
- Step 9 vs adopted-objects text ("The StorageClass is not written at all") vs step 6/8 (created when absent, TestRepair "deleted is back"): pre-existing, say "never changed or deleted".
