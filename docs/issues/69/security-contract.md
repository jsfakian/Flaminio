# Issue #69: status peer and allocation boundaries

Proposed, unapproved appendix to [design](design.md). These post-approval corrections address
SEC69-R1-01/02; the historical 2026-11-13 approval does not approve them. They specify future
implementation and fixtures, not executed TLS, runtime or allocation evidence.

## Exact metadata peer (S1, S6)

For status, require existing global `--server host:port`, `--cert`, `--key`, `--ca` and
`--trust-domain`, plus new global `--metadata-node <node-id>`. The endpoint must be one explicit
DNS/IP host and port, with no discovery/resolver-selected peer. Validate the node with the
existing 1..128-byte ID grammar and require a nonempty configured trust domain. The expected
URI is exactly `spiffe://<trust-domain>/flaminio/node/<metadata-node>/meta`; neither the server
nor a response supplies this expectation. One invocation uses one endpoint/ClientConn. A
future multi-endpoint option must configure a finite endpoint-to-exact-URI mapping first.

Parse `status [--json] [--timeout <duration>]` before dialing. Timeout defaults to30s and
must satisfy `0 < timeout <= 30s`; it covers dialing, collection, derivation and encoding.
Validate a complete matching client certificate/key/CA set and exactly one Reader URI in the
configured trust domain before creating the connection. Missing/invalid identity or configuration
fails with exit1 and empty stdout; no administrator credential, insecure transport or plaintext
fallback is permitted. The existing CLI's generic no-certificate plaintext branch is unreachable
for status. Certificates/keys/tokens never enter the View or diagnostics.

Build on `pki.Files.ClientConfig`, retaining TLS1.3, configured RootCAs and standard endpoint
hostname/IP, validity, chain and ServerAuth EKU verification with `InsecureSkipVerify=false`.
Set TLS ServerName from the validated endpoint host. Install `tls.Config.VerifyConnection`
before `credentials.NewTLS`/dial. On every handshake, including resumed connections, call
`pki.PeerIdentity(state, trustDomain)` to require a verified chain and exactly one URI SAN;
then require Node==configured metadata-node, Component==`meta`, Admin==empty and Reader==empty,
and the canonical URI equal to the expected string. A shared PKI helper may accept this
caller-specific predicate; generic same-domain identity parsing alone is insufficient.
Any mismatch fails the handshake before accepting an RPC result. There is no response-based
trust check, arbitrary same-CA role acceptance or temporary bypass during rotation.

## Stock transport and finite application admission (S5)

Use the pinned stock grpc1.85.0/protobuf1.36.12. Keep full-message receive limit4,194,304 bytes,
set identity-page receive limit524,288 and identity-tombstone inspection receive limit32,768;
retain4MiB for the separately validated legacy tombstone adapter. Set received header/trailer
list limit `grpc.WithMaxHeaderListSize(4096)` (name+value+32 bytes per field). Set request send
limit65,536. Use one ClientConn, at most four RPCs in flight, a fixed four-worker collection
loop, no application retries and `grpc.WithDisableRetry()` for service-config retries. Allow
at most32,768 application RPC invocations, including page and failed calls; stock transparent
transport retries remain library behavior within those calls/deadline. The 30s total deadline
and4096 node/4096 volume-ID limits remain in force.
Page/cursor loops also obey [view-contract](view-contract.md) and
[tombstone inspection](tombstone-inspection-contract.md); no successful truncation is allowed.

One status invocation owns a64MiB (`67,108,864`) application credit ledger shared by all four
calls, retained collection/View data, derivation indexes/sort scratch and final encoded output.
These are conservative application/work charges, not a cap or proof for the Go heap, RSS,
stock gRPC buffers/caches/HTTP2 state, compression or runtime/allocator metadata. Use the normal
library receive/header bounds; no fork, private transport-memory ownership scheme, daemon
containment or guard restart lifecycle is part of this command.

Before invoking each RPC atomically reserve `R + 1,048,576 + 65,536` credits: R is that call's
receive limit, the next term is its fixed scan-plan capacity, and the last term bounds its
encoded request. Validate/charge the small request before Marshal. Failure to reserve returns
ResourceExhausted before invoking it. Queued work retains only already charged IDs/cursors;
it must not allocate one goroutine or closure per volume/page. Never wait beyond the common
deadline for credit, and never exceed four active reservations.

