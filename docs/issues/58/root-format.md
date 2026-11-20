# Issue #58 — proposed isolated-root format v1

Proposed review revision, 2026-11-19; **unapproved**. This is a concrete allocation against the
source and dependency snapshots recorded in report.md, not a change to the implemented format.
[Design](design.md) · [wire contracts](wire-contracts.md). All on-disk integers are little-endian.

## Compatibility and allocation

Keep superblock major1/minor0 and allocate `FEATURE_INCOMPAT_DR_ROOTS = 1 << 1` (`0x2`). Bit0
remains pipelined commits. New destination stores must be provisioned with bit1 before any root
operation. The old decoder's unknown-incompatibility refusal happens before checkpoint/replay.
No in-place enablement, conversion or disk reformat is part of this increment. A source may use
ordinary v1 snapshot layers. New code reads ordinary stores unchanged and refuses root operations
on one without bit1. Metadata/target profile `dr-roots-v1` additionally guards admission and is
retained after promotion; the feature bit alone is not a service rollout guard.

Allocate mandatory journal types9..16 and page types8..14 below. Existing types1..8 and pages1..7
are unchanged; #51 adds no disk type/bit, and current #64/#68 reserve none. Implementing these
proposals updates the format spec/constants/codecs/fixtures together; whichever dependent change
lands second uses this allocation, never a second interpretation of the same number.

Every page uses the existing4096-byte FLPG wrapper: type:u16 at4, page_version:u16=1 at6,
format nonce:u64 at8, checkpoint sequence:u64 at16, physical page number:u64 at24, payload at32
through4091 and CRC32C of bytes0..4091 at4092. All offsets below are payload-relative. Pool refs
are existing pair-index+1, zero absent; reject out-of-pool, wrong ownership/nonce/type/location,
cycles and nonzero reserved/padding. Pair selection is highest valid seq≤completed checkpoint K.

## Root state and ownership

A slot present in RootDirectory uses the root DAG exclusively. Its ordinary ChunkHeader keeps
identity/geometry/fence/last-write fields and one canonical empty legacy layer (generation0,
page_table_ref0, layer_count1); ordinary layer records4..7 refuse for that slot. Root-aware store
entry points resolve the catalog. Ordinary unqualified store reads/writes refuse rooted slots;
only a profile-validated Live session uses the selected live root. Nonrooted slots on the same
bit1 store continue using unchanged v1 layers. An absent active root cannot be read as zeros.

A chunk has at most64 simultaneously allocated roots, indexed0..63. Identity is the complete
(volume UUID, first_epoch, chunk index, root UUID), never index alone. Root UUID is ship UUID
for a ship root, promotion operation UUID for a promoted live head, and snapshot operation UUID
for a later live head. Catalog ordinals increase monotonically, never wrap or decrease; a dropped
UUID/ordinal cannot be recreated by retrying an old operation. Reused indices retain UUID checks.

States: Mutable=1, Prepared=2; value3 is reserved and refused. Kinds: Ship=1, Live=2. A new root overlays either zeros
(base UUID zero) or an existing immutable Prepared root of the same incarnation/chunk. Bases are
acyclic and never mutable. UNMAPPED falls through the base; ZERO terminates lookup with zeros;
a physical entry owns a private cluster. No map aliases another root's physical allocation.
A partial write reads the inherited cluster, overlays changed bytes and writes a complete new
private cluster; subsequent writes may replace only this mutable root's private allocation.
Prepared roots and their maps are immutable forever. This retains COW sharing through immutable
bases without modifying shared blocks or requiring a new mutable physical reference count.

Promotion creates/selects a Live child of exactly the committed root; it never selects staging.
Ordinary post-promotion I/O uses that child under the root-aware session profile. A #51 snapshot
cut freezes the live root and creates/selects its next Live child in one journal batch, after
its ordinary target barrier. Before(g) names the immutable root frozen at destination cut g;
its complete base chain is retained. Root cap includes retained ships, snapshot cuts, staging
and live head: admission reserves space for the proposed root and a future promotion head.
Before a first receive, catalog admission reserves one of its64 indices for promotion and
withholds two free pool pairs (descriptor+empty map table) per rooted chunk with active_index
absent. Every other pool allocation subtracts this deterministic reserve, reconstructed from
the checkpoint/replayed catalogs at open. Promotion alone may consume it in the atomic
Create/Select batch; no timeout releases it. Mutable data space is the already charged base
volume, while each retained ship also has its snapshot charge.
No individual root/snapshot garbage collection is added: aborted staging roots retain their
charge/slot until whole-incarnation deletion. Repeated aborts can exhaust the explicit root cap;
there is no timeout-based reclamation. Whole-volume deletion frees every unique owned cluster/
page once under the existing deletion/tombstone barrier. Existing checkpoint quarantine delays
reuse of freed clusters/page pairs. RootDrop is reserved below, never emitted by this increment.

