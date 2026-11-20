# Issue #58 — local lifecycle, authority and integration

Proposed review revision begun2026-11-19; reconciled2026-11-20; **unapproved**. Historical approvals/settlements remain
in [decisions](decisions.md). [API](contracts.md) · [wire](wire-contracts.md) ·
[root format](root-format.md). No runtime feature is implemented by these documents.

## Identities, records and complete operation flow

All keys below are under `/flaminio/v1/`; JSON values have `schema:1`, reject unknown/duplicate
fields and are at most8192 bytes (CA-bearing pairs98304). UUIDs are 16 bytes, serialized as 32 lowercase hex digits;
volume/node IDs use `domain.ValidID`, 1–128 ASCII bytes. `cluster` is #52's existing immutable
`{cluster_uuid}`; never create an independent DR cluster identity. `features/dr` is an immutable
activation record `{schema,profile:"dr-roots-v1",enabled_by,time}`. An operation UUID is chosen
once by the initiating administrator and retained for every retry; reuse with changed immutable
inputs is AlreadyExists. Expected revisions are etcd ModRevision, not wall-clock timestamps.

| Key | Essential value and ownership |
|---|---|
| `dr/pairs/P` | peer cluster UUID, canonical trust domain, exact CA bundle, allowed shipper/receiver URI, foreign endpoint, generation, revoked; local administrators own configuration |
| `dr/relationships/R` | source cluster/volume/first epoch; local pair UUID+generation; destination volume/first epoch; geometry; RF/minISR; receiver node; active_ship; generation; state; last ShipPoint; promotion op/lineage |
| `dr/incoming/R/S` | immutable source/base/target refs and manifest root, admitted pair generation, destination incarnation, receiver epoch, destination_cut, state Charging/Receiving/Prepared/Committed/Cancelled, counters/error class |
| `dr/outgoing/S` | source incarnation, target/base refs+atomic grant refs, full bound manifest root/hash, outcome/required/closed masks, local worker URI/node, destination pair/relationship/incarnation, immutable request digest, manifest root, state Queued/Reading/Sending/Committed/Closing/Closed/Cancelled |
| `dr/promotions/O` | relationship/incarnation/selected point, new lineage, actor/acknowledgement, elected local node, epoch, state Pending/Ready |

One active incoming operation per relationship; one active outgoing operation per source volume.
Terminal records remain in this increment (no TTL/name reuse); at most 4096 relationship records,
4096 pair records and 65536 terminal ship/promotion records per cluster. Refuse new admissions
at these caps; never evict a terminal record and accidentally admit an old operation. A future
collector is a follow-up, not implicit deletion. Lists page at most 128 records/1 MiB, with an
opaque cursor containing the last exact key, never an arbitrary caller-controlled prefix.

1. Each administrator runs `dr enable`, then `dr pair` with the remote immutable cluster UUID,
   exact peer domain, CA and exact role URIs. Pair IDs are local: each side records its own pair
   UUID. A foreign Hello presents the receiver's local pair UUID/generation, obtained in the
   explicit exchanged standby descriptor; it is never interpreted as the sender's pair ID.
2. Destination `dr standby` supplies operation+relationship UUIDs, local pair, exact source
   cluster/volume/first epoch and source geometry, plus its new system volume name/RF/minISR.
   CreateStandby admits #76's creation claim with the immutable DR marker and relationship
   together. No visible intermediate ordinary volume exists. The same #76 helpers finalize
   placement/accounting, then atomically set destination first epoch in the relationship and
   visible header. Its returned StandbyDescriptor contains both cluster UUIDs, destination
   incarnation, relationship/generation, destination pair/generation, geometry and endpoint.
3. Source `dr ship` supplies operation UUID, local destination pair and the administrator-verified
   StandbyDescriptor, target snapshot name and optional expected last committed manifest. The
   service verifies its local source incarnation, a committed #51 snapshot and exact geometry;
   encrypted/resizing/root-profile sources and chains are refused. It records Queued and binds
   an explicitly configured local `backup` worker URI and `shipper` URI. It never derives trust
   from an unauthenticated destination descriptor; TLS must match the configured pair.
