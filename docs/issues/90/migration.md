# Issue #90 — restartable enrollment and activation

Normative proposed appendix to [design](design.md), **unapproved; not implemented**.
The historical #61 administrator-evidence policy is provenance only. This API makes that policy
executable; it cannot prove historical data/fence continuity from a UUID observed today.

## Barrier, records and ownership

Under `/flaminio/v1/`, `identity-mode` is a never-deleted strict schema1 record
`{mode:LEGACY|ENROLLING|ACTIVE,operation:uuid16|null,generation:u64}`. Generation starts1,
increments on mode transitions without wrap; overflow refuses. ACTIVE has no downgrade API.
Every identity-aware runtime authority writer compares its ACTIVE mode revision in the same
transaction: registration/heartbeat/report, create/attach/move/replacement/ChangeIsr, cleanup,
forget, catalog/root/pin and any active feature's protected mutation. #73 admission/ownership
checks remain additional checks, never replaced by this mode. Read-only status stays available.
Bootstrap finishes with this same mode compare. Existing old binaries cannot be fenced by a
new key they ignore: an operator must stop/remove **all** old metadata binaries and stop all
old targets/replicas/provisioners before Begin. There is no claim of online mixed-version safety.

Before the barrier, settle pending snapshot reservations/cuts/creation under the installed
supported implementation. If an interrupted operation cannot settle safely, preserve its whole
affected volume, pending records and conservative charges as explicit quarantine; ordinary
identity activation does not wait for an unimplemented future recovery mechanism. Inventory
records that choice and never silently drops a pending operation.

The new service initially runs maintenance-only in LEGACY for an existing database. A new
empty cluster also initializes through this path (empty inventory then Activate). Before
activation, no data-plane session/Attach or ordinary mutation is accepted by the new service.
New IO can report an opened UUID only through `ObserveEnrollmentStore`, with no assignment,
lease, capacity admission or session created. Old process/session credentials are not reused;
activation starts new targets through fresh Attach epochs as ordinary restart rules require.

`store-enrollment/<operation>` is durable strict schema1:
`{operation,inventory_generation,actor,evidence_digest,state,inventory_cursor,inventory_digest,
 inventory_count,enroll_cursor,verify_cursor,verify_digest,converted_count,quarantined_count,verification_generation,last_failure,
 expected_schema,active_profiles,binary_inventory_digest}`. operation random nonzero UUID16;
SHA256 digests exactly32, actor authenticated admin SPIFFE identity, state
INVENTORY|ENROLLING|VERIFYING|READY|ACTIVE. Cursors are opaque canonical record keys chosen by
service, not arbitrary keys. Every scan step stores last key, rolling digest and count in the
same transaction as its progress; a revision/operation CAS makes retry/resume idempotent.
Only one operation exists while mode ENROLLING; terminal job is retained for provenance.
No API silently starts another operation or overwrites an interrupted job.

`store-observation/<operation>/<node>` records `{uuid,io_identity,challenge,observed_at,
node_revision,observation_digest}` (≤1024 bytes); challenge random32 bound to current IO process.
Observations assert only what is open **now**. `store-evidence/<operation>/<node>` records admin
actor, evidence_digest, observation_digest, known_original:boolean and reason≤256. Evidence is
an operator-reviewed inventory manifest stored externally; its digest and exact inventory
binding are durable. An unchanged disk is enrolled only when that manifest establishes continuity
of original data **and fences** for the named volume incarnations/memberships. Current UUID,
empty disk, matching size or an administrator role alone does not establish that continuity.

