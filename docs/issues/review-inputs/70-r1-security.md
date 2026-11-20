# Issue #70 plan review — round 1, security focus

Scope: `docs/issues/70/` (intent, spec, design, adr-proposal) in `/tmp/flaminio-plans-2026-11-12/70`,
checked against that worktree's code and, where the plan relies on them, the #79 and #80 drafts.
Paths below are relative to the #70 worktree unless they start with `#79:` / `#80:` / `#78:`.

### R1-S1 — High

**XFS formatting contradicts #79's format authority, and XFS is absent from #79's contract.**
Spec S7 says "Format only a verified blank device"; the design says "Stage blank XFS via
`mkfs.xfs DEVICE` only after signature/size checks" (design.md:120). #79, whose rules S7 says it
retains, forbids exactly that: "Formatting is permitted only with the metadata one-shot
initialization grant ... missing history, raw use, restore/import and legacy state never grant
first formatting" (#79: spec.md:42-45, 52-55) and "Existing/Unknown never mkfs" (#79: design.md:93).
#79's contract also has no XFS value: `UseProfile` is Unknown/Raw/Ext4, the FLCI ticket carries
`u8 profile=2` and rejects unknown profiles, and ReadyFs rejects a mismatched fs type
(#79: initialization-contract.md:10, 34-39, 88-93). #70's Contracts and Files-touched sections add
none of this. As written, XFS staging either cannot format at all once #79 lands, or it formats
on a blank-signature probe, which is the data-loss path #79 closes (raw use with zeroed leading
blocks, a killed pre-signature mkfs).
Fix: replace "verified blank device" in S7 and the design with "#79's one-shot initialization
grant plus the signature check"; add the XFS profile to the shared contract (UseProfile value,
`CreateVolumeRequest.use_profile`, FLCI profile byte, ReadyFs fs-type match, #80's
`CheckCreateVolume` identity compare); name a test that a blank-looking Unknown/Raw/uncertain
volume is never passed to `mkfs.xfs`.

### R1-S2 — Medium

