# Plan review: issue #44, round 6 (incremental), design-code and design-security
Reviewed: git diff 6fec02a..f2655aa -- docs/issues/44/ (report.md excluded).

## Round-5 findings
- R5-L1 fixed: reconcile.sh:57 is `sudo timeout 300 dd ...`; the design now cites it only for that; grep finds no `timeout` on any flaminio-meta/metactl call under scripts/.
- R5-L2 fixed: "about 310 s" with 300 s + one 0.5 s sleep + one poll of at most 10 s. Worst case checks: last poll that starts under 300 s ends at about 299.9, sleep 0.5, poll 10, clock read: about 310.4 s. Consistent.
- R5-L3 fixed: lib.sh:28-29 confirmed (15 x 8 = 120 s); design says keepalives only end a dead connection, host wait is bounded by the watcher's limit, cleanup neither waits for nor depends on it; the watcher only reads. Script-test row says "without waiting for the watcher".
- R5-L4 fixed: S11 last bullet now names the lost-watch case; design calls it invalid (S11); the S12/S13 row matches (255 before second line: invalid, cleanup runs).
- R5-N1 fixed: decisions.md rewritten to match.

## Findings of the revision
None at Low or worse. S11's bullet, design wording ("fails the run as invalid") and the script-test row name the same case the same way. A dropped ssh leaves at most a read-only poller on T for under about 190 s; it cannot touch the next run's state, and a `get` on a deleted volume only fails and is polled again.

## Nits
- The S12/S13 row could also name the "watcher exits non-zero at its own limit" case as S11's lost-watch/not-full bullet; it is covered by "still exits at its limit", so optional.
No new safety rule, wire, token or on-disk change; no injected bug owed.
