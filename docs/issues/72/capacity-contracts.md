# Issue #72: Capacity contracts

Normative appendix to [design](design.md): what the Go service, the Rust data plane, the CLI
and the lab build against. Accepted with the plan, 2026-11-13.

## Stored records (etcd, under `/flaminio/v1/`)

- **`capacity/<node>`:** the node's ledger. Readable: `{"schema":1,"bytes":B,"slots":N}`
  (uint64). Being initialized: `{"schema":1,"init":true,"prev":{"bytes":B,"slots":N}}`, `prev`
  the value the mark keeps (below), omitted when none. Any other value is unreadable, and its
  read is logged. Created comparing absence (revision 0); otherwise written or deleted
  comparing the revision read. Writers: `RegisterNode` (a new node, zero), `InitCapacity`, the
  `CreateVolume` claim, `MoveReplica`, #70's growth steps (spec S3), and the release rule
  (design) in `ConfirmDeletion` and #13's `ConfirmRelease`.
- **`capacity-claims/<volume>`:** `{"schema":1,"charges":{"<node>":{"bytes":B,"slots":N}}}`, the
  claim's charge per node (at most 116 entries). Written by the claim, without a compare: a key
  left by an older binary's deletion is overwritten. Deleted by the creation's last header write,
  and by `DeleteVolume`'s tombstone transaction.
- **`alive/<node>`:** written by `PutTTL` as today. Its value is `1` (no report) or
  `{"schema":1,"data_bytes":..,"used_bytes":..,"quarantined_bytes":..,"slots":..,"slots_used":..}`
  (uint64). Any other value reads as no report; it still counts as alive.
- **`tombstones/<id>`:** today's `nodes` and `confirmed`, plus `charges`:
  `{"<node>":{"bytes":B,"slots":N}}`, one entry per node in `nodes` (a fence holder's includes
  its one slot). Absent on a tombstone written
  before this change or by an older binary, or for a volume whose header does not parse.

## Transactions