## Checkpoint and page layouts

With bit1, checkpoint(type1) payload offsets0..39 keep their meanings. Offset40:u64 is the
RootDirectory head ref;48:u64 is its total entry count; bytes56..4059 are zero. Empty directory
is ref0/count0. The checkpoint commits root pages, directory and bitmaps under the existing
write-dirty-pages→flush→checkpoint-header→flush protocol. It never advances the replay start
before the root/receipt pages and allocation bitmaps are covered.

| Page type | Exact payload |
|---|---|
|8 RootDirectory|first_slot:u32,count:u32,next_ref:u64, then count entries(slot:u32,reserved:u32=0,catalog_ref:u64); count1..252; sorted slots without duplicates across the chain; unused bytes zero|
|9 RootCatalog|slot:u32,schema:u16=1,root_count:u16,volume_uuid16,first_epoch:u64,active_index:u16 (65535 absent),reserved6,last_create_ordinal:u64,descriptor_refs[64]:u64; bytes560..4059 zero|
|10 RootDescriptor|slot:u32,index:u16,state:u8,kind:u8,root_uuid16,base_uuid16,ordinal:u64,writer_epoch:u64,relationship16,ship16,relationship_generation:u64,manifest32,map_table_ref:u64,receipt_head_ref:u64,manifest_blob_head_ref:u64,cut_generation:u64,last_mutation_seq:u64,base_index:u16,reserved6,receipt_set_sha25632,manifest_root_ref:u64; bytes216..4059 zero|
|11 RootMapTable|slot:u32,index:u16,reserved:u16,root_uuid16,map_refs[504]:u64; bytes4056..4059 zero|
|12 RootMap|slot:u32,index:u16,reserved:u16,root_uuid16,first_cluster:u32,reserved:u32, then E entries with the v1 phys:u64 + CRC32C per4KiB-block encoding; E=floor((4060-32)/(8+4×cluster_size/4096)); unused bytes zero|
|13 RootReceipts|slot:u32,index:u16,count:u16,root_uuid16,next_ref:u64,first_entry_ordinal:u64, then count64-byte entries; count1..62; remaining bytes zero|
|14 RootBlob|slot:u32,index:u16,kind:u8(1page proof,2manifest root),reserved:u8,root_uuid16,page_index:u32,fragment_index:u16,fragment_count:u16,next_ref:u64,total_bytes:u32,fragment_bytes:u32, then fragment bytes≤4012; remaining bytes zero|

RootDescriptor offsets are fixed by the table's order. Its base_index=65535 iff base UUID is zero.
A catalog descriptor ref is zero iff its index is free; root_count equals the nonzero count.
An active_index must name a Mutable Live root. RootMap E is167/55/15 for16/64/256KiB clusters;
504 map refs cover the1GiB maximum chunk for all supported cluster sizes. first_cluster is a
multiple of E; map pages outside actual chunk bounds refuse. Phys0=UNMAPPED,UINT64_MAX=ZERO,
otherwise physical cluster index+1; checksums of UNMAPPED/ZERO entries must be the canonical
all-zero encoding except ZERO's actual zero-data CRC32C values. Physical data CRCs remain the
v1 read/replay integrity checks, not cryptographic peer authentication.

A receipt entry is unit_index:u64,length:u32,kind:u8(1DATA,2ZERO),reserved3,hash32,
receipt_seq:u64,manifest_entry_ordinal:u64. Entries are sorted unique unit indices;
length is4096-aligned,1..1MiB and equals the canonical manifest entry. receipt_seq is the
target-assigned logical mutation sequence supplied identically to every replica, not its private
journal sequence. Its hash authenticates
the exact logical data/zeros already verified against the admitted manifest. Receipt-set hash
is SHA256 of their concatenated64-byte entries in order (SHA256 of empty bytes when none).
RootDescriptor.manifest_root_ref is zero only while a Mutable Ship root is unbound, or for a
Live root. A bound Ship root points to one kind2 RootBlob containing the complete immutable
ManifestRootV1 (≤842bytes): page_index0,fragment_index0,fragment_count1,next_ref0 and
total_bytes=fragment_bytes=the exact root length. Its SHA256 must equal descriptor.manifest.
Kind1 blob fragments store the canonical manifest page plus its bounded Merkle proof from
wire-contracts.md; total_bytes≤6144 and fragment
count≤2. fragment_index starts0, indices are contiguous and all identity/length fields agree.
Traverse at most the allocated pool-pair count; reject missing fragments/cycles. A replica need
retain only manifest pages covering its chunk, with proof to the global manifest root.