Every ordinary inventory item is a bounded record with stable key/hash and source mod_revision:
node/store-lifetime, volume header/first_epoch, create plan, chunk replica/ISR/move/release,
header fence holder, bounded tombstone root/ref-charge rows and active snapshot/root/source-lifetime
references. Inventory includes creating/deleting/forgotten nodes and header-absent tombstones,
not just ListVolumes results. Existing #13 release/fence bounds apply. One item is an existing
bounded metadata record, not a whole volume response. Tombstones use contracts.md's root and
ref-charge row units, never a whole-volume union. A legacy monolithic tombstone has a distinct
projection adapter: keys-only discovery, one exact-key GetBounded capped at1,572,864 bytes
(the configured1.5MiB backend envelope), then streaming JSON projection into at most32-entry/
16KiB virtual items identified by parent key, source revision/hash and typed entry ordinal.
Only one raw1.5MiB source buffer exists per service; no whole-array unmarshal. Every reread
checks the same frozen revision/hash. Convert into shadow root/ref rows, replacing the legacy
root only after all projections reconcile. Oversized/corrupt values become explicit quarantine;
newly generated large unions use bounded rows and never this legacy monolith exception. Scalar geometry must be≤65536 chunks;
root sessions retain4096. Larger or malformed legacy records become explicit quarantine items,
not silently skipped. Snapshot/root profiles use their owning issue's schema converters when
present; ordinary identity enrollment does not require those future features.

`store-quarantine/<operation>/<item_hash>` (≤2048 bytes) holds source key/hash/revision,
legacy node refs, volume/first_epoch, reason, evidence_digest, actor and resolution state.
Rewritten schema2 records carry `identity_state=QUARANTINED` plus this reference and preserve
legacy identity only as diagnostic provenance. No authoritative empty/default ReplicaRef is
created. Such volumes/chunks/cleanup records cannot serve, admit replicas, grant a source or
confirm deletion/release. Only status and a separately justified future administrative recovery
may resolve uncertain provenance; activation alone does not recover them. No disk deletion or
reformat is performed by enrollment. Known exact old cleanup refs remain exact and charged;
unknown legacy cleanup remains quarantined and conservatively charged under #72.

## Exact RPCs and CLI

All methods except Observe are **administrator only**, enforce mTLS and refuse insecure mode.
With #73 active they require its cluster-admin authority; no system-provisioner, audit-reader,
delete-override, tenant user, agent or ordinary IO permission is inherited. Observe is own-node
IO only, checks matching operation/challenge and accepts neither caller generation nor binding.
Identifiers/hashes/UUIDs use contracts.md canonical validation. Failure order: malformed
InvalidArgument, wrong role PermissionDenied, unknown operation NotFound, changed request UUID
AlreadyExists, changed revision/cursor Aborted, wrong phase/profile/provenance FailedPrecondition,
size/allocation cap ResourceExhausted, unavailable KV Unavailable. No partial success implied.

| RPC | Request fields (tag:type) | Response fields (tag:type) |
|---|---|---|
| BeginStoreEnrollment | operation1:bytes16,evidence_digest2:bytes32,binary_inventory_digest3:bytes32,active_profiles4:repeated u32,expected_mode_revision5:u64 | enrollment1:EnrollmentStatus,revision2:u64 |
| ObserveEnrollmentStore | operation1:bytes16,node2:string,store_uuid3:string,challenge4:bytes32 | observation_digest1:bytes32,revision2:u64 |
| InventoryStores | operation1:bytes16,expected_revision2:u64,cursor3:bytes,limit4:u32 | items1:repeated InventoryItem,next_cursor2:bytes,complete3:bool,status4:EnrollmentStatus,revision5:u64 |
| EnrollStores | operation1:bytes16,inventory_generation2:u64,expected_revision3:u64,batch_id4:bytes16,bindings5:repeated EnrollmentBinding | status1:EnrollmentStatus,revision2:u64 |
| GetEnrollmentItem | operation1:bytes16,item_key2:bytes | item1:InventoryItem,binding2:EnrollmentBinding,current_revision3:u64,current_hash4:bytes32,conversion_revision5:u64,job_revision6:u64 |
| CorrectStoreEnrollment | operation1:bytes16,inventory_generation2:u64,expected_revision3:u64,correction4:bytes16,item5:InventoryItem,expected_current_revision6:u64,expected_current_hash7:bytes32,expected_conversion_revision8:u64,binding9:EnrollmentBinding | status1:EnrollmentStatus,revision2:u64,current_hash3:bytes32 |
| VerifyStoreEnrollment | operation1:bytes16,expected_revision2:u64,cursor3:bytes,limit4:u32 | next_cursor1:bytes,complete2:bool,status3:EnrollmentStatus,revision4:u64 |
| ActivateStoreIdentity | operation1:bytes16,inventory_generation2:u64,expected_revision3:u64,inventory_digest4:bytes32,verify_digest5:bytes32,binary_inventory_digest6:bytes32 | mode_revision1:u64,status2:EnrollmentStatus |
| GetStoreEnrollment | operation1:bytes16 | status1:EnrollmentStatus,revision2:u64 |