| Operation | Compares | Writes |
|---|---|---|
| `CreateVolume` claim | volume absent, tombstone absent, k ledgers | header, claim key, k ledgers |
| Creation's last header write | the claim, as today | header, claim key deleted |
| `MoveReplica` | chunk, header, the destination's ledger | chunk, that ledger |
| #70's growth step | as #70's "Charges" bounds them | its chunks and header, each used node's ledger |
| `DeleteVolume` tombstone | header, as today | header deleted, tombstone, claim key deleted |
| `ConfirmDeletion` | tombstone, the node's ledger (revision 0 if absent) | tombstone, ledger by the release rule |
| `ConfirmRelease` (#13, with spec S11's change) | header, n ≤ 118 chunks, the node's ledger | n chunks, header, ledger by the release rule |
| `RegisterNode`, new node | node absent, ledger absent | node, zero ledger |
| `InitCapacity` mark, per batch | each ledger as read | each mark (`TxnRev`: this call's mark) |
| `InitCapacity` write, per node | the ledger at this call's mark | the ledger |

k ≤ 116 is a margin, not a derived limit: the claim fits 118 (2 + k compares, 2 + k writes), and
#76's admission (4 compares with #78's gate, 2 writes) still fits 116. Mark batches hold at most
120 nodes. Every row stays within `kv.MaxTxnOps` (120 compares and 120 writes), which only the
etcd store enforces; the tests that fill a row run on both stores.

**Initialization and releases, the one rule** (spec S6; design and ADR defer to it):
1. A mark keeps `prev`: the readable value it replaces, or the `prev` of the mark it replaces.
2. Every write `InitCapacity` makes, a recorded charge or a restore from `prev`, compares the
   revision of this call's own mark; a failed compare leaves the node marked, with no retry.
3. Every release on a mark rewrites it: `prev` less the charge, or the same mark when it keeps
   none. Its revision moves, so no write of a value read before the release can land, and the
   release is in the next run's snapshot and in `prev`. No release is lost.

## Protobuf (`api/proto/flaminio/meta/v1/meta.proto`)

```proto
// A node store's capacity and use, sent with the node's heartbeat by its data plane.
message CapacityReport {
  uint64 data_bytes = 1;         // data clusters × cluster size
  uint64 used_bytes = 2;         // allocated clusters × cluster size, incl. freed, not checkpointed
  uint64 quarantined_bytes = 3;  // freed clusters held back from reuse × cluster size
  uint64 slots = 4;              // chunk replica slots (superblock)
  uint64 slots_used = 5;         // chunk replicas on the store
}

// A node's capacity as the service sees it. Set by ListNodes; ignored in RegisterNode requests.
message NodeCapacity {
  CapacityReport report = 1;     // unset: no valid current report
  bool ledger = 2;               // false: no readable ledger
  uint64 committed_bytes = 3;    // 0 without a readable ledger
  uint64 committed_slots = 4;
}

message NodeHeartbeatRequest { string node = 1; CapacityReport capacity = 72; }
message Node { /* fields 1–5 as today */ NodeCapacity capacity = 72; }

// Recomputes ledgers from the metadata records (spec S6). Caller: an administrator.
rpc InitCapacity(InitCapacityRequest) returns (InitCapacityResponse);
message InitCapacityRequest { bool all = 1; }  // false: absent, unreadable or marked ledgers
message InitCapacityResponse {
  repeated NodeCharge nodes = 1;  // every registered node, by id
  repeated string claims = 2;     // volumes holding a claim key: being created, or interrupted
}
message NodeCharge {
  string node = 1;
  bool initialized = 2;          // this call wrote the ledger
  bool had_ledger = 3;           // a readable ledger, or a mark's prev, existed before the call
  uint64 previous_bytes = 4;     // that value; 0 without one
  uint64 previous_slots = 5;
  uint64 recorded_bytes = 6;     // the charge of the node's named copies in the snapshot
  uint64 recorded_slots = 7;
  string blocked_by = 8;         // what blocked it; set for this call's targets only
  bool marked = 9;               // still being initialized (the CLI's lines say what to do)
}
```

- **Tags this plan reserves:** `NodeHeartbeatRequest.capacity = 72`, `Node.capacity = 72`, and
  the new messages above with their own numbering. Checked against the drafts on the same
  messages: #45 takes `Node` 6–10, #61 takes `Node` 20–21.
- **Unchanged:** `CreateVolumeRequest` (no device class in this step; #73 holds 30 and #75 40
  there, #79 holds 9), `Volume`, `GetVolumeRequest`, `ChunkMeta`, `IsrChange`,
  `MoveReplicaRequest`, `Tombstone`. #80's response bound is untouched.
- **Rust:** `flaminio-metaclient/src/pb.rs` mirrors `CapacityReport`, `NodeCapacity` and the two
  tag-72 fields by hand. The data plane never calls `InitCapacity`.

## Errors

- **Room (`CreateVolume`, `MoveReplica`):** `RESOURCE_EXHAUSTED`, one clause per refused node,
  joined by `; `:
  `capacity: <node>: committed <L> B/<Ls> slots, use <U> B/<Su> slots, budget <B> B/<Bs> slots
  at <w> ppm, <c> B and 1 slot per copy`.
- **Left out of a create:** appended to either create error: `; no capacity report: <ids>`,
  `; no readable capacity ledger (flaminio-meta capacity-init): <ids>`.
- **`MoveReplica` destination:** `FAILED_PRECONDITION`, `node <to> has no current capacity
  report`, or `node <to> has no readable capacity ledger: run flaminio-meta capacity-init`.
- **Too many nodes:** `FAILED_PRECONDITION`, `placement uses <k> nodes, at most 116`.
- **Contention** on a ledger after 32 attempts: `ABORTED`, `too much contention`, as today.
- **`NodeHeartbeat`:** a report from any identity but the node's data plane:
  `PERMISSION_DENIED`, nothing written. An invalid report: `OK`, stored as none; the warning
  `capacity report refused` (node and the five figures) is logged each time a node's report
  turns invalid, per service instance.
- **`InitCapacity`:** `PERMISSION_DENIED` for any caller but an administrator; store errors as
  today (`UNAVAILABLE`); otherwise `OK` with every row, blocked ones included.

## Authorization (`meta/service/auth.go`)

- `InitCapacity`: administrators only.
- `NodeHeartbeat`: without a report, unchanged (the node's agent or data plane, or an
  administrator). With one: only the node's data plane (`io` identity of that node).

## CLI (`flaminio-meta`)

- `heartbeat <node> [<data-bytes> <slots>]`: with both, sends a report with those figures and
  zero use (sizes take `create`'s K/M/G/T suffixes). It is a test input: the service accepts it
  only with the node's own `io` certificate. One of the two alone: error, no call, exit 1.
- `capacity-init [--all]`: calls `InitCapacity`, prints the response as JSON like every command.
  Exit 0 when no row is `marked` or has `blocked_by` (set only for the call's targets, so a node
  the call did not touch never makes it exit 2); exit 1 on an RPC error; otherwise exit 2, with one
  line per such node on stderr:
  - marked, not blocked: `capacity-init: <node> still marked: run it again`;
  - marked, blocked, `had_ledger` (its restore raced a release): `capacity-init: <node> blocked
    by <id>: raced a release: run it again`;
  - marked, blocked, not `had_ledger`: `capacity-init: <node> blocked by <id>: wait for its
    confirmations or repair it, then run it again`;
  - restored from `prev`: `capacity-init: <node> blocked by <id>: kept its earlier ledger; run it
    again with --all once <id> clears`.
- `nodes`: unchanged; its JSON now has `capacity` per node (protojson names: `dataBytes`,
  `usedBytes`, `quarantinedBytes`, `slots`, `slotsUsed`, `ledger`, `committedBytes`,
  `committedSlots`).

## Upgrade order

Every metadata-service instance first, then every data plane, then `capacity-init --all`, and
`--all` again once every tombstone an older binary rewrote is gone (spec Q3). `scripts/lab.sh
meta up` for the whole cluster runs `capacity-init` once. Until a node's data plane reports, the
node takes no new copy: creates and moves fail closed, naming it.
