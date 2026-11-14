# #79 permanent initialization authority — contract accepted with the plan, 2026-11-13

This is part of [design](design.md), accepted with the plan, 2026-11-13; not implemented. It replaces local missing-profile
inference. #15 at `8af7aeb` remains the sole full device-lifetime/conditional-detach contract.
Required unimplemented dependency: #83 [fresh-process contract](../83/fresh-process-contract.md)
at `5a2fcd8`; its FLCH handoff is process authority, while FLCI remains initialization authority.

## Durable authority and admission (S2–S3, S8)

Add `Header.use` and canonical `CreateSpec.use_profile`: Unknown=0, Raw=1, Ext4=2.
CSI CreateVolume requires all requested capabilities to agree on block vs filesystem and normalized
filesystem type (currently ext4); mixed requests fail InvalidArgument before creation. New explicit
Raw creates SealedRaw; new explicit Ext4 creates FreshFs. Omitted profile remains Unknown for old
direct clients. CreateSpec.use_profile is immutable create identity, not a conversion request or observation.
Header.use.profile is the effective mode (initially that profile); sealing Unknown sets it once.
Volume.use_profile reports canonical create identity; InitializationState additionally has
UseProfile effective_profile=6, so Unknown create identity never hides a later permanent raw seal.
Only genuinely empty new-volume creation may set FreshFs. Restore, clone, import and legacy migration
MUST force Unknown/nonfresh regardless of their desired mount mode; never infer emptiness from bytes.

Persist `{schema:1, profile, state, op, node, epoch, target_nonce}` in the initial hidden creating header transaction,
retained through #76 finalization/recovery and all mutations. FirstEpoch is the incarnation key,
not initialization evidence. Valid states: Unknown, SealedRaw, FreshFs, Reserved, Started, ReadyFs.
Reserved/Started include one UUID operation, node ID, fencing epoch and daemon nonce32;
ReadyFs retains that operation as permanent history; ReadyFs sealed from Unknown instead has
empty op/node/nonce and epoch0, never fresh.
Unknown/SealedRaw/FreshFs have empty op/node/nonce and epoch0. No missing-field
fallback to FreshFs, no reset-to-fresh API and no automatic transfer/rollback from Reserved/Started.
Delete removes authority only with that incarnation; stale operations also compare the current
header FirstEpoch, revision, non-Creating/non-Deleting state and tombstone absence. ID reuse cannot
reuse an old grant. Local profile absence or loss changes none of this state.

Ordinary Meta.Attach and every token-issuing/admin/failover path reject FreshFs/Reserved/Started;
Unknown and SealedRaw have no formatting authority. SealedRaw rejects filesystem staging; ReadyFs
rejects raw use and mismatched fs type. Unknown permits a raw attachment or mounting an already
recognized filesystem, never mkfs; its first successful mode admission CAS seals Raw or ReadyFs
before a normal target token is issued. Prepare/Attach accepts a requested profile, not a claim
that bytes are empty. Add AttachRequest.use_profile=5; omission/Unknown on direct Attach seals Raw,
explicit Ext4 seals ReadyFs without formatting. Existing unknown filesystem with blank/ambiguous signature
fails; mode observation cannot establish FreshFs. All nodes/services must enforce the new admission
profile before enabling it; mixed old token issuers/engines are unsupported.

## Exact additive API and authorization

In metadata protobuf add enum UseProfile as above, CreateVolumeRequest.use_profile=9,
Volume.use_profile=9 and Volume.initialization=10 (`InitializationState {state=1,operation=2,node=3,
epoch=4,first_epoch=5}`, state enum ordered as listed above). These tags are free in the reviewed
base; integration must detect conflicting pending additions rather than renumber existing fields.
New messages use the following field numbers; epochs are uint64, IDs strings, profiles/states enums,
volume fields Volume, operation UUID canonical lowercase36. Max op/node/ID checks precede reads:

- `PrepareVolumeUseRequest {volume=1,node=2,first_epoch=3,profile=4}` -> `VolumeResponse {volume=1}`.
- `InitializationRequest {volume=1,first_epoch=2,operation=3,node=4,expected_epoch=5,
  bytes target_nonce=6}`. Reserve requires exactly32 nonzero bytes from attach-challenge.
  Start/Finish require the stored nonce; Get may omit it but never returns credentials.
