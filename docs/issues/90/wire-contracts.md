# Issue #90: composed identity replication profile

Proposed, unapproved; normative [contracts](contracts.md) appendix. #51's snapshot version2
and #58's root version3 are distinct existing draft profiles. Identity does not insert bytes
into either. The last implementation to land adopts the composed version4 below, updating the
#51/#58 golden vectors in that same change. #72 supplies no wire or ReplicaRef definition.

## Common bytes, negotiation and dispatch

Mutual TLS1.3 remains mandatory. Require exact ALPN `flaminio-repl/4`; an absent/different
negotiation closes before a Hello/body/core/store action. No legacy fallback after identity
activation. Frame is `[u32le body_length][u8 tag][body]`; body_length includes tag, excludes its
four-byte prefix. Integers are little-endian; `string`/`bytes` means u16 byte length then exact
bytes. UUID is raw16 network UUID bytes. IDs are validated ASCII1..128 bytes. Arithmetic, full
length/count checks and fallible bounded allocation precede decoding/allocation/effects.
No extra/trailing bytes, zero required identities, duplicate/out-of-range chunk IDs or unknown
profile/mode/version is accepted. Signed tokens keep their original big-endian canonical bytes.

| Tag | Exact body after tag |
|---|---|
| `0x32 HelloIdentity` | version:u16=4,profile:u8,mode:u8,req:u64,epoch:u64,session:u64,first_epoch:u64,volume:string,receiver_node:string,expected_device_uuid16,expected_store_generation:u64,token:bytes,size:u64,chunk_bytes:u64,chunk_count:u32,chunks[count]:u32 |
| `0x51 FencedIdentity` | req:u64,actual_device_uuid16,actual_store_generation:u64,epoch:u64,session:u64,row_count:u32,rows[count]:(chunk:u32,extent_count:u32,extents[count]:(start:u32,len:u32)) |

Profiles are0 `identity-ordinary-v4`,1 `identity-snapshot-v4`,2 `identity-roots-v4`,
3 `identity-legacy-ordinary-v4`,4 `identity-legacy-snapshot-v4`. Mode must
be0 for ordinary/snapshot and both legacy profiles; roots uses1 Receiver,2 Promoter,3 Live, exactly #58's specialized
mode semantics. `receiver_node` must equal the authenticated local receiver IO node and the
grant's exact receiver ref; the signed token's target is instead checked against the peer
target IO identity. FLCT does not claim a receiver node. Epoch/session/G are nonzero. Profiles0/1/2 require positive first_epoch matching metadata and
FLCTv2/FLDT. Profiles3/4 require first_epoch0 **and** signed FLCTv3's nonzero legacy_incarnation
matching the proven header, grant and SessionRef; they never use an inferred empty incarnation.
FLCTv3 is the existing big-endian FLCT layout with version:u8=3, volume:u16len+bytes,epoch:u64,
target_node:u16len+bytes,issued_at:i64,first_epoch:u64=0,legacy_incarnation:rawUUID16,signature64
(Ed25519 over all preceding bytes including magic `FLCT` and version3). Max623 bytes under the
existing255-byte token string limits; metadata IDs remain<=128. Claims expose that exact UUID;
v3 with positive first_epoch, v2 with zero first_epoch, zero/missing LID and v1 all refuse.

Ordinary positive-incarnation token remains FLCTv2(max607); root Receiver/Promoter uses #58
FLDT(max4096), root Live FLCTv2. The mandatory Hello first_epoch plus signed LID (legacy only)
compose #79 initialization/use authority without permitting a token alone to grant INITIAL.
Legacy profiles exclude INITIAL, roots and #52/#58 positive-incarnation source/ship operations.
They preserve original fences/data/snapshot ordinals: no DeleteEarlierIncarnation is issued
for first_epoch0, no invented fence rebase or unsigned legacy UUID is accepted. #79 still checks
use/initialization before token issuance; legacy metadata/bootstrap semantics are defined by
[core-design](core-design.md) and [migration](migration.md). Strict local target/device/CSI
composition with #15/#79/#83 is [local-target-contract.md](local-target-contract.md): unchanged
FLCH134 hashes the exact FLCTv3 (including signed LID), and paged Attach carries nonce/handoff.
No new Hello layout/tag is required:
the signature binds LID in its token and bootstrap separately compares that LID with metadata.

| Profile | Permitted payloads after authorized/durable-fenced Hello |
|---|---|
| Ordinary0 | existing Heartbeat2,Write3,Read4,BeginResync5,Copy6,Settle7,Digest8,Mapped9,Discard10; ordinary replies0x41..0x47 with their existing bodies |
| Legacy3/4 | profile3 uses Ordinary0 payloads; profile4 uses Snapshot1 payloads; no INITIAL or root operation |
| Snapshot1 | #51 requests0x11..0x19, replies0x48 Layers/0x49 Changes; unchanged Heartbeat2/Settle7/BeginResync5 only as the exact granted catch-up operation allows; ordinary generic replies0x41..0x47 retained |
| Roots2 | #58 requests0x21..0x31 and replies0x4A..0x50 with their exact version3 bodies/common req+epoch+session prefix; Heartbeat/Settle and generic replies retained; #51 view requests0x13..0x19 and replies0x48..0x49 only in Live3 against its selected live root |

