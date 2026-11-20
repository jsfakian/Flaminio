# Issue #90: Bind replica membership to store identity — specification

[Intent](intent.md) · [decisions](decisions.md) · [design](design.md).
**Unapproved #90 draft; not implemented.** Historical #61 approvals apply only to the source
snapshot at `5696573a126c0abe26347d3c9700e557489613cf`, not this moved/revised plan.

## Terms

- **Store identity:** nonzero disk UUID from the opened valid superblock, renewed by format.
- **Store lifetime:** service-owned monotonic generation; UUID changes/enrollment advance it,
  address/label changes and restart preserve it. Retention prevents node-forget and UUID ABA.
- **ReplicaRef:** node and exact disk UUID; #90 owns the shared type, one opened store/node.
- **Observed store:** authenticated node's currently registered identity/lifetime.
- **Earned replica:** membership bound to the store that durably synchronized/acknowledged it.
- **Replacement:** same node name, different store UUID; never an ordinary dirty-set rejoin.
- **Complete frontier:** all current data/holes/live writes plus every committed snapshot stage
  and committed/pinned root/receipt required by the volume's active profiles.
- **Grant:** complete bounded metadata authorization for one immutable Hello session/role set;
  it has a current linearization point, and never replaces later epoch/session/lease fencing.

## The invariant

Only the exact store that earned a replica may count as that member for read/repair/resync
source, lease or write acknowledgement. Losing data/fences on a new UUID cannot restore an old
target's authority or make an empty store in sync. Existing epoch/ISR/minimum rules decide
availability: no surviving authoritative store means unavailable, never zeros or force promotion.

## Behaviour

**S1. Record identity.** Data nodes report their actual opened store before replica service.
Service owns lifetime generation; agents/other roles cannot overwrite it. Placement captures
exact refs under current admission checks. Registration never rebinds earned membership.
Compute-only/witness nodes do not invent stores. UUID change invalidates old-store capacity/
alive reports atomically; delayed old reports cannot make the replacement eligible.

**S2. Exact session identity.** Remote Hello, synthetic local Hello, replies, queued operations
and fast reads check exact expected/actual store, lifetime, incarnation, epoch and session at
admission and completion. Identity checks precede mutation; old replies/acks/lease credit cannot
transfer to a new ref. Cancellation/supersession/restart prevents delayed callbacks publishing
an obsolete session while preserving actual device-I/O buffer/pin lifetime.

**S3. Authorized durable bootstrap.** Every new Hello obtains complete current metadata grant
for token target/rights, first_epoch, epoch, exact ref and each chunk/role/operation before
cleanup/fence/create. MEMBER never recreates a missing chunk. Only explicit initial placement
or a destination role creates a nonauthoritative empty chunk. INITIAL is consumable per exact
chunk/ref/operation: after durable creation/fence, current metadata completion consumes it;
new Hello must acquire earned membership before credit. No client I/O until all initial copies
of that chunk are earned. Consumed creation authority never reappears from a retained create
operation, even on missing-chunk restart. Persist checked fence before
Fenced/readiness. Immutable session grants reject ungranted BeginResync; later moves/roles
require new Hello. Metadata outage blocks new bootstrap; accepted ordinary I/O/heartbeats
add neither metadata calls nor locks. Bounded pages/queues/timeouts never confer authority.

**S4. Treat loss honestly.** Observed UUID change leaves old memberships named and unavailable.
Current target shrinks ISR only where existing minimum permits; otherwise waits. Lost refs
cannot be read/repair/resync sources or credited leases/acks. Membership/replacement mutations
are current-target/current-epoch/expected-version CAS, never driven by liveness observation alone.

**S5. Replacement admission.** Explicit replacement starts only after old ref is excluded by
committed ordinary ISR removal, with authoritative surviving ISR, current destination lifetime,
no conflicting operation and capacity admission preserving old-copy charges. Reconstruct complete
frontier before binding/join: dirty-only or current-only reconstruction is insufficient. A new
cut/root pin or takeover invalidates stale completion evidence; join rechecks current frontier
under the normal metadata mutation/CAS rules. Unsupported active profiles refuse before any
allocation, not through a permissive fallback. Same-host stores never count as independent hosts.