## Status-local preflight before typed construction

Implement `control/status/boundedproto.go` through per-call `grpc.ForceCodecV2`, Name=`proto`.
Its Unmarshal accepts only ListNodesResponse, strict ListVolumesResponse, GetVolumeResponse,
ReadIdentityVolumePageResponse and ListTombstonesResponse, under an explicitly selected installed
legacy/identity profile. Strict ListVolumes sends require_complete20=true and empty
candidate_metadata_id21; accept only complete20=true and the installed mode21/revision22 pair,
without requesting guard candidate history. Identity inspection sends inspect_node_history24=true
and requires node_history_inspected22=true, plus complete21/next_cursor20; legacy node-only
responses cannot satisfy this profile. Scan the supplied CodecV2 BufferSlice fragments before
materializing typed children or copying the entire input. Other destinations/unknown fields
refuse. The last implementation to land may share the bounded scanner/charged decoder from
#91's [reader contract](../91/reader-contract.md#cardinality-checks-before-application-construction),
adding these status destinations/profiles; #91 remains the sole owner of its strict-list/page
authorization extension. Sharing does not import its guard-only NotFound or process lifecycle.

Pass1 uses a fixed depth32 stack and at most1MiB pre-reserved flat allocation plan. Check all
varints, wire types and lengths with overflow-safe arithmetic. Reject groups, duplicate singular
fields, duplicate map keys, unsupported schema/profile fields, more than262,144 total field
occurrences or32,768 nested message/map entries. Count empty repeated messages, map envelopes
and every packed element before allocating children. Invalid UTF-8 strings refuse. Exhausted
plan/cardinality/credit refuses; it never spills into recursive/unbounded maps or retains
unknown fields. Wire field order is irrelevant: collect scalar/profile/count facts in the
bounded plan and validate the complete structure before Pass2.

| Response/submessage | Bounds checked before typed construction |
|---|---|
| ListNodes | At most4096 nodes; ID1..128 bytes; address<=512; labels<=128/node, key<=256, value<=1024; installed scalar/lifetime fields only |
| strict ListVolumes | At most4096 unique IDs1..128 bytes; mode<=16; installed completeness/mode scalars only; status does not request guard candidate history |
| GetVolume | Exactly one Volume; nonzero checked geometry and RF1..5; identity full views obey B90 and RF-specific chunk maxima; legacy chunks<=65,536, additionally subject to global entry/credit bounds |
| identity page | Exactly one header with zero chunks and<=128 IndexedChunk entries, each with exactly one ChunkMeta; all page/incarnation/revision fields validated |
| identity ChunkMeta | Replica/ISR refs each<=RF+1<=6; initial pending<=RF<=5; released<=1; at most one of each move endpoint; legacy projections empty |
| legacy ChunkMeta | Replica/ISR node sets each<=RF+1<=6; each ID<=128; move endpoint strings<=128; no identity fields or mixed projections |
| header/profile children | ID/target<=128; fenced refs<=16; snapshot catalog<=63; strings/bytes<=4096 and the owning installed profile's stricter limits; operation16/LID0-or16/UUID36 exact bounds |
| identity Tombstones | <=32 summaries, each<=512 encoded bytes; volume<=128, one exact ref, operation16, LID0-or16, bounded scalar charges/revisions; legacy Nodes/Pending empty; cursor<=1024 |
| legacy Tombstones | <=4096 rows; volume<=128; Nodes/Pending each<=4096 IDs<=128; global occurrence/entry/credit bounds also apply; identity fields/cursor absent |

Every ref has node1..128 and canonical nonzero lowercase UUID36. All rows also consume global
field/entry credits. A structurally expensive but wire-sized response may conservatively fail
ResourceExhausted. Complete installed schema/profile/semantic validation remains mandatory after
construction; preflight does not make malformed data healthy or permit mixed profiles.