`EnrollmentStatus={operation1:bytes16,inventory_generation2:u64,state3:u32,
inventory_count4:u64,converted_count5:u64,quarantined_count6:u64,inventory_digest7:bytes32,
verify_digest8:bytes32,active_profiles9:repeated u32,binary_inventory_digest10:bytes32,
enroll_cursor11:u64,verification_generation12:u64}`.
`InventoryItem={key1:bytes,source_revision2:u64,source_hash3:bytes32,kind4:u32,
volume5:string,first_epoch6:u64,legacy_nodes7:repeated string,converted8:bool}` (≤16KiB).
Keys are server-minted returned opaque bytes,≤512, checked against this operation's inventory;
clients cannot construct an unrelated KV path. `EnrollmentBinding={item1:InventoryItem,
refs2:repeated ReplicaRef,observations3:repeated bytes32,evidence_digest4:bytes32,
known_original5:bool,quarantine_reason6:string}`. Inventory item kinds enum NODE1,HEADER2,
CREATE_PLAN3,CHUNK4,TOMBSTONE5,SNAPSHOT6,ROOT7,SOURCE8,TOMBSTONE_REF9,LEGACY_TOMBSTONE_PROJECTION10; unknown0 refuses. The conversion maps
all legacy role arrays one-to-one in stored canonical order; duplicate/missing role positions
or extra refs refuse rather than guessing. One binding≤16KiB. `known_original=false` requires
reason and creates explicit quarantine instead of installing refs. No status response emits
tokens, store challenges or raw evidence documents.

CLI `metactl enroll-store begin|inventory|bind|verify|activate|status --operation UUID …`
uses these RPCs (including `item` and `correct` for correction), JSON input file for bind/correct
(≤256KiB), stdout one bounded JSON response and
service error/exit1 on refusal; success exit0. `--evidence-sha256` and `--binary-inventory-sha256`
are explicit hashes, not a force option. Inventory/verify use returned cursor/revision; bind
names explicit `--batch UUID`; activate takes the returned inventory/verify digests. IO startup
maintenance mode submits Observe and exits/refuses service rather than becoming a target.
No new shell/socket line accepts an unvalidated ID/path.

## Ordering, batches and restart

1. **Begin:** CAS mode LEGACY at expected revision and absent job; set ENROLLING with operation
   and persist job. Inventory generation is the newly incremented identity-mode generation, service-owned,
   not supplied by admin. Retrying same operation and byte-identical immutable inputs returns
   it; another operation/input conflicts. External old-binary stop and binary inventory are
   operator attestations, explicitly recorded, not machine-generated proof of deployment state.
2. **Inventory:** server scans the fixed kind/prefix order above in pages≤16 items/256KiB;
   no mutation changes source records in INVENTORY. Each inventory page appends durable per-item
   source hashes/revisions and advances job cursor/digest by CAS job+mode;≤18 compares/17 writes.
   Digest chain is SHA256(previous_digest32 || u32 kind || u32 key_len || key || u64 revision ||
   source_hash32), seeded SHA256(`FLSTORE-INVENTORY-1`||operation16). End marks ENROLLING and
   fixes inventory_count/digest. New IO observations live outside inventoried runtime prefixes.
