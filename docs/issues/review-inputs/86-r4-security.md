# Issue #86 plan review — round 4 (incremental), security focus

Scope: `git diff 3d89853..3f063b8 -- docs/issues/86/ ':!docs/issues/86/report.md'` in
`/tmp/flaminio-plans-2026-11-12/86`, checked against the code of that worktree. HEAD (f685343)
adds only `report.md` on top of 3f063b8.

## Earlier findings

- R3-S1: fixed. Design "Prune and restart proof", spec S3, the ADR and decisions.md now state
  what the drop closes (restart cover set in the fenced epoch, a lower version, a maximum above
  the session's bounds) and that a bound of the session's own epoch is the holder's unchecked
  claim, as today. It is a line under Risks and an open question in the spec. The ADR sentence
  that was false for records is gone.
- R3-S2: fixed. Cases (a), (b), (c) of `hello_over_budget_is_refused_before_create` are exact
  (budget 4, 6-chunk volume). Each fails for a misplaced check: (b) and (c) with the check
  before the earlier-incarnation delete, (a) with it after the create.
- R3 Nit (spec S1): fixed. S1 says the refusal comes after an earlier incarnation is discarded
  and changes nothing of the volume the Hello names; the quarantine deletes nothing.
- R3-C1: fixed. The fault is applied in `Sim::send` only while `self.chaos`, draws only above 0,
  is 0 in `Config::random`, covers the target's own replica; `forged_epochs_leave_runs_clean`
  is named.
- R3-C2: fixed. `accepted_in(volume, count, epoch, session, chunks)` on 6-chunk volumes; every
  refusal is asserted by its text, so `out of range` cannot pass for the budget refusal.
- R3-C3: fixed, with the R3 Nit and R3-S2.
- R3-C4: fixed. The `mutation!` call, the settings function `forged` and its type are named.

## What the changed lines claim, checked

- Restart cover clears only on `e < p.epoch` (`flaminio-repl/src/replica.rs:235,251`), and it is
  set to the fenced epoch (`:130,142`): no bound of that epoch clears it, a forged epoch + 1
  does. `Version` orders by epoch first (`types.rs:19-23`): a lower-epoch version is below any
  bound of the session's epoch. The session's epoch equals the fenced epoch whenever a request
  is current (`replica.rs:171-185`).
- The same-epoch statement matches the code: push then `retain(w >= p)` (`:232-234`), Settle
  `retain(w >= below)` (`:250`). The sender is the token-checked target of the fenced session.
  `early_settle` and `prune_reconcile` exist and are the target-side mutations of an early
  claim (`target.rs:75,79,1350,2245,2267`).
- The budget in the refusal text: `hello()` and `create_moved_chunk` return `Result<_, String>`
  (`flaminio-node/src/replica/mod.rs:268-273`), so the text can carry B. It reaches the peer
  only as a `Refused` reason, cut at 1024 bytes (`flaminio-net/src/frame.rs:337`), to a peer
  that already passed the token check; B is a build constant. The text holds integers only.
  `Reply::Close(String)` (`mod.rs:58,350`) carries `create chunk 4: ... more than 4 chunks`,
  so the moved-chunk test can assert it; chunk 4 is in range on a 6-chunk volume (`:277`).
- `too many chunks` fits `&'static str`; no other text in the folder still quotes the old
  reason.
- Simulator: `Sim::send` is at `sim/mod.rs:648`; `local_read_fail_pct` is guarded by
  `pct > 0 && self.chaos` (`:749-757`); `Configure` is `fn(u64) -> Config` (`sim/tests.rs:213`)
  and the four-argument `mutation!` arm takes it (`:386-392`); a Settle is tracked in flight
  (`target.rs:2498-2506`) and times out like a Write (`:811-827`).

No finding, no Nit.

Reviewed commit: f685343f892f2a1f123ec985821a019422f9a640
