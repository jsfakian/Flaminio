# Plan review: issue #45, round 6 (incremental), design-code and design-security

Reviewed commit: 7f66c24b8429e0122a70fe52783f4034d193b643 (HEAD of /home/jsfakian/Documents/src/flaminio-45; the brief named 4bb2ba9, the diff f902a18..4bb2ba9 was reviewed as given, and HEAD only adds report.md-type changes beyond it).

## Earlier findings
- R5-1: fixed in substance. Writing B0 + T with a sequence compare is idempotent: a late commit of write 1 either lands before write 2 (write 2's compare fails, reread, write 3 puts a larger total) or after write 2 (its own sequence compare fails, it is refused). Only this owner writes while it holds the ownership (the ownership compare refuses a prior term), T is monotone, so the stored credit never decreases. Two edges remain, R6-1 and R6-2.
- R5-2: fixed. 6 s is stated the same in S5, design (two places), the ADR proposal, the settled-item note, the VM-clock paragraph and the round-5 entry; the credited amount is bounded by the 1 s cap per tick and at most six ticks with the sighting at most 5 s old. A test is named.

## New findings

### R6-1 - Low
**B0's read is not tied to the ownership commit, and a read before it lowers the credit.** design.md:139-140: "on taking the ownership, the owner reads the clock's credit". If the read could precede the ownership transaction (or the same transaction's snapshot), the previous owner's last write in between raises the stored credit above B0; the new owner's first write is refused on sequence, the reread takes the sequence only, and the next write stores B0 + T below the stored credit: the credit decreases, a failure is delayed (never an early failure, so no safety effect). Also unstated: T resets to zero and B0 is reread at every new ownership revision (the "owner that loses and regains" case, design.md:275), and what happens when the B0 read fails (the owner must credit nothing and not write until it has it).
**Fix:** say B0 is read after the ownership transaction commits (the ownership compare then fences every older write), T starts at zero with each ownership revision, and no credit or write precedes a successful B0 read. Test: an old owner's write in flight at takeover is refused, and the new owner's stored credit is never lower than the old owner's last committed value; a regained ownership rereads B0.

### R6-2 - Low
**Tests: two of the three named do not fail without the rule.** design.md:342: "a lost reply after the write committed, and one without a commit" were handled by round 4's rule too and pass either way; only "times out, reread sees the old value, then commits late" separates the rules. Fine as regressions, but the "new owner's B0 misses credit" case (a previous owner's committed credit read as B0, then added to) has no named test beyond "restart/owner ABA".
**Fix:** name a test that a new owner's first write is exactly the previous stored credit plus its own T, and that the late-commit test asserts the stored value after the late commit is older and then replaced.

### R6-3 - Low (drift)
**"rereads the sequence only" versus deciding timeout Failed "against the clock as reread".** design.md:144-148 says the reread is of the sequence only, then the pass decides against the clock as reread. The decision needs the stored credit and revision, so the reread is of the whole clock. It may hold an older total than B0 + T (a late commit pending), which only delays a failure; say so, and that the decision never uses the in-memory B0 + T.

### Nit
- design.md:121 explains the 6 s as "one tick" to see the rise; a tick up to 2 s late still credits (capped at 1 s), so the sighting can lag by up to 2 s. The credited amount is still at most 6 s because of the cap; the explanation is slightly off, not the bound.
