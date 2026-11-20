# Plan review #17 round 8 (incremental, design-code + design-security)
Diff 30cf82d..7d5d9a0. Reviewed commit: 7d5d9a0.

## Round-7 findings
- R7-B1: fixed. `maintenance_generation_end` exists (design.md:~620-637), covers each way G ends, restores the R5-2 assertion; node test split into one event per row. Residual: L1, L3 below.
- R7-B2: fixed. Two table rows, socket line, test. Residual: L2.
- N1, N2, N3: fixed (design invariant and item 55; `starting | Detached` row and `gone` rows; item 54 lists 30/39/40/45/52/53). The table and the invariant text agree.

## Findings
### L1 - Low: "the report comes first" cannot fail in the engine test
design.md maintenance_generation_end: "the report comes first in the output sequence" and "an engine that ... reports after `Detached` fails it". The report to the `Maintainer` is an in-engine call, and `Detached{G}` is an Output taken after the same locked step. Within one step the order is unobservable, and a test that takes outputs cannot tell it. Also "exactly one report" is hidden by `on_ended` ignoring a repeat (:472). What is observable is the log line count and the `EndMaintenance` calls on the fake metadata client.
Fix: say how the report is seen (a recording sink or the log lines, counted), and state the rule as "reported in the same step as, and not after, the step that emits `Detached{G}`": test that the `Maintainer`'s state (no `StartMaintenance` for 5 min on the fake clock) already holds when `Detached{G}` is taken.

### L2 - Low: spec S5/S12 still say "never ... made to wait", and the 30-s error's CSI code is unnamed
spec.md S5 ("never refused or made to wait for maintenance") and S12 ("not refused") were not amended; only the Tests line mentions the 30-s error. The design now refuses a client attach after 30 s (design.md:227, :244-249). The design says "kubelet retries the stage" but #15 maps attach failures to specific codes, deliberately not `Internal` (#15 design.md:397-402). The new text is not in that map, and no plugin test names it.
Fix: add to S5/S12 "a wait of at most 30 s, then an error with nothing reserved". Name the mapping (`Unavailable`) in the `control/csi` section, with a plugin test that the new line gives it.

### L3 - Low: the R6-B4 hook has no placement rule
design.md "the hold" case: the hook is "called each time the starting thread lets the registry's lock go". It catches reserve and send in two holds only if every release calls it; a later refactor that adds a hold without the call passes. Nor does it say how production code carries the hook (cfg(test), a field).
Fix: name the mechanism (as #15's pause hooks, #15 design.md:753) and put the call in the lock guard's drop path for that function, not at call sites.

### L4 - Low: an `on_ended` repeat is a `Maintainer` rule, listed in the engine test
`maintenance_generation_end` (design.md:~635) asserts "an `on_ended` repeated for a `(K, epoch)` changes nothing". The engine test cannot call `on_ended` (the `Maintainer` is inside the engine), and the rule is a `flaminio-repl` unit property.
Fix: move it to a `maintenance.rs` unit test and the simulator, or drive it through a duplicate `Input::MaintainFailed`.

## Nit
- N1: the 30-s rows (:226-227) do not say which wins when the entry goes `gone` in the same hold as the wait expires. Say: the wait reads the state under the registry's lock; gone gives `ok` (root) or the reserve (client).
