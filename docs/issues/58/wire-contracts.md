# Issue #58 — proposed shared reader, manifest and DR-v1 bytes

Proposed review revision, 2026-11-19; **unapproved**. [Design](design.md) · [root format](root-format.md).
This fixes the shared #51/#52/#58 byte seam in this draft. Their current plans remain untouched;
whichever implementation lands second adopts these bytes/golden vectors rather than guessing.
No implementation, protocol compatibility or global ADR acceptance is claimed.

## Common encoding

Unless a table says otherwise, wire integers are little-endian, UUIDs raw16bytes, SHA256 raw32,
strings u16 byte length followed by exact bytes. Volume/node/snapshot IDs use existing validated
ASCII segment grammar and1..128bytes; trust domains1..253 ASCII; UTF8 free text is never an ID.
No embedded NUL, duplicate/unknown fields, nonzero reserved bytes, trailing bytes or unchecked
integer addition/allocation is accepted. Counts and total frame/body limits are checked first.
Zero UUID/hash is allowed only where explicitly absent. Canonical hash inputs use these bytes,
never protobuf field order, JSON whitespace, local path or node wall timestamps.

## SnapshotRefV1 and SourceProofV1

The complete canonical SnapshotRefV1 is magic8 `FLSREF01`,cluster_uuid16,volume_id:string,
first_epoch:u64,snapshot_name:string,cut_generation:u64,snapshot_operation_uuid16,
size_bytes:u64,chunk_bytes:u64,logical_block_bytes:u32=4096,data_encoding:u8=0,
encryption_descriptor_len:u16=0. IDs/incarnation/generation/geometry come from #51/#52's committed
grant and validated header; cut_generation1..63 and operation UUID nonzero. Size>0 is4KiB-aligned,
chunk_bytes is a supported64MiB..1GiB power of two. This increment refuses encrypted refs.
Maximum encoded ref is339bytes. A later profile changes magic/version; unknown is refused.

SourceProofV1 is magic8 `FLSPRF01`,node_id:string,device_uuid16,store_generation:u64,
chunk_index:u64,isr_version:u64,snapshot_ref_len:u16,snapshot_ref bytes. Maximum length519bytes;
nonzero device/store/ISR identities are established by #52's RegisterSnapshotSource transaction,
never asserted from a peer name. Revalidate current grant/source membership around each bounded
read batch, preserving #52's deletion/close barriers. Store snapshot views retain #51's
Current/Before semantics; these are logical snapshot identities, not invented physical roots.
A paired source remains ordinary snapshot-v2 storage even when its metadata profile is
dr-source-v1; accepted_dr_profile gates capability, not a claim that the source has bit1.

Diff cursor is fixed48bytes: SHA256(`FLDIFF01`||refA||refB||chunk_index:u64),next_cluster:u64,
reserved:u64=0. A full baseline uses refA length0 and all-zero baseline; otherwise refs match
cluster/volume/incarnation/encoding/geometry except allowed growing size and increasing cut.
Diff returns sorted nonoverlapping cluster-aligned candidate extents {offset:u64,length:u64}
within the requested chunk, at most1024/page, with a cursor bound to both refs and chunk.
Every changed4KiB block is covered; false-positive clusters are allowed. #52 repository assembly
continues canonical4MiB blocks. DR assembly uses1MiB units below; both consume this same diff.

## Snapshot-reader/v1 nested schema

Retain #52's separate listener, TLS1.3/backup identity, FLBR grant/source validation and close
barrier. Frames are u32LE body_len plus one protobuf Envelope, body≤1MiB+16KiB. Envelope has
oneof Hello=1,Read=2,Diff=3,Digest=4,Close=5,Accepted=64,Data=65,Ranges=66,Digests=67,Closed=68,
Refused=69. Reject unknown fields/variants, duplicate singular fields/oneof members, bad wire
kinds, nonminimal/overflowed varints and trailing bytes before allocating or calling the store.
All nested messages are exact below; fields not marked repeated occur once unless optional.