4. The administrator invokes `flaminio-dr ship --operation S --config PATH` as the unprivileged
   `flaminio-dr` user. This run-and-exit Rust binary owns data reads/DR networking. Its local
   metadata/reader certificate is `.../flaminio/backup/W`; its separate foreign client certificate
   is `.../flaminio/shipper/W`. It verifies both before starting and keeps private keys in
   separate 0700 directories owned by that user. It never receives admin/FLCT credentials.
   There is no hidden daemon: retries explicitly invoke this same command/operation. The local
   service authorizes exact grants as below; worker registration updates only its own operation.
5. Worker reads target and, for an incremental, base through independently pinned #52 grants;
   produces the canonical paged manifest, calls BindDrManifest to persist its full root/hash
   and enter Sending, then sends DR Hello/Begin only after successful bind/read-back. Destination checks the exact
   pair/generation/source/relationship/incarnation, allocates/advances #51's monotonic snapshot_next_ordinal as destination_cut and charges
   the pending snapshot through #51's bounded reservation before Receiving; partial Charging
   resumes on the same Begin and admits no root write. A local
   administrator invokes `flaminio-dr receive --relationship R --node N`; its administrative
   setup call selects N durably. The node's ordinary `io` identity then obtains AttachStandby
   for itself and receives the FLDT only after that transaction commits. The orchestration
   helper passes it to `dr-start`; the foreign shipper never sees a local ticket.
6. The receiver applies/probes durable ranges under normal local replication. A reconnect
   reuses S and verifies the same immutable manifest/base. A receiver restart requires a new
   AttachStandby epoch; it reconstructs roots/receipts, reconciles normally and probes durable
   progress. A lost response is read back from the same incoming record/manifest; no guessing
   from a connection close or bitmap. Different input under S is a conflict, never overwrite.
7. After Finish and the immutable-root barrier, CommitShip publishes one ShipPoint. Foreign
   Status is authenticated on the configured destination endpoint and returns that exact point.
   Source records Committed only from this evidence, then completes/closes both reader grants;
   ReportDrProgress(check_closure=true) advances Closing→Closed only from actual required grant
   states/absence under frozen claims; completion/cancellation outcome remains recorded. A crash can retain pins longer,
   not release them early. Repeating the same operation resumes the recorded phase.
8. Destination `dr promote` needs no source connection. It CASes the selected last point into
   Promoting and a new lineage/epoch. `flaminio-dr promote --operation O --node N` resumes the
   elected local node through typed `dr-promote-start`. Completion is emitted only after its
   core activates selected roots on the current required replicas; CompletePromotion CASes
   Promoted and clears receiver authority. Ordinary Attach is then permitted to root-aware
   targets. Unfinished incoming roots never become the selected point.

Deletion first sets relationship REVOKED, increments its generation and raises the header
fence under the deletion CAS; it marks noncommitted incoming operations Cancelled and invalidates all
receiver authority and uses #73/#52/#76's ordinary deletion/reader/owner barriers. Source volume
deletion is blocked while either grant is not closed; destination deletion fences incoming and
promotion operations before tombstones. Cleanup pages at most 100 record deletions/transaction,
compares the header absent and exact destination/source incarnation. Terminal DR records remain,
so recreating a name cannot revive a relationship, ship receipt or ticket. A creating standby's
#76 revocation cancels its relationship in the same revocation transaction.

## Immutable target barrier and bounded metadata decisions

The trusted local receiver core, in `flaminio-repl`, owns prepare just as #51's target owns the
snapshot cut. It closes writes to S, drains all sent range writes/repairs and waits for durable
Prepare on every current ISR member and any member being admitted. The prepared manifest/root
set is immutable. A member may join only after it holds every committed point and every pinned
incoming/prepared root, receipts included. An ISR removal may reduce required members only by
the normal committed membership transition; minimum ISR is never weakened. A late message cannot
mutate Prepared. Takeover reconstructs roots/receipts and this barrier under its fresh epoch;
a previous target's prepared response is not a transferable authorization to commit.

