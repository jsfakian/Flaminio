# Issue #58: Requested snapshot shipping and promotion — design

[spec](spec.md) · [decisions](decisions.md) · [historical ADR proposal](adr-proposal.md)

Historical approval:2026-11-13. Review revisions below are **unapproved**, not implemented.
This is the first requested-shipment increment; no periodic RPO, failback, groups or full issue
closure. Exact proposed contracts are [metadata](contracts.md), [lifecycle](lifecycle.md),
[root format](root-format.md) and [wire/reader/manifest](wire-contracts.md). They resolve the
former future-ABI gate now; active source/format/dependency documents remain unchanged.

## Facts and chosen structure

Current meta.proto has no pairing/standby lifecycle. Header.ID is a JSON string, so extra flags
alone are ignored by older decoders. DeleteVolume explicitly removes unparseable headers
(service.go, deletion); a marker cannot promise protection from unsupported legacy deletion.
Current store applies top-layer records1–8 to one chain; no inactive root exists. Snapshot #51,
reader #52, tenancy #73, restore #54 and device-less #17 are plans, not implemented features.

Choose destination inactive COW roots, immutable after Prepare. The metadata current-target
barrier reuses #51's trust boundary; no unbounded vector of ISR comparisons. Foreign peer TLS
and roles remain outside local administration/replica trust. All new safety rules live in
flaminio-repl and have named model checks plus caught injected bugs below.

## Lifecycle and commit (S1–S9)

[Lifecycle](lifecycle.md) fixes pair/relationship/incoming/outgoing/promotion records and complete
calls from both administrative pairings through resumed ship and promoted attach. Source operation
binds explicit destination descriptor; CreateStandby binds exact source/pair/incarnation/geometry.
#76 creation installs a DR-marked claim and system owner before ordinary visibility. A named
run-and-exit Rust shipper uses local backup reader identity separately from foreign shipper identity.
The local receiver helper elects a node and starts a typed device-less target with FLDT, never FLCT.
Source/base #52 grants are claimed atomically with outgoing references under exact source-guard/
noncancelled-operation compares. BindDrManifest persists full root/hash before foreign Begin.
Both grants and manifest binding survive process death; cancellation first freezes future claims.

Prepare closes range admission, drains writes/repairs, and durably freezes complete roots on
current ISR and joining members. Every join holds all committed+pinned roots/receipts first.
Then only the current trusted receiver core emits CommitShip. Its O(1) transaction compares
header/target/epoch, incoming, relationship and exact unrevoked pair revision; it atomically
publishes point/catalog mapping. Epoch/manifest/mode mismatch rejects delayed outputs. A newer
receiver reconstructs the barrier under its fresh epoch; no old proof reopens a frozen root.
Exact source/destination cuts and canonical page/root/proof bytes are fixed in the appendices.

Pair configuration/revoke CAS is an O(1) authority barrier. Every batch admission and commit
binds pair generation/revision; relationship epoch/session fanout is resumable cleanup only.
Already admitted bytes can finish into isolated roots, never publish after revoke. Metadata
outage pauses new shipping batches; source application I/O has no peer/metadata call or lock.

Destination CommitShip returns a post-CAS signed FLDRCOM1 receipt, using the local token-signing
key with a distinct domain; pairs exchange only its public key. Source verifies exact incarnation,
operation, manifest and point before completion/pin close. DR grant outcomes explicitly extend
#52's ArchiveRef-only completion; no worker assertion or upload ACK alone proves protection.
Local cancellation closes reader barriers but does not falsely claim an already complete remote
upload cannot publish. The next incremental authenticates its exact last common base.

Promotion selects last committed point in one metadata CAS, closes receive authority and allocates
new lineage. This first profile requires explicit possible-dual-primary acknowledgement; it invents
no source-exclusion provider. A typed local promoter fences/reconciles, activates only selected
roots and a new live head, then CompletePromotion enables ordinary root-aware Attach. No peer
contact is required; status always reports possible divergence. Failback remains out of scope.

## Format, compatibility and node admission (S3, S5–S8)

[root-format.md](root-format.md) supplies exact proposed feature/page/record/root-map/checkpoint
contracts, immutable base/zero semantics and physical retention/deletion. Old disk readers refuse
the incompatibility before replay. This increment never migrates/reformats an existing store.
[wire-contracts.md](wire-contracts.md) fixes shared reader nested bytes, paged canonical manifest,
DR frames and purpose-qualified local root sessions. No independent guessed allocation remains.

FLDT binds destination first epoch, node/epoch, pair generation, relationship generation and ship.
Only typed receiver/promoter modes accept it. Ordinary engine/control/agent/CSI/ublk/maintenance
paths independently refuse Standby/Promoting and FLDT, including existing-device shortcuts.
Device-less entries count under #15 slots, return no device, and never gain #48 ublk recovery flags.
Only direct mode is supported; #74 privsep refuses before reserve/helper action. Restart needs a
fresh epoch and root reconciliation, never a saved start line. Full status/detach/delete/error
semantics and the exact409-byte maximum ticket are in lifecycle.md.