| Message | Protobuf fields |
|---|---|
|Hello|uint32 version1=1; string worker_uri2≤512; bytes source_token3≤1128; bytes snapshot_ref4≤339; bytes source_proof5≤519; uint64 request_id6; uint64 grant_generation7; optional bytes base_token8≤1128,base_ref9≤339,base_proof10≤519,uint64 base_generation11|
|Read/Digest|uint64 request_id1; uint64 grant_generation2; bytes ref_sha2563=32; uint32 chunk4; uint64 offset5; uint32 length6|
|Diff|uint64 request_id1; uint64 grant_generation2; bytes ref_sha2563=32; uint32 chunk4; bytes from_ref5≤339 (empty baseline); bytes cursor6=0 or48; uint32 max_extents7(1..1024)|
|Close|uint64 request_id1; uint64 grant_generation2|
|Accepted|uint64 request_id1; uint64 grant_generation2|
|Data|uint64 request_id1; uint64 grant_generation2; uint64 offset3; bytes data4≤1MiB|
|Ranges|uint64 request_id1; uint64 grant_generation2; repeated Extent extents3≤1024; bytes next_cursor4=0 or48|
|Extent|uint64 offset1; uint64 length2|
|Digests|uint64 request_id1; uint64 grant_generation2; uint64 offset3; repeated fixed32 crc32c4, one per requested4KiB block,≤256|
|Closed|uint64 request_id1; uint64 grant_generation2|
|Refused|enum reason1(RETRY=1,CLOSED=2); uint64 request_id2; string diagnostic3≤256 UTF8|

Requests require nonzero IDs/generations, matching admitted target ref hash and chunk; Read/
Digest stay nonempty4KiB-aligned and within that chunk/volume. The optional base Hello group is
all present or all absent. It is required for nonbaseline Diff; base and target grants are
independently authorized/registered/validated under #52, same worker/node/chunk and incarnation.
The source listener validates both before Accepted, using the existing bounded metadata client;
either grant's close barrier closes the session. A baseline Diff needs only the target grant.
No extra target/write capability is created, and query IDs cannot reopen a closed grant.

FLBR-v1 token bytes are `FLBR`4,version:u8=1,grant_operation16,grant_generation:u64,
worker_uri:string,source_proof_len:u16,SourceProofV1,signature64; integer/string lengths LE,
Ed25519 over every preceding byte. Max1128bytes. The proof includes the exact SnapshotRef;
peer worker URI, token worker, grant source and ref must all agree. No expiration is a safety
claim. The local token key verifies it; ordinary target token decoders reject magic FLBR.
Worker/retry/close/pin outcome authorization remains #52 plus the explicit DR outcome in
contracts.md; this wire schema does not permit the foreign shipper identity on the reader.

## Canonical DR manifest

ManifestRootV1 is `FLDRMR01`8,base_ref_len:u16,base_ref bytes (empty only first/full ship),
target_ref_len:u16,target_ref,unit_bytes:u32=1048576,total_entries:u64,page_count:u32,
full:u8,reserved3=0,merkle_root32. Max842bytes. Its SHA256 is `manifest_id`. DR requires base/
target source incarnation, encoding, block/chunk sizes and size_bytes equal; snapshot cuts
increase. full=1 iff base absent. Logical unit count=ceil(size_bytes/1MiB)≤2^32; checked volume
and existing snapshot/capacity bounds still apply. Entries refer to unit indices, whose offsets
are unit_index×1MiB; only the final unit may be short and remains4KiB-aligned.

A full manifest covers all units exactly once. An incremental manifest covers the sorted unique
union of1MiB indices intersecting shared Diff candidates for every chunk. Read exact selected
snapshot bytes through authorized proofs; classify an all-zero unit as ZERO, otherwise DATA.
Both hash SHA256 of exact length bytes; there is no compressed/ciphertext interpretation here.
A clean overinclusive diff is allowed; omitted candidate units, overlapping/duplicate/out-of-range
entries, source lifetime change or unresolved read fails manifest construction. No manifest is
assembled by reading Current after the selected cut. Source and destination stream pages; they
never allocate a volume-sized list from page_count/entry_count.

Each canonical page is exactly4096bytes: offsets0..7=`FLDRMP01`;8:u32 page_index;
12:u16 entry_count(1..60);14:u16 reserved0;16:u64 first_entry_ordinal=page_index×60;
24:u64 total_entries;32..127 zero;128..3967 sixty64-byte entry slots;3968..4091 zero;
4092:u32 CRC32C of0..4091. Last page has the remaining count; unused entry slots are all zero.
Entry offsets:0:u64 unit_index;8:u32 length;12:u8 kind(1DATA,2ZERO);13..15 zero;16..47 hash32;
48..63 zero. Entries increase strictly across page boundaries. page_count=ceil(total_entries/60),
and total_entries≤logical_units. An incremental unchanged snapshot has entries/pages0; its
Merkle root is SHA256 of the six ASCII bytes `FLDRM0`. Full empty coverage is never valid.