`0x01 Hello`, `0x10 HelloV2`, `0x20 HelloRoot` and `0x40 Fenced` are forbidden in all version4
sessions. They never alias the new messages. Snapshot1/legacy4 refuse ordinary unversioned data/view encodings;
Ordinary0 refuses snapshot/root frames; Root Receiver/Promoter refuses ordinary data access and
ungranted operations. There is no interpretation selected by body length. Roots2 Live writes
use #58 LiveRootWrite, not legacy Write. The #58 foreign DR and #52 snapshot-reader listeners,
ALPN/credentials/frames remain separate and cannot use this local replica listener.

A connection captures profile, exact receiver ref/G, volume/first_epoch, epoch/session and the
immutable accepted per-chunk role/operation/frontier grant. Every creation/reconstruction frame
checks that grant before mutation. New chunks/roles/operations require a new Hello/session,
as [core-design](core-design.md) defines. Other replies need no repeated UUID: the local adapter,
remote connection and outstanding request retain the captured full tuple; delayed replies cannot
be routed through a current node-name cache. Local synthetic Hello/Fenced/fast-read admission
and completion use the identical tuple validation and core transitions.

## Exact bounds and refusal

Ordinary/snapshot body MAX is1,048,704 bytes (#80/#86); root body MAX is1,064,960 bytes (#58).
Checked Hello body size is `87 + volume_bytes + receiver_bytes + token_bytes + 4Q`.
Q<=65,536 ordinary/snapshot/legacy and Q<=4096 roots, ascending unique within volume geometry; token
<=4096 in every profile and also its stricter token-format limit. Worst ordinary Hello with
both128-byte IDs, max4096 ticket allowance and Q=65,536 is266,583 bytes (ordinary FLCT is
actually smaller). Worst root Hello is20,823. Prefix adds4. No encoder assertions or partial
socket writes on invalid input. Root-specific payload/count bounds remain #58's stricter ones.

Fenced rows include every held chunk, including explicit clean rows and retained/source extras,
while required held-member rows follow #13/#86 plus #90 role/NotHeld refusal. An unknown MEMBER
is refused, never manufactured blank with a clean row. Row IDs are increasing/unique; extents
are checked, nonempty, disjoint/increasing, within that chunk; total extents T<=65,536 and
rows N<=65,536. Body size is exactly `53 + 8N + 8T`; N=T=65,536 gives1,048,629 bytes,75 bytes
below ordinary MAX. The uncertainty algorithm remains #86's single bounded arena; identity does
not drop extents or claim uncertainty vanished to fit. Validate all fields and required rows
before opening the session, lease extension, first-fence installation or accepting any ack.
A Stale reply still deposes before malformed payloads could hide a higher authoritative epoch.

## Literal shared codec vectors and planned tests

These are complete frames including length prefix, using volume `v`, receiver `n`,
req1/epoch2/session3/first_epoch1, UUID00000000-0000-0000-0000-000000000001, G1,
size=chunk_bytes=1MiB, chunk0. Token `00` is deliberately an opaque codec fixture: it round-trips
in the codec and is refused by authentication; it is never an authorized store-effect fixture.

```text
ordinary0/mode0: 5e0000003204000000010000000000000002000000000000000300000000000000010000000000000001007601006e000000000000000000000000000000010100000000000000010000000010000000000000001000000000000100000000000000
snapshot1/mode0: 5e0000003204000100010000000000000002000000000000000300000000000000010000000000000001007601006e000000000000000000000000000000010100000000000000010000000010000000000000001000000000000100000000000000
roots2/mode3: 5e0000003204000203010000000000000002000000000000000300000000000000010000000000000001007601006e000000000000000000000000000000010100000000000000010000000010000000000000001000000000000100000000000000
fenced, one clean chunk0: 3d00000051010000000000000000000000000000000000000000000001010000000000000002000000000000000300000000000000010000000000000000000000
```

Planned Go/Rust shared `api/testdata/replica-identity-v4.json` contains these literal bytes,
maximal lengths for every profile, all feature payload variants using #51/#58 canonical bytes,
runtime-generated signed FLCT/FLDT counterparts from fixed test seeds and full/refused bootstrap adapter outcomes. Assert
raw UUID order, exact size and prefix inclusion; verify an old/v2/v3 decoder returns controlled
UnknownType/refusal for0x32/0x51 and a v4 decoder refuses all legacy Hello/Fenced variants.
Wrong profile/ALPN, absent/wrong UUID/G/first_epoch/LID, FLCTv1/v2/v3 cross-use, changed chunk-set or role, duplicate rows,
N/T/body boundary+1, arithmetic overflow, truncated/count-lie/trailing bytes and session
supersession are negative/fuzz regressions. Max N=T reports round-trip through production
checked send/decode and target validation with no panic, silent extent loss or raised limits.
`make contracts test lint` owns bytes/interop, `make sim` owns the separate safety/mutant proof.
Legacy codec counterparts set profile3/4 and first_epoch0 in the same Hello byte positions;
runtime-generated FLCTv3 fixtures use fixed test signing seeds and literal signed-body bytes,
checking all16 UUID bytes, big-endian0 first_epoch, exact length623 maximum and signature.
They exercise old decoders' version refusal and v4 local/remote accepted legacy bootstrap
without any DeleteEarlierIncarnation call or loss of acknowledged data/fences/history.
No runtime test or new globally accepted protocol/ADR is claimed by this draft.