**S6. Lost source and fencing.** All earned ISR stores lost means refuse reads/writes/repair/
resync until original authority returns or a separately designed administrator recovery exists.
Never promote out-of-ISR/blank data automatically. Old targets, stores, delayed callbacks and
delete/recreate cannot bypass UUID/lifetime/first_epoch/epoch/session fences. Restart conservatively
re-verifies incomplete reconstruction and grants; no volatile completion becomes durable truth.

**S7. Format, cleanup and contracts.** Explicit format/claim makes a new UUID and reports loss.
Exact ref/lifetime possession and authorized durable deletion are required for cleanup; a new
same-node disk cannot confirm an unplugged old disk's release/deletion. Retain old ref/fence/
tombstone charges and bounded references until exact cleanup. Publish one composed wire/profile,
size and transaction budget; mismatch refuses without silent legacy projection trust. Status
shows observed/earned mismatch, quarantines and blocked chunks without tokens/grants.

**S8. Legacy activation.** Stop old binaries/targets/provisioning under explicit maintenance.
Administrator-reviewed evidence enrolls only known unchanged original stores, including fences,
all assignments and cleanup/source references; unknown provenance converts to explicit quarantine.
A revision/hash/evidence-checked correction can rebind or quarantine an already processed item
and restarts full verification; READY is immutable. Proven zero-first-epoch ordinary/snapshot
volumes use explicit signed legacy incarnation identity while preserving data/fences/history;
bootstrap never deletes their preserved chunks or guesses a positive epoch. Composed strict
local target/CSI admission binds that same LID in the exact device tuple and durable normal
intent, with current-process FLCH and fresh Attach epoch for both full and paged paths; it
never permits legacy fresh-filesystem initialization.
Bounded durable inventory/binding/verification cursors and conditional operation identity make
interruptions/lost replies safe. Activation is atomic only after full conversion or quarantine,
complete verification and installed active-profile adapters; no identity-unaware writer or node-only
fallback resumes. Valid proven legacy geometry has a bounded coherent paged path where needed.
UUID alone is not evidence of historical durability or resistance to rollback/cloning.

**S9. Executable proof and scope.** Shared sans-I/O bootstrap/replacement transitions run in the
simulator and production adapters. Every new safety rule has an independent checker, caught
named mutant and bounded clean counterpart; include current/snapshot/root completeness, authority,
callbacks and no-source refusal. Exercise actual remote/local adapters, both metadata backends
and transport size boundaries. Production qualification obeys #83 physical admission; pre-#83
VM exploration is distinct. No new cloud policy, failure classifier or #45 stage is introduced.

## Tests

- **S1/S7:** memory+real etcd registration/placement/capacity/cleanup races, wrong UUID/lifetime,
  A→B→A, label CAS, forget/recreate, delayed reports, exact release/deletion and budget+1 fixtures.
- **S2/S3:** actual remote TLS and local Hello, fast-read admission/completion, old token before
  mutation, fence durability, forged/mixed grant cursors, changes across pages, compaction/restart,
  cancellation/late callbacks, valid-token flood with bounded reserved control progress.
- **S4–S6:** independent simulator checks/mutants in core-design.md plus real whole-range/hole/
  live-write/history/root reconstruction, old store return, min=RF, all ISR lost, join/takeover races.
- **S8/S9:** both-backend migration restart/phase/CAS/auth/activation and mixed-version refusal; initial pending consumption, durable-before-earned and consumed replay
  mutants; zero-first-epoch bytes/history preservation and verification-failure correction;
  known-original and ambiguous quarantine cases. Later separately authorized physical RF2 proof;
  no production VM, cloud or disk action is claimed here.

## Open questions

None deferred as missing mechanism. The concrete appendices and issue-local ADR are proposed
for fresh #90 review; historical approvals and a clear review alone do not accept these revisions.
