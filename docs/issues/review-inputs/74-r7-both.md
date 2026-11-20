# Issue #74 plan review round 7 (incremental), design-code and design-security
Diff b1913c0..03b2658 (design.md only).

## Round-6 findings
- R6-1 (Low): fixed. `request_read_after_closing_dropped` (design.md:126-128) fails without the post-recvmsg check: the injected receive sets `closing` then returns a valid `add`, so only the check after the receive stops it. The queued-requests test now pauses the server on a latch, so the pre-read check is what it covers, and it fails without that check. Both agree with outcome 4 (design.md:112-117) and S3.
- R6-2 (Nit): fixed. design.md:281 adds that 0 and 1048575 are accepted; consistent with the 1048576 bound.

## New findings
### 1. Low: the receive seam is named only inside a test description
The "injectable receive" appears only in design.md:126. The Reading bullet (design.md:70-72) and the `serve.rs` test list do not mention it. The seam must keep the ancillary-data, MSG_TRUNC/MSG_CTRUNC and zero-length handling on the real side of the boundary, or `descriptors_in_a_request_are_closed` and `truncated_request_refused` (which use a real socketpair) would stop testing real recvmsg behaviour. Fix: one sentence in Reading: the loop takes `recv` as a parameter (production: the `recvmsg` wrapper that does the ancillary/trunc checks; test: a closure), and the injected closure runs without the table lock held (it needs the lock to set `closing`).

### 2. Nit-level drift, listed because it is misleading
decisions.md:202 (R5-B1) still says `queued_requests_after_child_exit_ignored` covers a request read after `closing` is set. It no longer does; `request_read_after_closing_dropped` does. Fix: update that line, or note in report.md that it was superseded.
