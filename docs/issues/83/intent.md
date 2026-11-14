# Issue #83: A supported lease clock contract — intent

[Issue #83](https://github.com/jsfakian/Flaminio/issues/83), [follow-ups](../../followups.md).
From audit finding D02, an unverified safety assumption, not a reproduced stale-read bug.
Approved 2026-11-13; not implemented.

## Why

1. **One unsupported clock assumption crosses all read paths.** `Engine::us` derives local
   microseconds from `std::time::Instant` and narrows without checking
   (`flaminio-node/src/engine.rs:604`). Engine turns and fast-read completion consume that
   clock (`:713,2227`); copied and zero-copy ublk paths independently pass Instant at admission
   and completion (`flaminio-io/src/ublk.rs:1590–1699`). Linux monotonic time excludes host
   suspend. Long scheduler delay, host suspend and virtual clock stop are different events.
2. **The core already has stronger gates than accepted ADR 0005's old prose.** Send-time lease
   expiry, whole-ISR fencing/reconciliation and completion membership checks must remain
   (`flaminio-repl/src/types.rs:25–50`, `target.rs:650–739`). Proposed audit ADR 0007 records
   the correction and clock limitation; it is not accepted and this issue must not restore
   the historical immediate-read shortcut.
3. **Current simulation proves a narrower environment.** Simulator clocks are affine positive
   rate plus offset (`flaminio-repl/src/sim/mod.rs:290–304`), with generated rates within the
   configured drift bound. They do not model suspend discontinuities or VM snapshot rollback.
4. **BOOTTIME solves one boundary.** Including physical host suspend in elapsed time prevents
   a suspended old target retaining an artificially young lease. It does not establish
   hypervisor clock continuity, detect coordinated rollback or measure a cross-node rate bound.
   Startup probes and local sampling must state those limits rather than claim certification.

## What we want

1. A reviewable initial profile: explicitly provisioned physical Linux hosts with suspend-aware
   elapsed time, bounded relative rate rho=100,000 ppm and no live storage/VM snapshot rollback.
   Unknown or detected virtual deployments fail admission pending a separately qualified profile.
2. One clock source and validity generation for core admission and every local/remote completion
   path. Detectable suspend-offset changes, backwards samples and clock failures quarantine the
   daemon before any further successful admission, without weakening durable fencing.
3. Explicit restart/reattach under a new metadata epoch and ordinary reconciliation after a
   fault. No automatic reopening from an old token, session or cached clock sample.
4. Deterministic runtime and simulator tests that distinguish supported pauses from excluded
   clock stops/rollback, with an invalidation mutation caught. Publish detection limits and a
   read-only clock check that #71's doctor can consume.

## Out of scope

- Claiming arbitrary VM pause, migration, hidden virtualization or snapshot rollback is safe.
- Cross-node clock certification by a short startup test or wall-clock synchronization.
- New fencing authority, a timeout-based substitute for epochs, or changes to lease/takeover
  membership and reconciliation rules.
- General operations CLI (#71), a full chaos scheduler (#49), runtime implementation or any
  shared-lab suspend/VM test without explicit authorization.
