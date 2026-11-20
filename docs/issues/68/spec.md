# Issue #68: Instant restore — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Protocol/model groundwork only, as the existing intent scopes it. No restore command,
store decoder, fetch service or CSI support is delivered. The issue stays open through the
runtime and lab stages. The ADR is Proposed; nothing records human acceptance.
Settled at approval, 2026-11-13: the ADR is accepted with the plan (see decisions.md, Settled at
approval).

## Terms

- **Base:** immutable, authenticated backup identity, volume incarnation and geometry.
- **Remote block:** a 4 KiB logical block whose content still comes from the base.
- **Local block:** client data, fetched base data, or an explicit zero, durably represented at
  replicas. An unallocated ordinary volume block remains zero; it is never a remote block.
- **Hydrating volume:** opt-in restore with a pinned base and some remote blocks.
- **Hydration:** materializing verified base data, without changing the logical register value.
- **Completion:** epoch-fenced transition to an ordinary volume after no remote blocks remain.

## The invariant

The logical register starts with the base's content. Successful reads and writes are per-block
linearizable against that initial value. Missing local data never implies zeros. A successful
write is durable on every current ISR member. A crash, move or repair cannot turn an
acknowledged client write into base content. Hydration changes representation, not content.

## Behaviour

**S1. Explicit origin.** Only an explicitly requested instant restore is hydrating. Its base,
geometry and incarnation are immutable. The ordinary #54 incomplete restore remains exclusive
and unattachable by applications. The two modes cannot be switched mid-restore.

**S2. Missing is a state.** Every read path distinguishes remote, local data and explicit zero.
Uncertain-state reconciliation, digests, mapped extents, resync and moves preserve that
meaning. Replicas acknowledge state changes only after durable commit; restart preserves it.

**S3. Reads.** A remote read obtains authenticated base data or returns an explicit unavailable
or corrupt-base result. It never succeeds with fabricated zeros or unchecked bytes. Local
reads need no repository operation. Fast reads must prove the whole extent local, or fall back.

**S4. Writes.** A write covering a whole logical block can replace it without fetching. A
partial-block update merges the base where necessary before committing. Larger physical
allocation units do not zero neighboring remote blocks. An explicit zero is local content.

**S5. Races.** Hydration cannot overwrite a client write, including after session replacement,
failover, restart and an ISR change. Old completions have no authority in a new epoch. Ordinary
write acknowledgement and minimum-ISR rules still apply.

**S6. Recovery and copying.** Reconciliation chooses and installs both content and presence
state before readiness. A resync or move does not need the repository to transfer remote
references and local data. Joining the ISR requires agreement on both. Detached volumes may
wait for their next attach to hydrate; already-local blocks remain usable without the bucket.

**S7. Availability.** Repository failure does not block local I/O or metadata lifecycle work.
Only operations needing remote bytes fail or wait according to the ADR's explicit policy.
A missing or corrupt object is distinguishable from temporary unavailability. Failure names
the base and logical range without exposing credentials or plaintext.

**S8. Completion.** Only the current target may complete, with current membership evidence
that every block is local and durable on every ISR member. Membership races invalidate that
evidence. Once completed, attach, failover, reads, writes and resync need no repository access.
Repeated completion is harmless. A crash before completion may leave a fully local volume
marked hydrating; it must never leave remote data marked complete.

**S9. Authority.** Fetch authority is scoped to the pinned base, read-only, revocable and
unavailable to applications or arbitrary replicas. Hydration cannot issue arbitrary bucket
requests. A base remains retained until completion or deletion releases the dependency.
Snapshots, backups and shipping are refused while hydrating in this step's protocol.

**S10. Executable model.** The simulator models a nonzero base, presence state, delayed fetches,
repository loss/corruption, concurrent writes, crash/restart, failover, resync and moves. Each
new safety rule has a deliberate mutation which its check catches. Ordinary-volume behavior
and existing lease/fencing checks remain covered.

**S11. Honest delivery.** Protocol and architecture distinguish modeled rules from runtime
support. Store/format/wire, worker, command/CSI and measured pod-start/hydration checks remain
required later stages before #68 can close.

## Tests

- S1–S3, S6: typed-state unit tests and simulator reads with nonzero base; restart, digest,
  repair and resync of mixed local/remote extents; fast-read admission model.
- S4–S5: single-block and partial update histories, delayed hydration after newer writes and
  epochs, allocation-unit boundary cases; mutations must demonstrate each violation.
- S7–S9: missing, corrupt and unavailable repository; detached failover; stale completion and
  membership change; no repository event after completion, and lifecycle work during outage.
- S10–S11: reproducible seeded chaos, explicit rule/mutation mapping and documentation check.
  Runtime, fuzz/crash-format and lab tests are later stages, not evidence supplied here.

## Open questions

The [proposed ADR](adr-proposal.md) recommends target-coordinated hydration with a separate
bounded fetch worker and explicit transient I/O failure. Approval of that availability tradeoff,
the new durable presence format and the credential distribution protocol is outstanding.
The repository and snapshot contracts of #52/#54/#51 must be settled before runtime design.

Settled at approval, 2026-11-13: target-coordinated hydration with a separate bounded read-only
worker; explicit typed I/O failure on outage; explicit durable Remote presence, its concrete format
designed later (see decisions.md, Settled at approval).

Settled 2026-11-13 (user's instruction): the worker takes repository credentials through #52's
worker input, the runtime design fixing their delivery and revocation; #52's, #54's and #51's
repository and snapshot contracts as their own approved plans settle them (see decisions.md).
