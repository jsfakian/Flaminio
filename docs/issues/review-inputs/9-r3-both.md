# Plan review: issue #9, round 3 (incremental), design-code + design-security

Reviewed commit: 22fca47c1d2d49b18ff4c3f3d376825b8d2d32a5. Diff: a01f2fb..2f5194a (docs/issues/9, no report.md).

## Round-2 findings
- R2-B1 (Medium): fixed in substance (trap, stop on another volume, docs for writes, node-fault check). Two defects remain: R3-A, R3-B.
- R2-B2 (Low): fixed. The shared wait and the post-create `chunks_in_isr` check hold (write_volume loop lab.sh:1881-1899 is movable as is; NewVolume sets ISR = replicas, domain.go:205).
- R2-C1 (Low): fixed (ranking dropped; the figures are consistent).
- R2-C2 (Low): fixed, with a precision gap: R3-C.
- R2-B3 (Low): fixed as a record; #15's plan is not in this checkout, so unverifiable.
- R2-N1..N5: fixed. N3's check is runnable as written; N4 holds: the Makefile (line 287-290) does not set VOL today, so the "make supplies the default" is a Makefile change that must be in the next step's list.

## New findings

### R3-A — Medium: "stops at its attach when another volume is attached" is false when the node restarts
design.md "Letting go"; spec S7 last paragraph and the new lab check ("with vol1 attached on fl-b, make bench-reads stops, naming vol1, which stays attached").
cmd_bench_reads (lab.sh:1509-1515) restarts NODE (`cmd_node up`, which stops then starts the unit: node.sh:35, 44) whenever its flags differ from the run's (default MODE=zero-copy, QUEUES=1; node-fault leaves a default node). That restart, before the attach, drops vol1's device silently; the bench then proceeds and never stops. So the lab check passes only if the node already runs in the bench's mode, the rule "moves no volume not named bench-" is broken by the restart, and the check as written is not reproducible.
Fix: before the restart, ask NODE for `status` (control socket "ok V /dev/ublkbN") and stop, naming V, if V is not the bench's volume; or state that the restart is the exception and make the lab check set up matching flags (and add a second check with differing flags). The first is the one that keeps S7 true.

### R3-B — Low: the trap's scope and clearing are unspecified
design.md "When the bench ends". lab.sh clears every other EXIT trap with `trap - EXIT` after its protected span (lines 843, 1125, 1184, 1468). The design sets the trap but does not say it is cleared. If #15's `bench-ab` (or any later caller) runs cmd_bench_reads inside one lab.sh process, the trap fires at process exit: the bench volume stays attached through the following write phase (which formats NODE's disk), and a later `trap ... EXIT` in that caller replaces it. Also the trap runs an ssh `node.sh detach` on exit even after `lost` (exit 255), so it can wait on a dead VM; state a timeout or skip when the cause was a lost connection.
Fix: say that the bench clears the trap and detaches explicitly on its normal path (`detach; trap - EXIT`), keeping the trap for failures; and add a script-test (or a lab check) that an interrupted bench leaves nothing attached.

### R3-C — Low: the closing condition is not exactly computable from summary.txt
intent "What closes #9"; bench_summary (lab.sh:1595-1625). The medians are printed with `%d` (truncated, e.g. 13 5xx IOPS, 66xx MiB/s), so a ratio of the printed medians is off by up to about 1.5e-4: it cannot decide 0.8999 vs 0.9000. The exact figures are in runs.tsv (per repetition), in the same directory. Also summary.txt records the queue count (the cpus line) but not `POLL_US` or the mode; only the directory name does (`reads-<mode>-q<n>-poll<p>-...`), so "without spinning" is not checkable from summary.txt.
Fix: say the ratio is computed from runs.tsv's per-repetition values (the median of each side), or have bench_summary print the unrounded share and the poll setting; add one of them to S3's list.

### R3-D — Low: untested path of the new rule
The script-test vector for the post-create check covers a create output without NODE. It does not cover the cases it was added for: a create output with some chunks on NODE (partial), and the stop message's fix command (`VOL=… make vol-delete`) being valid (lab.sh `vol delete` takes $VOL; check that VOL from the environment reaches it, as `vol1` is the default in lab.conf).
Fix: add a partial-placement vector and note that the message is a script-test string check.

## Nits
- decisions.md R2-B1 says the bench "keeps S7's rule that it moves no volume not named bench-"; true only after R3-A is fixed.
