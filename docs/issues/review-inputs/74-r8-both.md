# Issue #74 plan review round 8 (incremental), design-code and design-security
Diff 03b2658..f40e9ad (design.md Reading bullet; decisions.md R5-B1).

## Round-7 findings
- R7-1 (Low): fixed. design.md:72-75 says the loop takes its receive as a parameter, production passes recvmsg with the ancillary/truncation/zero-length checks, and the test closure runs without the table lock. Agrees with outcome 4 (design.md:115-118, :129-131) and the table lock rule (design.md:86, never held across a kernel call).
- R7-N1 (Nit): fixed. decisions.md R5-B1 now names request_read_after_closing_dropped, with queued_requests_after_child_exit_ignored for requests queued before.

## New findings
None.
