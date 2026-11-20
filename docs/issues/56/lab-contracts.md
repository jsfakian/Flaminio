# Issue #56 — coordinator history and owned recovery contracts

Proposed after the 2026-11-13 approval; unapproved. Test artifacts and helper interfaces only.
[Design](design.md) · [specification](spec.md). Production wire/format/authorization are unchanged.

## Coordinator and remote helper

Planned `flaminio-edge-history coordinate --run-dir DIR --volume ID --blocks CSV --min-isr 1|2`
runs on the management host; `check_edge_history --history PATH` runs the Rust adapter there.
The harness supplies a private, bounded inventory as below; no arbitrary command or host flag.
`flaminio-edge-history serve --run NONCE --endpoint N --volume ID --device DEV --epoch E`
runs by fixed argv through the existing SSH inventory on the selected data node. The coordinator
alone allocates IDs and writes authoritative history; helpers never compare clocks across hosts.
Serve accepts framed Read/Write requests, Stop and one response per operation over stdin/stdout.
Each payload is nonce[32] followed by the256-byte body below; successful Read replies append
4096 raw bytes. Frames are u32 LE payload length≤8192, payload, CRC32C over length+payload.
Request bodies use type2 Invoke, replies type3 Return; sequence/manager_ns are zero on this
transport, and other fields follow their history record. Replies echo run/endpoint/epoch/op/kind/
block, with errno or complete read content. Write bytes are regenerated from the value identity.
Unknown/reserved fields, mismatches, duplicate IDs or excess bytes fail the endpoint. Stop type8
and Stopped type9 carry endpoint/epoch with all other body fields zero (block=UINT64_MAX),
beside the nonce prefix. Stop joins operations and closes the device before Stopped. These two
transport types are forbidden in the history stream.

Only owned, unmounted test devices unused by other processes are accepted: exact `/dev/ublkb[0-9]+`, checked
against control-socket status for the expected volume before open. The harness has exclusive
administration of its test volume and records the acquired metadata epoch before attaching,
then records readiness and device binding before opening an endpoint. It never infers an epoch
from a helper's arbitrary echo. An endpoint is bound to that acquired attachment generation;
new attaches get new IDs, including on the same node. No other writer/attachment is allowed.
No token is persisted in history or diagnostics. Helpers use direct aligned4096-byte I/O and
validate full #49 value encoding; one I/O per block per endpoint may be outstanding, with at
most32 total per endpoint and64 per run. Two endpoints may probe the same block concurrently.

The coordinator fdatasyncs Invoke before transmitting a request; failed persistence stops all
issuance. Response time is sampled only after receiving/validating a complete reply, then Return
is appended/fdatasynced before acknowledgement to the harness. A transport timeout is not a
Return: stop issuing on that endpoint and retain its pending operations. A later valid reply
before closure is recorded normally, even after the timeout; successful writes are mandatory.
No retries reuse an op ID or resend an uncertain write. Remote errors stay recorded and fail a
case that requires successful I/O; their write effects are optional in the safety checker.

Stop has a30s drain bound. If it expires, terminate the exact owned helper process group and
join/close its SSH process; a blocked kernel I/O that cannot be joined fails cleanup and prevents
checkpoint/seal. Hypervisor-confirmed power-off plus closed/joined transport can close an
endpoint without a VM-local End; it supplies fault-effect evidence, never #82 recovery authority.
Drain buffered replies before EndpointClose. A closed endpoint accepts no later Return; such a
record is malformed. Killing only the manager-side SSH client does not prove the remote helper
stopped. Helpers use owned transient systemd units `flaminio-edge-RUN-ENDPOINT.service`, with
RUN the full64 nonce hex digits. Persist the typed helper action below before launch; verify its
recorded invocation identity before termination. After a VM return, verify its former helper is
absent before any new device access.
Management process/boot loss yields Undecided; healer recovery never resumes its history clock.

## FLEDGE01 artifact

All integers below are unsigned LE unless result:i32. Header: magic8 `FLEDGE01`, u32 payload
length≤4096, payload, u32 CRC32C over magic+length+payload. Payload: nonce[32], manager boot UUID[16],
volume_len:u16 + valid ID bytes≤128, chunks:u32=64, chunk_bytes:u64, block_count:u16 (1..16),
unique sorted block_indices[block_count]:u64, endpoint_slots:u16 (1..32), followed by that many
fixed inventory descriptors: id:u32 (1..slots), node:u8 (1=A,2=B), device_id:u32. Remaining bytes
are forbidden. Validate geometry/alignment/overflow against the owned volume before issuing I/O.
Nonce is fresh cryptographic randomness; CLI spells its32 bytes as64 lowercase hex digits.