For P>0, let L be the next power of two≥P. Leaf i<P is SHA256(0x00||page4096); padding leaf
i≥P is SHA256(0x02||i:u32LE). Parent=SHA256(0x01||left32||right32), continuing to one root.
A page response is page_index:u32,page4096,sibling_count:u8,siblings[ count ]32; count must equal
log2(L)≤32. At level j, bit j of page_index chooses left/right. Verify the path against the
root before installing or acknowledging the page. Max page response5125bytes. Checked page/
root hashing includes geometry/source/base identities through ManifestRootV1; hashes alone
never grant authority. A replica retains relevant pages/proofs durably and proves its exact
chunk's entry coverage before RootSeal; first/full also checks its dense logical unit interval.
A malicious source can choose bytes for its own authorized snapshot; this is not a proof of
source software correctness or application consistency beyond #51's cut.

## Root-qualified local replica sessions

Use the existing replication framing `[u32le body_len][u8 tag][body]`, max1MiB+16KiB, with
explicit hello version3. Allocate request tags0x20..0x31 and replies0x4A..0x50; #51 owns
0x10..0x19/0x48/0x49, current baseline ends0x46 and #13 owns0x47. No tag is reinterpreted in
legacy or v2 sessions. New/old decoders must refuse before core/store action.

Every new request with tag0x21..0x31 begins `req:u64,epoch:u64,session:u64`; responses repeat req/session.
They use the local cluster's mutual TLS node/io identity, never a foreign role. A HelloRoot
carries version:u16=3,mode:u8(1Receiver,2Promoter,3Live),volume:string,first_epoch:u64,
node:string,epoch:u64,session:u64,size:u64,chunk_bytes:u64,token_len:u16,token bytes,
chunk_count:u16,chunks[ count ]:u32. Token≤4096, chunk_count1..4096, ascending unique bounds;
mode1/2 accepts only exact FLDT claims and durable specialized-mode admission; mode3 only
ordinary FLCT with ready Promoted/dr-roots-v1 metadata. Live mode still checks #50/#51 accepted
profiles as applicable. Local replicas verify ticket/Hello/peer node/volume/incarnation/epoch;
old ordinary Hello cannot mutate a DR-root slot. A promoter ticket binds the current local
Promoting operation while the selected root retains its original historical ship/generation;
a revoked/offline pair does not prevent that locally authorized selection. Modes1/2 cannot issue ordinary live writes,
reads, maintenance, ublk or root selection without the corresponding core operation.

