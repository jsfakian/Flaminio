# Issue #70: CSI volume expansion and XFS support — design

How the [spec](spec.md) is built: the offline raw block and ext4 step. [Decisions](decisions.md)
record the review's choices and the user's approval of 2026-11-13; nothing is implemented. Review
rounds 2 to 9 came after the approval. Settled 2026-11-13 (user's instruction): accepted (see
decisions.md); among them, the node-wide wait (Risks), the charges and #72 as an ordering
prerequisite (Metadata growth). New architecture: [ADR](adr-proposal.md).

**Prerequisites** (plans read at these commits; none implemented): #18 `a736df6`
(`Header.Placement()`); #77 `8a2233b` (validated `readVolume`, Attach pinned to its revision);
#78 `f672280` (`placement-gate`, `ensurePlacementGate`); #80 `632f550` (`pkg/geometry` admission,
CSI range selection, engine refresh and Hello guards); #79 `6f6df4b` (stage and publish records,
`Inspect`, effective readonly, tools on pinned descriptors); #15 `8af7aeb` (`device-info` pinning);
#72 `db06456` (the capacity ledger, `Room`, `Place` with a room map, `ErrCapacity`). #72 is also an
ordering prerequisite: #70 lands with or after #72, as #72 does for #13 (Charges).

The facts it starts from:
- **CSI:** no expansion RPC or capability (`control/csi/controller.go:88–99`, `node.go:41–52`);
  ext4 only (`controller.go:104–128`); no resizer, expansion off (`deploy/csi/driver.yaml:63–80`,
  `:235`). NodeStage attaches, formats only a blank device, mounts (`node.go:81–128`). The
  image's pinned Ubuntu base brings e2fsprogs, `resize2fs` included (`scripts/dev/image.sh:3–4`).
- **Metadata:** the chunk count derives from size and chunk size (`meta/domain/domain.go:37–59`).
  Creation counts load over every chunk record and places on alive, non-witness, non-draining
  nodes (`meta/service/service.go:384–404`). Attach, ChangeIsr, MoveReplica compare the header
  revision read (`:725`, `:774`, `:907`); deletion marks the header, then lists chunks (`:539–570`).
  A transaction carries at most 120 compares and 120 ops (`meta/kv/kv.go:33`).
- **Target:** built once at attach from the volume's size and chunks (`flaminio-node/src/engine.rs:1596–1679`);
  `Attached` is reported then, before any chunk is fenced (`:1490–1500`). Client operations queue
  whatever a chunk's phase, with no deadline (`flaminio-repl/src/target.rs:747–766`); a chunk
  leaves Fencing once its ISR is fenced (`:2218–2252`). The refresh converts the whole reply
  (`engine.rs:1544–1579`); `apply_view` indexes the chunk vector with every chunk of the view (`target.rs:2510–2512`).
- **Replica:** Hello refuses a known volume of another size or chunk size (`flaminio-node/src/replica/mod.rs:213–220`)
  before it calls the core (`:234`), which accepts a Hello not below the fence
  (`flaminio-repl/src/replica.rs:170–184`). The Fence arm creates missing placed chunks, failing the
  session on any error (`replica/mod.rs:235–258`). Recovery takes a volume's size from the first
  chunk found (`:137–144`, `:156–172`).
- **Device:** size set at creation (`flaminio-io/src/ublk.rs:948–958`); no socket command changes
  it (`flaminio-io/src/control.rs:84–107`); detach stops it (`:192–212`) and every attach adds a
  new one (`ublk.rs:945`).

## Metadata growth (S1–S3)

New `meta/service/expand.go`, `ExpandVolume`, in this order:
1. `!s.opts.VolumeExpansion`: `FailedPrecondition` `volume expansion is off: run flaminio-meta
   serve with --volume-expansion`. Then, from #73 (Charges), a context with neither the
   `inactiveRules` mark nor a role: `PermissionDenied` `no-role`, active or not, as #73, before
   any read. Then `!domain.ValidID(id)` or `size_bytes == 0`: `InvalidArgument`.
2. **Load**, once per call: count replicas per node over every chunk record, as creation does
   (`service.go:384–396`); each committed step adds its placements. Load is a balance hint only.