Records: u32 length=256, body[256], u32 CRC32C over length+body. Body offsets:

| Offset | Field |
|---|---|
|0|type:u8:1 EndpointOpen,2 Invoke,3 Return,4 EndpointClose,5 Checkpoint,6 Seal|
|1|kind:u8:1 Read,2 Write;0 for endpoint/seal records|
|2|flags:u16:bit0 foreign read value; others0|
|4|result:i32:0 success or positive errno; only Return|
|8|sequence:u64, starts1 without gaps|
|16|manager monotonic_ns:u64, nondecreasing in this boot|
|24|op_id:u64; zero for endpoint/seal records|
|32|endpoint_id:u32; zero for Seal|
|36|reserved:u32=0|
|40|acquired attachment epoch:u64; zero for Seal|
|48|block_index:u64; UINT64_MAX for endpoint/seal records|
|56|value_id:u64|
|64|value_nonce[32]|
|96|value_block_index:u64|
|104|foreign_sha256[32]|
|136|aux1:u64|
|144|aux2:u64|
|152|endpoint boot UUID[16]|
|168|reserved[88]=0|

EndpointOpen has its acquired epoch and actual helper boot UUID; all value/aux fields zero.
Each endpoint opens once, before Invoke, and closes once after replies drain. Invoke/Return
carry the same endpoint/epoch/block/kind/op_id; boot and aux fields zero. op_id's high32 bits
are endpoint_id and low32 a counter starting1; a Write's value_id=op_id. Use #49's4096-byte
encoding with this run/value_id/block, without importing its segment parser's ordering rules.
A successful Read records the complete validated value identity; foreign/torn bytes set bit0
and their SHA256, never a usable register value. A Write Return repeats its invoked identity;
error Reads have zero value fields. Checkpoint references a successful Read's op_id/identity;
it creates no extra operation. All fields not assigned to a record above are zero.
EndpointClose uses aux1=1 joined helper or2 confirmed powered-off VM, aux2=0; its boot equals
EndpointOpen's. Seal uses aux1=preceding record count, aux2=preceding file length in bytes;
all other unused fields zero. Seal is last and is written only after all opened endpoints close.
Never manufacture a Seal after manager failure. A truncated/missing Seal or CRC/identity/order
error yields Undecided, preserving raw bytes. Header slots may remain unopened, with no probes.

Cap the history at128MiB and65,536 Invokes/run, with at most128 operations per block between
eligible checkpoints and1,000,000 memo states per search. Reserve bytes for all outstanding
Returns/Close/Seal before issue; any cap stops issuance and marks Undecided. Helper frame/output
caps and concurrency are independent of untrusted responses; excess/unsolicited bytes fail.
Other evidence totals≤8MiB; bounded logs indicate truncation and fail a required-evidence claim.

## Shared register adapter

Add `sim/history::edge` and a thin `examples/check_edge_history.rs`. Decode FLEDGE01 explicitly;
never reinterpret FLHST001 or FLRUN001. Reuse #49's value decoder and budgeted register search
(initial value and result/state-budget API); whichever implementation lands second adds this
adapter without changing either format's semantics. No production model/rule changes.

Successful operations have [Invoke.manager_ns, Return.manager_ns] envelopes. Equal times retain
overlap; never invent strict ordering by sequence. Successful writes are mandatory; foreign
successful reads are violations. Error/pending writes are optional; error/unanswered reads impose
no value. A write's error Return is not proof that remote in-flight effects stopped. For an error
or unanswered write from epoch e, latest effect is the first successful response on that block
from an endpoint acquired at epoch>e; absent such a response it remains unbounded. A later received successful Return
before closure makes the write mandatory with its actual response envelope. Verify the entire
stream and classify all Returns before applying checkpoints, so late replies cannot be dropped.
A closed old request cannot resurface after that later-epoch response, as protocol §9 requires.
If a later-epoch response already precedes an optional old Invoke, that old request is
retained as necessarily no-effect, never given a newer deadline that permits resurfacing. Map
validated envelopes/deadlines to checked integer ranks; infinity stays after every finite rank.
Apply #49's bounded per-block register search; retain every optional operation or its proved
no-effect classification in the report.