| Tag | Body after common request prefix |
|---|---|
|0x20 HelloRoot|the HelloRoot body above, without an additional common prefix; req:u64 precedes version|
|0x21 CreateRoot|chunk:u32,root_uuid16,base_uuid16,kind:u8,ordinal:u64,relationship16,ship16,relationship_generation:u64|
|0x22 PutRootRange|chunk:u32,root_uuid16,unit_index:u64,length:u32,kind:u8,hash32,mutation_seq:u64,data[length] for DATA, absent for ZERO|
|0x23 PutRootManifestPage|chunk:u32,root_uuid16,page_response_len:u32,page response bytes≤5125|
|0x24 PrepareRoot|chunk:u32,root_uuid16,manifest_root_len:u16,ManifestRootV1,cut_generation:u64,mutation_seq:u64|
|0x25 ProbeRoot|chunk:u32,root_uuid16,after_unit:u64,limit:u16(1..1024); UINT64_MAX starts at first|
|0x26 CopyRootRange|chunk:u32,root_uuid16,unit_index:u64,length:u32,kind:u8,hash32,receipt_sequence:u64,manifest_entry_ordinal:u64,data[length] for DATA|
|0x27 RootState|chunk:u32,after_ordinal:u64,limit:u16(1..64)|
|0x28 SelectRoot|chunk:u32,selected_prepared_root16,new_live_root16,new_ordinal:u64,operation16,mutation_seq:u64|
|0x29 DropRoot|chunk:u32,root_uuid16,ordinal:u64,reference_generation:u64,mutation_seq:u64|
|0x2A LiveRootWrite|chunk:u32,active_root16,offset:u64,length:u32,mutation_seq:u64,data[length]|
|0x2B BeginRootCatchup|chunk:u32,root_uuid16,stage:u64|
|0x2C ReadRootRange|chunk:u32,root_uuid16,offset:u64,length:u32|
|0x2D MappedRootRange|chunk:u32,root_uuid16,offset:u64,length:u32|
|0x2E DigestRootRange|chunk:u32,root_uuid16,offset:u64,length:u32|
|0x2F BindRootManifest|chunk:u32,root_uuid16,manifest_root_len:u16,ManifestRootV1 (1..842bytes),mutation_seq:u64|
|0x30 GetRootManifest|chunk:u32,root_uuid16|
|0x31 GetRootManifestPage|chunk:u32,root_uuid16,page_index:u32|
|0x4A RootAck|req:u64,session:u64,chunk:u32,root_uuid16,unit_index:u64,length:u32,kind:u8,hash32,receipt_sequence:u64|
|0x4B RootReceipts|req:u64,session:u64,chunk:u32,root_uuid16,count:u16,entries[count]64,next_after_unit:u64; UINT64_MAX means end|
|0x4C RootStates|req:u64,session:u64,chunk:u32,count:u16,entries[count]: (root_uuid16,base_uuid16,ordinal:u64,state:u8,kind:u8,writer_epoch:u64,relationship16,ship16,relationship_generation:u64,cut:u64,manifest32,receipt_hash32)|
|0x4D RootPrepared|req:u64,session:u64,chunk:u32,root_uuid16,cut:u64,manifest32,receipt_hash32|
|0x4E RootMappings|req:u64,session:u64,chunk:u32,root_uuid16,count:u16,entries[count]: (offset:u64,length:u32,kind:u8(1DATA,2ZERO))|
|0x4F RootManifest|req:u64,session:u64,chunk:u32,root_uuid16,status:u8(0Unbound,1Bound),manifest_id32,root_len:u16,ManifestRootV1 bytes≤842|
|0x50 RootManifestPage|req:u64,session:u64,chunk:u32,root_uuid16,page_index:u32,status:u8(0Missing,1Present),page_response_len:u16,page response bytes≤5125|

CreateRoot allocates an unbound Ship root. BindRootManifest must commit the full immutable
preimage on all current ISR before PutRootManifestPage/PutRootRange/receipt admission. Its
RootManifest reply is sent only after the local RootBindManifest record is durable; same bytes
retry returns the stored binding, different bytes refuse. PrepareRoot must repeat that exact
binding, never install it late. Live roots require no external DR manifest.

GetRootManifest/GetRootManifestPage are local typed recovery/catch-up reads, never foreign data
access. Bound/Present replies carry the exact stored bytes and request/session/root correlation;
Unbound has root_len0/hash zero, Missing has page_response_len0. Neither implies zero data or
complete progress. Receiver verifies SHA256 root identity, full canonical refs/geometry and
every page Merkle proof before copying it, then copies receipts/data only under that binding.
The core reconciles bases first and excludes a member missing required Ship-root/page
preimages from join; Live roots use #51 snapshot semantics without an external DR manifest.
A new epoch/restart can obtain all bound metadata without contacting the source sender.

Range offsets derive from unit index and checked geometry, lengths1..1MiB and aligned4KiB.
RootAck is local durable evidence, not an all-ISR or committed-point assertion. Requests outside
a session's mode/operation/root set refuse and close before store action. Existing Heartbeat/
Settle/fence-response correlation is retained in v3; ordinary read/digest/mapped v2 requests
remain available only in mode3 and are resolved through the selected live root, including #51
snapshot views. LiveRootWrite is mode3 only; root create/prepare/copy are typed target
operations; DropRoot is reserved and refused in this increment, not foreign passthroughs. CopyRootRange is only outside-ISR catch-up after current
session fencing; it never manufactures a manifest membership proof or complete receipt from
one unverified peer. RootState/probe replies page under the same bounds. Snapshot cut uses the
current target's root Seal/Create/Select barrier, not an unversioned ordinary LayerCreate.
PrepareRoot allows manifest length0 only for a mode3 Live snapshot cut. ReadRootRange/Digest
return existing Data/Digests with captured request/root correlation; mapped replies describe only
private overlay extents, never physical addresses, at most1024 sorted extents. Reads are nonempty
4KiB-aligned and≤1MiB within one root/chunk. Catch-up enumerates bases before children, copies
private mappings/zeros plus manifests/receipts, then checks the advertised state/receipt digest
before preparing the destination copy. Logical bytes of an already Prepared root cannot change;
a corrupt Prepared root fails closed rather than accepting a logical rewrite. A current target
may rebuild an outside-ISR chunk from a proven complete authoritative source through ordinary
fenced whole-chunk deletion/recreation, with actual old-I/O drain; otherwise recovery blocks. BeginRootCatchup resets the #51
copy filter keyed by (root UUID,stage) only under an epoch/session-valid outside-ISR copy. Live
forwarding marks that same root's blocks; a delayed copy never overwrites newer forwarded data.
No replica joins before the complete committed+pinned root set and filters are verified.

