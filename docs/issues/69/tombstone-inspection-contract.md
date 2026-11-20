# Issue #69: bounded read-only tombstone inspection

**Proposed, unapproved R1 correction**, normative for [design](design.md) S1/S2/S4–S6.
#90 remains sole owner of exact tombstone roots/rows/index, lifetime and confirmation.
This whoever-lands-second adapter adds only administrative observation for a Reader; it is
not possession evidence and grants no confirmation, deletion, bootstrap or mutation authority.

## Additive request mode and least authorization

Actual proto has request node1 and response tombstones1. [#90 contracts](../90/contracts.md)
allocates request ref20,observed_store_generation21,limit22,cursor23 and response
next_cursor20,complete21; no other mapped #71/#77/#90/#91 allocation uses these next tags.
Add only `ListTombstonesRequest.bool inspect_node_history=24` and
`ListTombstonesResponse.bool node_history_inspected=22` (default false). Existing fields retain
their #90 types/bounds. No new RPC. Update proto caller comments, Go/Rust generated bindings,
`meta/service/auth.go` and method validation; `make proto contracts` checks tags/interop.

In ACTIVE a Reader may call ListTombstones **only** with inspect_node_history=true,
valid node1, absent ref20,observed_store_generation21=0,limit22=1..32 (default32), and
cursor23 empty or<=1024 bytes. Administrator may use the same inspection mode. Agents/IO
cannot use it. Exact-ref lists/confirmations retain #90's own-node IO/admin rules and
observed-generation requirements; the Reader gains neither. Mixed inspection/ref/G shapes
are InvalidArgument, wrong roles PermissionDenied. Authorize still denies every writer,
ConfirmDeletion/ConfirmRelease, token/grant/page issuer and future unlisted method. The
only extra Reader RPC beyond the historical four remains #91's sole-owned page-read hook.

Require a currently registered node, read its revision and ACTIVE mode/revision, and return
node_history_inspected=true on every successful history page. A node disappearing mid-scan
fails collection, never silently removes a registered node's coverage obligation. Unsupported
old server false, wrong shape/mode or an omitted marker fails visibly; do not retry as admin,
current-UUID-only or legacy node-only. This read flag cannot satisfy any confirmation input.

## Existing index, bounded work and cursor

For registered node N, scan only `/flaminio/v1/tombstone-index/<N>/` in byte-key order.
Its UUID path component includes **all retained UUIDs**, not merely Node.store_uuid: both
unplugged A and replacement B are visited. No UUID inventory, full tombstones prefix read,
full-prefix clone/sort, maintenance privilege or new persistent index is introduced.
Use #90/#76 ScanKeys/GetBounded in both Etcd and ordered Mem backends: at most limit<=32 examined
index keys/32KiB key bytes per page, key<=512 bytes,index value<=128 bytes; exact root<=4096
and ref row<=512 bytes, bounded before decode. At most32 returned summaries<=512 bytes each,
whole response<=32768 bytes including marker/cursor, at most68 backend calls/page and66
final compares, reusing #90's page envelope. All arithmetic is checked before read/append.

Validate each canonical key suffix UUID/volume/deletion_operation and the bounded matching
root+row identity/charge/schema. Return only PENDING root and PENDING row observations.
BUILDING/DRAINED/RECLAIMING, confirmed rows and absent stale roots/rows consume examined-key
budget but return nothing. Malformed keys/JSON, duplicate/unknown fields, key-row/root/ref/
incarnation/operation contradictions are DataLoss; do not treat corruption as a stale skip.
Finish with a read-only Txn comparing returned root/row revisions plus node and ACTIVE mode
(max2*32+2 compares); conflict is Aborted with no successful page/cursor. No KV writes occur.
An index may change outside those returned observations; completion is scan coverage, not a
snapshot of all roots. Retain #90's exact summary fields and omit legacy Nodes/Pending.

Cursor is signed bytes `FLTSHI01`8,caller_URI_SHA25632,node:string,mode_revision:u64,
after_index_key:string,Ed25519_signature64. Strings are u16-LE byte lengths plus ASCII bytes;
integers LE, total<=1024. Signature uses the existing metadata token signer with this distinct
domain/magic and covers all preceding bytes. Verify signature, exact authenticated caller,
requested node/mode and canonical after-key **inside that node prefix** before any scan.
Empty cursor starts before the prefix; a nonempty cursor is exclusive last **examined** key,
including skipped entries. A forged/mixed/oversized cursor is InvalidArgument; mode change
is Aborted. Signing these bounded cursor bytes grants only continuation of this read.

If more keys exist, complete=false and next_cursor advances to the last examined key;
empty nonfinal pages are legal. Final complete=true has empty next_cursor only after the
bounded scan establishes no remaining key. At least one key is examined on a nonfinal page.
Each page has its own linearizable read/compare point; insertions behind the cursor and
changes after reads are outside this observation. A future invocation starts at the prefix.
No server-side cursor/session registry or implied deletion/possession authority is created.

## Collector, row identity and coverage

For **every** node from the validated ListNodes observation, send limit32 and follow all
pages, including empty nonfinal pages. Require marker=true; complete implies empty cursor,
nonfinal implies a new nonempty cursor. Reject repeated/nonprogress cursors, duplicate row
keys, contradictory fields or malformed page bounds. Validate signed-format cursor length
and embedded after-key monotonicity locally (signature verified by server); never trust a
cursor that changes node/caller/mode scope. All page calls and retained rows consume the
same32768-application-invocation,30s,four-call and64MiB limits. Any error/budget overflow discards the whole
status view; no successful partial coverage. A user may run a new command after failure.

Key exact retained rows by `(volume,first_epoch,LID-or-none,deletion_operation,node,UUID)`;
never by volume or node name alone. Keep root/ref revisions per observation. JSON row is
`{volume,incarnation,deletion_operation,ref,pending:true,charged_bytes,charged_slots,
root_revision,ref_revision,legacy:null}`; operations/LID use canonical UUID strings.
Sort by that full key. Every returned root/ref revision is positive and all summary identity
fields satisfy #90. Several rows can share a volume/operation and remain separate; distinct
incarnations/operations observed across pages/name reuse never erase one another.

`tombstone_coverage` is `{scope:"registered_nodes_only",adapter:"identity-v4"|"legacy-node-v1",
scanned_nodes:[{node,pages,observed_rows}],complete_registry_scan:true,observed_rows,
total_pending_refs:null,total_pending_volumes:null}`. Completion certifies all bounded scans
for that initial registry were consumed. Observed rows are what those pages actually returned;
unregistered-node refs and behind-cursor insertions are unknown. Do not sum these observations
into a claimed exact global pending count or reconstruct #90's omitted full Nodes/Pending.
An empty registry has scanned_nodes=[]/observed_rows0 with unknown global totals, not a
claim of no pending cleanup. Human output shows this scope and unknown totals explicitly.

## Separately validated pre-#90 adapter

Only the explicit strict-list UNINSTALLED/revision0 adapter may send current node-only
ListTombstones, with all identity/history/cursor fields absent. A successful ACTIVE error
never triggers it. The current legacy service lists a whole response without paging; cap
it at4096 rows and each Nodes/Pending set4096 IDs before typed construction through the
[security contract](security-contract.md). The shared aggregate/cardinality caps still apply.
Require canonical unique Nodes/Pending, pending subset of Nodes and queried node pending;
no malformed/duplicate volume per single node response. Each queried node observation is
retained separately, keyed `(volume,queried_node)` rather than volume deduplication. Its row
has incarnation fields null,deletion_operation/ref UUID/charges/revisions null, ref.node set
to the queried node, and `legacy:{nodes,pending}` preserving only that response's validated
sets. No UUID, operation, incarnation or global set is inferred. Legacy coverage uses the
same envelope with adapter=legacy-node-v1,one page/node; observed_rows counts node observations,
not exact refs. Legacy mode and all unknown totals remain visible.

## Future service-to-status fixtures (not executed)

Actual Mem/etcd service through real Reader mTLS must cover permission/default deny, no
confirmation rights, registered node with old-A/new-B pending rows, >32 rows, skipped entries,
empty nonfinal pages, final empty prefix and forged/wrong-caller/node/mode/repeated/nonprogress
cursors. Cover same-name incarnation/operation churn, duplicate ref rows, malformed bounds,
missing/stale versus corrupt rows, node removal, late page failure and every budget boundary;
none may publish successful partial stdout. Include legacy set validation/coverage and
old-server marker=false refusal, descriptor tags and unchanged #90 own-IO/admin exact-ref
confirmation fixtures. Implementation runs `make proto contracts test race lint`; no runtime,
lab, tests or source changes are results of this draft.