## Mandatory journal records and replay

Use the unchanged record header(type:u16,flags:u16=0,body_len:u32), eight-byte alignment and zero
padding. No root record is optional. `seq` is the target's ordered root mutation sequence in the
writer epoch, persisted in its descriptor; older/conflicting replay requests refuse. On-disk
replay applies committed records in log order and checks the same state transitions as live I/O.

| Type | Exact body, in order |
|---|---|
|9 RootCreate|slot:u32,index:u16,kind:u8,reserved:u8,root_uuid16,base_uuid16,ordinal:u64,epoch:u64,relationship16,ship16,relationship_generation:u64 (96bytes)|
|10 RootMapUpdate|slot:u32,index:u16,flags:u16 (0DATA,1ZERO),root_uuid16,first_cluster:u32,count:u32,csums_per_cluster:u32,reserved:u32,phys_first:u64,epoch:u64,seq:u64,csums[count×csums_per_cluster]:u32|
|11 RootRangeReceipt|slot:u32,index:u16,kind:u8,reserved:u8,root_uuid16,unit_index:u64,length:u32,reserved:u32,hash32,receipt_seq:u64,manifest_entry_ordinal:u64 (88bytes)|
|12 RootSeal|slot:u32,index:u16,kind:u8,reserved:u8,root_uuid16,manifest32,receipt_set_sha25632,cut_generation:u64,epoch:u64,seq:u64 (112bytes)|
|13 RootSelect|slot:u32,index:u16,reserved:u16,root_uuid16,base_uuid16,promotion_or_snapshot_operation16,epoch:u64,seq:u64 (72bytes)|
|14 RootDrop (reserved)|slot:u32,index:u16,reserved:u16,root_uuid16,ordinal:u64,reference_generation:u64,epoch:u64,seq:u64 (56bytes)|
|15 RootManifestFragment|slot:u32,index:u16,kind:u8=1,reserved:u8,root_uuid16,page_index:u32,fragment_index:u16,fragment_count:u16,total_bytes:u32,fragment_offset:u32,fragment_bytes:u32,reserved:u32,epoch:u64,seq:u64,data[fragment_bytes]|
|16 RootBindManifest|slot:u32,index:u16,reserved:u16,root_uuid16,epoch:u64,seq:u64,root_len:u16,reserved6,manifest_id32,ManifestRootV1[root_len] (80+root_len bytes)|

RootCreate initially leaves a Ship root unbound (manifest zero/root ref0), with no range/page
progress acknowledged. First RootBindManifest requires Mutable Ship, matching ticket/session/epoch,
complete canonical root decoding, expected source/base/target incarnation/geometry and exact
SHA256 manifest_id. In one committed journal record it installs the full root in a kind2 blob
and updates descriptor hash/root ref. The normal journal data/page/checkpoint ordering covers
this blob; it is not reconstructed from a hash. A bound Ship root, Mutable or Prepared, accepts
only an identical bytes/hash retry without allocating a blob or changing state; any different binding refuses even before a page/range exists.
Every current ISR must durably acknowledge binding before the core sends any page/range or
reports their progress. A joining copy performs the same bind before copying page/receipt data.
Record15, map changes, range receipt and RootSeal for a Ship root require the durable bound
preimage; page proofs are validated against its Merkle root and geometry before admission.
Replay refuses reordered/unbound page or receipt records rather than accepting their hashes.

Create requires a fresh increasing ordinal and immutable valid base, capacity reservation and
matching current fence. Same live identity/bytes is idempotent; an older absent ordinal is Stale,
never a recreated root. MapUpdate requires Mutable and a valid epoch/session. ZERO has phys_first0
and canonical zero checksums, allocating no data. DATA checks count/physical/checksum overflow
and uses newly allocated full clusters. Freeing replaces only private entries in that Mutable
root, subject to checkpoint quarantine. A receipt is committed in the same batch as all map
changes for its canonical range, after its full DATA write/flush or ZERO verification; duplicate
range+hash returns its existing durable sequence, conflicting content refuses. A new receipt
without its full manifest entry and page proof is invalid. No data ACK precedes this batch's
ordinary all-current-ISR durable completion.