- `InitializationResponse {volume=1,operation=2,epoch=3,credentials=4}`; credentials present only on
  successful reserve/recovery of Reserved to the same operation/node/epoch/nonce; never on
  Started/ReadyFs query. Changed nonce refuses; no old-epoch handoff mint or nonce-rebind API.
- RPCs `PrepareVolumeUse(PrepareVolumeUseRequest)->VolumeResponse`,
  `ReserveInitialization(InitializationRequest)->InitializationResponse`,
  `GetInitialization(InitializationRequest)->InitializationResponse`,
  `StartInitialization(InitializationRequest)->InitializationResponse`,
  `FinishInitialization(InitializationRequest)->VolumeResponse`.
  Get returns no credentials; Start's winning response is one-shot permission, not a reusable ticket.
- Compose #83 fields: AttachRequest.bytes target_nonce=6 and AttachResponse.bytes target_handoff=3;
  nonce validation before writes, unchanged method auth. A normal handoff requires new-epoch Attach
  CAS, not unless_target/read-only mint; legacy empty nonce cannot start a strict daemon.

Explicit `auth.go` entries: Prepare is admin only (CSI controller); Reserve/Get/Start/Finish allow
admin read via Get only, or node component=agent with caller Node==request.node. Service additionally
requires current target==node and matching operation/incarnation/epoch; wrong-role/node is
PermissionDenied, malformed fields InvalidArgument, conflicts FailedPrecondition, concurrent CAS
Aborted. Reader/io/anonymous cannot mutate; node/io may use GetVolume for admission verification.
Existing CreateVolume stays admin-only. Unknown volumes still require #15 nonzero verified
FirstEpoch; legacy FirstEpoch0 is unsupported rather than inventing a lifetime. No wildcard auth.

ControllerPublish uses Prepare instead of ordinary Attach for FreshFs: CAS target reservation with
an epoch advance but no target token. Exact same-target retry returns the same reservation; another
target cannot change Reserved/Started. For normal states Prepare delegates existing attach admission.
NodeStage persists unbound intent, then Reserve atomically FreshFs->Reserved and epoch advance for
that target, storing the process nonce and signing purpose2 FLCH only after the new-epoch CAS.
Exact same-op/node/epoch/SAME-nonce Reserved retry may recover its original credentials; changed
nonce returns FailedPrecondition without a ticket/handoff, epoch reuse or new attachment.
Get may use expected_epoch=0 to recover the exact op after a lost Reserve reply;
mutations require the observed epoch (same-op Reserve replay compares the stored op first).
Other-op/node requests refuse. Header target and use state change in one conditional TXN.

Initialization ticket: purpose-specific `FLCI` envelope, metadata Ed25519 key as for capability
tokens. Exact body, integers big-endian: magic4=`FLCI`, u8 version1, u16 volume length + ASCII
volume, u64 FirstEpoch, u64 epoch, u16 node length + ASCII node, UUID16 bytes (RFC4122 network
order), u8 profile=2; append Ed25519 signature64 over the whole body. Max362 bytes, no trailing
bytes, IDs1–128, nonzero FirstEpoch<epoch, canonical UUIDv4. Reject unknown version/profile, bad
length/claim/signature and wrong node/current incarnation/epoch. No expiry; epochs fence authority.
It is NOT accepted as a normal replica capability. Reserve returns
`InitializationCredentials {bytes replica_token=1; bytes initializer_ticket=2;
bytes target_handoff=3;}`. Handoff is exactly #83 FLCH134: magic4, version1, purpose2, nonce32,
SHA256(raw FLCT)32, metadata Ed25519 signature64. Verify exact length/version/signatures/digest,
current process nonce and normal volume/node/FirstEpoch/epoch checks; no alternate encoding.
Replica transport keeps existing epoch fencing; the normal token never becomes normal local attachment authorization
while metadata is FreshFs/Reserved/Started. No init ticket/token in logs or local JSON: persist the
operation/epoch/nonce, re-fetch only while Reserved with the same process nonce. Normal target
attach admission must check authoritative use state; engine cache from an older schema is not authority. No per-I/O metadata calls/locks.