3. **A step**, with the service's `retries` attempts and `backoff`. Before each attempt, as #73's
   race rule (#73 `design.md:65–68`): a call Authorize marked `inactiveRules` for which `s.active`
   is now true answers `Aborted` `tenancy was activated during this call: retry`, committed steps
   kept. Each attempt then reads, in order,
   `ensurePlacementGate` and the gate's revision G (#78, as its MoveReplica), `readHeader` (absent,
   creating, deleting: `NotFound`), `s.nodes()` with #72's reports, then #72's ledgers, then
   #73's `tenant/authorization` and `owners/<id>`, present or absent, and, for a tenant owner,
   the policy and `quota/<ns>/<class>` (all revisions the transaction compares). Then:
   - `h.SizeBytes >= size_bytes`: answer `readVolume` (#77). No write.
   - **#73's checks**, nothing written in each case, for a call with a role (so the store is
     active): no owner record read, `FailedPrecondition` `unassigned`, as #73's DeleteVolume,
     even if the volume is assigned after the step's read (a later call reads the new owner and
     charges it); a role other than the owner kind's (`csi-controller` for a tenant,
     `system-provisioner` for `system`), `PermissionDenied` `wrong-kind`.
   - A current geometry #80 refuses (partial last chunk, over its caps): `FailedPrecondition`, as
     #80's Attach. #80's admission of `(size_bytes, h.ChunkBytes, h.Replicas)` fails: `InvalidArgument`.
   - `h.Placement()` fails: `Internal`, #18's message.
   - `k = min((size_bytes − h.SizeBytes)/h.ChunkBytes, growStep)`, `const growStep = 64`.
   - `domain.Place(candidates, k, h.Replicas, h.Target, level, policy)`, candidates as creation
     (`:398–402`), with #72's room (Charges); `ErrPlacement`: `FailedPrecondition`.
   - `h.SizeBytes += k*h.ChunkBytes`; one `Txn` comparing `volumes/<id>` at the header's revision
     and `placement-gate` at G; ops: `chunks/<id>/<i>` for the k new indices, each built as
     `NewVolume` builds a chunk (ISR = sorted replicas, version 0, no move; `domain.go:199–208`),
     and the header marshalled from the struct read, so every other field stays.
   - Applied: add the placements to the load; the next step starts with fresh attempts. Not
     applied: the next attempt reads everything again.
4. Attempts exhausted: `Aborted` `too much contention`. KV error: `errStatus` (`Unavailable`).
   Context: `Canceled` or `DeadlineExceeded`. Committed steps stay in every case.