**Committed growth turns chunk-slot exhaustion into loss of the prefix's replicas, and the
expected behaviour is not specified.** The design allocates nothing before the commit ("No
physical suffix allocation precedes metadata commit", design.md:50) and never rolls size back.
After a commit, every Hello lists the suffix chunks placed on that node; `hello` creates the
missing ones and fails the whole session if one cannot be created
(`dataplane/crates/flaminio-node/src/replica/mod.rs:247-256`,
`flaminio-store/src/store.rs:1207` `OutOfSpace("chunk slots")`; #80's preflight also makes it a
session refusal). So on the next attach, restage or failover a full replica node refuses the
session for the prefix chunks too: acknowledged data loses a replica, or the volume cannot
attach at all. Any PVC owner can trigger the commit, and another tenant filling the slots
completes it. The ADR describes this only as "a committed larger size pending node completion"
(adr-proposal.md:54-55); the test row "slots exhausted after metadata commit" states no expected
result.
Fix: state the rule and test it. Either a suffix chunk that cannot be created must not refuse
the session for chunks that already exist on that replica (per-chunk refusal, with the suffix
chunk leaving the ISR through the normal path), or suffix replicas are created fenced and empty
before the final commit with the delete inventory covering them. Say what an attach does when no
replica of a suffix chunk can be created. #72's draft requires a reservation before growth is
usable (#72 spec S7); record that dependency.

### R1-S3 — Medium

**A pending growth has no abort or supersede path, and in offline mode it blocks every Attach.**
Placement is per batch after the claim (design.md:35-41); "Failed placement never publishes an
incomplete suffix" (S2) and "Different growth sizes serialize; contenders return Aborted"
(design.md:32-33) leave the record claimed. "Attach refuses while an offline growth operation is
pending" (design.md:64-65). If a batch cannot be placed (a node draining or down under a strict
domain), or the controller crashes and the next request has another size (the resizer's
recovery path lowers the request), nothing completes or removes the operation: the volume cannot
be attached, including by its own agent's `only_if_target` restart and by failover. Only
DeleteVolume clears it.
Fix: define an abort transition (conditional delete of the prepared suffix and of the operation,
legal because nothing physical exists before the commit), used on placement failure and by a
request with a different size; or place every suffix chunk before the claim, as `createVolume`
does (`meta/service/service.go:404`). Bound how long a pending offline operation may refuse
Attach. Tests: placement failure mid-operation then Attach; smaller request after a stuck larger
one.

### R1-S4 — Medium

**The rollout gate is a sentence, not a mechanism; an older metadata binary corrupts the new
state.** `Attach` reads the header into the Go struct and writes it back
(`meta/service/service.go:709-725`), so a pre-growth binary drops `GrowthGeneration`, `Growth`
and the use fields. Dropping the `Growth` pointer during a pending operation leaves "an unknown
suffix without a matching pending operation", which the design declares corruption
(design.md:47-48): the volume becomes unreadable. Dropping `GrowthGeneration` resets a
generation that targets and completions treat as monotonic identity (design.md:86-87, 108). An
older `readVolume` also fails every read while a suffix is prepared
(`service.go:312-318`). The design only says "old header writers must not drop new fields"
(design.md:195), and offline expansion is on by default.
Fix: keep the growth record, committed generation and use state in keys an old writer does not
rewrite, compared with the header in each transaction, so an old Attach fails closed (epoch
mismatch) and loses nothing; or add an etcd-held enable marker that `ExpandVolume` compares in
its claim and that an operator sets after rollout. Name a test with an old-shape header write
between batches and after commit.

### R1-S5 — Medium

**`flaminio.io/growth-v1` and the resize/tool profile are self-asserted, merge-only labels with
no writer, verifier or removal specified.** The design calls them "verified" (design.md:70-72)
but RegisterNode lets a node's own agent or io component set any label
(`meta/service/auth.go:71-72`), and labels are merged, never removed
(`meta/service/service.go:196-200`). After a data-plane or CSI image rollback the label stays.
The metadata service then places suffix chunks on, or allows ONLINE growth with, a node whose
engine indexes past its chunk vector on a longer view
(`dataplane/crates/flaminio-repl/src/target.rs:2510-2512`, a process panic) or whose replica
refuses the larger Hello (`replica/mod.rs:212-220`). S9's "mixed/incompatible nodes cannot
bypass the mode" depends on this label; the "mixed-profile refusal" test does not cover a stale
one.
Fix: say which component writes each capability (io for growth, the CSI node plugin for the tool
profile), make capability labels replaced on every registration of that component (absent means
unsupported) rather than merged, restrict the reserved keys in `auth.go`/RegisterNode, and test
a downgraded node.

### R1-S6 — Medium

**The RESERVED invariant is tied to "metadata Attach", not to the header epoch.** "Every
successful metadata Attach records RESERVED" and offline growth needs "RELEASED at current
use_epoch" (design.md:55-64) do not say that RELEASED is valid only when `use_epoch` equals
`Header.Epoch`. #79's draft adds epoch-advancing paths that start a device without Attach:
`PrepareVolumeUse` and `ReserveInitialization`, followed by `attach-init` and mkfs
(#79: initialization-contract.md:77-86, 104). A new volume is created RELEASED, so offline
growth could commit while that private device exists, against S4's "unproven-use" rule. The same
gap applies to any header write by a binary that does not know the field.
Fix: state the invariant on the header: every write that advances the epoch or changes the
target sets RESERVED in the same write, and ExpandVolume and its final CAS require RELEASED with
`use_epoch == Header.Epoch`. Test an epoch advance by a non-Attach path, then offline expand.

### R1-S7 — Medium

**Suffix placement is not protected against a drain mark.** Batches compare "the node-label
revisions used for that batch's placement" (design.md:36-38), but draining is a separate key
(`meta/service/service.go:825`, read in `nodes()` at 252-259) and liveness another; neither is in
the node record. The prepared suffix is invisible to volume readers, so a drain started after a
batch sees no replica there, completes, and the node is removed; the final CAS then publishes
chunks whose replica set names it. New chunks start under-replicated and the drain's result was
wrong. #78's draft covers the same hazard for hidden creation and is not in #70's coordination
list (design.md:188).
Fix: compare the drain key of every node used, per batch and again at the final CAS (or account
the prepared suffix in drain completion under #78's contract); add #78 to the dependencies; test
a drain mark between a batch and the commit.

### R1-S8 — Low

**The replica's size ratchet has no stated order against the epoch check, and no abuse test.**
"Keep the maximum known size and never shrink it" (design.md:89-91) on an unsigned Hello field.
Today the size comparison runs before the core's fence decision
(`replica/mod.rs:212-234`); a ratchet in the same place lets a deposed target holding an old
signed token raise the bound permanently.
Fix: raise the bound only when the core accepts the Hello's epoch (the `Action::Fence` arm),
after #80's preflight; test a stale-epoch Hello with a larger size and a size above #80's
envelope, each leaving the bound, the store and the fence unchanged.

### R1-S9 — Low

**No path to RELEASED for UNKNOWN volumes or a dead last target.** "Unknown legacy state refuses
offline grow until the owning agent verifies release" (design.md:61-62), and only the target's
agent may release (design.md:151). A legacy volume that was never attached has no owning agent;
a volume whose last target node is gone stays RESERVED. The ADR lists this as open
(adr-proposal.md:64); the design has neither a mechanism nor a test.
Fix: specify it. A never-attached volume is provable from the header (`Epoch == FirstEpoch`,
empty target, `service.go:461-465`, `meta/domain` `Header.Attach`) and can be treated as RELEASED; otherwise document the
attach-then-unstage cycle on another node as the only route.

### R1-S10 — Low

**Mode reconciliation between metadata and CSI is unspecified.** "Require one reconciled
mode/profile; mismatch fails startup/readiness" (design.md:69-70), but no contract carries the
service's mode to the CSI driver, and the request's `online:5 bool` has no stated rule when it
disagrees with the service's configured mode or with the `mode` stored in a pending growth
record on retry.
Fix: store the mode in etcd or return it from an RPC; state that ExpandVolume refuses
(`FailedPrecondition`) an `online` value that differs from the configured mode or from the
pending operation; test both.

### R1-S11 — Low

**The ONLINE profile omits a kernel defect in the command it relies on.** The pinned crate
documents that `UBLK_U_CMD_UPDATE_SIZE` (from v6.16) dereferences a NULL disk when it races a
stop, fixed only by 25966fc09769 (v7.0-rc4, stable), and that libublk's own check "can still
race through that window" (`libublk-0.4.8/src/ctrl.rs:2755-2760`, 1820-1830). The design
serializes resize with the daemon's own detach/stop (design.md:105-106) but a device that dies
otherwise still races, and the result is a node kernel fault.
Fix: make the fix (or its stable backport) part of the validated ONLINE profile in S9 and the
design, in addition to the feature bit.

### R1-S12 — Low

**The `resize` socket line has unbounded and untested inputs.** `resize VOLUME EPOCH GROWTH_GEN
BYTES INSTANCE ATTACH_GEN [MS]` (design.md:155-158): no cap on `MS` (attach clamps to
`ATTACH_LIMIT`, `dataplane/crates/flaminio-io/src/control.rs:153-160`), no rule for `BYTES` that
differs from the committed size fetched from metadata, and the S6/S8 test row lists no malformed
or oversized line.
Fix: clamp `MS`, refuse `BYTES` unequal to the committed size, and add parser tests (arity,
non-numeric, overflow, invalid id) with no engine or device effect.

### R1-S13 — Low

**A crash in delete can orphan the growth record.** Delete "then removes operation state
conditionally" (design.md:48-50), after the header/tombstone transaction. A crash between the two
leaves `growth/<id>`; neither the header-absent delete retry (`service.go:529-535`) nor the
creation claim's leftover cleanup (`service.go:441`) is said to remove it, so a recreated volume
meets a "competing operation" from an earlier incarnation.
Fix: delete the growth record in the transaction that removes the header, and have the creation
claim clear a leftover under its own claim; test delete-crash-recreate-expand.

### R1-S14 — Low

**Hard prerequisites are written as if they existed.** Stale-completion fencing keys on
`(volume,instance,attach_generation,epoch,growth_generation)` and the socket contract says
"Preserve #15 attach/status/list formats" (design.md:108, 155-159); the base has no instance,
attach generation or `list` (`control.rs:84-107`). The local expansion intent lives "in #79's
durable state" (design.md:126-127), which also does not exist. S6/S8's safety against a
replacement attachment rests on them.
Fix: declare #15, #77, #79 and #80 as prerequisites with the commits read, or define the
identity #70 needs here.

## Nits

- **R1-S15 — Nit.** `VolumeUseState` RESERVED/RELEASED, `use_state`, `ReleaseVolumeUse`
  (design.md:54-60, 150-153) collide in name with #79's `Header.use` state Reserved, `UseProfile`
  and `PrepareVolumeUse`, which mean something else. Rename (for example `stage_use`).
- **R1-S16 — Nit.** `spec.md:3` and `design.md:3` link `decisions.md`, which does not exist.

Reviewed commit: 7a5a04e777421c7212a9b1a8daba5af6f28edadd
