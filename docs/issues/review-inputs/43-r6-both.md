# Plan review #43, round 6 (3b96e60..b8ace90), design-code + design-security

## Round-5 findings
- R5-1 (deadline outcome): fixed by removal. A request that fails or times out is an API error; the test now covers "does not answer". Gaps: R6-1, R6-2.
- R5-2 (ByObject): fixed. The reconciler holds only an uncached reader and a writer; ByObject is no longer relied on. No remaining mention of ByObject or of a cache limit. R6-4 is a small gap.
- R5-3 (pass time in bounds): fixed in form. Bounds are in passes, derived once in "The period", and S4, S6, the tests and the lab quote it. No stray figure in seconds contradicts it (grep of spec, design, adr-proposal, decisions, intent). Gap: R6-3.

## New findings

### R6-1 Low - "the client's timeout (10 s)" is stated as a fact, but nothing sets it
design.md "The period". client-go's rest.Config.Timeout defaults to none, and controller-runtime does not set one. With no pass deadline, a request to a hung API server blocks the pass forever, and S6 and S4 hold no longer. The design names no place that sets 10 s, and no test makes a request not answer: TestMidwayFailureResumes says "does not answer within the timeout" but the timeout is not a reconciler field (the period fields were dropped with the deadline), so the test would wait the real 10 s or cannot be written.
Fix: say the operator sets the timeout on the rest.Config it builds the reader and writer from (and note a Config.Timeout also bounds the watch's http client, so the resource's watch restarts at that interval: harmless, but state it or give the watch its own client). Make the timeout an injectable value for the test.

### R6-2 Low - "The status is the last request of every pass" contradicts "no request" and "writes nothing"
design.md l.63 and the S1-S3 row (`InvalidInput`, no request) versus l.94. An InvalidInput pass must write the status, so it makes a request. S8 / TestSecondPassWritesNothing ("only reads and dry runs on a second pass") fails if the status is patched on every pass, even with an equal body, because the counting transport sees a PATCH.
Fix: say the status is written only when it differs from the one read (and that this is the one write S8 allows), and word InvalidInput as "no request but the status". Also if the status cannot be written (the API does not answer), the next pass rewrites it: say so, since "ApplyFailed is in the status" in the test assumes the API answers the status patch.

### R6-3 Low - the lab's 60 s poll has no margin and omits leader changes
spec.md l.200, design "The period". 15 s (running pass) + 30 s + 15 s = 60 s is exact, the poll is 60 s, and the pass is allowed more than 15 s (up to 6 objects, 2-3 requests each, 10 s each). A restart or a loss of the lease adds up to the 15 s lease duration before any pass starts, which S6's wording ("the first pass that starts after the change") does not cover and the lab check's DaemonSet kill in check 4 or a rollout of the operator can hit. The design admits it is an allowance, so this is only a margin point.
Fix: poll 90 s, or say the allowance includes no leader change.

### R6-4 Low - the guard against a cached read is a test only
design.md "Triggers". "Holds no cached client" is true of the reconciler's fields, but the controller-runtime builder (`For(&FlaminioCluster{})`) still builds a cached client in the manager; nothing forbids a later change from using `mgr.GetClient()`. TestRunsAsAccount catches it, which is enough; just say the reconciler's constructor takes `client.Reader` from `GetAPIReader()` and a writer built from a non-caching client (`client.New`), not `mgr.GetClient()`, which is cached by default.

### Nit
- decisions.md l.51 says "read by name every 30 s"; it is 30 s after a pass ends.
