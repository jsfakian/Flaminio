# Issue #86 implementation units

Base: `610f367cfc90d1e6f19e20d2848b8020d8ecb84e` (main after #81 / PR #102).
Implement these units in order. Each gets a fresh implementer; the approved spec, design and
decisions remain the contract. Existing test-call-site updates needed to compile an API change
are part of wiring; new acceptance tests follow implementation.

| Unit | Scope | Design / rules | State |
|---|---|---|---|
| 1. Bounded replica state and lifecycle | `flaminio-repl/src/replica.rs`; `flaminio-node/src/replica/mod.rs`; minimal simulator/caller adaptation for fallible chunk APIs and configured budgets | Arena/coarse/restart coverage, foreign-epoch drop, quarantine, pre-create budget checks and final report rebuild; S1–S4/S8 | implemented; core/reference and node lifecycle acceptance pass |
| 2. Checked Fenced codec | `flaminio-net/src/frame.rs`, `session.rs`; mechanical callers if required | Shared checked encoder/errors, bounded fallible decoder and exact Fenced shape/body checks; S5/S6/S8 | implemented; codec/session/fuzz acceptance pass |
| 3. Captured Hello and target authority | `flaminio-repl/src/msg.rs`, `target.rs`; node engine/replica and simulator Hello adapters | Send-time required chunk set, explicit rows, complete validation before authority, bounded scans and validation mutation; S5/S7/S8 | implemented; all 75 Target tests and local/remote engine adapter tests pass |
| 4. Independent simulator assurance | `flaminio-repl/src/sim/{mod.rs,check.rs}` and necessary checker/fault wiring | Independent exact-version/restart coverage and allocation/work checks, budget/forged-epoch/invalid-reply faults and mutation support; S1–S8 | implemented; all 38 mutation catches and fresh 20,000-seed forged-field campaign pass |

Acceptance also repaired rejected-write replay, pre-snapshot pending coverage and stable
move-source retirement; permanent clean regressions and independent mutation controls pass.
Full `make fmt lint test race hygiene contracts sim` passes after the atomic adapter
preflight review fix; code/security review is clear after two rounds. Local sanitizers pass.
Final conformance passes S1–S9; ADR 0017 is accepted and the report records all gates.
Publish the final documentation and mark PR #103 ready after final CI. Wait for the user's
merge before the next issue.
No shared-lab action is required or authorized.
