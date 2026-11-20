# Issue #6 plan review, round 5 (design-code, design-security)
Reviewed commit: f9974eb (HEAD; diff 8fb5882..2da98d5, docs/issues/6 minus report.md)
R4-N1: fixed. Design says `--value` and `grep -qw -- --privsep`; decisions.md records it.
`grep -qw -- --privsep` works on `systemctl show -p ExecStart --value` output (the `{ path=... ; argv[]=... ; ...}` line; confirm.sh:78 and reconcile.sh:114 use the same form). `--` ends grep options; the match is preceded by a space.
Nit (no action needed now): -w treats `-` as a non-word char, so `--privsep-x` or `--privsep=0` would also match; fine if #74's flag is a plain boolean. Confirm when #74 lands (the design already says so).
No defects.