- **Two compares suffice:** every other chunk-record writer compares this header and writes below
  its count (`:774–790`, `:886–909`) or runs while it is hidden (creation, deletion); #77's read
  refuses a stray record. The gate covers drain marks and registrations (#78); liveness is a hint.
- **Preferred node:** the last target, as creation prefers the first consumer's node
  (`controller.go:233`); `Place` drops it if it is not a candidate (`domain.go:367–369`).
- **Unchanged:** Delete (a step after its mark fails its compare; one before is listed). Attach (a
  step between its commit and #77's pinned read makes it answer `Aborted`; its caller attaches
  again). No header field is added: an older metadata binary serves a grown volume as created.
- **Charges (#72, #73), the one rule:** each step charges #72's capacity ledger and #73's
  namespace and class quota in its own transaction, by their rules. #70 lands with or after #72,
  so #70 adds the ledger charge, with its test: it is the second lander of #72's "whichever of
  #70 and #72 lands second", and no growth ever runs uncharged (no refusal is needed until then).
  Whichever of #70 and #73 lands second adds #73's checks (step 3), the quota charge and its
  compares, and #73's `ExpandVolume` row (Contracts), with their tests.
  - **Ledger:** placement as #72's CreateVolume: soft watermark, `Place` with the room map, the
    preferred node only if its room holds the step's chunks, nodes without a valid report or
    readable ledger left out. The transaction compares each used node's ledger and writes it with
    the charge.
  - **Quota:** every step's transaction compares `tenant/authorization` and `owners/<id>`: a call
    marked `inactiveRules` compares `tenant/authorization` absent, as #73's race rule, and a call
    with a role at the revision read; `owners/<id>` at the revision read, absent included. So an
    activation anywhere between the race check and the commit, or any change of the owner record,
    makes the commit fail, and the next attempt's race check or reads decide. Once active, for a
    tenant owner, the step's added bytes go to the owner's namespace and class by #73's rule
    (its owner record and class usage written; those and the policy compared); a system owner
    record charges nothing.
  - No room or no quota: `ResourceExhausted`, nothing written, earlier steps kept and charged. A
    ledger or quota conflict reads again, like a header conflict. Counting every key above, the
    transaction must fit 120 operations (1 + k + D, plus #73's two writes) and 120 compares (2 + D
    + 2, plus the policy and quota key): k shrinks to the longest prefix of the placement that
    fits; one chunk always does. Delete releases both through their rules.
- **Flag:** `Options.VolumeExpansion bool` (`meta/service/auth.go:20`), from `flaminio-meta serve
  --volume-expansion`, default off. Operators add it in a second rollout, once every metadata
  instance and data plane runs this version; nothing enforces that order. An older binary given
  the flag exits with status 2 (`flag.ExitOnError`, `meta/cmd/flaminio-meta/main.go:84`) and
  systemd restarts it in a loop (`Restart=on-failure`, `scripts/guest/meta.sh:34`), serving
  nothing: a metadata rollback turns the flag off first. A data plane rollback after a growth
  stays unsupported. The usage and `main.go`'s doc (`:19–23`) say so, as #75's `--qos`'s do:
  `allow volumes to grow; only once every metadata instance and data plane runs this version: an
  older data plane cannot serve a grown volume, and an older metadata binary does not start with
  this flag, so remove it before a metadata rollback; a data plane rollback after a growth is
  unsupported`. Lab: #75's design, "Lab flags of the metadata service (S5, shared with #70)"
  (9083e69): `make meta-flags VOLUME_EXPANSION=0|1`, and its rule for every lab check of such a
  flag. Whichever of #70 and #75 lands first adds that mechanism; the other its flag and variable.
- **Admin CLI:** `flaminio-meta [--server host:7400] expand <volume> <size>` calls `ExpandVolume`
  and prints the volume, as `get` does (`main.go:404–407`); its line joins the doc and `usage`.

## CSI (S1, S4, S7, S9)

`ControllerExpandVolume` (`controller.go`):
1. No volume id or capacity range, or #80's signed range check fails: `InvalidArgument`. A
   capability given: `checkCapabilities` (block or ext4; XFS stays `InvalidArgument`).
2. `Meta.GetVolume`: its error as returned (`NotFound`). V is its size, C its chunk size.
3. Positive limit L and `V > L`: `OutOfRange` `volume V is N bytes, above the limit L: volumes do
   not shrink`. `V >= required`: answer V, with no metadata write.
4. T is #80's checked rounding of `required` up to C. `L > 0 && T > L`, or #80's admission of
   `(T, C, RF)` fails: `OutOfRange`.
5. `Meta.ExpandVolume(id, T)`: `Unimplemented` becomes `FailedPrecondition` `the metadata service
   has no ExpandVolume: upgrade it`; any other code as returned.
6. Answer the returned size (checked `int64`) and `node_expansion_required: true`.

Capabilities: Identity adds `VolumeExpansion` `OFFLINE`; controller and node add `EXPAND_VOLUME`.
Deploy: `driver.yaml` adds a `csi-resizer` sidecar, pinned by tag, with the attacher's args,
`securityContext` and `socket-dir` mount (`:71–76`: `--csi-address=/csi/csi.sock --leader-election
--leader-election-namespace=flaminio-system --timeout=60s`), and `allowVolumeExpansion: true`.
`rbac.yaml` adds persistentvolumeclaims/status patch and pods list, watch (the rest is granted,
`:31–45`, `:69–90`). A call cut at 60 s is retried and resumes from the committed size.

`NodeExpandVolume` (`node.go`), under `n.mu`; it writes no local state and never calls mkfs:
1. No volume id, volume path or capability: `InvalidArgument`; then `checkCapabilities` and #80's
   signed range check on an optional range.
2. #79's records: the stage at `staging_target_path` if given, else the stage or publication at
   `volume_path` (a publication leads to its stage), this volume's, phase `ready`. None: `NotFound`
   `V is not staged on this node`. Capability unlike the record's normalized one: `FailedPrecondition`.
3. Pin the device as #79's stage does under #15 (`device-info`, no-follow open, `device-info`
   again). A mismatch: `FailedPrecondition`, no effect.
4. `Mounter.DeviceBytes(fd)` (`BLKGETSIZE64`). `required > size`: `FailedPrecondition` `V's
   device is S bytes, R needed: unstage and stage it again (offline expansion)`. A limit is not
   checked: the controller chose the size. Block: answer `size`.
5. Filesystem: effective readonly (#79), or #79's `Inspect` showing a filesystem other than ext4
   or another device: `FailedPrecondition`. `Mounter.GrowExt4(ctx, fd)` runs `resize2fs --
   /proc/self/fd/N`, the pinned descriptor a child FD (#79's tool rule). Failure: `Internal` with
   at most 4 KiB of its output. Success: answer `DeviceBytes`. Context end: SIGKILL, then
   `DeadlineExceeded` or `Canceled` once the child exits; the kernel's online resize is journalled,
   so a retry runs it again. A child blocked on a chunk no replica serves cannot exit (Risks: the
   node-wide wait).

## Targets, replicas and the device (S5, S6, S8)

- **Core:** `apply_view` takes `view.chunks.iter().take(self.chunks.len())`: a view updates the
  chunks the target attached with; a shorter one applies as today. `on_meta`'s epoch rules stay
  (`target.rs:1099–1107`). Injected bug `target::Bugs::drop_grown_view`: `on_meta` skips a
  same-epoch view with more chunks than the target holds.
- **Engine refresh** (`engine.rs`, `MetaWait::Refresh`, `:1544–1579`): #80's check runs on the raw
  `pb::Volume`, before `rt.chunk_meta` (`:1555`) can add names to the node table (`:386–395`) and
  start a ListNodes call (`:1567–1574`). It compares a same-epoch reply with the attached
  geometry, which `TargetRt` keeps and never replaces. It passes
  equal chunk size, RF and minISR, a size equal or larger by whole chunks, and that size's record
  count; the engine hands the reply whole to the core. Any other same-epoch reply is refused as #80
  says: not installed, the core not called. Names only suffix chunks carry get ids, no session.
- **Attach:** unchanged. `start_target` builds from the committed size; every chunk starts in
  Fencing, so the suffix is fenced and reconciled before I/O; the device is created at that size.
- **Replica Hello** (`replica/mod.rs:213–220`): refused (same message) when the chunk size differs,
  or the size is smaller than known and `a.epoch >= self.core.fenced(v.id)`. A larger size goes on;
  in the Fence arm, once chunk creation and `store.fence` succeed, the known size becomes the
  Hello's. A Hello below the fence reaches the core as today and gets `Stale`: size, chunks and
  fence unchanged. #80's preflight runs first. The local path (`engine.rs:1955`) calls the same function.
- **Replica recovery** (`:137–144`): the known size is the largest `volume_size` of the volume's descriptors.
- **Slots exhausted at the attach after a growth:** #80's preflight refuses the Hello before the
  core runs, so that node's fence for the volume stays at the old epoch: for this volume it is an
  unreachable replica, safe under ADR 0005 (the new target fences another member before serving).
  A chunk with another replica follows the member-failure rules; a chunk with none queues its I/O,
  with no deadline, while `Attached` has been reported, until one of its replicas accepts the
  session, which needs slots there for every chunk of the volume (Risks: the node-wide wait). The
  Fence arm's failure (`:252–254`) is left to store faults: the known size is then not raised.
- **Format:** no byte, record or decoder change, so no version rule and no fuzz target.
  `on-disk-format-v1.md:196` now says `volume_size` is the volume's size when the chunk was
  created: a grown volume's chunks differ, and the largest is a lower bound of its size.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): S2, S4.
- `meta/service/expand.go`, `expand_test.go` (new); `auth.go` (`Options`, `:20`), `auth_test.go`; `meta/cmd/flaminio-meta/main.go`, `main_test.go`: S1–S4, S8, S9.
- `control/csi/controller.go`, `node.go`, `meta.go`, `mount.go` (with #79's `mount_linux.go`), `csi_test.go`: S1, S4, S7–S9.
- `dataplane/crates/flaminio-repl/src/target.rs`, `meta.rs`, `sim/mod.rs`, `sim/tests.rs`; `flaminio-node/src/engine.rs` (#80's refresh check), `replica/mod.rs`, `replica/tests.rs`, `tests/engine.rs`: S5, S6, S8.
- `deploy/csi/driver.yaml`, `rbac.yaml`; `scripts/lab.sh` (`csi_expand`), `deploy/lab/csi-expand-block.yaml` (new), `Makefile` (`csi-expand`), `scripts/tools/meta-flags-test.sh`; #75's lab-flag mechanism (`meta.sh`, `cmd_meta`, `meta-flags`) if #70 lands first: S9.
- `docs/format/on-disk-format-v1.md`, `docs/replication-protocol.md`, `docs/ARCHITECTURE.md`, `README.md`, `docs/lab.md`, `docs/followups.md`: S5, S9.
- **Not touched:** `flaminio-metaclient`, `flaminio-io`, `flaminio-net`, `flaminio-store`, `flaminio-format`.

## Contracts

- **Proto:** `rpc ExpandVolume(ExpandVolumeRequest) returns (ExpandVolumeResponse);` commented
  "Grows a volume to at least size_bytes by whole chunks; never shrinks. Administrators only."
  `message ExpandVolumeRequest { string id = 1; uint64 size_bytes = 2; }`,
  `message ExpandVolumeResponse { Volume volume = 1; }`. No `Volume` field, no other RPC changes.
- **auth.go:** `ExpandVolume` joins the administrator-only case (`auth.go:64`). #73's role table
  gains, from whichever of #70 and #73 lands second: `| ExpandVolume | administrator name |
  csi-controller or system-provisioner; the method requires the one of the owner's kind; an
  unassigned volume is refused | – |`.
- **Answers:** Metadata growth, steps 1–4; success is the volume at its committed size.
- **Go client:** `(*Meta).ExpandVolume(ctx, id string, size uint64) (*metav1.Volume, error)`. The
  Rust client calls nothing new; `make contracts` regenerates `api/gen` and runs interop.
- **Server flag:** `--volume-expansion` (bool, default false), with the usage above. Lab: `make meta-flags VOLUME_EXPANSION=0|1` (#75's).
- **CSI:** the codes and messages above; `Mounter.DeviceBytes(*os.File) (uint64, error)`,
  `Mounter.GrowExt4(context.Context, *os.File) error`, and their fakes. **Lab:** `make csi-expand`.
- **None:** control-socket lines, wire frames, tokens, on-disk records.

## Tests

| Rules | Test (file, case, level) |
|---|---|
| S1, S4 | `csi_test.go` `TestControllerExpand`: negative and contradictory ranges, V above a positive limit (OutOfRange, no ExpandVolume call), V already enough (no call), rounding to C, over #80's cap, XFS, NotFound, Unimplemented, ResourceExhausted returned as is, ExpandVolume answering more than T (that size is answered), node_expansion_required for block and ext4, target and epoch unchanged; capabilities. Unit. |
| S2 | `expand_test.go` (memory and etcd): `TestExpandAppendsChunks` 2→5 chunks, prefix records byte-equal, suffix by recorded zone/strict and RF, ISR = replicas, epoch, target and the header's JSON keys unchanged; `TestExpandSteps` +150 chunks is three transactions; an `interleaveKV` hook after the first applied write of `volumes/v` (`txnKey`) drains every node, so the second step answers `FailedPrecondition` with +64 committed and readable, and after undraining a retry ends at +150; `TestExpandRefusals` each refusal of steps 1–3 (the refused step writes nothing; earlier steps stay); `TestExpandFaults` with a new `faultKV` wrapper (every `Txn` from the second not applied; the second `Txn` an error; the context cancelled after the first): `Aborted`, `Unavailable`, `Canceled`, each with the first step committed; `TestExpandLegacy` first epoch 0 grows. `TestExpandChargesLedger`: each step raises its nodes' ledgers by its chunks; a step beyond a node's room answers `ResourceExhausted` with earlier steps kept and charged, for bytes and for slots alone (bytes fitting); nodes being initialized, without a ledger, with an unreadable one or without a valid report get nothing; a preferred node whose room is below the step's chunks is not preferred; a ledger rewritten before the commit makes it read again; with many nodes, the 120 bound holds; delete releases every chunk. `TestExpandChargesQuota`, added by whichever of #70 and #73 lands second, on both stores, calling the handler directly with the context Authorize would leave: a step's bytes charged to the owner's namespace and class; over the limit, `ResourceExhausted` with earlier steps kept; a system owner record charges nothing; a context with neither the mark nor a role answers `PermissionDenied` `no-role`, active or not, also for a volume already at the requested size, with nothing written; with the other role, `wrong-kind`; on an active store, no owner record answers `FailedPrecondition` `unassigned`, nothing written, also when an `interleaveKV` hook after the read of `owners/<id>` assigns the volume, and the call ends there; a hook after the read of `quota/<ns>/default` that charges another volume of the namespace makes the step read again and charge once. The race, with the `inactiveRules` mark: an `interleaveKV` hook after the step's read of `owners/<id>` (the first Get of that key, after the attempt's `s.active` check) activates tenancy, or activates and assigns the volume; the step's commit fails on the `tenant/authorization` compare, and the next attempt answers `Aborted` with earlier steps kept and no chunk uncharged. Without that compare the step commits uncharged, so the test fails. With the mark, a hook after the step's read of `volumes/v` (the first Get of that key on a direct call) activates tenancy: the step reads `tenant/authorization` present, its compare of it absent fails, and the next attempt answers `Aborted` with nothing written; a compare at the revision read would commit uncharged, so this case fails. Activation between two steps answers `Aborted`; a later `AssignOwner` charges the grown size; delete releases it; the 120 bound with #73's keys. |
| S3 | `expand_test.go` `TestExpandRaces`, `interleaveKV` hooks: after the header read (`key: volumes/v`) an Attach, a ChangeIsr or MoveReplica of chunk 0, a DeleteVolume (NotFound; tombstone names the committed suffix nodes), a second ExpandVolume (the larger wins, no index written twice); after the drain list (`prefix: drain/`) a DrainNode of a chosen node or a RegisterNode label change (the gate moved: placed again, without the drained node). |
| S4 | `auth_test.go`: admin allowed; agent, io and another node's identities `PermissionDenied`. Once #73 lands (whichever of #70 and #73 lands second adds it): on an active store, Authorize lets `csi-controller` and `system-provisioner` through and refuses `cluster-admin`, `deletion-override`, `audit-reader` and a bare administrator name; the owner-kind check is the handler's (S2 row, `TestExpandChargesQuota`). |
| S5, S8 | `sim/tests.rs` `grown_view_keeps_prefix_isr_changes` (scripted: attach, `Action::Grow`, cut a replica's link for good, write: it completes; with `drop_grown_view` it never does and the run fails settling). The seed sweep with `Config::grow_pct` (drawn last): `Metadata::grow` appends a chunk, which the sim creates empty on its placed replicas at once, as `build` does (`sim/mod.rs:392–396`), so the sim does not exercise the Fence-arm creation (`replica/tests.rs` does); clients draw from the live target's chunk count (`:1049`), moves and the final check from the metadata's (`:1262`, `:1352`); `Report::growths` and `Report::suffix_writes` (writes completed on appended chunks, so by a target attached after the growth) are both > 0 on some seeds. `target.rs` `apply_view_ignores_appended_chunks`. |
| S5, S6, S8 | `replica/tests.rs`: `a_larger_hello_raises_the_size`, `a_stale_larger_hello_changes_nothing`, `a_smaller_hello_is_refused`, `recovery_keeps_the_largest_size`, `a_failed_creation_keeps_the_old_size` (`fail_after` fails the second suffix chunk's creation: the Hello errs, a test accessor `known_size` is unchanged; after a crash and reopen the created chunk is fenced at the new epoch, a Hello at the old epoch gets `Stale`, and the larger Hello creates the rest and raises the size). `tests/engine.rs`: `a_grown_view_leaves_the_running_target_alone`; `a_malformed_refresh_is_refused` (smaller size, not whole chunks, changed chunk size, RF or minISR, record count unlike its size: each refused, a prefix ISR change it carries not applied, the node table unchanged, no ListNodes call); `an_attach_after_growth_serves_the_new_chunks` (with store reopen); `a_full_replica_refuses_the_grown_volume`, with the full node remote and with it the target's own (`engine.rs:1955`): the ISR drops it, data is read from the other, its fence for the volume stays at the old epoch; `a_suffix_chunk_without_a_slot_waits` (RF 1, the full node holds prefix chunk 1 and suffix chunk 2: chunk 0's I/O completes, chunks 1 and 2's waits, none fails; then a slot freed on the full node and time past `reconnect`: the waiting I/O completes). |
| S6–S8 | `csi_test.go` `TestNodeExpand` with fake owners, device-info and tools: block; ext4 (one `resize2fs`); a publication leading to its stage; device too small, readonly, another fs, capability mismatch, unowned path, a record not `ready`, `device-info` changing between its replies, a stage mounted from another device (each with no tool run); tool failure (output cut at 4 KiB); `GrowExt4` honouring cancellation (`DeadlineExceeded`) and ignoring it (the answer comes once it returns, `n.mu` held until then); repeat. `TestNodeStage` with an `FSType` fake that ignores cancellation: NodeStage answers once it returns, and the volume's NodeUnstage waits for `n.mu` until then. |
| S9 | `scripts/tools/meta-flags-test.sh` (#75's, run by `make lint`) gains: `VOLUME_EXPANSION` set, cleared and kept, alone and with `QOS`; `VOLUME_EXPANSION=2` refused before any remote call; `csi_expand`'s probe with a fake `metactl`: probe ids `csi-expand-probe-<vm>-<T>-<P>` that `valid_id` accepts; only w's instance refuses the expansion: the check fails naming w, and a `delete` of w's probe is recorded; all passing deletes every probe. `meta/cmd/flaminio-meta/main_test.go`: `expand` parses its arguments and prints the volume. Capability tests (S1, S4 row). The lab check below. |

Proved by `make lint test race sim contracts`. Nothing ran while drafting.

## Lab

`make csi-expand` (`csi_expand` in `scripts/lab.sh`), after `make csi-deploy`, `make meta-up` and
`make meta-flags VOLUME_EXPANSION=1` at this build. Its flag check follows #75's rule for such
checks (9083e69): for each VM of `cluster`, through that VM's own instance (`metactl "$VM_A"
VM`, `scripts/lab.sh:529–535`, `:671–675`), it creates `csi-expand-probe-<vm>-<T>-<P>` (T the
run's unix time, P lab.sh's pid, checked with `valid_id`; 1M, one 1 MiB chunk, RF 1), expands it
by one chunk, then deletes it. An instance that refuses or lacks `ExpandVolume` stops the check
naming that VM. Its probes are deleted by the trap rules of #75's `qos-check` probe (`code =
NotFound` in the `2>&1` output counts as deleted), the trap cleared once every probe is deleted.
It reads no flags file. Then it waits for the controller pod, resizer included, to be Ready, and
sets its own trap. It starts as
`csi_test` does (`lab.sh:1105–1106`): it detaches the lab volume on every VM (`:1083–1088`) and
deletes the `flaminio-test` namespace, with any volume an earlier `KEEP=1` kept. A node serves one
volume at a time (`node.go:32–36`), so the modes run in turn. Each resize: scale the StatefulSet to
0, wait for its VolumeAttachment to go, resize the PVC from 4Gi to 6Gi, wait for the PV at 6Gi,
scale to 1.
- **Block first:** `deploy/lab/csi-expand-block.yaml`, in namespace `flaminio-test` (so the
  start's deletion covers a block volume a failed `KEEP=1` run kept), a checksummed region at 0.
  Pass: `blockdev --getsize64` is 6Gi; a checksummed region at 4 GiB is written; both verify,
  then again on the other node. The namespace is then deleted, always.
- **ext4:** `deploy/lab/csi-test.yaml` (a checksummed 256 MiB blob). Pass: `verified`, PVC
  capacity 6Gi, a checksummed file larger than the free space before growth written; both verify,
  then again on the other node. Then `(KEEP=1 csi_failover) || die "csi-expand: failover failed"`
  (`lab.sh:1142`), which powers the pod's VM off and on: its EXIT trap (`:1158`, cleared at
  `:1184`) stays in the subshell, and `KEEP=1` keeps the namespace it would delete (`:1200–1203`).
  Then an exec verifies the added file and the PVC's 6Gi, before the cleanup below.
- **Restore:** a trap uncordons and, unless `KEEP=1`, deletes the namespace whatever happens. No
  disk is reformatted. Runs only on request. Repeated requests: the unit tests.

## Docs

`README.md` and `docs/ARCHITECTURE.md` §17.1: offline expansion of block and ext4, online and XFS
later. `docs/lab.md`: `make csi-expand`, results once it has run. `docs/followups.md`: online
resize, XFS, and #73's quota charge, until whichever of #70 and #73 lands second adds it.
`docs/replication-protocol.md`: the attached-size rule and the replica's known size; the format
doc as above. `docs/performance-plan.md`, benchmarks: none. `README.md`'s operator upgrade note:
the second rollout that adds `--volume-expansion` (one note with #75's `--qos`), the rollback
rule (flag off first), and the node-wide wait (Risks). Makefile help: `csi-expand`; #75's
`meta-flags` help gains, after its warning, `; VOLUME_EXPANSION: a data plane rollback then cannot
serve grown volumes`.

## Risks

- **Safety:** no epoch, ISR or version of an existing chunk changes; the suffix is fenced like any
  chunk at attach; a stale Hello cannot raise a replica's size. **Crash points:** a step is one
  transaction; an interrupted request leaves a smaller committed size, usable, that a retry
  resumes; an interrupted `resize2fs` is run again.
- **Security:** only administrators call `ExpandVolume`; NodeExpand touches only #79-owned stages
  and #15-pinned devices; ids are validated before any read.
- **Availability, accepted 2026-11-13:** a node out of chunk slots at the attach after a growth
  refuses the session; its prefix chunks lose that member or stall at minISR. Any PVC owner
  allowed to resize can cause it. Expansion does not wait for #72, which later reserves capacity.
  *Changed after approval (rounds 5, 6).* Settled 2026-11-13 (user's instruction): #70 lands only
  with or after #72, so growth always charges #72's ledger (Charges) (see decisions.md).
- **The node-wide wait: found after approval.** Settled 2026-11-13 (user's instruction): accepted
  as stated here (see [decisions](decisions.md)). Growth that does not fit #72's ledger is refused
  before commit (Charges), so one case is left: a node's physical chunk slots run out under its
  ledger (copies no record names, charges an older binary dropped). That node then refuses the
  grown volume's session at the next attach (Targets, Slots exhausted), and a chunk none of whose
  replicas accepts it waits, with no deadline. For ext4, every node plugin step that reads or
  writes the device under `n.mu` waits with it: NodeStage's `blkid -p` (`node.go:108`,
  `mount.go:37–40`) or `mount` (`node.go:124`, `mount.go:69–77`), NodeExpand's `resize2fs`, and
  NodeUnstage's `umount` (`node.go:135–137`, `mount.go:81–84`), which flushes the filesystem's
  writes. Every other call there that takes `n.mu` waits behind it. Today a node serves one
  volume, so its staging stops; with #15, other tenants' volumes on it stop too. Raw block: the
  pod's I/O to the chunk waits. This plan gives no release beyond the operator's investigation of
  why the node's physical use exceeds its ledger, which #72's reports show. Until #73 lands, only
  a Kubernetes ResourceQuota on `requests.storage`, which counts resizes, bounds a tenant's
  growth; once both land and tenancy is active, #73's quota bounds growth of tenant-owned volumes,
  and unassigned volumes do not grow (Charges). Settled 2026-11-13 (user's instruction): accepted
  as stated (see decisions.md).
- **Compatibility:** an older replica refuses a grown volume's Hello and an older running target
  panics on a grown view: the flag goes on only after every data plane node is upgraded; a data
  plane rollback after a growth is unsupported. Older metadata binaries answer `Unimplemented`.
- **A CO that breaks OFFLINE:** growth still commits; the running target keeps its size; moves of
  suffix chunks wait for the next attach; NodeExpand refuses until a restage. Steps make its
  ChangeIsr and MoveReplica attempts retry; a long growth can exhaust them (`Aborted`).
- **Tools:** `resize2fs` on a `/proc/self/fd` operand must find the mount by device number (the
  lab check and #79's ext4 test prove it). **Settled** ([decisions](decisions.md)): `growStep = 64`;
  the last target as preferred node; expansion advertised while the metadata flag is off
  (refusals name the flag); the operator's rollout order, with no automatic version check.

## ADRs

- **Relied on, consistent:** 0001 (Go metadata and CSI, Rust core); 0002 (conditional etcd
  transactions); 0003 (offline needs no `UPDATE_SIZE`: 6.6 stays enough); 0005 (leases unchanged;
  the suffix is served only after a new target's fence). 0004 and 0006: untouched.
- **Accepted with this design (2026-11-13):** [growth in committed steps, served from the next
  attach](adr-proposal.md). It moves into `docs/adr/` with the implementation.

## Parallelizable?

Yes. The Go units (metadata, CSI, auth, deploy) and the Rust units (core, simulator, replica)
share no new contract: Rust calls no new RPC and decodes no new field. The lab check joins them.
