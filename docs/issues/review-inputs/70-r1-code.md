# Issue #70 — plan review, round 1, code focus (design-code + plan code checklist)

Scope: `docs/issues/70/{intent,spec,design,adr-proposal}.md` in `/tmp/flaminio-plans-2026-11-12/70`,
checked against that worktree's code and, where the plan relies on them, the drafts of #79, #80,
#77, #76, #15 and #18. The code facts the intent and design start from (file:line claims, libublk
0.4.8 `get_features`/`update_size`, ADR 0003's 6.6 baseline, #18's tags 9/10) are correct.

### R1-C1 — High
**Online growth has no workable way to create and fence suffix chunks on a replica that already
has a session; the design's "refreshed session" contradicts the core.**
Design §Core, sessions (lines 89–95) says old sessions "remain bounded to their admitted range",
suffix operations "require a refreshed session listing the placed chunks", "reconnect uses
existing session IDs/fencing rules without dropping acknowledged history", and "No new frame".
The code does not allow that combination:
- A replica creates chunks only while handling `Hello`
  (`dataplane/crates/flaminio-node/src/replica/mod.rs:234–257`), and a target's chunk leaves
  `Phase::Fencing` only on a `Fenced` reply, which only `Hello` produces
  (`flaminio-repl/src/target.rs:1311–1328`, `flaminio-repl/src/replica.rs:173–184`).
- A replica serves one session per volume; a newer `Hello` replaces it, clears the resync filters
  and drops the older session's requests (`replica.rs:175–190`). The engine closes the old
  connection when it sends a new `Hello` (`flaminio-node/src/engine.rs:1990–1997`). Two sessions
  to one replica cannot coexist.
- The target has one session per peer; replacing it is `fail_peer`, which calls `member_failed`
  for every chunk with that member in the ISR (`target.rs:1214–1309`): an ISR removal wherever
  the ISR is above minISR (the default RF 2 / minISR 1), a write stall otherwise.
