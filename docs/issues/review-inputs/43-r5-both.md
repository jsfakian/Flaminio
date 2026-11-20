# Plan review #43, round 5 (ff76f39..3b96e60), design-code + design-security

## Round-4 findings
- R4-C1: fixed. Every outcome requeues after 30 s, no error returned, period and deadline injectable; TestRepair covers "after ApplyFailed". Gaps: R5-1, R5-3.
- R4-C2: named (GetAPIReader, ByObject) but the ByObject claim is wrong: R5-2.
- R4-C3: fixed (S4 sentence, 60 s polls). Number is off: R5-3.
- R4-C4: fixed (docs sentence says detached and data plane removed; gate recorded as follow-up).
- Nit (events group ""): fixed.

## New findings

### R5-1 Low - A pass cut by its deadline has no stated outcome, status or test
design.md "The period": the 10 s deadline is new; "an API error" ends in a requeue, but no rule says what a deadline hit is (reason in the status, whether the status patch still runs on the expired context, whether a half apply is "ApplyFailed"). The resume of a half apply is covered by TestMidwayFailureResumes (a failed third apply), but no test makes a pass exceed the deadline (slow/blocked API call) and checks: the cut pass leaves applied objects, the next pass completes, status is written, and the next pass is 30 s later.
Fix: say a deadline hit is an `ApplyFailed`/API-error outcome with the status written on a fresh short context; add a test with an injected delay beyond the deadline (period and deadline are already fields).

### R5-2 Low - `cache.Options.ByObject` does not limit the cache to one kind
design.md Triggers and decisions.md say the cache "is limited to FlaminioCluster by ByObject". In controller-runtime ByObject only configures per-type options; any other type read through the manager's cached client still starts an informer (list+watch, refused by RBAC). The real guard is only "never the cached client" in code discipline, plus TestRunsAsAccount failing afterwards. (Names are recorded as from memory; this is a semantics point, not only a name.)
Fix: state the mechanism that enforces it: build the pass's client for managed kinds with `client.Options.Cache.DisableFor` (or a direct client / only APIReader, with the cached client handed only to the controller for the resource); keep ByObject only as a narrowing of the resource's informer if wanted.

### R5-3 Low - "Within a minute", "up to 30 s" and the 60 s polls ignore the pass time
The period is the pass time (up to 10 s) plus 30 s, so the status lags up to ~40 s (spec S4 says 30 s) and a repair is up to 10+30+10 = 50 s after the deletion (S6's minute, TestRepair "two periods" = 60 s with real periods; fine only with ~10 s of margin). The lab 60 s poll for a status that needs a pass to see a change has the same ~50 s worst case, and a node-upgrade rollout change adds a further pass if the pass read before it. Margins are thin, not wrong.
Fix: say "30 s after a pass ends, a pass is at most 10 s", state S4's lag as "about 40 s", and poll 90 s in lab checks (or keep 60 and say worst case 50).

### R5-4 Nit
S9's limit sentence (spec.md ~l.89) agrees with design and S23. design.md line ~100 "from memory" note covers events group; fine. Nothing else to report.