Pass1 calculates the entire typed allocation charge before constructing children. Each generated
message costs `align16(unsafe.Sizeof(concrete type))+64`; exact-capacity slice/string/byte backing
costs `align16(bytes)+64`; map capacity costs1024 per entry plus charged key/value backing,
including empty entries. Use checked arithmetic and regenerate the conservative charge table for
the pinned Go/generated types when they change. Reserve this total atomically while the call's
wire/plan/request reservation is still held; insufficient remaining credit refuses before Pass2.
Pass2 constructs directly from the plan using charged allocation helpers, exact-capacity slices
and capacity-planned maps. It must not call generated Unmarshal/proto.Unmarshal/protojson or
append past a planned capacity. No allocation may escape the charge plan.

The per-call codec records its own typed error: bounds/credit exhaustion is ResourceExhausted,
malformed data is DataLoss, unsupported schema/profile is FailedPrecondition. The status adapter
preserves that classification after stock gRPC wraps codec failures as Internal; it never infers
it from server-controlled error text. Other RPC/context failures preserve their own code.

## Ownership, derivation and publication

Transfer decoded ownership/credits into the retained collection without copying it. When a DTO,
derived string, index, sorting scratch or page-header copy is needed, reserve its charge before
allocation; retain both charges while both objects exist. Page accumulation transfers the unique
rows and retains one validated header, charging any overlapping temporary header before release.
Derive may reference immutable collected strings/rows without a second backing charge; copying
requires new credit. Cursor/history tracking and omitted-volume IDs remain charged too.

On success/failure, drop application-owned references before releasing their credits; wire-work
and scan credits last until codec/call unwind. Failed decode never transfers a partial candidate.
Cancellation cancels sibling calls, joins all four workers, drops their partial results and
returns failure. This makes no assertion that stock transport caches were freed. A fresh command
may run normally; no guard process recycling machinery is required.

Encode either JSON or human output into one owned bounded result before writing stdout. Perform
a no-output counting pass with the exact same escaping/formatting rules, checked arithmetic and
deadline checks; include escaped UTF-8/control characters, delimiters, timestamps and final newline.
Reserve exact output capacity while retained View/index/scratch charges still exist, then allocate
and fill that capacity through a bounded writer; no unbounded json.Marshal, bytes.Buffer growth or
temporary string per row. A mismatch/overrun returns failure and publishes nothing. Check context
again before publishing. JSON escaping and human control-character escaping have no raw ANSI or
shell interpretation. Write only the completed result; a later OS stdout write error returns1
and may expose an incomplete physical write, never a reported successful partial view.

## Required implementation fixtures

- Actual TLS client/server fixtures: valid metadata URI; same CA+hostname+ServerAuth but wrong
  component/node/admin/Reader URI; foreign trust domain/CA, missing/multiple URI, expired cert,
  wrong hostname or EKU, missing expected-node configuration. Wrong peers fail before status RPC
  results are accepted. Retain Go/Rust Reader agreement, issuance and replica-refusal fixtures.
- Codec fixtures: repeated empty Node/Tombstone/ChunkMeta/map entries, many small legal rows,
  duplicate singular/map fields, depth/field/plan boundaries, malformed lengths/overflow/UTF-8,
  unknown schema/profile and every installed feature's maximum valid response. Allocation hooks
  prove refusal before constructing excess typed children; no stock heap/RSS claim is inferred.
- Credit fixtures: four simultaneous maximum calls plus retained pages, reserve failure before
  RPC/append, planned versus actual allocations, maximal escaped output, bounded-writer overflow,
  output charge with retained data still live, decode failure/cancellation and late page failure.
  All collection/encoding failures return exit1 with empty stdout; genuine empty complete
  observations and #69's explicitly allowed bounded NotFound omissions retain their own semantics.

## Source checks

Static basis: `pkg/pki/pki.go:193-260,301-314` parses identity but ClientConfig installs no role
predicate; issuance at111-127 supplies requested EKUs. `meta/cmd/flaminio-meta/main.go:305-339`
has the generic plaintext fallback to exclude. Pinned grpc `rpc_util.go:1051-1065` checks receive
bytes then invokes codec; `encoding/proto/proto.go:85-96` materializes and calls proto.Unmarshal;
protobuf `internal/impl/codec_field.go:438-455` allocates each repeated child before append.
#90 [budgets](../90/budgets.md) owns B90/full/page bounds and [contracts](../90/contracts.md)
owns exact-ref/tombstone shapes; these dependencies are not edited or reopened here.