Only this barrier emits `CommitShip{operation,epoch,relationship_generation,pair_generation,
manifest,barrier_digest}`. The digest is a deterministic commitment to the canonical manifest,
root-format version and destination incarnation, **not** an unbounded per-chunk proof list.
Metadata trusts its current authorized target's barrier, as CommitSnapshot trusts a target's cut;
all chunk membership safety remains in the model. No staged ISR-vector reads are advertised as
an atomic snapshot. Join-before-admission and prepared immutability keep that evidence true if
membership changes after the output. Peer assertions never substitute for a local barrier.

| Decision | Exact compared records (at read revisions, absence included) | Writes |
|---|---|---|
| AdmitDrShip claim | header, relationship, pair, incoming absent, destination catalog absent: 5 | header ordinal/count, relationship active_ship, incoming Charging, catalog pending: 4 |
| AttachStandby | header, relationship, incoming, pair, node, forgotten absent: 6 | header epoch/target, incoming receiver epoch: 2 |
| CommitShip | header, relationship, incoming, exact unrevoked pair, pending destination snapshot catalog: 5 | relationship last point/active_ship cleared, incoming Committed, header snapshot generation, snapshot catalog Committed: 4 |
| CancelIncomingShip | header, relationship, incoming: 3 | header newer epoch/target cleared, relationship active_ship cleared, incoming Cancelled: 3 |
| PromoteStandby | header, relationship, promotion absent: 3 | header epoch/mode, relationship Promoting, promotion: 3 |
| AcquireDrPromoter | header, relationship, promotion, node, forgotten absent: 5 | header fresh epoch/target, promotion epoch: 2 |
| CompletePromotion | header, relationship, promotion: 3 | header Promoted/target cleared, relationship Promoted, promotion Ready: 3 |
| Pair configure/revoke | pair (absent for first configure), active authorization when role-gated: ≤ 2 | pair: 1 |

Role-gated mutations also compare #73's exact authorization record once active, or its absence
under inactiveRules; add one compare to the relevant row. No decision has a per-chunk vector.
CreateStandby uses #76's bounded claim/placement transactions, with two extra records (feature
and relationship) and #73's existing owner/allocation/authorization compares; shrink each
placement batch by those extra comparisons so total compares and operations each stay ≤ 120.
Its finalization adds the relationship compare/write. No plain CreateVolume path can manufacture
these internal fields. Transaction-count tests exercise maximal batch sizes and activation races.

## Pair revocation and foreign admission

A pair-record CAS is the linearization point for revoke or configuration replacement: generation
increments and revoked is set, or a new explicit config is installed. Every incoming record and
FLDT binds that exact admitted pair generation. AttachStandby and CommitShip compare the same
unrevoked pair revision; after the CAS they cannot publish even if a relationship's epoch has
not yet advanced. Per-relationship epoch cleanup and session closure are bounded resumable
fallout, never an atomic fanout assumption. Promotion remains locally available after revoke.

Each foreign control/data batch obtains one linearizable CheckDrAdmission with pair, incoming
and relationship revisions. Its fixed permit is for at most eight ranges/8 MiB and one second
of issuance; it does not authorize publication. After revoke an already admitted batch may
finish into isolated roots, but no next batch or commit is admitted. Metadata unavailability
stops new batches. Receiver periodically closes sessions on observed pair changes; stale TLS
sessions cannot bypass the per-batch check. TLS1.3 uses no session resumption on DR endpoints. Client SNI is p-PAIRHEX.dr; the server
selects only that pair's CA/config before TLS, and Hello must repeat the same pair. At most8
pair configurations are cached under the global budget; uncached lookup uses the same bounded
metadata-call admission. Receiver URI is the stable .../dr-receiver/CLUSTERHEX service identity
with separate eligible-node leaf keys, so endpoint failover does not silently change authority.
Only the configured peer CA, exact domain, exact role URI and remote cluster UUID combination
passes; peer roots never enter local service/replica CA stores. No same-domain/same-UUID pair.

