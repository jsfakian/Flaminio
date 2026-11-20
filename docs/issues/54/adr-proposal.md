# Accepted with the plan, 2026-11-13: a restored volume is unavailable until verified and its writer is closed

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
  Changed after approval in plan review rounds 1 to 5 (decisions.md).
- **Date:** 2026-11-12
- **Issue:** [#54](https://github.com/jsfakian/Flaminio/issues/54)

## Context

The existing [intent](intent.md) scopes unencrypted new-volume restore into another cluster.
The current Header/Attach/CSI paths cannot distinguish an incomplete image and can format or
expose it. #52 supplies an unimplemented exact-version repository contract; #53 qualifies locked
history. Source cluster/epochs cannot become destination authority. This draft preserves that
scope; broader recovery remains unimplemented and the full issue stays open.

## Decisions proposed

- Reuse #52 cluster-origin/FirstEpoch/archive identities for provenance. The home UUID is #52's
  random `cluster` record, created when absent, never caller-supplied, and an archive of the same
  cluster is refused (R1-C3, R1-S3); it grants no token/disk/recovery authority.
  A restored volume gets destination placement/incarnation and empty snapshot history.
- Claim restore job/state in the FIRST Creating header. Only an exact same-job retry resumes;
  ordinary users/admin/CSI/raw-control attach paths refuse until Ready. Expose progress without
  keys/tokens; retain job identity across deletion/recreation to stop stale writers.
- Use a purpose-bound restore ticket plus destination target epoch/normal durable replication.
  The ticket is the restore target's only capability: replicas accept it as a target token,
  ordinary attach refuses it, and the restore's own start checks the restore phase, so it attaches
  nothing in Finalizing or after Ready (R2-S1). An unprivileged reader under its own system user,
  holding no Flaminio pair, reads/validates unencrypted archive bytes; a
  root helper with the destination's administrator pair writes them only through the specialized
  local target (R1-C1, R1-S1). A fresh writer takes a fresh epoch and replays/reverifies; no
  direct store bypass.
- Finalizing freezes membership, closes/drains restore writes, reads/verifies each chunk on
  EVERY current ISR member and persists generation-bound proofs in bounded batches. Detach
  writer/device before Ready CAS; the closure Ready needs is the end of the attachment through
  which that finalization's proofs committed, so no later attachment of the generation can be
  live (R3-C3); stale tickets/completions cannot write after publication. A restore device
  is never kept across an io restart (no ublk user recovery), so no held restore write is
  issued again (R4-C2). A restore therefore runs only on a direct-mode node in this increment:
  #74's helper `add` takes no flag from the child, and under `--privsep` with #48 it asks for
  both recovery flags and keeps the device, so restore-start refuses under `--privsep`, and, for one
  rule, also without #48, until #74 and #48 add an unrecoverable form (R5-C2, R6 Nit, R7 Nit).
- Preserve logical sparse zeros with nonzero 4 KiB writes into a fresh volume; destination physical
  cluster allocation may differ. Exact page-map/hole equivalence is not inferred from #52's 4 MiB
  map. This is an explicit qualification against the intent's “unwritten” wording, not hidden.
  Settled at approval, 2026-11-13: this qualification (see decisions.md, Settled at approval).
- Cancel invalidates writer/completion authority in DeleteVolume's mark, then deletes as any
  volume; a lost writer is fenced by #30's first epoch, not by a closure proof (R1-C9, R1-S5).
  Timeout or liveness loss is not exclusion. No force wipe/revision bump/old-disk authority
  selection ships.

## Options

- **Create then mark incomplete:** transient ordinary attach/mkfs can expose partial data;
  rejected as the proposed direction.
- **Clear mark when bytes submitted/one replica verifies:** ignores durable ISR completion,
  stale callbacks and late restore writes; violates the intended publication boundary.
- **First-header gate plus frozen all-ISR proof/closed writer:** adds job/proof lifecycle and
  may block on unavailable members, but establishes a checkable monotonic completion boundary.
  Settled at approval, 2026-11-13: this option (see decisions.md, Settled at approval).

## Consequences and open choices

Read #52's cluster record; agree new RPC/ticket/tag contracts, writer/proof freeze and
sparse-layout interpretation before implementation. Missing/incompatible metadata/node/CSI gates
fail closed; coordinated rollout is required. Lost space/objects or unreachable writer/member
leaves a diagnosable incomplete/blocked job, not a partial Ready volume. No accepted risk/approval
or runtime verification is recorded.
Settled at approval, 2026-11-13: origin, contracts, writer and proof freeze and sparse layout as proposed (see decisions.md, Settled at approval).

#50 key/escrow/encrypted recovery, #52 schedules/GC, #53 full ransomware runbook, #58 root-swap
shipping and #68 instant restore are not implemented by this increment. Proposed audit ADR0008
requires proven revision/exclusion/membership/incarnation/key evidence for later metadata recovery;
a fixed bump, maximum sequence, arbitrary-disk majority or new UUID is insufficient. Preserve
surviving disks. Audit ADR0008/9/11 remain Proposed; accepted fencing/ISR/etcd/language rules hold.

## Confirmation

[Design](design.md) and [contracts](contracts.md) map S1–S11 to atomic claim/admission/auth,
restore crash/replay, all-ISR proof/membership/closure and caught early-Ready/stale-writer model
mutations. Later isolated second-cluster lab verifies source-offline exact restore and refusals.
No implementation, runtime test, lab result or architectural acceptance is claimed by this draft.
Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).
