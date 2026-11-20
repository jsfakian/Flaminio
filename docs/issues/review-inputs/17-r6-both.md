# Plan review #17 round 6 (incremental, design-code + design-security)
Diff 5455e49..747b560.

## Earlier findings
- R5-1: fixed. design.md:288-299 words it "no longer a maintenance record (unheld, still naming that node)", a standing record is no failed start, and the test is at design.md:572-580.
- R5-2: partly fixed. Root's end and step 5 use `Input::DetachMaintenance`. S12's client attach still sends a by-volume `Input::Detach`: R6-B1.
- R5-3: partly fixed. The running mark and the ordered waiter are specified. The timeout-while-starting path opens R6-B2 and R6-B3.

### R6-B1 — Low
S12's detach of a maintenance entry is a maintenance detach, but design.md:242-244 and the new bullet (:219-222) keep `Input::Detach`. `Engine::detach` acts by volume alone (engine.rs:1856-1860). The stated safety ("it reserves only after the entry ended, and its attach follows on one channel") holds for one client. Two concurrent client attaches of the volume break it. Both look up the maintenance entry. A reserves, attaches and runs. B's `Input::Detach` was looked up earlier but is delivered later, so it ends A's target by volume. That is the R5-2 hazard with a different sender. The text also says "`Input::Detach` with its generation", but the engine's Detach takes no generation.
Fix: send S12's detach as `Input::DetachMaintenance { volume, generation }` too, since it already holds the generation. Add a test: two client attaches over a maintenance entry leave the first's target running.

### R6-B2 — Low
Step 5's timeout removes the entry in the main thread before the engine has ended anything (design.md:171-180). Two gaps follow.
- A client attach (S12) that finds no entry can reserve and send its `Input::Attach` before the main thread's `Input::DetachMaintenance` is sent. The removal and the send are not one step. The engine answers "already attached" or "already attaching" (engine.rs:640-653), a transient refusal.
- If the engine-side attach reaches `Attached` meanwhile, the order the text lists is `DetachMaintenance` first, then `MaintainFailed`. The S11 end therefore wins: `End::Detached` and an "ended by root" log line. `MaintainFailed` is then ignored, so no 5-minute back-off applies and the volume can be restarted at once. The text says only that "whichever report comes second" is ignored, which was the R5-3 ask to specify the End kind and the log line. The test also says no such line.
Fix: send `MaintainFailed` before `DetachMaintenance`, so `StartFailed` wins and the back-off holds. Say which log line a late `Attached` produces. Or remove the registry entry only after the engine's `Detached`, or hold a place until then. Test the starting-timeout case: a late engine `Attached` after the removal leaves no target, one back-off, no second epoch.

### R6-B3 — Low
Timeout with no entry. The main thread reads the running mark "of the entry" (design.md:171-176), but the router may already have routed `Attached` then `Detached` and removed the entry during the wait. The text does not say what the main thread does when the entry is absent. It could read as a failed start and report `MaintainFailed` for a start that ran and ended. Fix: an absent entry at the timeout means the start ran and ended. Send nothing and report nothing, since S11 did the end. Add it to the time-out test.

### R6-B4 — Low
Root's `maintenance end` can reach the engine before the start's own `Input::Attach` (the entry is reserved, and root looked it up). The new rule leaves "anything else" alone (design.md:219-222), with no `Detached` and no memory. The start's `Input::Attach` then runs and root's end, answered ok, is lost. R3-C2 only covers a client's detach.
Fix: the main thread checks an "ended" flag set by root under the registry's lock before sending `Input::Attach`, or root's end waits for the entry's end and errors on 30 s. Add a test.

Reviewed commit:
ebd8e23c87c2efd8869855d29c3f2e94096ef2f0