Liveness limits (not fences): at most 16 preauthentication sockets/node, 8 authenticated sessions,
2 per pair; excess connections close without queueing. TLS and Hello each get 5 s; each complete
frame body 10 s; metadata admission 1 s; idle 60 s; one outstanding admission call/node, with
node-wide burst8/refill4 per second and per-pair burst2/refill1 per second. Over limit is Retry
without queue. Each authenticated session has at most eight outstanding ranges. A global64MiB
reservation precedes **all** network/frame/manifest/proof/page allocations; 8 MiB of it is reserved
for control/heartbeat work, so data cannot exhaust control progress. Pages spool to the private
per-ship disk reservation, not an unbounded Vec. Node receipts/index/manifest metadata reserve
an additional checked `ceil(size/4096)*96 + chunks*65536 + 16MiB` bytes; stage data uses #51's extra pending-snapshot charge for one full logical volume per selected
replica, before Begin; this is the reservation, not a second unaccounted copy charge. The original
volume charge conservatively reserves the eventual promoted live head. Keep one root slot
for that head; root/layer exhaustion refuses another ship. Base/committed roots remain pinned;
ENOSPC/cap exhaustion refuses/pauses new work without deleting the last point. Source grants,
spool and operation records survive disconnect; no timeout declares unshipped data safe to drop.

## Reader pins and authenticated completion

StartShip's durable Queued record precedes grant creation. Its authorized handler creates target
and optional base grants by #52's AuthorizeSnapshotRead implementation, one at a time, with
operation UUIDs `first16(SHA256("FLDRGR01" || ship_uuid || kind:u8))`, kind1=target,2=base.
Each claim compares the exact current QUEUED/READING outgoing and source header guard and
atomically writes the grant/pin plus its outgoing GrantRef. There is no grant/reference gap.
A lost response reads the same deterministic record; it never allocates another ID. Cancelled,
Closing/Closed or a guard naming another operation forbids a new claim. See contracts.md for
at most9 compares/5 writes (including prior-DR collection eligibility); #52's header compare already protects the exact source guard.
Grant worker is the outgoing's local backup URI, never its foreign shipper URI. Reader source
registration, FLBR, revalidation and close/ack/forgotten-node barriers remain #52's. Incremental
read requires the same incarnation/geometry and an older committed base; neither ref is Current.
Only actual authorized grants pin data. The target/base SourceProofs use the exact shared bytes
in [wire](wire-contracts.md), not informal worker-local ownership flags.

Whoever of #52/#58 lands second adds grant `purpose=backup|dr` (absence decodes as backup),
`dr_ship_uuid` for dr grants, and a tagged completion outcome. Backup retains its ArchiveRef;
DR completion has `{source_operation,destination_cluster,relationship,generation,ship,manifest,
point,receipt}` with no ArchiveRef coercion. `CloseSnapshotRead` gains outcome `dr-completed`
only for its bound dr grant. The worker cannot invent this root: RecordDrCompletion verifies
the destination's FLDRCOM1 signed receipt against the exact current source pair's pinned
Ed25519 receipt public key, source/destination incarnation, relationship, ship, manifest and
outgoing request. The source pair must still be unrevoked at that generation; otherwise pins
remain until explicit cancellation/newly authorized recovery. Metadata compares pair and
outgoing revisions, then records the verified receipt and Committed. The worker may close a
dr-completed grant only with that stored receipt hash; handler compares outgoing plus grant.

Destination CommitShip signs the deterministic receipt **after** its committed transaction,
using the destination's existing Ed25519 capability-signing private key with distinct FLDRCOM1
domain. Pair configuration explicitly exchanges its public key32; no private token key or local
capability is disclosed. A lost response re-signs the same immutable committed point. The local
receiver cannot obtain a committed receipt before the metadata decision. The foreign Status
returns this receipt over the exact paired TLS endpoint; source verifies both transport and
signature. Pair configuration/key changes invalidate new completion admission, never silently
alias an old key. Old committed records and source pins are retained.