So with the design as written, every online growth removes each suffix holder from the ISR of all
its prefix chunks (or stalls prefix writes), which breaks the spec invariant ("preserves ...
membership and durability") and S3/S5 ("Prefix I/O and its ISR ... remain valid", "without ...
losing in-flight writes").
**Fix:** choose and specify one mechanism in the sans-I/O core, with its simulator check: (a) a
same-epoch session reopen that does not count as a member failure (retransmit of in-flight
writes as `target.rs:1330–1344` already does, effect of `filters.clear()` on a running resync,
uncertain-extent handling of the second `Fenced`); or (b) an in-session "add chunks" request,
which is a new frame and withdraws "No new frame/token version"; or (c) accept the ISR
remove-and-rejoin and change the spec invariant/S3/S5 to say so. Cover the local replica path
(`engine.rs:1147`) too.

### R1-C2 — High
**A pending growth can never be abandoned or re-placed; in offline mode that makes the volume
unattachable.**
Design §Admission (lines 27–41, 64–65): the operation is claimed first, placement is computed per
batch, "A retry retains already placed suffix chunks; never re-place them", "Different growth
sizes ... return `Aborted`", "Attach refuses while an offline growth operation is pending". Spec
S2/S3 define only completion and Delete. When placement fails after the claim (fewer eligible
nodes than RF under a strict policy, a node down, nodes without `growth-v1`), the operation stays
pending: every Attach of an otherwise healthy volume is refused, and a request for another size
(Kubernetes lets the user lower a failed resize) is `Aborted` forever. Also, earlier batches are
never revalidated: the final CAS checks only "complete cursor and unchanged incarnation/committed
old size", the batch compare covers node-label revisions but not `drain/<node>`, and the drain
controller cannot see a pending suffix (`control/drain/drain.go:50–65` reads committed volumes
only), so a drain can report 0 replicas left and the growth then commits chunks on that node.
**Fix:** add an abort transition (conditional delete of the suffix records and the operation;
safe because "No physical suffix allocation precedes metadata commit"), say who triggers it
(placement failure, a request for a different size, an operator), and let Attach proceed after
it. Revalidate or re-place prepared chunks whose nodes became ineligible before the final CAS.
Add both to S2/S3 and to the `expansion_test.go` row.

### R1-C3 — Medium
**The design contradicts the #79 draft it builds on.**
- XFS formatting: design line 120 formats "only after signature/size checks". #79 S3 and
  `79/initialization-contract.md` allow a first format only under the metadata initialization
  grant, with `UseProfile {Unknown, Raw, Ext4}`, an FLCI ticket that rejects unknown profiles,
  and `ReadyFs` refusing a mismatched fs type. XFS needs a `UseProfile` value, the ticket/profile
  change and CreateVolume's capability normalization; none is in #70's contracts or files.
- Names and states collide: #79 adds `Header.use`, `PrepareVolumeUse` and the state `Reserved`;
  #70 adds `use_state`/`use_epoch`, `ReleaseVolumeUse` and `RESERVED` with unrelated meaning.
- "Every successful metadata Attach records RESERVED" does not cover #79's other epoch-advancing
  paths (`PrepareVolumeUse` in ControllerPublish, `ReserveInitialization`, `attach-operation`).
- "Persist local expansion intent ... in #79's durable state; phases `device`, `filesystem`,
  `done`": #79's schema 1 has `kind=stage|publish`, phases `reserved_unbound|bound|preparing|
  ready|removing`, and refuses unknown/missing fields. The record kind and schema change are not
  stated.
- Design line 152 says tags 9/10 "belong to #18"; #79 also claims Volume 9/10 (and #13 tag 9).
**Fix:** state how expansion and XFS compose with #79's contract (enum value, grant, epoch paths,
record kind/schema version), rename the use-state items, and list the tag conflict as open.

### R1-C4 — Medium
**Growth support is required of new suffix holders only; an existing prefix holder on old code
refuses the grown volume.**
Design lines 39–40 exclude nodes without `growth-v1` from new placement; line 72–73 requires
support "on existing/new replicas" for ONLINE only. An un-upgraded replica that knows the volume
rejects a `Hello` with a different size (`flaminio-node/src/replica/mod.rs:213–220`), for all its
chunks. After an offline growth and restage that member cannot be fenced: it is dropped from the
ISR of its prefix chunks, or the attach blocks at minISR. The label is also not proof of support:
`RegisterNode` merges labels and never removes one (`meta/service/service.go:197–201`), so a node
rolled back to old code keeps `growth-v1=true`.
**Fix:** make ExpandVolume admission (both modes) require support on every node in the prefix's
replica sets and move destinations, and define how the label is cleared or versioned on
downgrade. Add the refusal to the S2/S9 tests.

### R1-C5 — Medium
**The profile and mode reconciliation has no mechanism, contract or file.**
Design lines 69–73: "Require one reconciled mode/profile; mismatch fails startup/readiness" and
"Label metadata nodes with verified `flaminio.io/growth-v1=true` and runtime resize/tool
profile". Not specified: which component probes `UblkCtrl::get_features` (only `flaminio-io` can)
and the tools (only the CSI node container has them), which one registers the label (today the
engine registers id and address only, `flaminio-node/src/engine.rs:583–593`; the CSI node plugin
registers topology labels, `control/csi/topology.go:70,95`), and how CSI learns the metadata
service's mode (Contracts has no RPC or field for it). "Files touched" lacks
`meta/cmd/flaminio-meta/main.go` (the `--expansion-mode` flag of the server),
`control/csi/topology.go` or `control/agent`, and the data-plane side of the probe.
**Fix:** name the prober, the label writer and value format, the mode-comparison call, and the
files; add a test that a node with the label but without the probe result is refused.

### R1-C6 — Medium
**Mixed-version and downgrade behaviour of the metadata records is asserted, not designed.**
Design line 195: "Full metadata/node rollout precedes enabling growth; old header writers must
not drop new fields." Nothing enforces either. An old metadata instance (rolling upgrade, or a
rollback after a growth) (a) fails every GetVolume/Attach of a volume with a pending suffix,
because `readVolume` requires `len(entries) == h.ChunkCount()` (`meta/service/service.go:312`),
and (b) rewrites the header JSON from its own struct on Attach (`service.go:723–725`), dropping
`GrowthGeneration`, `Growth` and the use fields: the generation returns to 0 (the next growth
reuses generation 1, which the stale-completion key `(…, growth_generation)` relies on being
unique), and a pending suffix loses its operation, which the design then classifies as
corruption. The default `--expansion-mode=offline` means growth is enabled as soon as the binary
runs; there is no "off".
**Fix:** add a disabled default (or an explicit enable step) and say what is supported: no
metadata rollback after the first growth claim, or a detection rule (e.g. generation derived so
that a dropped field is detectable) and its test. State it in S9 and the ADR consequences.

### R1-C7 — Medium
**RESERVED has no path to RELEASED when NodeUnstage never runs.**
Design lines 54–62: only NodeUnstage releases; ControllerUnpublish stays a no-op
(`control/csi/controller.go:296–301`); "RESERVED includes failed/pending attaches until verified
cleanup", but nothing performs that cleanup. ControllerPublish already attaches
(`controller.go:281`), so a pod deleted before NodeStage, or a node lost or replaced while
staged, leaves the volume RESERVED and offline growth refused indefinitely, although nothing
uses it. The same gap is the legacy `UNKNOWN` case the ADR lists as open.
**Fix:** specify the reconciliation: e.g. the node plugin's existing sweep
(`control/csi/tombstones.go`) releases volumes whose target is this node, at the recorded epoch,
after verifying no stage, publication or device; and document the recovery for a lost node
(attach elsewhere and unstage cleanly). Add the publish-without-stage and restart cases to the
S4 tests.

### R1-C8 — Medium
**Only S5 has a simulator check and injected bug.**
Design test table: S2/S3 (suffix invisible until the final CAS, no rollback of size/generation,
prefix records unchanged) and S4 (offline growth and Attach mutually exclusive, a stale release
cannot clear a newer attach) are proven by Go service tests only. `AGENTS.md` asks a simulator
check and an injected bug per new safety rule; `make meta-vectors` today generates ISR vectors
only (`Makefile:105–106`), and the design says just "covers new model transitions".
**Fix:** say which of these rules are modelled in `flaminio-repl/src/meta.rs` and the vectors,
with a named mutant each (e.g. publish an incomplete suffix; accept a stale-epoch release), or
state why a rule is not a safety rule.

### R1-C9 — Low
**`ExpandVolumeRequest` does not match the shared #80 helpers, and `online` is undefined.**
Design line 149: `required_bytes`/`limit_bytes` are `u64`; #80's `CheckCreateVolume` uses
`int64` and rejects negative ranges at the service (`80/contracts.md`), and spec S1 requires the
same range helpers. `online:5 bool` is a caller claim next to the service's own
`--expansion-mode`; the result of a mismatch is not stated.
**Fix:** use #80's signed fields, or say that CSI alone validates signs; define `online` versus
the service mode (refuse with FailedPrecondition) or drop the field.

### R1-C10 — Low
**Delete leaves the growth record in a crash window.**
Design lines 48–50: Delete "then removes operation state conditionally", a step after the header
delete. A crash between them leaves `growth/<id>`; `DeleteVolume`'s retry path for an absent
header only clears chunks (`service.go:527–533`). After the id is created again, the orphan is a
"competing operation". `<id>` is also ambiguous (volume or operation id).
**Fix:** delete the growth record in the header-delete transaction (one more op), or have the
claim replace a record whose `first_epoch` differs; say what `<id>` is; add the crash test.

### R1-C11 — Low
**Legacy volumes: an approach without a rule or a test.**
Design line 28–29 refuses expansion for `FirstEpoch == 0` "pending an explicit migration". The
spec has no compatibility rule for existing volumes, the migration is not defined anywhere, and
no test row covers the refusal.
**Fix:** add the rule to the spec (refused, with which code), drop or define the migration, and
add the test.

### R1-C12 — Low
**How xfsprogs gets into the image is not specified.**
Design line 121: "Package xfsprogs and e2fsprogs plus libraries in both shipped architectures".
`scripts/dev/image.sh` builds without a container engine by appending `/usr/local/bin` files to
a pinned Ubuntu base; there is no package step, and xfsprogs is not in that base.
**Fix:** name the mechanism (pinned, checksummed packages extracted into the layer per
architecture, or another pinned base) and the image test that runs `mkfs.xfs -V`/`xfs_growfs -V`.

### R1-C13 — Low
**The lab plan is not runnable as written.**
Design line 183: "online resize on validated nodes and offline ... on baseline nodes". Both lab
VMs run kernel 7.0 (`docs/lab.md:17`); no baseline (6.6, no UPDATE_SIZE) node exists. The target
is added to the Makefile, but `scripts/lab.sh`/`scripts/checks`, through which every lab command
runs, are not in "Files touched".
**Fix:** say that offline is exercised by mode on the 7.0 VMs and UPDATE_SIZE absence only by the
fake tests (or name a 6.6 VM), and list the script files.

## Nits
- `spec.md:3` and `design.md:3` link `decisions.md`, which does not exist in the folder.

Reviewed commit: 7a5a04e777421c7212a9b1a8daba5af6f28edadd