Add daemon `attach-init VOLUME OP INITIALIZER_TICKET REPLICA_TOKEN HANDOFF` and
`initialization-status VOLUME OP`. Each token is unpadded base64url, no whitespace.
Strict ID/UUID/arity/size validation precedes effects; each token operand bounded at4096 bytes and redacted.
Engine::start_target verifies current-process FLCH purpose2 and its exact FLCT digest in addition
to FLCI claims/signature plus current metadata target/epoch/use=Reserved/Started, and binds
op/purpose immutably to its entry. Keep #83 ran_epochs: no second same-epoch target session.
Replaying a Reserved operation returns that existing entry,
never allocates a second one; different operation/purpose conflicts. Init attachment is private to
CSI, not publishable. initialization-status returns `ok none`, `ok starting OP EPOCH`, or
`ok bound OP EPOCH` followed by the exact #15 device-info identity fields (excluding its leading `ok`).
Lookup returns only the matching op/purpose; a different entry is stale, never a name-only adoption.
Normal daemon Attach (including direct callers) refuses initialization states/tickets and checks
mode admission before constructing a target; no older code path can bypass the gate.

Start is a ONE-TIME Reserved->Started CAS. Duplicate/replayed Start never returns permission,
even for the same op: FailedPrecondition/already-started. Before calling Start CSI fsyncs its local
started marker; it spawns mkfs only after the first successful Start reply, with the bound fd and
no other effects. Lost Start reply or crash before tool spawn intentionally sacrifices availability:
no retry format. Nonblank probe on FreshFs refuses without Start; it is never adopted as ReadyFs.
Finish requires Started/current op/epoch and CSI's durable successful tool result; CAS ReadyFs.
Repeated Finish for the same ReadyFs operation succeeds. Lost Finish reply is read back; Started
without locally durable success requires quiesced manual inspection, not another mkfs. Ordinary
cleanup/target movement never clears Started. No API can convert uncertain work to FreshFs.
After ReadyFs, conditionally detach the init entry using #15 tuple, fsync absent, then normal
OnlyIfTarget attach under a fresh epoch with purpose1 FLCH for the current challenge. Only that
normal attachment can mount/publish. FLCT/FLCI bytes and replica protocol stay unchanged.

## Local intent and lost reply (S2)

Operation UUID is generated/fsynced with canonical path, profile and read-only GetVolume FirstEpoch
BEFORE Reserve/Prepare/Attach/device() can cause attachment; tuple=null and phase=reserved_unbound.
Controller target placement itself has no local device cleanup; CSI node reserves before all its
mutating calls. Read attach-challenge, fsync nonce with the intent, send it in Reserve, then fsync
Reserve's operation/epoch before attach-init; verify/pin #15 identity
and fsync bound tuple before any probe/format/mount. A normal noninitializing attach also requires
this initial intent; it never formats. Read the current daemon challenge and use agent-owned
OnlyIfTarget Attach(target_nonce) to obtain an actual fresh epoch and purpose1 FLCH. Fsync the
returned metadata epoch and nonce before daemon attach; never persist either token.
If the metadata Attach reply is lost, no daemon attach was sent: read the incarnation and retry
OnlyIfTarget under a fresh epoch, retaining the intent; no device is inferred from that reply.
Add normal `attach-operation VOLUME OP REPLICA_TOKEN HANDOFF` and `attachment-status VOLUME OP` using
the same op-bound/idempotent registry and status forms above, purpose=normal. Tokens still bind
FirstEpoch/node/epoch; normal command rejects init states and init tickets; each bound/starting reply includes its
verified token epoch, checked against the fsynced metadata epoch before #15 identity binding. Fsync the operation
and returned epoch before dispatch. On lost daemon reply query that exact op, triple-check its
#15 tuple/current epoch/process nonce, then bind. Absence may retry the current handoff only
within that same daemon process before ran_epochs is consumed. A restarted daemon requires a new
challenge and fresh normal Attach epoch; an existing consumed-epoch retry may only return its
existing live device. Changed op/purpose refuses and preserves intent. No legacy attach fallback.

After lost attach reply/crash/binding-fsync failure, the durable intent survives. Query current
metadata op and initialization-status; matching op/FirstEpoch/epoch and #15 triple-check may bind.
Absent Reserved entry may resend attach-init ONLY with its stored nonce still equal to the
current daemon challenge, matching current epoch and unconsumed ran_epochs. A new daemon/nonce
refuses credential recovery and old-epoch starts: preserve bytes/history for quiesced manual
recovery, even if no local replica supplies a fence. Same-daemon plugin restart may recover the
exact live entry; Starting waits with Unavailable. Started may reconcile/detach its matching entry but cannot resend Start or mkfs. Different entry/current
incarnation refuses; never detach it. If local intent is lost, metadata Reserved/Started blocks new
initialization on this or another node; only quiesced manual recovery is allowed. Thus there is
always durable operation authority before a private device, even when no bound tuple was persisted.