A checkpoint uses a dedicated Read issued only after gating new operations on that block and
draining its earlier requests; it never references an earlier workload read. Wait for a successful
nonforeign Return, append Checkpoint immediately with no intervening Invoke on that block, then
release the gate. The decoder verifies this sequence against the entire stream. No optional effect
may occur after that read: an error or closed pending write crosses candidate checkpoints until
its later-epoch deadline is resolved; carry it, never cut it at EndpointClose. Search the entire
preceding slice through this dedicated Read, including every preceding mandatory write and
carried optional effect, then carry its read value as the next slice's initial register state.
Require one acknowledged initializing write per block before fault probes, and a successful final read of each block after all endpoints' pending
requests are resolved/closed with the required deadline. If that cannot be achieved within
caps, verdict Undecided. Safety violation→Fail; parser/budget/incomplete evidence→Undecided;
Pass requires all requested cases and cleanup plus valid history, not just a successful decode.
Production qualification additionally refuses legacy exploratory evidence.

## Private owned-action ledger and conditional holds

The management run directory is `$LOCAL_STATE_DIR/edge/NONCE`, owner current lab administrator,
mode0700, exclusively created. Files mode0600; no symlink following in any path component,
no absolute/dot/dotdot paths supplied by a record, no shell sourcing. Ledger `recovery.json` is
strict version1 JSON≤64KiB and≤128 actions: reject duplicate/unknown/missing keys, invalid types,
truncation and numeric overflow before any remote mutation. Replace via exclusive same-directory
temporary file, fsync file, rename, fsync directory. Preserve the previous valid ledger and
report failure on an uncertain write. The independently runnable `edge-two-node.sh heal NONCE`
accepts no path or command from a record; resolve the fixed private run tree itself.

Top-level exact fields: version, nonce, inventory_digest, actions. Inventory is captured from
fixed lab configuration before mutation; digest is SHA256 of its retained exact bytes. Actions
have id (1..128), kind, target, phase (intent|applied|healed), and kind-specific fields below.
No free-form command/unit/host/IP field is executed. Validate the entire ledger and inventory,
nonce/path match, stable VM UUIDs and exact configured overlay/management addresses first;
foreign/replayed run or changed inventory refuses before action. Concurrent run directories are
refused by an exclusive management flock; run-specific owner stamps protect post-crash healing.
Privileged helpers build fixed argv after validation; no shell evaluation of ledger values.

- **pair:** target=A|B|W, peer=A|B|W distinct, saved exact INPUT/OUTPUT rules and run comment.
  Derive addresses from inventory, reject management overlap, install only this run's exact
  comment/peer rules and delete only those rules. A foreign run's rule is a conflict, never
  adopted. Intent is durable before the first rule; heal scans both endpoints for exact rules,
  including partly applied intents. Status/counter/effect probes must succeed before a case.
- **power:** target=A|B|W, domain_uuid, prior=running, owner_stamp=nonce and saved domain metadata≤8KiB. Refuse a VM already
  off or carrying another owner's stamp. Persist intent, install/verify nonce metadata on the
  exact UUID while running, then power off and record confirmation. Heal may power on only
  that UUID with the matching stamp and prior running state; observe health before restoring
  its saved metadata conditionally. Changed UUID/stamp or ambiguous state requires manual
  recovery and fails. This is fault ownership, never a fence/recovery permission.
- **controller_hold:** only flaminio-system/flaminio-failover, saved Deployment UID/generation/replicas,
  spec SHA256 (excluding replicas), saved Node UID/taint snapshots and owner-annotation absence
  verified in one GET. Record intent; JSON Patch tests UID/resourceVersion/replicas from that
  exact GET, then adds the owner annotation and sets replicas0 atomically. Annotation absence
  is a precondition verified in the GET and protected by its resourceVersion test, not a test
  operation on a nonexistent JSON path. If annotations is absent, add the annotations map with
  only the owner key; otherwise add the escaped owner-key path, preserving all other entries.
  Any conflict aborts the hold attempt without retry or fault injection; preserve the concurrent
  update even when it has no owner annotation. Never reuse the saved restoration snapshot
  against a later resourceVersion. Require saved replicas>0; an already stopped Deployment
  needs no hold and is kept stopped. Save the resulting generation (old+1); on intent-only recovery
  this expected generation plus unchanged spec digest validates the partial application.
  Require no owned Pods/processes alive,
  no other controller copy and no out-of-service taint before any fault. Failed hold refuses
  injection. Healer handles intent-only partial application by reading the exact UID/owner;
  an unstamped unchanged Deployment needs no restore, anything ambiguous remains blocked.