For a Ship root, RootSeal only reasserts the existing binding: its hash must match the stored
preimage, which it cannot create/change. It requires all manifest entries for this chunk have
durable receipts and all submitted
range writes/repairs drained. First/full ship also proves complete dense unit coverage. It
freezes the maps, base and manifest. For an ordinary Live snapshot cut, relationship/ship/manifest
and receipt digest are zero and cut_generation identifies #51's pending snapshot; no DR range
receipt is fabricated. Seal+Create(next Live child)+Select is one atomic local batch. A ship's
assigned destination cut was reserved before prepare and is published only by CommitShip.
RootSelect requires the chosen immutable base and newly created empty Live child, matching
promotion/snapshot operation, current fence and durable committed-point proof from the current
target; no ordinary wire request selects roots. Replay of a selected point must complete the
same operation before exposing a device. RootDrop has a reserved layout only: this profile refuses it and has no emitting API. A future
GC profile needs a distinct incompatibility feature and proven closed-reference protocol. Whole
chunk deletion remains type2, now traversing all owned root state and ordinary layers only under
the lifecycle deletion/tombstone barrier. Before any physical/page reuse, close root admission
and drain all reads/writes/repairs to actual I/O completions, including old-session work; timeout
is not a drain. Each completion checks root UUID/ordinal and captured local chunk-handle
generation. Uncertain I/O preserves allocations. Checkpoint quarantine still precedes reuse.

Takeover reads the bounded full root through GetRootManifest and its relevant persisted pages/
proofs through GetRootManifestPage. It never tries to invert manifest_id. A crash before binding
has no acknowledged page/range progress; the new target may bind from the exact full root in
the durable Incoming record. A bound root recovers from journal/checkpoint without the foreign
sender, including an empty incremental manifest (bound root, zero pages/receipts). Base-first
catch-up fetches and revalidates the full root, pages/proofs, mappings/data and receipts, in
that order, before Prepare/join.

On takeover, reconcile the complete committed+pinned root catalog, manifest proofs, receipts
and data before declaring ready. A joining member receives every committed/pinned root, not
merely the active root; only then may the core emit membership admission. Prepared roots never
reopen on a new epoch. The current-target barrier and O(1) metadata predicate are in contracts.md;
raw journal replay cannot itself publish metadata or decide a newer point.

## Reservation and planned validation

Before admission reserve, per destination replica, worst-case private DATA clusters for one full
logical volume plus all existing retained roots, one RootCatalog, one descriptor/map table per
new root, dense RootMap pages, receipt pages, one complete-manifest-root blob pair and two blob
pages per relevant manifest page.
Checked sizing uses the exact capacities above and fixed manifest limits; page-pool/data
exhaustion refuses before prepare without deleting the prior point. This does not enlarge a v1
page pool or pretend metadata pages fit for free. Reservations survive restart and are charged
through the lifecycle/node-capacity contract; abort retains allocated root/metadata reservations until whole-incarnation deletion; an unused
reservation may be released only if no root or I/O was ever created.

Planned codec vectors include every allocated tag/page with known nonce/sequence/location,
zeros/reserved bytes and CRC; old decoder rejects bit1 before replay, new decoder reads bit0-only
fixtures unchanged. Negative/fuzz cases cover bad feature/tag/length/CRC/padding/UUID/owner/index,
cyclic/missing base or page refs, ordinal replay, physical aliasing and overflow; unbound page/
range/receipt, conflicting bind retry, corrupt/missing manifest preimage and hash-only recovery.
Store crash matrix cuts create, manifest-bind journal/blob/checkpoint, page/receipt, data write,
batch commit, seal, selection, each checkpoint flush and whole-incarnation deletion;
recovered selected bytes and allocation counts must match the clean reference. Model mutants
include mutate-prepared-root, join-without-pinned-root, receipt-before-data, select-staging,
stale-epoch-select and free-referenced-base. RootDrop decoding/dispatch explicitly refuses. Every new safety rule needs a named caught witness.

R2-C2 planned fixtures: empty incremental binds/seals without pages; crash before/after bind
and before seal; receiver restart on another node with the sender disconnected; joining-member
base-first transfer of full root/page proofs and receipts; missing/corrupt preimage refuses
readiness; identical binding retry allocates once, conflicting binding has no effect. A caught
model mutation admitting a range/receipt before immutable manifest binding is required.
