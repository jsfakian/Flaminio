# Plan review: issue #45, round 5 (incremental), design-code and design-security

## Earlier findings
- R4-1: fixed. The S5 change is stated plainly and flagged for the user in decisions.md (settled item note, the VM-clock paragraph, the round-4 entry), spec S5, design and ADR proposal, with the same bound (5 s of each outage). See R5-2 for a small off-by-one-tick in that bound.
- R4-2: fixed. Revision and rise start unset, and are unset after a failed read, a late tick or an owner change; tests for a new owner and after a late tick.
- R4-3: fixed. Only Active data nodes at the last pass count; test named.
- R4-4: fixed. Read order `nodes/`, `alive/`, `node-state/`, `drain/` stated, so the `drain/` hook runs after the state read; `TestForgottenRefusedWithoutFailed` is consistent with design.md:214-217.
- R4-5: fixed. `ListRevs` with etcd `WithKeysOnly` does carry `ModRevision` (the response KeyValue omits only the value; `Rev: kv.ModRevision` in `meta/kv/etcd.go:79` would work unchanged). `clockMu` is held for in-memory updates only, never across a call: sound.
- R4-6: partly fixed. See R5-1.
- R4-N1: fixed.

## New findings

### R5-1 — Medium
**The lost-reply rule double counts credit when the lost write commits after the reread.**
design.md "Persisting the credit" (~:136-148): after an unknown outcome the pass rereads; if the value is not exactly the intended one (seq s+1, P+A, nonce), "A stays, and the next write goes against the reread sequence". But the design itself says a write can commit after its context ends. Sequence: write 1 (s+1, P+A) times out; the reread sees s (not yet applied) so A stays; ticks add B, so A2 = A+B; write 2 is built from the reread (s, P) as (s+1, P+A2); write 1 then applies, so write 2 fails its sequence compare; the reread finds (s+1, P+A, this nonce). It differs from write 2's intended value (P+A+B), so A "stays" although P+A already holds it. The next write stores P+A+A+B: A counted twice. Credit inflates, so a node fails early. It costs copies only, by the invariant, but this is the exact failure the rule is meant to prevent, and no test covers it (the tests: a lost reply after commit; a lost reply without a commit).
**Fix:** make the write idempotent. Keep a monotone per-owner total T of credit added since taking ownership, and the base B0 read when ownership was taken; always write B0 + T (snapshot of T). A late commit then only stores an older value, no subtraction is needed, and a reread equal to the owner's own nonce tells nothing more. Or keep every unconfirmed intended value and accept any of them on a reread. Add a test: a write that times out, reread sees the old value, then it applies late: persisted credit equals the true credit once.

### R5-2 — Low
**"At most 5 s of each outage" is up to 6 s.** The rise time is when the owner saw the rise (design.md:~108-113), up to one tick (1 s) after the heartbeat committed. An outage that begins right after a commit is credited until 5 s after the sighting, so up to 6 s after the last heartbeat. spec S5 ("the time from the last heartbeat seen before it"), the ADR ("committed within 5 s") and decisions.md say 5 s or "committed within 5 s".
**Fix:** state the bound as 5 s plus one tick (up to 6 s), in S5's gloss, the design and the ADR/decisions, or time the window from the rise with the tick's read time of the previous tick; test the bound as stated.

Reviewed commit: f902a187cc5529a524909b536b9ed2fbf45d391c