## Dedicated foreign DR-v1 protocol

Only the configured peer uses this TLS1.3 listener, with session resumption disabled. The client
uses SNI `p-<receiver-local-pair-uuid-hex>.dr`; the server selects only that pair's isolated CA
configuration before certificate validation. Hello's pair must match SNI. Peer chain, exact
configured trust domain, source cluster and shipper role all match together. The client verifies
`spiffe://<destination-td>/flaminio/dr-receiver/<destination-cluster-uuid-hex>` against that pair's
CA, never local metadata/replica roots. Eligible receiver nodes have distinct leaf keys for this
same cluster-service role. A configured stable endpoint can fail over without changing the peer
pair generation. The O(1) admission/pair CAS predicates and finite global/per-pair resource limits
are in lifecycle.md; cached TLS configuration never substitutes for fresh publication authority.

Frame is `body_len:u32,version:u16=1,kind:u16,request_id:u64,body`, total header16bytes;
body_len counts bytes after its four-byte prefix and is12..1MiB+4096. Read the fixed12-byte inner
header before allocating the bounded body. Request IDs strictly increase within a connection,
never wrap/reuse; at most8 outstanding. Replies echo request_id. Reconnect starts new request IDs;
durable retry identity is ship/manifest/unit/hash, never a connection ID. Unknown version/kind,
wrong exact length, scope mismatch, malformed identity or limits refuse/close before mutation.

Hello body `ScopeV1` is pair_uuid16,pair_generation:u64,relationship16,
relationship_generation:u64,ship16,source_cluster16,source_volume:string,source_first_epoch:u64,
destination_cluster16,destination_volume:string,destination_first_epoch:u64 (max372bytes).
These are the receiver-local pair and exact source/destination incarnations. The certificate is
the identity; self-asserted Hello fields supply no authority. `scope_hash=SHA256(ScopeV1)` binds
every subsequent body to this admitted connection. Each such body begins scope_hash32.

| Kind | Exact body after scope_hash, except Hello |
|---|---|
|1 Hello|ScopeV1 only, no scope_hash prefix|
|2 Begin|manifest_root_len:u16,ManifestRootV1|
|3 ManifestPage|page_response_len:u32,page response≤5125|
|4 PutRange|manifest_entry_ordinal:u64,unit_index:u64,length:u32,hash32,data[length]|
|5 PutZero|manifest_entry_ordinal:u64,unit_index:u64,length:u32,hash32|
|6 Probe|namespace:u8(1manifest pages,2ranges),after_index:u64,limit:u16(1..1024); UINT64_MAX starts first|
|7 Finish|manifest_id32|
|8 Status|expected_manifest32 (zero only before Begin)|
|64 Accepted|phase:u8(1Hello,2Receiving),receiver_node:string,receiver_epoch:u64,destination_cut:u64,manifest_id32; phase1 has epoch/cut/hash0|
|65 Ack|subject:u8(1manifest page,2range),index:u64,length:u32,kind:u8(0page,1DATA,2ZERO),hash32,receiver_epoch:u64,receipt_seq:u64|
|66 Progress|namespace:u8,count:u16,entries[count]: (index:u64,length:u32,kind:u8,hash32,receipt_seq:u64),next_after_index:u64; UINT64_MAX end|
|67 Committed|completion_len:u16,CompletionV1 bytes≤509|
|68 Refused|code:u16,reason_len:u16,reason UTF8≤256|
|69 State|state:u8(1Receiving,2Prepared,3Committed,4Cancelled,5Promoting,6Promoted,7Revoked),manifest_id32,destination_cut:u64,receiver_epoch:u64,verified_bytes:u64,completion_len:u16,CompletionV1 bytes (empty except Committed)|