- **helper:** target=A|B, endpoint (a header slot), epoch, device_id, node boot UUID, phase and invocation_id
  (null in intent; otherwise32 lowercase hex digits). The unit name is derived from full nonce
  and endpoint; executable/path/build are fixed by the saved inventory. Before any start, persist
  this action and require no existing unit/remote claim. A root-owned private claim at the fixed
  `/run/flaminio-edge/NONCE/ENDPOINT` path uses no symlink following and exclusive creation,
  recording nonce/node/endpoint/epoch/device and initially null invocation_id. Its typed bounded
  bytes use the ledger's parser/atomic-write rules; no commands or supplied paths are read from it.
  Launch with Restart=no and one process: serve uses goroutines/threads, never child processes.
  Before opening a device or accepting I/O, take the claim's exclusive lock, require its matching
  identity and null invocation_id, publish systemd INVOCATION_ID atomically once, then verify the current unit has that same identity and exact expected ExecStart.
  Only then acknowledge startup and persist the invocation_id in the management action.
  The immutable nonnull claim lets a healer recover when management died before that ack.
  Intent-only recovery with absent unit/claim needs no signal; a unit with matching published
  claim is the owned invocation. A present unit with missing/null claim remains ambiguous:
  it cannot pass its startup gate, but cleanup blocks without signalling it. Never infer ownership
  from unit name alone. A mismatching nonnull claim, invocation or ExecStart preserves the unit.
  To terminate, open a pidfd for the unit's MainPID before inspecting it; verify its executable,
  expected run/endpoint argv, cgroup and current InvocationID against the action/claim. Signal
  that pinned process with pidfd_send_signal, not a name-based systemctl kill/stop; a replacement
  invocation cannot receive the signal. Require process exit and unit cgroup empty, or report
  cleanup failed. Recheck invocation ownership before any unit/claim removal; changed state is
  preserved. After a power-off, retain the action/claim until boot return confirms no old helper;
  never relaunch that endpoint or resume its prior history after management/boot loss.

Controller restoration is last, after pair/power/helper cleanup, healthy nodes/services/quorum,
full ISR/data verification and unchanged Node UID/no new out-of-service mark. Re-read Deployment;
require saved UID, recorded hold generation/spec digest, replicas0 and matching nonce annotation. Conditional
JSON Patch tests its resourceVersion/UID/replicas/owner, restores only saved replicas and removes
only the owned annotation. Changed spec/UID/foreign mark refuses restoration; never clear taints.
The local watchdog attempts each owned cleanup once with30s per remote operation/10min total;
continued failure is named with exact manual healer instructions. Subsequent heal is repeatable.
Management loss can leave owned faults/hold in place; no successful cleanup or Pass is inferred.

## Planned regression fixtures

Go recorder/helper and Rust adapter: correct concurrent histories; lost acknowledged write;
stale/torn/other-block reads; timeout then late successful reply; powered-off helper with no local
End; dedicated drained checkpoint Read and immediate cut; an older read followed by ACK(v1)
cannot seed a later slice with v0 or make a lost write Pass; optional old write crossing a
checkpoint and resurfacing after a later-epoch read; unresolved
request prevents slicing; unknown endpoint/epoch/op/nonce; replay/duplicate/truncated frames; manager
boot change; helper remains blocked after SSH exit; output/concurrency/search/byte caps. Identical
nonlying devices pass; intentional lies fail or incomplete evidence is Undecided, never Pass.

Script fakes: #83 VM admission refuses before all mutations; legacy run never production Pass;
power sample never calls node recovery; unsafe controller still live prevents injection; stop/kill
wrong invocation refuses; crashes before helper start, after start before claim publication,
after publication before management ack, and after identity persistence; missing/null claim
blocks safely and replaced/foreign invocation stays alive; absent annotations map is supported,
concurrent foreign owner annotation defeats the resourceVersion test; an ownerless concurrent
replica-count or spec update also aborts the hold without retry, mutation or fault injection; interrupted hold
before/after patch, foreign controller update or new
taint prevents restore; malicious/foreign/replayed/truncated ledger causes no remote action; power
UUID/stamp changed is preserved; partial pair installation heals exactly; management restart uses
healer, not old history; helper drain failure and unhealed state fail. No live lab is needed.
