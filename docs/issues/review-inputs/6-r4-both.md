# Issue #6 plan review, round 4 (incremental), design-code and design-security
Reviewed commit: 8fb5882d860273d6a49c6858fbabcfab75a49e8a (diff 8fde090..d0a31f5)

R3-B1: fixed. Settle, 5 s re-read retry, and the failure message with the last blockdev error are in design (Lab check) and spec S1.
R3-N1: fixed. S6 Docs bullet names both rewordings.

Checks asked:
- Unit name: `flaminio-node` is what scripts use (guest/node.sh:15, confirm.sh:27). `systemctl show -p ExecStart --value flaminio-node | grep -o -- '--device ...'` is already used in confirm.sh:78 and reconcile.sh:114, so a grep for `--privsep` in that output is sound. No defect.
- Left-over `PRIVSEP=0`: current spec.md and design.md are clean. Remaining hits are historical (decisions.md:38 round-1 entry; report.md:64 round-1 entry), and decisions.md:138 says it is dropped. Fine.

Findings: none above Nit.

Nit: design says "reads --privsep" without saying to match the whole word (grep -qw -- '--privsep'); trivial, and #74 is unbuilt so the flag name is an assumption to confirm when #74 lands.
