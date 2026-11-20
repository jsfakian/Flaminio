# Plan review: issue #45, round 7 (incremental), design-code and design-security

Reviewed commit: 08d955078809187532239e07084688a20e230c84 (HEAD of /home/jsfakian/Documents/src/flaminio-45; the brief named 60737cb, the diff 7f66c24..60737cb was reviewed as given).

## Earlier findings
- R6-1: fixed. design.md:140-147 reads B0 after the ownership commit, resets T per ownership revision, credits and writes nothing before B0 is read; three tests named (design.md:349). Residual: R7-B1.
- R6-2: fixed. The new-owner test and the late-commit assertion are named. The fixer's claim checks out: round 5's rule (7f66c24 design.md:139-141) already had B0 + T with "reads on taking the ownership", so the new-owner test does not separate round 4 from round 5; it fails only on an implementation that reads B0 before the ownership commits or does not reset T, as the text says. The late-commit test is the one separating the rules.
- R6-3: fixed. design.md:148-152: whole-clock reread, decision against the stored value, never the in-memory B0 + T.
- R6-N1: fixed in wording; one residual, R7-B2 (Nit).

## New findings

### R7-B1 - Low
**"Ownership revision" is ambiguous: acquisition or each renewal.** design.md:97 says `rebuild-observer` is a 5 s TTL key; design.md:144-145 resets T and rereads B0 "at each ownership revision". If the key is renewed by a PUT (a new mod revision each time), T resets and B0 is rewritten on every renewal: credit between the last write and the reset is dropped (delays only, never early), but "no credit or write before the B0 read succeeds" would then stall the clock after every renewal. If renewal is a lease keepalive, no problem. The text does not say which, and the test "a regained ownership rereads B0" does not cover a renewal.
**Fix:** say the reset and B0 read happen on acquisition (a CAS that creates or replaces the key), not on a renewal; add a test that a renewal changes neither B0 nor T. If a renewal does change the revision, say the owner keeps B0 and T across a renewal by the same nonce.

### R7-B2 - Nit
**"At most six ticks" assumes 1 s spacing.** design.md:121-123, decisions.md:424-428, spec S5, ADR proposal: the bound holds because each tick's credit is its monotonic interval capped at 1 s, so total credit in the window (sighting tick's interval plus 5 s) is at most 6 s; ticks closer than 1 s (after a late tick) could be more than six, each crediting less. The bound is right; "six ticks of 1 s each" is the wrong reason for it.
**Fix:** "the sighting tick's interval (at most 1 s) plus the 5 s after the sighting".

Reviewed commit: 08d955078809187532239e07084688a20e230c84
