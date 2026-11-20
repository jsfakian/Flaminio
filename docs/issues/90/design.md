# Issue #90: Store-bound replica membership — design

[Issue](https://github.com/jsfakian/Flaminio/issues/90) · [spec](spec.md) ·
[decisions](decisions.md) · [Proposed ADR](adr-proposal.md). **Unapproved draft; not implemented.**
Moved from #61 at `5696573a126c0abe26347d3c9700e557489613cf`; its historical approvals are
provenance, not approval of #90 or of these corrections.

The facts it starts from:
- The valid superblock already has a random `disk_uuid` at offset40; `Store::superblock`
  exposes it (`dataplane/crates/flaminio-store/src/store.rs:698`). Engine registration omits
  it (`flaminio-node/src/engine.rs:584`); `nodeRecord` contains address/labels only
  (`meta/service/service.go:154`). A reusable node name is not durable replica identity.
- `ReplicaService::hello` deletes earlier-incarnation chunks before its fence and creates every
  missing requested chunk (`flaminio-node/src/replica/mod.rs:190–258`). `BeginResync` also
  creates absent chunks (`flaminio-repl/src/replica.rs:256–261`). Both must use the same grant.
- Current `ReplicaCore::handle` immediately fences Hello (`replica.rs:173–184`). Simulator
  setup precreates chunks and crash preserves stores (`sim/mod.rs:395,1107`). Neither proves
  the authorization/durability/replacement transitions specified here.
- Network accepts and reply channels are unbounded (`flaminio-node/src/net.rs:176–212`);
  metaclient has one unbounded serial FIFO (`flaminio-metaclient/src/lib.rs:110–120`).
- #13 adds retained fences/releases; #51 adds all-stage catch-up, #58 committed/pinned roots,
  #72 capacity accounting and #83 physical clock admission. Their plans are dependencies,
  not implemented facts. #90 owns ReplicaRef; #72 currently defines no shared identity ABI.

## Identity, registration and admission (S1, S4, S7)

Use the exact [metadata/cleanup/capacity contracts](contracts.md) and
[wire contract](wire-contracts.md). Read UUID from the opened, validated store, never config.
Only authenticated own-node IO reports a store; agents may merge labels without supplying
service-owned generations. UUID change updates observed identity/lifetime, invalidates old
capacity/alive reports and advances placement admission atomically. It never rewrites earned
memberships. The lifetime generation survives node forgetting/recreation and A→B→A changes;
label/address-only changes and process restart preserve it. Missing/corrupt identity fails shut.

Create and move capture exact destination ref/lifetime under the #76/#78 claim/gate protocol.
A lost ref stays placed and unavailable; current target may remove it from ISR only where the
existing minimum permits. A node address cache cannot substitute a new ref in old pending
writes, source selection, lease confirmations or fast-read handles. Same-node old/new refs are
separate operations but never independent host redundancy. Status shows observed/earned refs,
store generation, quarantine and blocked chunks without exposing tokens/grants.

## Bootstrap and reconstruction (S2–S6)

[Core design](core-design.md) is normative for the sans-I/O transition/event/action interfaces,
immutable session roles, paged metadata authorization, cancellation and bounded scheduling.
Authorization precedes prior-incarnation cleanup, allocation and fence changes. Successful
Fenced requires the checked fence durable. A MEMBER grant never creates a missing chunk.
Only authorized INITIAL or exact MOVE/REPLACEMENT_DEST operations create an empty destination.
INITIAL consumes per-ref durable pending authority through CompleteInitialReplicas after actual
create/fence durability, then reopens as MEMBER; all initial refs must be earned before client I/O.
A destination stays ineligible as an earned source until its independent reconstruction/join.

Every request checks its immutable granted chunk/ref/role/operation. A session opened before a
move cannot process its BeginResync: the target closes it and sends a new Hello/session after
the move CAS, authorizing the complete new set. No metadata call or lock enters ordinary data
I/O or heartbeats. A metadata outage blocks new sessions and membership work as existing rules
require; accepted sessions continue under ordinary epoch/session/lease fencing.

`ReplaceReplicaStore` uses the exact contract and #72 ledger admission: oldRef already removed
from effective ISR by committed ordinary shrink, unchanged current target/epoch/chunk version,
current observed newRef/lifetime, no conflicting move/release, and at least one surviving earned
ISR source. It records an operation-bound move to newRef and retains oldRef charges/cleanup.
No source or min=RF blockage is converted to automatic promotion.

Replacement reconstructs **all** current chunk blocks, explicit holes/zero masks and live
writes. When snapshot profile exists, run #51 Catchup stages0…current in increasing order:
sealed h reads Before(h+1), each stage's copies/discards durable before advancing; no current
live forwarding until every lower stage is complete. A cut immediately suppresses forwarding
past the frontier and invalidates pending join evidence. A below-top historical mismatch blocks
join; do not erase/recreate history. With root profile, transfer and durably verify every
committed **and pinned** root and its receipts using #58's root/barrier transitions, including
roots retained by ongoing ships/readers. Current-only reconstruction is never sufficient.

A pure `CompletionFrontier` binds first_epoch, epoch, newRef, replacement operation, chunk
version, snapshot catalog/header revision, current generation and root-set/barrier digest.
Cut/root/membership updates share the existing target metadata mutation slot. Before proposing
join, recheck against the latest view; `ChangeIsr` compares header/chunk versions plus operation
and replica gate. A conflict restarts frontier verification, never joins on an old completion.
Takeover/restart discards volatile completion and re-verifies every required stage/root under a
fresh session/epoch. Exact oldRef drops to #13 cleanup only after newRef's committed join.
The implementation that lands second with #51/#58 adds these calls, fields and vectors; their
absence rejects the requested feature profile before starting replacement, not after allocation.

## Maintenance migration (S8)

[Migration](migration.md) defines administrator-only begin/inventory/enroll/verify/activate RPCs,
durable operation/cursors, per-batch CAS limits, correction of converted items, quarantine and
restart behavior. Proven first_epoch0 ordinary/snapshot volumes use explicit signed legacy LID
profiles with preserved data/fences/views and no DeleteEarlierIncarnation; no automatic
provenance inference or mixed-version fallback is permitted. The historical administrator-
evidence policy is retained; these concrete APIs and revised protocols are proposed here.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen`, Rust metaclient `src/{pb,lib}.rs`:
  refs/profiles, bootstrap/enrollment RPCs, explicit roles and bounded fair control calls.
- `meta/domain`, `meta/service/{service,auth}.go`, `meta/kv/{kv,mem,etcd}.go`:
  exact refs/lifetimes/gates, replacement/capacity/cleanup, pinned paging and migration.
- `dataplane/crates/flaminio-repl/src/{types,msg,meta,replica,target}.rs`, `src/sim/`:
  authorization/fence callbacks, roles, replacement frontier, checker and caught mutations.
- `flaminio-node/src/{engine,net,replica/mod}.rs`, `flaminio-net/src/{frame,session}.rs`:
  accepted remote and synthetic local Hello, bound callbacks/channels and actual store identity.
- `flaminio-io/src/{node,ublk,control}.rs`, disktool and agent registration:
  UUID startup, exact fast-read/completion identity, diagnostics and compatibility refusal.
- #13 release/tombstone sweep and CLI, #72 reports/ledger, #51 catch-up and #58 root adapters:
  second-to-land integrations listed in the appendices; use no duplicate identity authority.

## Contracts

Normative appendices: [core/bootstrap](core-design.md), [migration](migration.md),
[metadata/cleanup/capacity](contracts.md), [wire](wire-contracts.md),
[response and transaction budgets](budgets.md), and [strict local target/CSI identity](local-target-contract.md).
Generate Go protobuf and Rust client together
with `make proto`; `make contracts` includes shared golden vectors and max-bound fixtures.
Existing UUID and #13 fence records suffice; this issue adds no disk format. #51/#58 retain
ownership of their versioned history/root format. Positive-incarnation FLCTv2 fencing stays
intact; explicit proven FE0 profiles use the bounded
FLCTv3/LID extension in wire-contracts.md and preserve existing durable fences. Whoever lands
second with #15/#79/#83 composes exact LID device/intent identity and normal current-process
FLCH for full and paged Attach using local-target-contract.md; legacy initialization stays refused.

## Tests

Every new recovery safety rule has an independent simulator assertion, named caught mutant and
clean bounded scenario in [core-design.md](core-design.md). Both accepted remote and synthetic
local Hello adapters execute that same core. The appendices name registration/auth/migration/
cleanup/capacity tests for **both** memory and real etcd backends and Go/Rust wire fixtures.
Future checks: `make lint test race hygiene contracts sim`; crash-test/sanitize/miri for changed
store/callback/pin lifetimes. These are implementation acceptance tests; none ran for this draft.

## Lab and documentation

A separately authorized, **pre-#83 exploratory** VM campaign may reformat only fl-b's explicitly
owned test disk at RF2/min1, under verified writes and detached attach, then verify each member's
bytes independently. It is destructive and provides no production qualification. With #83
active, the shared VMs refuse storage admission and this campaign cannot run. Production proof
requires separately authorized qualified physical fixtures admitted by physical-boottime-v1,
same RF2 workload and exact disk ownership; it is unavailable in the current VM lab. Never
invent physical attestations or a bypass. Save topology/claims, stop load, delete only owned
volumes through exact cleanup, restore configuration and verify baseline after authorized work.

Implementation updates protocol §§1/3/6/7/9, architecture §10.1, README, lab guide/follow-ups,
Make help and CLI to actual behavior and evidence. #61 remains cloud policy and #45 recovery
composition; #90 does not add cloud claims, failure classification or autoscaling stages.

## Risks and ADRs

The [issue-local ADR](adr-proposal.md) is Proposed. Historical #61 acceptance does not accept
these revised bytes/APIs. Accepted ADR0001/0002/0004/0005/0006 are consistent: existing fencing,
all-ISR durability, edge minimum and physical-first scope are preserved. New-session metadata
availability and maintenance downtime are costs; no below-minimum/force-recovery risk is
accepted. Same-UUID rollback/clones, malicious device provenance and cloud claiming stay out
of scope. Source proof #52 uses this single service-owned store lifetime; it is not the grant
cursor or volume gate generation.

## Parallelizable?

Go and Rust adapters can be built against the fixed appendices. Core authorization/durability/
frontier/checker integration is one safety unit. Last-landing #13/#51/#52/#58/#72/#76–#78/#80/
#81/#83 hooks apply only when that feature is present; no future mechanism is made a prerequisite
for ordinary identity-only operation. Activation requires the implemented active profiles and
current binary inventory, not completion of unrelated future issues.
