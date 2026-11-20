# Issue #75 plan review: round 9 (incremental, design-code and design-security)
Diff 02853e8..597b861.
R8-1 fixed: main.go:77-79 prints "flaminio-meta: <err>" (gRPC "rpc error: code = ..."), exit 1; service.go:533 returns NotFound via errStatus.
R8-2 fixed: META_WAIT default 60, subshell around wait_for matches lib.sh:211-219 (die inside exits the subshell, `|| die` adds VM and undo); stubbing wait_for removes the 5 s sleep.
R8-4 fixed: ids with $$ pass valid_id (lab.sh:1633); StorageClass bullet reindent is fine.
No defects.
Nit: metactl prints errors on stderr (through ssh); the trap and the probe must capture 2>&1 for "output holding code = NotFound" to work. The design says "output" only.
