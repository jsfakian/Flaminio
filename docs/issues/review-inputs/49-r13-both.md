# Plan review: issue #49 (Chaos test suite), round 13, design-code and design-security

Scope: `git diff 33d9433..0a75543 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. The diff changes the kill-phase rule (design.md:93-98),
the S3/S4 and S9/S10 test rows (design.md:351, 353) and decisions.md (the round-11 entry's note and
a new round-12 entry). I checked them against the recorder rules (design.md:52-66), the segment end
step (design.md:115-126), the kill helper and `kill-at` (design.md:263-308), `in_flight_at_kill`
(design.md:164-168) and `run_detached` (design.md:323-326). I also checked the ublk device's flags
(dataplane/crates/flaminio-io/src/ublk.rs:907). It sets no `UBLK_F_USER_RECOVERY`, so when the
target dies its in-flight I/O is aborted with errors. A client exiting on a recorder failure
therefore does not hang in `Pwrite`.

Counts: Low 1, Nit 1.

## Earlier findings

- R12-B1: fixed. A recorder failure after `kill-ready` now makes the client exit non-zero at once
  (design.md:96-98). Its `.rc`, present at the kill, fails the run (design.md:122-124), whenever the
  failure falls. Say it falls before `kill-at` fires, and the unrecorded Return leaves a Write that
  looks outstanding. `kill-at` may then fire on it, but the run fails on the `.rc` anyway. Say the
  client exits before the gate sees an outstanding Write: `kill-at` refuses at 10 s instead. A
  Return that could not be recorded therefore never passes as S9's evidence. Both new tests are
  named (design.md:351, 353).
- R12-B2: fixed. The case is now a client that creates `kill-ready` and exits **0** after
  `kill-at`'s SIGKILL and before its unit's SIGKILL (design.md:353). Round 10's rule would let that
  client pass. decisions.md matches it.

### R13-B1 — Low
**The recorder-failure test does not cover the client's waiting state.** In that state a recorder
failure is most likely, and an implementation is most likely to miss it. The rule says a recorder
failure makes the client exit at once (design.md:96-98). It covers both post-`kill-ready` states:
still issuing, and stopped after the kill's first failed operation and waiting. In the second
state the client is still appending the Returns of its other outstanding operations (the kill's
errors). Those `write(2)`s are where a post-kill recorder failure happens. The test is "a recorder
failure after `kill-ready` exits non-zero" (design.md:351; decisions.md, R12-B1). A test that
injects the failure while the client is still issuing would pass an implementation that stops
watching recorder errors once it has entered the wait. Such a client keeps waiting and is
SIGKILLed with no `.rc`, so the failure is silent. That is the gap R12-B1 closed.
**Fix:** design.md:351. Make the case two: a recorder failure (1) while issuing after
`kill-ready`, and (2) on a Return appended after the first failed operation, once issuance has
stopped. Each exits non-zero at once, without waiting for a signal.

### Nit
- design.md:93-98: "never exits on its own, with one exception: at its first failed operation …
  it stops issuing … and waits …; but a recorder failure makes it exit". The colon puts the waiting
  behaviour where the exception should be, so a reader takes the first failed operation as the
  exception. Reword: "never exits on its own except on a recorder failure, which makes it exit
  non-zero at once …; at its first failed operation …, it stops issuing … and waits …".

Reviewed commit: 435e159d8f0d2279ad2e42236ea7d125980032d7
