# Plan review: issue #44, round 4 (incremental), design-code and design-security
Reviewed: git diff 093f431..65ba058 -- docs/issues/44/ (report.md excluded).

## Round-3 findings
- L1 fixed. The patterns /^metactl()/,/^}/p, /^data_vms()/,/^}/p and /^valid_id()/p, run read-only on scripts/lab.sh, each give exactly one complete function (valid_id is one line, lab.sh:1633). What they use is available: vssh, vms_of (lib.sh:38,145), vmvar (lib.sh:68), META_PORT (lab.conf:99, sourced by lib.sh:16), LOCAL_STATE_DIR and VM_C (lib). Same idiom as reconcile.sh:15 and resync.sh:12. The S7-S9 test row now runs the extraction against the real lab.sh.
- L2 fixed. Design refusal 3, spec S12 and the S12/S13 test row all say a failing `metactl get V` refuses.
- N1 fixed (docs/lab.md only, no help change). N2 fixed: `VOL=<id> make vol-delete` works: Makefile:261 runs `lab.sh vol delete`, which calls delete_volume "$VOL" (lab.sh:727-729, :1733), VOL coming from lab.conf and the environment; it detaches everywhere, deletes metadata (NotFound tolerated), deletes replicas and waits for the tombstone. It is also the RERUN text lab.sh itself uses. Safe for a run that does not own the id, because the pre-create NotFound check means any volume of that id is the run's. N3 fixed: 300 s from the watcher's first line, test at 299/301 s.

## Findings of the revision
None at Low or above.

## Nits
- N1: the 300 s is counted "by the watcher", but the design does not say what bounds the watcher if one `metactl get` or the ssh hangs (no outer timeout is named). Say the watcher's loop checks its clock each poll and the script has a hard ssh timeout slightly above 300 s.
- N2: refusal 3 now has three outcomes for fl-c (ok none, ok V with no overlap, anything else or failing get refuses); one sentence listing "failing get" as a refusal is already there, so no change is needed beyond keeping the test row's order matching.

No new safety rule, wire, token or on-disk change; no injected bug owed.