Local path schema1 retains R1's exact fields: `schema,path,kind,volume,stage,capability,readonly,
phase,created,leaf_dev,leaf_ino,first_epoch,daemon_instance,attach_generation,boot_id,
mount_namespace,device_path,device_rdev,device_diskseq,mount_id,mount_root,mount_fs,
mount_options,super_options,source_ino`; add `operation,authority_epoch,target_nonce,purpose`.
No local authority_revision exists: RPCs supply epoch/operation, while header-revision CAS remains
server-internal. Never synthesize a storage revision from a returned volume or device reply.
kind=stage|publish; phases=reserved_unbound|bound|preparing|ready|removing. Only reserved_unbound
may omit the tuple; corrupt/missing required fields refuse. Never persist credentials.

## Integration, bounds and tests

Read dependency plans at #76 `3f21c35`, #80 `632f550`; neither implements this amendment. Amend
#76 canonical fullidentity and hidden creation/recovery fixtures to include use_profile and initial
use state atomically; same scalar geometry/different profile is AlreadyExists, never a legacy match.
Amend #80 CheckCreateVolume.desired identity to compare profile too, including its new-format
missing-identity rejection. Actual use state is mutable history, not desired create identity.
Legacy Unknown cannot satisfy an explicit new Ext4/Raw create retry via the scalar exception.

Cap encoded new authority at1024 bytes including framing and nonce32; IDs≤128 ASCII, UUID36, fixed enums,
u64 and no arbitrary history/list. #79's combined schema uses conservative base B=3072 rather than
#80's2048: `3072 + N*(288 + 264*(R+1)) ≤ 4194304`, checked before create/enable and for full replies.
Keep chunk/R scalar limits and RPC caps unchanged. Complete maximum-field Header/Check/Get/List/
Attach/Initialization fixtures include FLCH134 and must fit this profile, otherwise admission
refuses; never truncate an old volume. This is accounting, not permission to raise the RPC cap.
Other optional schema layers compose their own explicitly accounted bounds, not this bare base.

- `meta/domain` pure use-authority model + `flaminio-repl` deterministic simulator adapter checks
  at most one format-start, no grant after raw/unknown use, no grant from missing/local history,
  no public init attachment; mutants regranting FreshFs or reusing Start MUST be caught by `make sim`.
  Independent `NoPublicInitAdmission` oracle observes actual ordinary token/handoff issuance and
  public-target/exposure events against authoritative use history; it does NOT call the admission
  predicate or infer safety from accepted target sets. For FreshFs, Reserved and Started, each
  ordinary credential issuance/public start/publication is a violation. Named mutant
  `bypass_public_init_admission` removes ordinary Meta.Attach's use-state guard: with all normal
  authentication/nonce/CAS conditions valid, a Started schedule mints a normal FLCT+purpose1 FLCH.
  The oracle MUST catch that issuance immediately even if the engine's separate gate still refuses
  public start. Replay the same schedule in service/CSI/daemon integration with zero public effects.
- `meta/service` memory+etcd: new-volume atomic FreshFs, hidden/resume/rollback, Create retry profile,
  node A raw then fresh node B FS with empty cache (zero probe/mkfs), lost profile, legacy/import/
  restore Unknown, all CAS/concurrent ops/target moves/delete-recreate, explicit per-method auth.
- CSI/controller/daemon: positive new Ext4 stage->publish->cross-node remount without second mkfs;
  crash/fsync failure before Reserve, before attach, lost reply, before/after tuple bind, before
  Start, lost Start reply, pre-signature tool kill, successful tool/Finish lost reply. Every missing
  intent prevents attach; every uncertain start prevents tool. Private init cannot publish.
- Go/Rust contract vectors cover ticket purpose/claim mismatch, expired current epoch, op/purpose
  reply mismatch, forged/wrong-node direct callers and unavailable/old metadata admission.
  #83 cases: same-daemon plugin restart recovers exact Reserved operation; daemon restart with
  no local replica fence changes nonce and refuses old credentials/epoch (zero extra target/mkfs);
  Started returns no credentials; normal ReadyFs recovery uses a genuinely newer Attach epoch.
  `make proto test race contracts sim`; no tests/lab/implementation claimed by this document.
