# Issue #86 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Budget and cover policy** ([spec](spec.md), header, "policy constants remain Proposed";
  [ADR proposal](adr-proposal.md), Decision proposed and Consequences; [design](design.md), Lab,
  docs, risks and ADRs): the Proposed policy. B = 65,536 slots per volume, per-chunk coarsening to
  a maximum-Version cover, the foreign-epoch drop, quarantine of unsupported held sets, and one
  `encode_checked` shared with #80. Its stated costs are accepted: routine coarsening makes each
  chunk written at N+R=B reconcile whole at the next takeover, and an empty row carries only
  today's trust until #13/#61 land. The [ADR proposal](adr-proposal.md) is accepted with the plan.
- **Idle residue** ([spec](spec.md), Open questions, first item; [design](design.md), Risks):
  it stays. Pruning idle chunks at the source is not added; it would be a new replica rule with
  its own proof.
- **Quarantined earlier incarnation blocks its name** ([spec](spec.md), Open questions, second
  item; [design](design.md), Risks, "Proposed trade"): the proposed trade. The name stays blocked
  on that node until the replica delete or an operator removes it; no automatic erase.
- **Same-epoch prune and Settle bounds** ([spec](spec.md), Open questions, third item, and S3;
  [design](design.md), Prune and restart proof and Risks; Choices below, R3-S1): not checked by
  the replica, as today; a bound of the session's own epoch stays its holder's claim. The
  residual risk that a faulty target of the fenced epoch clears records early is accepted; the
  existing `early_settle` and `prune_reconcile` mutations keep checking the honest target.
- **B is per volume** ([spec](spec.md), Open questions, fourth item): B bounds each volume, not
  aggregate memory or CPU across volumes; an aggregate limit is #75's work.

## Choices

- **R1-C1, R1-S2 — coarsen per chunk, not per volume.** At N+R=B only the written chunk is
  coarsened (for a new chunk, the lowest-numbered chunk holding records). A chunk whose cover
  clears is precise again: a cleared cover represents no write. The volume-wide sticky mode is
  dropped, because a sequential fill of a 128 GiB volume at the defaults reaches it and it then
  makes every later takeover reconcile each written chunk whole.
- **R1-C1 — the idle residue stays.** Pruning idle chunks at the source would be a new replica
  rule with its own simulator check and mutation; it is not proposed here.
- **R1-C2, R1-S1 — quarantine, specified.** The mark lives in `ReplicaService`, outside `vols`
  and the core. A Hello is refused before the earlier-incarnation delete, whatever its first
  epoch, so a quarantined earlier incarnation blocks its name on that node. The existing replica
  delete still works and ends the state. Chunks and durable fences are untouched.
- **R1-C2 — no chunk-count check in `Engine::start_target` here.** #80's design owns that
  validation; a second copy would need its own test and adds no defence the replica's refusal
  and the Fenced bounds lack.
- **R1-C2, R1-C7 — the budget is a constructor parameter** (`ReplicaCore::with_budget`,
  `ReplicaService::with_budget`, `Config.uncertainty_budget`), with an explicit `Default` of
  65,536, so node and simulator tests run at 4, 8 or 16. The three constants are tied by a
  compile-time assertion in `flaminio-net`.
- **R1-C3, R1-S3 — an empty row is today's missing row.** Create-on-Hello is unchanged; no
  `NotHeld` or bootstrap refusal exists in this tree. #13/#61 define how theirs meets the
  required-row rule when they land. The refusal half of the S7 test is not written here.
- **R1-C4 — the Hello message carries its chunk set** (`Request::Hello { chunks }`), computed
  by the Target. Engine and simulator use that copy; no accessor.
- **R1-C5, R1-S6 — whichever of #80 and #86 lands first adds `encode_checked`,** `EncodeError`,
  `SessionError::Encode` and the `Conn::send` switch, with the one signature both plans state;
  the other adds its frame's preflight. #86 is expected first. #80's text needs no edit.
- **R1-C6 — `restart()` returns every chunk to restart cover with no record or coarse cover,**
  as a new process does; an add coarsens before it inserts, so the new chunk starts empty.
- **R1-S4, R2-S3 — foreign-epoch Writes and Settles are dropped: a safety rule.** A prune or
  Settle bound of a higher epoch clears records and restart cover, and a version of a lower
  epoch is pruned by any bound of the session's, so without the drop a report can under-cover.
  (Round 1
  recorded this as an input check that only prevents over-cover; that was wrong.) It has its
  unit cases, a simulator fault that forges the epoch (`Config.forge_epoch_pct`, default 0) and
  the mutation `accept_foreign_epoch`, which the coverage shadow must catch.
- **R1-S5 — a reply to Hello must be Fenced and valid** before the session opens or the lease
  extends; Stale still deposes first.
- **Nits — one mutation name,** `bypass_uncertainty_budget`; `tag (u8, 64)`, `body 13+8N+8T`,
  `ADR 0007`.
- **R2-S1 — the over-budget Hello check runs after the earlier-incarnation delete,** and
  before `vol_id`, `core.handle`, any create and the fence: an earlier incarnation's chunks do
  not count against a new volume of the same name.
- **R2-S2 — `create_moved_chunk` asks the core before the store create,** with its own node
  test: a store holding one chunk more than the core would be quarantined at the next restart.
- **R2 Nits — the node test helper `write()` takes the session's epoch;** numbers and units are
  spaced throughout the folder.
- **R3-S1 — no rule for a bound of the session's own epoch.** The drop closes what a forged
  epoch does (restart cover of the fenced epoch, a lower version, a maximum above every later
  bound). A same-epoch prune or Settle stays its holder's claim, as today: the replica cannot
  check it, and this plan does not try.
- **R3-C1, R3-C4 — the forged-epoch fault runs only during chaos** and draws from the rng only
  above 0. The clean campaign `forged_epochs_leave_runs_clean` and the mutation's settings
  `forged` (20%) are named.
- **R3-C2 — node tests use 6-chunk volumes** (`accepted_in`) and assert each refusal by its
  text. The budget refusal names the service's budget; the quarantine reason is `too many
  chunks`.
- **R3-C3, R3-S2, R3 Nit — the over-budget refusal leaves the named volume unchanged,** after
  an earlier incarnation is discarded as today. Three cases pin the position of the check.

No finding was judged not to apply.