A destination header ID object remains dr-roots-v1 after promotion. A paired ordinary source
uses dr-source-v1 while its geometry guard is active. Older string/xts-only decoders cannot
rewrite/Attach these records; DR-aware GetVolumeRequest profile and root-session admission reject
old targets. Profile fields compose explicitly with #50/#51; encryption/chains remain refused.
The legacy DeleteVolume/drain risk is explicit: activation requires every old metadata instance
stopped and the new serve profile flag installed. Unsupported rollback is not safe while any
DR profile/header/store remains. No claim that a marker repairs an old deletion implementation.
#70 growth and every later step refuse source guard or destination mode; #76 CreateSpec preserves
all DR identity fields. The hot source I/O path does not inspect metadata for this guard.

## Authorization and bounded resources (S1, S2, S4, S7, S10)

Lifecycle's before/after #73 matrix gives system-provisioner only system-owned CreateStandby,
cluster-admin configuration/ship/promotion and audit-reader DR status. Receiver and worker rules
are exact self/current-operation rules; every method refuses insecure mode. Activation/owner
compares stay in the same #76 transactions. Protected deletion and #52 close barriers remain.
New DR audit events are an explicit follow-up; diagnostic actor records are not an audit claim.

Foreign waits/counts/queues/metadata-call rate and one global64MiB allocation budget are explicit
in lifecycle.md. Canonical manifest pages/receipts spool under checked node disk/index reservation;
no unbounded in-memory range/ISR vector. #51-style snapshot charge reserves incoming full data
and preserves prior roots; caps/ENOSPC pause/refuse without deleting last point or source pins.
Limits are liveness/resource controls, never epochs/fencing proofs. Clock quality/age is explicit;
#83's elapsed clock is not cross-cluster UTC synchronization or interval-derived RPO.

## Implementation files and contract integration

- api/proto + generated Go/Rust metaclient: all messages/auth handlers/CLI, make proto/contracts.
- meta/domain/service/kv: feature/pair/operation/marked-header lifecycle and bounded transactions.
- flaminio-repl: receiver barrier, immutable roots/membership/promotion modes and simulator mutants.
- flaminio-format/store: proposed root pages/records/checkpoints/replay and source/feature refusal.
- flaminio-net/node/io: DR TLS/framing, root sessions, typed control/slots/direct-mode receiver.
- flaminio-dr Rust binary: local snapshot grants/reader+foreign shipper; no data bytes over Go RPC.
- #52 shared grant/read codecs; #15/#17/#48/#54 states; #50/#51/#70/#73/#76/#77/#80 hooks,
  added by whichever implementation lands second, with cross-contract vectors. Dependencies stay
  read-only in this drafting change; their actual commit hashes are recorded by the parent report.

## Tests and model mutants

| Rules | Concrete future check | Injected bug required to be caught |
|---|---|---|
| S1/S2 | multi-relationship revoke/config CAS races, queued sessions, metadata outage, wrong CA/domain/role | omit_pair_compare |
| S3 | FLDT/FLCT/FLRT/FLBR cross-decode; ordinary/control/maintenance/CSI reuse and privsep/slot/restart matrix | expose_receiver_as_ordinary |
| S4 | source/base grant recreation, StartShip crash each phase, local incoming cancel/commit race, wrong destination, source guard/growth | drop_ship_pin_early; late_grant_after_cancel |
| S5/S6 | root/receipt replay/crash boundaries, prepare vs repair, late writes, join/root transfer, current-target commit | join_without_pinned_roots; mutate_prepared_root |
| S6/S8 | delayed output/takeover, commit/promotion CAS both orders, no-complete-point refusal | stale_epoch_commit; expose_partial_ship |
| S8/S9 | repeated promotion, exact selected root/lineage and new live head, old receiver after fork | promote_newer_staging; reuse_lineage |
| S7/S10 | handshake/body stalls, global allocation/disk caps, large paged manifests, no queued flood, unknown clock | overlimit_allocation fixture (not a protocol mutation) |
| S11 | deterministic pair→full→incremental→resume→midship promotion, lying peer/root data, exact per-replica point | baseline clean counterpart for every caught mutation |

Auth tests cover every method/role, insecure refusal, owner/activation races and protected delete.
Contract vectors cover new/old cross-decode, exact sizes/zero/padding, signed receipt and all
reader/message nested bytes; fuzz both format and network decoders. Crash tests cut each ordered
root/receipt/checkpoint/publication step. Run future make lint test race hygiene contracts sim
crash-test miri sanitize as applicable. No runtime tests or lab campaign ran for this plan.

## Lab, docs, risks and ADRs

[Lifecycle](lifecycle.md) fixes make dr-dest-up/down and dr-test using explicitly disjoint fresh
DR disk/units/ports on fl-c, preserving #54's destination and data. Active #83 physical-only
admission blocks VM mutation first; separately authorized pre-#83 exploration is not production
qualification. RF1 destination evidence has explicit limits; multi-ISR safety is model/crash tested.

Implementation updates architecture/protocol, docs/lab.md, Make help and followups to actual
behavior/evidence. Historical issue-local ADR acceptance is preserved; this revision is proposed
and unapproved. Accepted ADR0001/0002/0004 and current fencing are unchanged. Proposed audit
0009/0011 reference/commit obligations remain. New format/authority choices are concrete here,
not accepted by a review-clear claim. Terminal record/retained-root growth and legacy mixed-version
delete/drain risk are named, not newly accepted. Runtime/lab/ADR acceptance is separate.

## Parallelizable?

Yes against these exact proposed seams: Rust transport/store/model and Go lifecycle can implement
independently after approval; shared golden vectors are integrated by whichever lands second.
No byte/authority mechanism is delegated to an unspecified future coordination gate.