3. **Enroll:** batches≤16 items, exactly next inventory positions (or idempotent retry of last
   batch ID). Transaction compares mode/job, every source record revision and all coupled
   node/lifetime/gate/ledger revisions. Build the exact coupled-key plan, deduplicating shared keys; each transaction accepts only
   a prefix that fits **120 compares and120 writes** including mode/job. Request maximum16
   items is an additional count bound, not permission to exceed KV limits. Before submitting,
   the service reports the accepted prefix length in its returned enroll_cursor; every legal
   item must fit alone (up to16 fence holders, their bound evidence/ledger keys and the source
   remain below120). An item that cannot fit alone refuses before mutation. Source and its coupled gate/lifetime/ledger updates are atomic. Store
   latest batch ID/input hash/result revision in the same job; lost reply returns its recorded
   result, changed payload at same batch ID AlreadyExists. A source/coupled-key conflict returns
   Aborted, advances nothing and requires reread/review, not an unbounded loop. Capacity conversion
   preserves conservative charges and no double-credit. A legal item's
   source record is never partially rewritten across transactions; unrelated items may form
   separate bounded batches.
4. **Verify:** only after enroll_cursor reaches inventory_count. Re-scan **all runtime prefixes**
   in bounded pages, ensuring schema2 exact refs/lifetimes or explicit quarantine, no node-only
   authority, gates initialized, no node-only legacy reader/writer mode, valid counts/digests/charges and
   no dangling unconverted cleanup references. Verify records final schema hashes with separate
   `FLSTORE-VERIFY-1` chain and durable cursor. ENROLLING mode blocks runtime writes; enrollment
   bindings/observations stop changing once VERIFYING starts. Failed verification records the
   item/reason, returns to ENROLLING with verify_cursor/digest cleared, increments verification_generation
   and requires CorrectStoreEnrollment for an already resolved item (the enroll_cursor stays at
   the end);
   it never marks READY. Full successful scan sets READY; compaction is irrelevant because
   progress uses durable source/final hashes and current CAS reads, not an expired KV snapshot.
5. **Activate:** compare job+mode, matching operation/generation/digests/binary inventory,
   state READY and complete counters; set mode ACTIVE and job ACTIVE in one2-compare/2-write
   transaction. This is the activation linearization point. Verify's barrier and immutable
   READY state prove the complete scan still applies, avoiding an unbounded final transaction.
   Repeat after lost reply returns the same retained activation result. A new old-version process
   appearing after the deployment inventory is an operational barrier violation, not authorized
   fallback. Stop it; no API manufactures assurance it is safe.

All operation state/progress/quarantine survives metadata restart. New process reads mode/job
and resumes the same returned cursor; scans never restart at a different inventory generation.
Interrupted binding leaves either the complete atomic batch or none. New IO during migration
may add an observation only in INVENTORY/ENROLLING, cannot register earned authority or rewrite
an inventoried node; repeated observations with changed UUID invalidate affected evidence and
verification readiness through the job CAS. Changing a disk after binding forces explicit
CorrectStoreEnrollment quarantine/review before
VERIFYING. After READY no Observe is accepted. Post-ACTIVE registration of a different UUID
is ordinary lost-store observation with a new lifetime, never silent enrollment.

Activation additionally requires every active binary implementing the declared profiles, all
active metadata/proto/sweep writers comparing mode/gates, supported schema/codec vectors, and
no legacy-only reference in a runtime authority path. Quarantined objects count as explicitly
converted but remain unavailable, with diagnostics naming recovery work. Ordinary new creates/
growth satisfy B90; explicitly enrolled older geometry may use `paged_legacy=true` **only** if
[budgets.md](budgets.md)'s bounded coherent ReadIdentityVolumePage/AttachIdentityVolume path is
installed and scalar≤65536. It retains committed historical views but refuses new snapshot/root
reservations or growth above B90. Root limits remain4096. Unsupported geometry/profile is quarantined,
not silently truncated or made an unrelated future-feature gate.

## Correcting already converted items (SEC90-R1-03)