CancelShip first CASes outgoing Cancelled/outcome CANCELLED, freezing new claims, then
discovers both required deterministic grant IDs (not only references seen by the caller) and
closes any present grant `aborted` through the same #52
barrier. CancelShip retries invoke the same closure reconciler, so worker death does not prevent
an administrator from driving the barriers to Closed. The same reconciler preserves a committed
outcome while finishing its closure; it never converts that outcome to cancellation. Worker observes cancellation before new read/DR batches and must stop; the source
waits for grant source closure/exclusion before Closed. Local cancellation does **not** claim a
remote already-complete upload cannot publish. Before the next incremental, the worker obtains
signed current Status and the source service verifies its point as the exact base; otherwise
only a new full ship is allowed. An administrator may cancel after loss of the peer; no timeout
removes a registered reader source's barrier. Retained committed #51 snapshots stay as #51's
first increment requires; this plan adds no snapshot GC or key-retention exception.

## Purpose ticket and node/control modes

FLDT v1 is an Ed25519 signature over the following big-endian bytes, followed by signature64:
`"FLDT" || u8(1) || u16(volume_len) || volume || u64(first_epoch) || u16(node_len) || node ||
u64(epoch) || relationship_uuid16 || u64(relationship_generation) || ship_uuid16 || pair_uuid16 ||
u64(pair_generation)`. IDs use domain.ValidID; UUIDs nonzero. Maximum total body with signature:
409 bytes. No expiry claim. It uses the local capability-signing key with distinct magic;
FLCT/FLRT/FLBR/FLDT cross-decode must refuse. Promotion uses the same exact ticket envelope
with ship_uuid=selected committed ship, but is authorized by the durable Promoting record and
must enter the separately typed promotion input; receiver admission independently requires
Standby/Receiving. A ticket alone cannot switch one mode into the other. Every token is issued
only after the corresponding metadata epoch/target/op transaction commits.

Ordinary Engine::start_target, control attach/attach-init/attach-start/agent restart/startup
`--attach`, maintenance AttachMaintenance/StartMaintenance, ublk creation/recovery and CSI existing-
device shortcuts refuse Standby/Promoting and FLDT independently. An ordinary FLCT cannot
open a receiver root session. Root replica hello verifies FLDT signature, volume/first epoch,
node/epoch, relationship/ship/pair generations plus its admitted root mode; old replica Hello
never accepts it. Receiver/promoter carries only root-qualified local operations defined in
wire-contracts. No guest-visible ublk device exists for either specialized entry.

Control revision adds the following bounded lines; operation UUIDs are 32 lowercase hex:

| Line | Successful answer/effect |
|---|---|
| `dr-start V R S E TICKET_BASE64` | `ok receiver INSTANCE ATTACH_GEN`; fetch/validate exact metadata view and ticket before reservation, then specialized target |
| `dr-promote-start V R O E TICKET_BASE64` | `ok promoter INSTANCE ATTACH_GEN`; selected committed point only |
| `dr-close V R OP E` | `ok` after input admission closes and the exact target's outstanding work drains/cancels |
| `dr-detach V R OP E` | `ok` after the matching entry ends; absent/superseded is `ok absent`/`ok superseded` |
| `dr-list` | `ok` followed by zero or more bounded `V MODE OP E INSTANCE ATTACH_GEN` tuples |

All verbs validate IDs/strict decimal epoch and signed claims before state mutation; no path,
command or data bytes come from a control line. Line limit follows #80 (65536). Only direct mode
is supported: under #74 `--privsep`, both start forms return `error: V: DR needs direct mode`
before reserve/helper action. They use #15 reserve and count once under --max-volumes; no second
entry for the same volume. At limit answer existing `limit:`. Starting answers `is attaching`;
running answers `is receiving here` or `is promoting here`. An ordinary attach to either state
returns that typed error (CSI maps FailedPrecondition), never returns an existing block device.
`status`/`list` expose states `receiving`/`promoting` and no device path. `device-info V` is `ok
none`; `detach-device V` is `ok absent`, in both starting/running specialized states. Ordinary
detach/maintenance refuse specialized entries; explicit dr-detach is required. Tombstone sweep
`delete V E` ends an exact/superseded specialized entry before deleting replicas and preserves
unrelated entry generations. Starting entries answer `is attaching`, retried by the helper.

