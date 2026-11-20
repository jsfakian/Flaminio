# Plan review #17 round 9 (incremental, design-code + design-security)
Diff 24df440..2ad19b9 (checkout HEAD a28a267).

## Earlier findings
- R8-L1: partly fixed. The rule is restated as "same step, not after" and counting is named, but the "turns the engine" check can still pass a late reporter (R9-B4).
- R8-L2: partly fixed. S5/S12, ADR, decisions and design invariant agree; spec Tests and two design lines still promise "no client attach refused" (R9-B2); the mapping's string form is wrong (R9-B3).
- R8-L3: partly fixed. The guard-and-Drop mechanism is named, but as written it deadlocks or recurses, and does not fit #15's `Mutex<Registry>` (R9-B1).
- R8-L4: fixed (maintenance.rs unit tests).
- R8-N1: fixed in text; no test (R9-B5).

### R9-B1 - Medium: the `RegistryGuard` hook, as designed, deadlocks or recurses, and does not fit #15's registry
design.md:182-187, :623-628. The hook runs "in `RegistryGuard`'s `Drop`" and runs "root's lookup", which takes the registry's lock.
- In `Drop::drop` the inner `MutexGuard` is still held (fields drop after), so the lookup's `lock()` blocks on a std (non-reentrant) mutex: deadlock.
- Even if the lock is released first, the lookup's own `RegistryGuard` drops and calls the hook again: unbounded recursion.
- Panic during unwinding (a hook that asserts) aborts under #15's abort panic hook; only in test builds, but it turns a failed assertion into a lost message.
- #15 defines the registry as `Mutex<Registry>` (#15 design.md:61), so `Registry::lock()` is a method of the value inside the mutex. And "private to its module, would not compile outside" holds only if the registry is its own module. #15 and #17 (design.md:419) put the main thread, router and socket code in `node.rs`/`control.rs` with no registry module named; code inside the registry's module can still lock directly.
Fix: name a wrapper type (e.g. `SharedRegistry { inner: Mutex<Registry>, #[cfg(test)] hook }`) in its own module `registry.rs`, a `RegistryGuard { guard: Option<MutexGuard>, .. }` whose `Drop` takes the guard, drops it, then calls the hook, and a thread-local re-entry flag so the hook's lookup does not call the hook. The hook then runs after release, which is what catches "reserve and send in two holds". Say that the `Drop` is empty in release.

### R9-B2 - Medium: the narrowing is not carried to every live promise
- spec.md:196 (Tests): "keeping one end, one place freed, no client attach refused (S12)", followed in the same sentence by "a client attach answered with an error ... when the maintenance target has not ended within 30 s".
- design.md:612: the table test's invariant "no client attach refused for the maintenance entry".
- design.md:82-83 ("none is refused for maintenance") and decisions.md:331 (item for round 4) repeat it; the latter is history, the first two are live.
An implementer writing the invariant check at design.md:612 would fail the 30 s row.
Fix: in spec Tests and design.md:612 say "no client attach refused or made to wait except by the 30-s bound (no `limit` or `is starting` answer, and an answer only after 30 s)"; in design.md:82-83 say "refused by the metadata service".

### R9-B3 - Low: the plugin does not see `error: <volume>: ...`
design.md:134-137 and :552-555. `ControlSocket.command` (control/csi/dataplane.go:83) strips `error: ` and returns `data plane: <rest>`. The plugin's `device` (node.go:70-76) therefore sees `data plane: <volume>: the maintenance target did not end within 30 s`. A test that feeds `error: VOL: ...` to a fake, or a matcher on that prefix, passes or fails for the wrong reason. (#15 has the same wording; not this plan's to fix, but this plan should be right.) Also say that the wait is cut by the attach's own limit (`within`, dataplane.go:107): a caller with less than 30 s left gets #15's attach time-out (`DeadlineExceeded`), not this error.
Fix: name the matched string as `data plane: <volume>: the maintenance target did not end within 30 s` (or a typed error from `command`), build the fake's error with `command`'s own wrapping, and state the interplay with `within`.

### R9-B4 - Low: "the report holds when `Detached{G}` is taken" can still pass a late reporter
design.md:644-650. The test "takes `Detached{G}`, then turns the engine, and the `Maintainer` already acts on it". A variant that reports at the head of the next step passes, as the turn is that step. The test needs an observation that is not a step of the engine, or an injected bug that reports at the end of the next step.
Fix: read the `Maintainer`'s state (back-off, free place) through an accessor straight after taking `Detached{G}`, with no input given; name the late-report variant as the injected bug. Also say how log lines are captured in the test (a recording sink).

### R9-B5 - Low: "gone wins" at the 30 s expiry has no test
design.md:250-251, decisions.md:68. The 30 s case (design.md:632-635) only covers the engine that never answers. A rule that the expiry reads the state under the lock, and gone wins, is not exercised: an implementation that decides on the clock first and errors fails nothing.
Fix: add a case: `Detached{G}` removes the entry in the hold in which the fake clock passes 30 s (use the registry's release hook or a barrier); root is answered `ok`, the client reserves, and the place count is one freed.

Reviewed commit: a28a267d6cf967c4724b6716ecfa107dfd66b076