`store-conversion/<operation>/<item_hash>` stores the immutable original inventory identity,
current schema hash, resolution EXACT|QUARANTINED, evidence/observation hashes, charge identity
and last correction ID. Initial EnrollStores writes this bounded record atomically with the
converted item and advances enroll_cursor. EXACT and QUARANTINED counts are disjoint and sum
to enroll_cursor; changing resolution adjusts both counts once. Source inventory digest/generation
never changes. Current record/conversion mod revisions are returned by GetEnrollmentItem
(admin, response<=64KiB), so correcting a processed item does not require rewinding that cursor.

CorrectStoreEnrollment accepts **one** already resolved item, only in ENROLLING, including after
Verify has failed. It checks original key/revision/hash against the frozen inventory, current
record revision/hash, conversion revision, exact observation/evidence digests and expected job
revision. A replacement binding uses the same provenance rules as initial enrollment: today's
new UUID cannot be substituted for an old earned copy. A legitimately corrected original UUID
requires continuity evidence; otherwise the item/affected volume is explicitly quarantined.
No correction releases old-copy capacity, deletes media or makes ambiguous data authoritative.

Build the bounded whole-item coupled plan, including mode/job, item/conversion/evidence, exact
root/ref-index/ledger/gate keys where applicable and an immutable correction receipt; reject
before mutation if it exceeds120 comparisons or120 writes. Rebinding a stored ref rekeys its
charge ownership without changing the conservative total; quarantine retains that charge and
sets the item's bounded affected header/cleanup root unavailable marker in the same
transaction. A node-wide correction does not pretend to atomically rewrite unbounded dependent
volumes: job invalidation keeps all runtime writes barred, and full verification requires each
affected dependent item explicitly corrected/quarantined before READY. A multirow
legacy tombstone uses the already bounded projection/ref-row units; publication remains blocked
until the owning root's conversion is complete. No partial authoritative item is exposed.
Receipt `store-correction/<operation>/<correction>` contains original/current/new hashes,
resolution, evidence hash and result counters (<=2048 bytes). Same correction and identical
payload returns the committed receipt after restart/lost reply; changed payload AlreadyExists,
stale current/conversion/job revisions Aborted, unprocessed item FailedPrecondition.

Every successful correction and changed observation atomically increments verification_generation,
clears verify_cursor/digest/READY eligibility and records affected item; observations also CAS
and write the job so a concurrently planned binding/correction loses its job compare. A failed
verification records its exact item/hash and performs the same invalidation. After correction,
Verify starts at the **beginning** with a new verification generation; continuation carries that
job revision and cannot mix old pages. All corrections must be reflected by a complete rescan
before READY. READY/ACTIVE accept neither correction nor observation; no amendment API weakens
that final immutable predicate. Original historical provenance and every correction receipt remain.

## Proven legacy first_epoch=0 (R1-C5)

Do **not** invent a positive first_epoch or rewrite a fence/cut/receipt. For a proven unchanged
ordinary/snapshot volume with FirstEpoch0, enrollment assigns one random nonzero UUID16
`legacy_incarnation` (canonical UUID36 in JSON, bytes16 in protobuf/wire), stored immutably in its schema2 header and conversion receipt. Retries use
the same persisted value. This is an explicit versioned legacy identity `(volume,0,LID)`, never
a new creation: all proven chunk refs are already earned (`initial_pending_refs=[]`). New volumes
and deleted/recreated names always use positive first_epoch and cannot request this legacy mode.
Unknown continuity remains explicit quarantine, not a guessed legacy enrollment.

The wire contract defines profiles3 ordinary/4 snapshot and signed FLCTv3 carrying first_epoch0
plus this LID. Old FLCTv1 is refused even if otherwise signed/current; FLCTv2/positive profiles
cannot be cross-decoded as legacy. Attach/AttachIdentityVolume for the explicit legacy header
still increments its existing current epoch and issues only FLCTv3 after normal #79 checks.
This is not an inferred incarnation from Attach. OpenReplicaGrant, private grant record/digest,
SessionRef/read handles, source/ref operations and cleanup requests bind the same LID, as public
contracts specify. Metadata requires exact header LID and current epoch; a reused name with a
positive first_epoch cannot accept any old zero-incarnation token/grant.