There are no #48 recoverable ublk blocks/flags for these device-less entries. An io restart ends
them; persisted metadata/roots require a fresh epoch and reconstruction, never replay of an old
FLDT/start line. #12 SIGTERM closes new start admission, drains existing specialized work within
its ordinary process shutdown policy and leaves metadata operations resumable; no timeout
proves a root committed. The helper reconnects/list-matches its exact operation before retrying
late start replies and explicitly closes/detaches its own stale generation. Named slot/control
and helper restart fixtures cover every branch above. Whoever of #15/#17/#48/#54/#58 lands
second adds these state/error rows and the FLDT refusal to that boundary.

## Authorization, ownership and activation

Every DR method refuses `--insecure` with FailedPrecondition before reads/mutations. All node
rules require local-domain node/io; where a node field exists it must equal the authenticated
node, otherwise the header target must equal that node. Handlers then require
elected receiver/current epoch/operation. All worker rules require the exact locally bound backup
URI and outgoing operation before other record disclosure. Foreign shipper/receiver identities
cannot call any local metadata method. Unlisted methods remain default-denied.

| Methods | Before #73 activation | After activation |
|---|---|---|
| EnableDr | administrator name | security-admin |
| ConfigureDrPair, RevokeDrPair | administrator name | cluster-admin |
| CreateStandby | administrator name, system creation only | system-provisioner, system owner only |
| SelectDrReceiver, StartShip, CancelShip, CancelIncomingShip, EndDrSource, PromoteStandby | administrator name | cluster-admin |
| GetShip, ListRelationships | administrator name | cluster-admin or audit-reader |
| GetOutgoing, BindDrManifest, RecordDrCompletion, ReportDrProgress | outgoing's exact backup worker | same |
| AdmitDrShip, AttachStandby, CheckDrAdmission, CommitShip, AcquireDrPromoter, CompletePromotion | elected/current node/io | same |

`SelectDrReceiver` cannot select forgotten/non-Active/non-root-capable nodes. Once #73 is active,
CreateStandby writes its system owner in #76's initial creation transaction, refuses tenant-form
`pvc-<UID>` names and accepts no tenant owner. Preserve owner, quota/allocation and authorization
compares, including activation racing an inactiveRules call. Other creation, placement and
capacity defaults remain #72/#76/#78's. DR sources must be system-owned once tenancy is active;
no tenant shipping path is introduced. A source protected/deleting owner is refused for StartShip;
CancelShip/reader barriers remain available to end owned work. A deletion is still governed by
#73's owner kind, protection, audit reservation and override rules; DR never supplies a bypass.

No new audit event append is implemented in this increment. The implementation adds an explicit
follow-up entry for DR configuration/ship/promotion audit events through #73's bounded append
protocol; existing DeleteVolume events remain. Records retain actor/operation/acknowledgement
for diagnosis but are not claimed to be a tamper-evident audit chain. Security/deletion-only
identities do not inherit general DR powers from an administrator SAN after activation.

