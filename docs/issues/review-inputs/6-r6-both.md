# Issue #6 plan review, round 6 (design-code, design-security)
Reviewed commit: cfb782ca521e72d210c18bcac0b51c4e7625104b (diff f9974eb..4642aaa)
R5-N1: fixed. design.md and decisions.md now use grep -qE -- '(^| )--privsep( |;|$)'.
Tested on ExecStart-shaped lines: matches "--privsep" mid-list, last before " ;", and before ";"; does not match --privsep=0 or --privsep-x. A "--privsep ;" at the end of argv matches.
Opens nothing. Residual (not a defect): a "--privsep 0" two-argument form would match; moot if #74's flag is boolean, and the design already says to confirm the flag when #74 lands.
No defects.