For these profiles `GrantFinished` emits **no DeleteEarlierIncarnation action**. It verifies
existing known-original chunks and fences, then directly persists the checked current epoch
through #13's monotonic fence operation without truncating data/layers. A missing MEMBER chunk
refuses. No zero-map inference, create INITIAL or data/fence rebase exists. Current fence above
the metadata epoch indicates inconsistent provenance and refuses for administrator review.
The normal fresh target Attach/fencing/lease wait precedes I/O; a lost disk still requires exact
replacement reconstruction, never a UUID alias. When a later positive incarnation is created,
normal delete/recreate first-epoch fencing applies only after the legacy deletion has completed.

Keep existing snapshot keys under their zero-first-epoch prefix, ordinals/generations, map/zero
masks and durable layer records byte-for-byte. Identity grant/session LID supplies the missing
incarnation binding for local Current/Before views; no cut is retagged or made empty. The legacy
snapshot adapter must round-trip current and each committed view and obey full frontier proof.
Positive-incarnation-only #52 export/source-proof and #58 root/receipt operations explicitly
refuse legacy mode before creating pins/reservations. An FE0 record already claiming such an
incompatible root/source profile is explicitly quarantined during inventory; never rewrite an
immutable signed manifest/receipt or silently activate an unusable root session. This restriction
does not block ordinary/snapshot legacy I/O or ordinary identity activation on unrelated volumes.

Verification requires installed profiles3/4 only if corresponding proven legacy objects exist,
checks LID on every dependent cleanup/catalog reference and preserved store fences/history,
and rejects an unknown/empty LID for FirstEpoch0. No mutation assigns LID to a different data
history. When #15/#79/#83 strict local profiles are composed, verification additionally checks
[local-target-contract.md](local-target-contract.md)'s LID device/normal-intent and current-process
FLCH adapters, including paged Attach nonce/handoff, before enabling that combination. Fresh
filesystem initialization remains refused. Safe original-store proof remains the administrator's
responsibility, not UUID syntax.

## Future validation

`meta/service/store_enrollment_test.go` runs identical suites against Mem and embedded etcd:
admin/auth matrix, insecure refusal, wrong operation/inventory/expected revision,16/+1 requests,
120/+1 preflight key plans and bounded-prefix continuation,
record/coupled-key conflict, lost replies at every phase, metadata restart after every batch,
IO observation during binding, changed UUID/provenance, interrupted quarantines, forged cursor,
correction after the final batch and after failed verification, current-hash/evidence CAS,
correction lost reply/restart, counter/charge conservation, fresh verify-generation rescan and
READY correction refusal,
header-absent release/fence/tombstone conversion, active source/snapshot/root references, capacity
charge conservation, oversized paged legacy volumes and missing adapters. Activation races
against registration/create/move/cleanup and must have one valid linearization; while ENROLLING
all runtime authority writers refuse without mutation. Each phase's exact compare/write maximum
is checked against both KV backends (Mem must enforce MaxTxnOps too).

Real CLI/Rust metaclient tests round-trip status/cursors/errors and a stopped real Store fixture
supplies observations; no fake model call proves on-disk provenance. Known original enrollment
preserves data and durable fences; ambiguous legacy input remains quarantined. Both metadata
backends and actual remote/local Store fixtures cover proven FirstEpoch0 with acknowledged
nonzero bytes, existing retained fences/releases/tombstones and committed Before views: preserve
all bytes/views after FLCTv3 attach, emit no DeleteEarlierIncarnation, refuse FLCTv1/cross-profile/
wrong-LID/INITIAL replay, survive enrollment and first-bootstrap crashes, and reject incompatible
root/export mode explicitly. An independent `legacy_no_destructive_cleanup` simulator assertion
inspects preserved acknowledged blocks/views; mutation `legacy_zero_epoch_cleanup` deletes them
at bootstrap and must be caught by the same clean nonzero-data/snapshot witness. Runtime adapter
post-activation tests prove legacy Hello/projection-only requests and unsupported feature codecs
refuse before any store mutation. These are future tests; no shared lab or runtime feature is
executed by this docs-only revision.