EnableDr is an explicit rollout assertion only after every metadata instance and selected node
runs this implementation; missing feature refuses new DR operations. The stronger byte guard is
header `id={"name":"V","profile":"dr-roots-v1"}` for standby **and promoted** volumes.
Baseline Header.ID:string decoders and #50's older xts-only object decoder refuse before
rewrite/Attach instead of ignoring an extra DR flag. New common ID codec composes plain
string, xts-aes256-v1 and dr-roots-v1; this increment refuses encryption+DR, so one object profile
is enough. All header writers (#76 creation/finalization, Attach, ChangeIsr's header paths,
#17 maintenance, #70 growth, DeleteVolume and #51 snapshot catalog integration) preserve/check
this profile. Ordinary CreateVolume/CreateSpec helpers may not discard it during idempotency.

Proposed Volume.dr=32 is `DrDescriptor{profile1,relationship2,state3,selected_ship4,lineage5,
manifest6}`; profile is exactly dr-roots-v1, UUID fields16 and manifest32. Its maximal protobuf
addition is107 bytes, included in #80's worst-response/header budget fixtures by whichever lands
second. GetVolumeRequest.accepted_dr_profile=4 must equal dr-roots-v1 for the current target's
io caller, otherwise FailedPrecondition; open administrative reads show the descriptor, never
FLDT or foreign credentials. New ordinary node/root hello also checks negotiated root profile
before token/engine startup. Keep dr profile after promotion, because ordinary live IO now uses
the root-DAG format. Older clients/metadata/replicas fail closed; old disk readers reject the
incompat bit before replay. Rollback below this implementation is unsupported while any DR
profile header or feature-bearing destination store exists; never reformat to simulate rollback.

Source pairing persists an outgoing guard and ID object profile dr-source-v1 on the ordinary
source header while a relationship is active; its store remains ordinary snapshot-v2, no root
feature is required. DR-aware clients advertise dr-roots-v1 capability for either profile.
#70 ExpandVolume/growth admission and all later growth steps compare this guard and refuse
resizing a paired source or standby; destination geometry is immutable. RequestSnapshot remains
allowed, subject to #51's limit. The guard is set with StartShip's source operation and cleared
only by EndDrSource after every related outgoing grant barrier is closed; that same CAS restores
the plain source ID string, not when one
ship finishes. Header comparisons in #76 CreateSpec/idempotency include all DR identity fields;
source/delete/recreate cannot carry an old guard into a new first epoch. Source ordinary I/O
never reads this guard/metadata on its hot path.

EnableDr requires the new `serve --dr-profile dr-roots-v1` flag in every metadata unit; older
binaries exit on the unknown flag. Stop/verify every prior metadata process before activation,
then upgrade all selected nodes/clients before admitting DR work. A marker does not stop the
old DeleteVolume unparseable-header deletion or old listing/drain omission. Such mixed-version
deployment/rollback is unsupported and can destroy DR records/pins or retire occupied nodes;
never claim the byte marker makes it safe. No rollback below DR support while these objects exist.

## Lab and executable evidence

Planned `make dr-dest-up`, `make dr-dest-down` and `make dr-test MIN_ISR=1` route through
lab.sh `dr-dest up|down` and checks/dr.sh. Reuse #54's destination procedure, **not its existing
formatted disk/data**: DR requires a fresh feature-bearing store. Allocate disjoint fl-c
resources: domain flaminio-lab-dr, etcd client/peer2579/2580, meta7600, replica7611, reader7613,
DR7614; units flaminio-dr-dest-etcd/meta/node; `/etc/flaminio-dr-dest/pki`,
`/var/lib/flaminio-dr-dest/etcd`, `/run/flaminio-dr-dest/control.sock`; disk serial flaminio-dr,
size20GiB, source in LOCAL_STATE_DIR/pki-dr. #54's flaminio-dest units/ports2479/2480/7500/7511,
PKI, disk and objects remain untouched. Refuse any collision, existing Flaminio superblock,
mounted device, wrong serial/UUID or ambiguous ownership before future formatting. Formatting
is only part of a separately authorized campaign; no command is run while drafting.

Destination is direct mode, RF1/minISR1. The source cluster remains two data nodes+witness;
its worker on fl-w has separate local backup and foreign shipper keys under the flaminio-dr
user, no admin credential. DR source data store stays ordinary snapshot-v2. Active #83
physical-only enforcement returns Blocked **before disk/VM/network mutation**. Explicitly
selected authorized pre-#83 legacy exploration may exercise the flow but is never production
qualification; no fake physical profile/bypass. Local software fixtures and simulator can run
without VM admission. RF1 lab cannot prove multi-ISR durability: model/crash tests cover that;
a future separately authorized physical multi-node destination is needed for empirical evidence.

`dr-test` records exact source/destination versions, node/disk identities and baseline resources;
creates only run-prefixed system-owned volumes; cuts snapshots/full+incremental ships under
writes; confirms pair-specific link loss, receipt-based resume and last-point-only promotion
while another ship is incomplete. It compares every authoritative destination replica to the
selected source snapshot, checks ordinary/maintenance/old-target/foreign-role refusals and
reports actual point age with clock uncertainty. Management persists exact owned fault rules,
units and test object incarnations before mutation; rules affect only the selected DR port/path,
not local replica/SSH traffic. Cleanup removes only matching owned faults/objects and preserves
uncertain/foreign state; standalone cleanup can resume from that private ledger. Unhealed or
inconclusive cases fail. `down` refuses non-owned/live objects and never deletes #54's resources.
No deletion/reformat is inferred merely from a reused name.

Required fake checks: #83 refusal occurs before all mutations; destination serial/unit/port
collision preserves existing resources; interrupted create/format intent is conservative; wrong
first epoch blocks cleanup; partial rule/heal is idempotent; inherited PRIVSEP/FORMAT cannot
change the pinned direct-mode/explicit-fresh-device flow; no source/restore disk touched.
Root-format and wire appendices define golden/crash/fuzz vectors. All checks are future work.

#80 refresh admits only plain↔dr-source-v1 guard changes at the same first epoch/epoch/target/
geometry and unchanged data policy; this administrative guard does not alter source IO mode.
All other identity checks remain. Snapshot cuts continue under #51 while paired.

Promotion remains allowed after pair revocation: AcquireDrPromoter/CompletePromotion compare
only current local Promoting operation, selected historical point and epoch. The FLDT retains
original pair identity for provenance, not permission from the peer. Root activation selects
that point's immutable historical root/generation; it never rewrites root identity to the newer
relationship generation. Receiver writes/CommitShip still require current unrevoked pair.

This first increment emits no individual RootDrop/GC. Aborted staging, receipts, metadata
reservations and root slots remain until ordinary whole-incarnation DeleteVolume/tombstone
cleanup frees every unique allocation. A defined future format tag is not an exposed cleanup
API. Repeated aborts can exhaust the bounded root/snapshot cap and refuse new ships; the last
committed point remains intact. A later reference-aware collector needs its own safety protocol.

`dr-list` uses the existing maximum16 volume slots; its fixed-width validated UUID/epoch tuples
fit the4096-byte helper response bound, tested at maximal128-byte volume IDs and u64 values.

A local administrator ends an abandoned incoming operation with CancelIncomingShip, even after
pair revocation: its header/relationship/incoming CAS clears active_ship, marks Cancelled and
raises the receiver fence. Committed-before-cancel returns that unchanged point; cancel-before-
commit rejects the old commit. No root/charge is freed. New Begin uses a new ship UUID and next
reserved destination ordinal, never reuses a cancelled cut. Status reports Cancelled; a delayed
Begin/Finish for it cannot reopen it. Fixture covers both commit/cancel orders, lost reply and
new ship admission without releasing cancelled roots.


### Source publication/closure regression cases (R2)

Construct/bind/send/restart preserves full manifest bytes; lost bind response returns the same
Sending record; conflicting root and mismatched authorized refs fail; no Begin before bind.
Cancel before, between and after target/base claim transactions, including lost replies/restart,
finds both deterministic identities and forbids any later claim. Actual grant barriers alone
advance Closed and permit EndDrSource. Model check DrPinsNeverAppearAfterCancel must catch
late_grant_after_cancel (omit outgoing compare); the identical nonmutated schedule passes.
Deletion/recreation fixture requires Relationship.REVOKED plus cancelled incoming, never an
undefined Closed/Cancelled relationship enum. Delayed Begin/Commit/Attach under its prior
generation refuses; terminal metadata and retained roots remain until ordinary deletion cleanup.

A #52 collector retains a closed DR grant until its outgoing is durably Closed; compare the
bound outgoing in that collection transaction. Fixture races unrelated grant collection with
completion/closure progress, proving no verified barrier disappears before its closed mask.


### Durable terminal retries (R3)

Closure reconciliation begins with an already-Closed fast path for both ReportDrProgress and
CancelShip: return the unchanged recorded COMPLETED/CANCELLED outcome and durable all-closed
mask, without reopening/re-closing collected grants. The missing-completed-grant Internal rule
applies only before closure was durably established. Fixtures lose the final closure response,
collect the eligible grants, then repeat progress and cancellation for each outcome; all retries
return that same terminal outcome. Lost BindDrManifest response retries SENDING with UNSPECIFIED
outcome successfully; completed terminal retries return COMPLETED, cancelled outcomes refuse.
