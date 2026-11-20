# Plan review #17 round 5 (incremental, design-code + design-security)
Diff 958436b..517c8d6.

## Round-4 findings
- R4-C1: fixed in intent (router marks running at Attached under the lock), but see R5-3 (timeout and waiter order).
- R4-C2: fixed in intent, but the rule as worded now misfires: R5-1.
- R4-C3: fixed for the refusal; the claim "every Input::Detach names an entry its sender holds" is false for S12 and root's end: R5-2.
- R4-C4: fixed. S17 reads the same in spec.md:128, design.md:235-240 and :278. decisions.md items 37 and 82 are stale but are history.
- R4-N1, R4-N2: fixed.

## R5-1 Low: the failed-start rule, read literally, fires on a start that has not happened yet
design.md:262-270. Rule: at the last read the record was the node's maintenance record with a move pending, "and now it names that node, not held, and the move is still pending". After the ask counts as a read (R4-C2), the next step 5 s later, before the node's scan has taken StartMaintenance, finds exactly that: the record still names the node, not held, move pending. Literally that is a "failed start", so the drain asks another candidate and may depose a start just begun (AttachMaintenance has no expected epoch). The intent is surely "the record is no longer a maintenance record" (the end clears the name, design.md:179-180; "a maintenance attach stands" bullet at :255-257 says no ask), but nothing says so. Before R4-C2 the flag was rarely true at that point, so the ambiguity was harmless.
Fix: word it "the record is no longer a maintenance record (ended), a move still pending". Add a test: ask applied, next step finds the record still standing at the same epoch (node not yet started): nothing asked, with the flag true from the ask.

## R5-2 Low: Input::Detach acting "by volume alone" can still end a client's target for senders that do not hold the entry
decisions.md item 47 / design.md:174-176 claim every remaining Input::Detach names an entry its sender holds, so no engine generation check is needed. Not so: the client attach's S12 detach and root's `maintenance end` (design.md:222-224, :225) send a Detach for an entry they looked up, not one they hold. Engine::detach acts by volume alone (engine.rs:1856-1860). Root's end races with the entry ending on its own (no work): a client then reserves and attaches, and root's late Detach (by volume) ends the client's target: an I/O outage until the node plugin reattaches. S12's own client is safe only through FIFO on the one engine channel; root's is not.
Fix: make the maintenance Detach act only on a Kind::Maintenance entry (Kind is on TargetRt/Attaching), or check the generation, for these two senders; test: root's `maintenance end` racing a client attach leaves the client's target. Or correct the claim and keep a test of the race.

## R5-3 Low: "one thread ends it" is not shown for the timeout and for waiter ordering
design.md:196-200.
- If the router has marked the entry running and the main thread's 30 s wait then times out (or it races a failed mark), step 5 sends Input::Detach to an engine target that has reached Attached: that is an S11 end ("ended by root" line, on_ended), plus MaintainFailed. The "no S11 end" rule at :202-205 only covers an attach that never reached Attached. The repeat is ignored by on_ended, but the log line and the End kind (which one wins) are unspecified, and the test says no such line.
- "The main thread sees Detached first on the waiter" assumes an ordered queue. If the waiter keeps only the last message, an Attached then Detached pair (the router ended the running entry) looks like "Detached before Attached", a spurious failed start.
Fix: state that the waiter is an ordered queue, that the main thread classifies by the entry's running mark read under the registry's lock (running at timeout: not a failed start, leave the end to the router), and that the removal is by generation and idempotent, freeing the place only when it removes. Test: timeout concurrent with Attached.

## Nits
None.
