# Plan review: issue #44, round 5 (incremental), design-code and design-security
Reviewed: git diff 65ba058..6fec02a -- docs/issues/44/ (report.md excluded).

## Round-4 findings
- N1 fixed in substance: design.md now names a per-poll timeout with a clock check, and an ssh keepalive bound.
- N2 needed no change.

## Findings of the revision
- Low: the citation "as `reconcile.sh:57` bounds its reads" is wrong. scripts/checks/reconcile.sh:57 is `read_block() { sudo timeout 300 dd ... }`, a 300 s bound on a dd read. No `flaminio-meta get` in scripts/ is wrapped in `timeout` (grep finds none), and reconcile.sh's `meta()` (line 46) has no timeout. So `timeout 10` on each poll is new, not an existing idiom, and "both already in the scripts" (design.md, decisions.md) is false for it. Fix: drop the citation and "already in the scripts", say the watcher adds `timeout 10`.
- Low: 310.5 s is not derived. 300 + 10 (the last poll's timeout) gives 310; the extra 0.5 is presumably a poll sleep, not stated, and the bound does not count sudo or ssh setup. Fix: state the poll interval, or say "about 310 s".
- Low: the ssh bound is right (ServerAliveInterval=15 x ServerAliveCountMax=8 = 120 s, lib.sh:28-29; ConnectTimeout and ConnectionAttempts only apply at connect). But keepalives fire only on a dead connection, so the host's wait is bounded only because the watcher self-limits at about 310 s. Say so. Also an ssh dropped at 120 s leaves the watcher running on T for up to about 190 s more, while cleanup starts; the design should say the cleanup does not depend on it.
- Low: "an ssh that ends without the second line is a run over the limit" is not in spec S11 (its last bullet is only "not in every ISR within a limit"), and a dropped ssh is an invalid run, not an exceeded limit. The script-test row does cover it (255 before the second line, run invalid, cleanup runs), as does the hung-get row. Fix: add "or the watcher's ssh ends without its second line" to S11, or word the design as "invalid", not "over the limit".

No new safety rule, wire, token or on-disk change; no injected bug owed.

## Nits
- decisions.md R4-N1 repeats the "already in the scripts" claim; fix with the Low above.