Begin calls destination admission and reserves the assigned cut/capacity before accepting pages
or data. Its full canonical root is persisted in destination Incoming and bound durably on
local replicas before page/range progress; metadata retains more than its digest/page count.
Repeated exact Begin returns the same operation; any different scope/root/geometry/base
conflicts. Only verified manifest entries are writable. A page Ack or range Ack means durable on
all current ISR through the local receiver core, not a mere socket receive or one replica's
receipt. For ranges, receipt_seq is the target-assigned logical range mutation sequence persisted
identically by replicas; it is not a node's private journal offset/batch sequence. Receiver epoch
is diagnostic correlation, not permission to reuse old tickets. Probe reports only content-
verified ranges known durable on current ISR; after takeover/reconnect it reconciles receipts
and performs missing replication before advertising them. No standby data is returned.

Progress bounds include all entries/header in the same frame/buffer budget. Page entries use
kind0,length4096 and SHA256(page). Range length/hash exactly match the admitted manifest. Finish
can return Committed only after authenticated current-target publication; missing data/proof
returns Incomplete and preserves pins/staging. Status returns State or Committed for the exact
authenticated pair/source/relationship/ship, never a volume-wide lookup. Cancelled is terminal for that ship and cannot reopen its roots.
Destination cut comes from the monotonically reserved #51 snapshot_next_ordinal, not last+1
after a cancellation; gaps are preserved. Refused codes are
1Unsupported,2Denied,3Conflict,4Retry,5Invalid,6Corrupt,7Exhausted,8Incomplete,9Closed; unknown is
terminal. Diagnostics are sanitized and contain no token/certificate/payload. No foreign Cancel
pretends to undo uncertain publication; source cancellation/reader close is the local lifecycle
operation, with remote status resolving the next base before another ship.

## Destination completion receipt

CompletionV1 is signed by the destination metadata service's existing Ed25519 token key, under
a distinct domain. Pair exchange explicitly pins its public key32 separately from the peer CA;
no private key/local capability crosses clusters. Exact bytes are big-endian (as local signed
capability encodings), in this order:

`FLDRCOM1`8,version:u8=1,issuer_cluster16,pair16,pair_generation:u64,relationship16,
relationship_generation:u64,ship16,manifest32,source_cluster16,source_volume:u16len+ASCII,
source_first_epoch:u64,destination_volume:u16len+ASCII,destination_first_epoch:u64,
source_snapshot_cut:u64,destination_snapshot_cut:u64,lineage16,signature64.

Signature covers every preceding byte. Maximum509bytes; both volume IDs≤128. The destination
stores the exact deterministic receipt body in its committed incoming record and signs only
that post-CAS record; lost responses return the same bytes/signature. Source service, not an
untrusted worker report, verifies pinned receipt key, issuer, complete outgoing destination/
source/pair/relationship/ship/manifest/cut tuple and current unrevoked local pair generation
before recording completion and initiating grant close. A local worker cannot invent successful
completion, and upload/range/page ACKs never authorize pin release. Key/pair changes that make
an old receipt unverifiable preserve pins until explicit safe local cancellation/close.

## Golden vectors and planned negative checks

Implementation adds paired Go/Rust fixtures under api/testdata/dr-v1/ and format testdata,
generated from exactly these descriptions: maximal/empty refs; base+target Diff and cursor;
reader Hello with one/two grants and every reply variant; one/full/partial/empty manifest and
Merkle proof; every root/foreign frame; maximal FLDT (defined in contracts.md) and completion.
Golden tests assert exact bytes/lengths and hash/signature vectors. A one-entry ZERO manifest's
hash is computed from4096 zero bytes by the same SHA256 implementation in both languages;
fixtures carry the generated literal digest rather than two independently invented constants.

Decoder/fuzz cases include unknown/duplicate/nonminimal protobuf fields, truncated/oversized
strings/counts, extra bytes, alignment/overflow, wrong incarnation/worker/device/grant/pair,
manifest page reordering/duplicate unit/missing full coverage/bad Merkle sibling, tampered DATA/
ZERO/hash, lost ACK/resume, empty incremental manifest, bind/page/receipt crash-before-seal,
different-node takeover/base-first joining without the sender, wrong receipt key/tuple/cut and
old/new cross-decode. Socket-fragment
fixtures split every header/body boundary; complete bounded reads/writes must not assume a byte
stream read returns one frame. Liveness/cap exhaustion preserves the last point and pins;
no timeout or clock assumption becomes fencing/publication authority. All checks here are
planned, not executed runtime evidence.
