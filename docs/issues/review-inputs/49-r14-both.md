# Plan review: issue #49 (Chaos test suite), round 14, design-code and design-security

Scope: `git diff 435e159..70f1816 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. The diff rewords the kill-phase rule (design.md:93-99),
extends the S3/S4 test row (design.md:352) and adds the round-13 entry to decisions.md. I checked
them against the surrounding kill-phase text (design.md:81-99), the `--max-bytes` budget rule
(design.md:105-108), the segment end step (design.md:115-126) and the S9/S10 row (design.md:354).

Counts: none.

## Earlier findings

- R13-B1: fixed. The recorder-failure case in `recorder_test.go` is now two cases, each exiting
  non-zero at once without waiting for a signal: one while issuing after `kill-ready`, and one on a
  Return appended after the first failed operation, once issuance has stopped (design.md:352). That
  covers the waiting state, where post-kill Returns are appended. decisions.md matches it.
- Nit: fixed. The rule now reads "never exits on its own except on a recorder failure, ... whether
  it is still issuing or already waiting", then the waiting behaviour (design.md:93-99). The
  exception is no longer confused with the first failed operation.

The revision broke nothing. The reworded rule still matches the stop-client step's "non-zero when
its recorder failed" (design.md:119-120), the `.rc`-present-at-kill rule (design.md:122-124) and
the script test's recorder-failure fake client (design.md:354). The other waiting state, after
1,000 bursts with no failure, has no outstanding operations and no appends, so it needs no third
case.

No findings.

Reviewed commit: 70f1816880c0eea87a369630f0a0593172fd9da4
