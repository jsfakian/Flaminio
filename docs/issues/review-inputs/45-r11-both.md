# Plan review: issue #45, round 11 (incremental), design-code and design-security
Reviewed commit: c9e3472821c19428f26c07f3690837af9ea85f91 (HEAD; diff db80c68..6209c2b, design.md and decisions.md; c9e3472 only adds the report).

## Round-10 findings
- R10-L1 (Low): fixed. Test is now an answer at the 1 s context limit on a fake clock; send time is taken before Renew's Get; ends send+4 = answer+3.
- R10-L2 (Low): fixed. The bound is checked under clockMu before every credit, every use of B0/T and every write/ask; the acquisition no longer starts it; ownership is held from the first successful renewal after the commit. Tests added (tick between send+4 and next renewal; first renewal fails). B0 read, "credits nothing before B0" and the takeover tests agree with the new start. The "from the acquisition's commit on no older write can commit" argument still rests on the ownership compare, not on the renewal, so it is unaffected.
- R10-L3 (Low): fixed. ErrLost drops the registration under e.mu only if lease and revision are unchanged; test added in the kvtest row.

## Findings

### L1 - Low: the delay of the first credit is not stated
design.md:136-142 and :201: until the first renewal succeeds the instance reads no B0 and credits nothing. The text says "sent at once" but does not say what happens to the time between the acquisition and that renewal's success (the acquisition's tick, plus one more tick per failed renewal), nor whether the rise/sighting baseline for those ticks is still updated. If the baseline is not advanced, the first credited tick sees a long delta (and the "tick more than 2 s late adds zero" rule may swallow it or, worse, credit it); if it is, the lost time only delays a failure, as for a crash gap. Fix: state that the ticks before the ownership is held credit zero and still advance the sighting baseline (so no later tick credits their span), that the delay is the renewal round trip (at most the 1 s context) plus one tick per failed renewal, and that it only delays a failure. Add this to the "first renewal fails" test (after it succeeds, credit starts from that tick only).

### Nits
None.
