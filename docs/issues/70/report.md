# Issue #70 — report

## Review rounds

### Plan review round 1 — 7a5a04e777421c7212a9b1a8daba5af6f28edadd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70c1 (design-code), r70s1 (design-security).

- **R1-C1 — High:** Online growth needed a refreshed session per replica, but a replica creates
  chunks only on Hello, serves one session per volume, and the target treats a replaced session
  as a member failure for every chunk there: each growth would drop suffix holders from their
  prefix ISRs or stall writes. Evidence: `flaminio-node/src/replica/mod.rs:234–257`;
  `flaminio-repl/src/replica.rs:173–190`; `flaminio-repl/src/target.rs:1214–1328`;
  `flaminio-node/src/engine.rs:1990–1997`.
- **R1-C2 — High:** A claimed growth operation could never be abandoned or re-placed; in offline
  mode it refused every Attach, and a lowered request was `Aborted` forever. Earlier batches were
  not revalidated against drain marks the drain controller cannot see. Evidence: the draft's
  admission section; `control/drain/drain.go:50–65`.
- **R1-C3 — Medium:** The design contradicted #79: XFS formatting outside its initialization
  grant and `UseProfile`; colliding use-state names; epoch paths other than Attach; local
  expansion intent in #79's schema; Volume tags 9/10 also claimed by #79 and #13. Evidence:
  #79 `initialization-contract.md`; the draft's design lines 54–62, 120, 126–127, 152.
- **R1-C4 — Medium:** Growth support was required only of new suffix holders; an older prefix
  holder refuses the larger Hello, and the merge-only label survives a rollback. Evidence:
  `flaminio-node/src/replica/mod.rs:213–220`; `meta/service/service.go:197–201`.
- **R1-C5 — Medium:** The profile and mode reconciliation had no prober, label writer, RPC or
  files. Evidence: `flaminio-node/src/engine.rs:583–593`; `control/csi/topology.go:70,95`.
- **R1-C6 — Medium:** Older metadata binaries fail reads of a pending suffix and drop new header
  fields on Attach; the default mode enabled growth with no off switch. Evidence:
  `meta/service/service.go:312,723–725`.
- **R1-C7 — Medium:** RESERVED had no path to RELEASED when NodeUnstage never runs. Evidence:
  `control/csi/controller.go:281,296–301`.
- **R1-C8 — Medium:** Only S5 had a simulator check and injected bug. Evidence: `Makefile:105–106`.
- **R1-C9 — Low:** `ExpandVolumeRequest` used unsigned ranges unlike #80, and `online` had no
  rule. Evidence: #80 `contracts.md`.
- **R1-C10 — Low:** Delete could leave the growth record after a crash. Evidence:
  `meta/service/service.go:527–533`.
- **R1-C11 — Low:** Legacy volumes were refused with no spec rule, migration or test. Evidence:
  the draft's design lines 28–29.
- **R1-C12 — Low:** How xfsprogs reaches the image was unspecified. Evidence:
  `scripts/dev/image.sh`.
- **R1-C13 — Low:** The lab plan needed a 6.6 node the lab lacks, and omitted the lab scripts.
  Evidence: `docs/lab.md:17`.
- **Code nit — Nit:** `spec.md` and `design.md` linked a missing `decisions.md`.
- **R1-S1 — High:** XFS formatting contradicted #79's format authority, and XFS was absent from
  its contract. Evidence: #79 `spec.md:42–55`, `design.md:93`, `initialization-contract.md:10,34–39,88–93`.
- **R1-S2 — Medium:** After a commit, a replica out of chunk slots refuses the whole session at
  the next attach, so the prefix loses a replica; the expected behaviour was unstated. Evidence:
  `flaminio-node/src/replica/mod.rs:247–256`; `flaminio-store/src/store.rs:1207`.
- **R1-S3 — Medium:** A pending growth had no abort or supersede path and blocked every Attach
  in offline mode. Evidence: the draft's design lines 32–41, 64–65.
- **R1-S4 — Medium:** The rollout gate was a sentence; an older Attach drops growth and use
  fields. Evidence: `meta/service/service.go:312–318,709–725`.
- **R1-S5 — Medium:** Capability labels were self-asserted, merged and never removed. Evidence:
  `meta/service/auth.go:71–72`; `meta/service/service.go:196–200`; `target.rs:2510–2512`.
- **R1-S6 — Medium:** RESERVED was tied to Attach, not to the header epoch; #79's other epoch
  paths start devices without it. Evidence: #79 `initialization-contract.md:77–86,104`.
- **R1-S7 — Medium:** Suffix placement was not protected against a drain mark. Evidence:
  `meta/service/service.go:252–259,825`; #78's draft.
- **R1-S8 — Low:** The replica's size ratchet had no order against the epoch check. Evidence:
  `flaminio-node/src/replica/mod.rs:212–234`.
- **R1-S9 — Low:** No path to RELEASED for UNKNOWN volumes or a dead last target. Evidence: the
  draft's ADR line 64.
- **R1-S10 — Low:** Mode reconciliation between metadata and CSI was unspecified. Evidence: the
  draft's design lines 69–70, 149.
- **R1-S11 — Low:** The ONLINE profile omitted the `UPDATE_SIZE` stop race fixed in 7.0-rc4.
  Evidence: `libublk-0.4.8/src/ctrl.rs:1820–1830,2755–2760`.
- **R1-S12 — Low:** The `resize` socket line had unbounded, untested inputs. Evidence:
  `flaminio-io/src/control.rs:153–160`.
- **R1-S13 — Low:** A crash in delete could orphan the growth record. Evidence:
  `meta/service/service.go:441,529–535`.
- **R1-S14 — Low:** Hard prerequisites were written as if they existed. Evidence:
  `flaminio-io/src/control.rs:84–107`.
- **R1-S15 — Nit:** Use-state names collided with #79's.
- **R1-S16 — Nit:** `decisions.md` was linked but missing.

Changes/resolution: the plan is narrowed to the offline raw block and ext4 step; `intent.md`
says which step it is and what stays open (online resize, XFS, #72), and the later parts' rules,
contracts and fields are cut rather than guarded. Growth now commits in self-contained steps:
one transaction per at most 64 chunks, comparing the header and #78's placement gate, with no
pending operation, growth record, generation or new header field. A running target applies views
to its attached chunks only; the device takes the new size at its next attach. Per finding:
- R1-C1, R1-S11, R1-S12: resolved by the cut of online resize (no session refresh, no
  `UPDATE_SIZE`, no `resize` socket line).
- R1-C2, R1-S3: resolved by the cut of pending growth: each step is a complete volume, so there
  is nothing to abort and Attach is never refused; drain marks are compared through #78's gate.
- R1-C3: XFS, the use-state protocol, local expansion intent and new Volume fields are cut, so
  nothing composes with #79's grant, profiles or schema; NodeExpand only reads #79's records.
- R1-C4, R1-C5, R1-S5, R1-S10: capability labels and modes are cut; one metadata flag
  `--volume-expansion` (default off) gates growth after every data plane node is upgraded; a
  data plane rollback after a growth is documented as unsupported; an automatic version check is
  an open question.
- R1-C6, R1-S4: no header field is added, so older metadata binaries read, attach and delete a
  grown volume intact; growth is off by default; a test checks the header's keys are unchanged.
- R1-C7, R1-S6, R1-S9, R1-S15: resolved by the cut of the use-state protocol: a running target
  ignores appended chunks, so controller expansion needs no proof that the volume is offline.
- R1-C8: simulator scenario `grown_view_keeps_prefix_isr_changes` and seed sweep with growth,
  injected bug `drop_grown_view`; decisions say why metadata growth adds no model vectors.
- R1-C9: `ExpandVolumeRequest` is `{id, size_bytes}`; CSI validates the signed range with #80's
  helper; `online` is gone.
- R1-C10, R1-S13: resolved by the cut: no growth record exists to orphan.
- R1-C11: legacy volumes grow like any other; `TestExpandLegacy`.
- R1-C12, R1-S1: resolved by the cut of XFS; `resize2fs` comes from the pinned base's e2fsprogs.
- R1-C13: offline runs on the 7.0 lab VMs; `scripts/lab.sh`, `scripts/guest/meta.sh` and the
  block manifest are listed; the lab section says what it deletes and how it restores.
- R1-S2: the rule is stated (S6: the existing member-failure path) and tested
  (`a_full_replica_refuses_the_grown_volume`); waiting for #72 is an open question in Risks.
- R1-S7: each step compares #78's placement gate; #78 is a prerequisite; `TestExpandRaces`
  covers a drain mark before a step's commit.
- R1-S8: the known size rises only in the Fence arm after creation and fence succeed; tests for
  a stale larger Hello and a smaller one.
- R1-S14: #18, #77, #78, #80, #79 and #15 are declared as prerequisites with the commits read.
- Code nit, R1-S16: `decisions.md` created, with `## Approvals` (none).
- Cross-plan (not a finding): the design names #73's quota as a later dependency of each step.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 4057eb9ba1b5af7d487de4bc5bbd500e24d89734

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70c2 (incremental design-code), r70s2 (incremental design-security).

Round-1 findings: all confirmed fixed or cut, except R1-S2, partly fixed (now R2-S1).

- **R2-C1 — Medium:** The change to #80's engine refresh guard had no file and no test for its
  refusal half. Evidence: #80 `design.md:78–86`; `flaminio-node/src/engine.rs:1544–1579` not in
  Files touched.
- **R2-C2 — Medium:** A csi-resizer "with default flags" cannot start: its socket default is not
  `/csi/csi.sock`, and its 10 s timeout is short for a multi-step growth. Evidence:
  `deploy/csi/driver.yaml:67–68,76–77`.
- **R2-C3 — Medium:** No test could fail if the replica raised its size before chunk creation and
  the fence; with #80's preflight, slot exhaustion is refused before the Fence arm, so "as today
  (`:252–254`)" was wrong; the local path was untested. Evidence:
  `flaminio-node/src/replica/mod.rs:252–254`; #80 `design.md:93–98`; `engine.rs:1955`.
- **R2-C4 — Low:** Several rules had no test: metadata step 4's outcomes (no failing-KV
  wrapper), NodeExpand's pin mismatch, non-ready record, publication path, kill and output cap,
  ControllerExpand's larger concurrent size and V above the limit. Evidence:
  `meta/service/service_test.go:615–670`.
- **R2-C5 — Low:** `TestExpandSteps` named no hook that can fail a later step for good. Evidence:
  `service_test.go` `interleaveKV`; `retries`.
- **R2-C6 — Low:** The simulator work omitted suffix creation after build and drawing from the
  grown count, so the sweep's failover half exercised only the prefix. Evidence:
  `flaminio-repl/src/sim/mod.rs:392–396,1049,1262`.
- **Code nits — Nit:** the ADR's "(the first draft)"; the Hello refusal cited `:234`, the core
  call.
- **R2-S1 — Medium:** When every replica of a suffix chunk lacks a slot, the attach succeeds at
  the larger size, suffix I/O waits with no deadline, and `resize2fs` blocks in the kernel past
  NodeExpand's deadline while holding `n.mu`; the preflight path was misdescribed. Evidence:
  `flaminio-node/src/engine.rs:1490–1500`; `flaminio-repl/src/target.rs:747–766,2159–2180,2218–2252`;
  `control/csi/node.go:27–28`; #80 `design.md:93–98`.
- **R2-S2 — Low:** NodeExpand's device-identity refusals were untested (`device-info` changing,
  a stage on another device, a record not `ready`). Evidence: the design's NodeExpand steps 3, 5.
- **R2-S3 — Low:** The lab section omitted what it deletes, restored only partly, could not run
  in the stated order with one volume per node, and `csi-failover` could pass on a fresh volume.
  Evidence: `scripts/lab.sh:1083–1088,1105–1106,1144–1147,1179`; `control/csi/node.go:31–37`.
- **R2-S4 — Nit:** `Options` is declared in `meta/service/auth.go:20`, not `service.go`.
- **R2-S5 — Nit:** The engine refresh was called "unchanged" with `engine.rs` absent from Files
  touched (the R2-C1 defect).

Changes/resolution:
- R2-C1, R2-S5: the engine refresh is stated once (design, Targets): #80's check in `engine.rs`
  compares a same-epoch reply with the attached geometry kept in `TargetRt`, passes only a size
  larger by whole chunks with chunk size, RF, minISR equal and the matching record count, and
  refuses the rest (not installed, core not called). `engine.rs` is in Files touched;
  `a_malformed_refresh_is_refused` covers each refusal.
- R2-C2: the resizer takes the attacher's args (`--csi-address=/csi/csi.sock --leader-election
  --leader-election-namespace=flaminio-system --timeout=60s`), `securityContext` and socket mount;
  leases are already granted; the lab check waits for it to be Ready.
- R2-C3: slot exhaustion is refused by #80's preflight before the core, so the node's fence stays
  at the old epoch (an unreachable replica for that volume); the Fence arm's failure is left to
  store faults and tested by `a_failed_creation_keeps_the_old_size`; the full-replica test runs
  remote and local and checks the old fence.
- R2-S1: S6 states the attach's outcome; the design states it once (Targets, Slots exhausted) and
  corrects NodeExpand's context end; a chunk with no slot on any replica waits, and NodeExpand with
  it under `n.mu`. Recommended and recorded under the user's standing instruction: document it and
  add it to the accepted availability risk (a new volume's first stage on full nodes waits the
  same way today), not a daemon status field; #72's ledger removes it. Tests:
  `a_suffix_chunk_without_a_slot_waits`, a `GrowExt4` fake that ignores cancellation.
- R2-C4, R2-S2: `faultKV` (`Aborted`, `Unavailable`, `Canceled`, earlier steps kept); the missing
  ControllerExpand and NodeExpand cases, every NodeExpand refusal asserting no tool runs.
- R2-C5: the hook drains every node after the first applied header write; the second step answers
  `FailedPrecondition` with +64 committed and readable.
- R2-C6: the sim creates an appended chunk on its placed replicas as `build` does; clients draw
  from the live target's count, moves and the final check from the metadata's;
  `Report::suffix_writes` pairs with `Report::growths`.
- R2-S3: block first with its namespace always deleted; then ext4, the move and `csi_failover`,
  followed by a 6Gi and added-file check; the start's deletions are stated; the trap uncordons
  and deletes the namespace unless `KEEP=1`.
- Code nits, R2-S4: ADR option named without draft history; Hello citation split; `Options` in
  `auth.go:20`.
- Cross-plan: each step will also charge #72's per-node capacity ledger (dependency only); the
  operator note may describe `--volume-expansion` with #75's `--qos`. `decisions.md` notes that
  these fixes came after the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 82b7bec

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70c3 (incremental design-code), r70s3 (incremental design-security).

Round-2 findings: all code findings fixed; R2-S2, R2-S4, R2-S5 fixed; R2-S1 and R2-S3 partly
fixed (now R3-S1, R3-S2, R3-S3). The reviewers found the refresh check holds against stale,
deposed or malicious replies, and the replica's size rises only from a Hello its fence accepts.

- **R3-1 — Low:** The refresh check must run before the reply is converted: `rt.chunk_meta` grows
  the node table and `missing_addrs` starts a ListNodes call, so a refused reply could still
  change both, untested. Evidence: `flaminio-node/src/engine.rs:386–395,407,1555,1567–1574`.
- **R3-2 — Low:** `csi_failover` replaces and then clears the check's EXIT trap, and the added
  file is not checked after it. Evidence: `scripts/lab.sh:1158,1184,1179`.
- **Code nits — Nit:** S9's title against the settled answer (advertised while the flag is off);
  the garbled simulator sentence (the sim creates chunks before the Hello).
- **R3-S1 — Medium:** The plan presented the node-wide wait added in round 2 as accepted,
  although the user approved before it was written; the ADR was edited after acceptance and the
  spec kept its acceptance header. Evidence: `decisions.md:8–10,77–81` at 82b7bec;
  `design.md:221`; `adr-proposal.md:3,5,48–49,54–55`; `spec.md:4,69–71,78–79`.
- **R3-S2 — Medium:** For ext4 the wait starts at NodeStage's `blkid -p` under `n.mu`, not at
  NodeExpand; the volume's NodeUnstage waits behind the same lock; no release path was stated, and
  "mkfs under n.mu" named the wrong tool. Evidence: `control/csi/node.go:89–91,108,135–136`;
  `control/csi/mount.go:37–40`; `flaminio-repl/src/target.rs:829–836,1221`;
  `control/csi/tombstones.go:13`.
- **R3-S3 — Low:** The lab's restore trap does not survive `csi_failover`, which also deletes the
  namespace before the after-failover check unless `KEEP=1`; the block pod's namespace was not
  named. Evidence: `scripts/lab.sh:1158,1184,1200–1203`. Same defect as R3-2.
- **R3-S4 — Nit:** "until one can create it" is wrong for a waiting prefix chunk, which waits
  until one of its replicas accepts the session.

Changes/resolution:
- R3-1: the design says #80's check runs on the raw `pb::Volume` before `rt.chunk_meta`;
  `a_malformed_refresh_is_refused` asserts the node table is unchanged and no ListNodes call is
  made.
- R3-2, R3-S3: `(KEEP=1 csi_failover)` runs in a subshell, so its trap stays there and the
  namespace is kept; its failure fails the check; then an exec verifies the added file and the
  PVC's 6Gi, before the cleanup. The block pod is in `flaminio-test`.
- R3-S1: the accepted risk is restored word for word in the design's Risks and the ADR's
  Consequences. The node-wide wait is listed apart as found after approval, not accepted, in the
  design's Risks and header, the spec's header, the ADR's Consequences (a marked note; the date
  line says so) and `decisions.md`, which moves rounds 2 and 3 to a "Changed after approval"
  section and puts the choices to the user there. Scope is stated: today the node's staging
  stops; with #15, other tenants' volumes too. Narrowing in the docs: where tenants share data
  nodes, turn `--volume-expansion` on only once #72 lands.
- R3-S2: S6 and the design's Risks say ext4 NodeStage's `blkid -p` waits first, then NodeExpand,
  both under `n.mu`, which NodeUnstage needs too; "mkfs" corrected to `blkid -p`; the release
  path (free slots on the refusing nodes, which the tombstone sweep does outside `n.mu`, after
  which the target reconnects; or restart the data plane on the target's node) goes into the
  README upgrade note and `docs/followups.md`; `TestNodeStage` with an `FSType` fake that ignores
  cancellation.
- Code nits, R3-S4: S9 says every refusal names the flag; the sim sentence says
  `replica/tests.rs` covers the Fence-arm creation; S6 says a waiting chunk waits until one of
  its replicas accepts the session, which needs slots there for every chunk of the volume.
- Cross-plan (#75): `--volume-expansion` follows `--qos`: a second rollout that nothing enforces;
  an older binary exits on the unknown flag, so a binary-only rollback keeping it fails closed;
  `meta.sh` writes it only with `VOLUME_EXPANSION=1`, and `make meta-up VOLUME_EXPANSION=1` is the
  lab's second rollout; `make csi-expand` refuses unless every unit has the flag.
- Approval records: `decisions.md` adds the standard Spec and Design approval lines and a line
  that the round-3 fixes came after the approval; the ADR's status says the decision is binding
  for this plan's implementation from the approval on, numbered and moved into `docs/adr/` only
  with the implementation; its "Decision proposed" heading is marked settled at approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 91caea0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70c4 (incremental design-code), r70s4 (incremental design-security).

Round-3 findings: all fixed; every cited line checks out. R3-S2's new text was wrong in places
(R4-S1, R4-S2, R4-S4).

- **F1 (r70c4) — Medium:** How `VOLUME_EXPANSION` reaches `meta.sh`, and which `meta up` callers
  keep it, was not stated; `meta.sh up` rewrites the unit, so `cluster-up`, `make chaos` and
  #75's `make meta-up QOS=1` would drop the flag. Evidence: `scripts/lab.sh:509–525,1407`;
  `Makefile:301–308,331–332`.
- **F2 (r70c4) — Low:** The mechanism of `csi_expand`'s "every unit has the flag" check, and the
  form that makes the subshell's failure fail the check, were not named.
- **F3 (r70c4) — Low:** No test pinned the flag scripts or the refusal outside the lab.
- **Code nit — Nit:** "the target's reconnect ... is accepted" read as a decision.
- **R4-S1 — Medium:** Two release paths did not free the node: restarting the data plane
  re-hangs while kubelet retries NodeStage, and freeing slots cannot help when growth placed more
  chunks on a node than its whole slot table holds; the working release was untested. Evidence:
  `control/csi/node.go:58–77`; `flaminio-format/src/layout.rs:111–114`; #80 `design.md:29–32,93–97`;
  `flaminio-repl/src/target.rs:829–836`.
- **R4-S2 — Medium:** The narrowing waited for #72 to land, but neither plan made growth charge
  #72's ledger. Evidence: #72 `spec.md:4`, `intent.md:48`; the design's "this step builds neither".
- **R4-S3 — Low:** The spec's header understated what changed after approval (S6's third
  sentence, S9, the Tests list). Evidence: `git diff a3dbb5c..91caea0 -- docs/issues/70/spec.md`.
- **R4-S4 — Low:** The scope missed NodeStage's `mount` under `n.mu`, and assumed the waiting
  chunk is the last. Evidence: `control/csi/node.go:89,124`; `control/csi/mount.go:69–77`.
- **R4-S5 — Nit:** decisions.md and the ADR said "has a slot" where S6 says "accepts the session".
- **R4-S6 — Nit:** "fails closed" overstated: an older binary given the flag exits with status 2
  and restarts in a loop. Evidence: `meta/cmd/flaminio-meta/main.go:84`;
  `scripts/guest/meta.sh:34`.

Changes/resolution:
- F1, F2, F3: #70's own `VOLUME_EXPANSION` handling is replaced by #75's "Lab flags of the
  metadata service (S5, shared with #70)" (40cdf3f): `make meta-flags VOLUME_EXPANSION=0|1`, a
  flags file every `meta up` keeps; whichever of #70 and #75 lands first adds the mechanism, the
  other its flag. `csi_expand` reads the file on every cluster VM, as `qos-check` does, and runs
  `(KEEP=1 csi_failover) || die`. `scripts/tools/meta-flags-test.sh` gains the flag's cases and
  `csi_expand`'s refusal; it is S9's test.
- R4-S2: this plan owns the charge: once #72 lands, each step charges its chunks by #72's
  admission rule in the same transaction (ledger compares and writes, `ResourceExhausted`
  without room, a prefix that fits 120 operations), added by whichever lands second with
  `TestExpandChargesLedger`. Spec S2, the design's Capacity charge, the intent and a marked ADR
  note say so. The narrowing now waits for that charge; `docs/followups.md` names it.
- R4-S1: release (a) helps only within each refusing node's slot table, else the volume can only
  be deleted; release (b) removes the pod before restarting the data plane;
  `a_suffix_chunk_without_a_slot_waits` frees a slot and sees the I/O complete. With the charge, a
  growth beyond a node's room is refused before commit; the wait remains where physical slots run
  out under the ledger. The wait stays found after approval and not accepted.
- R4-S4: the scope is every NodeStage step that reads the device (`blkid -p`, `mount`), then
  `resize2fs`; "in the last suffix chunk" is gone.
- R4-S3: the spec header lists S2's last sentence, S6's third and last two sentences, S9 and the
  Tests list as changed after approval.
- R4-S5: "accepts the session" in decisions.md and the ADR.
- R4-S6: the flag text says the older binary exits and restarts in a loop, serving nothing; the
  rollback turns the flag off first; the usage and `meta-flags` help carry the warning, as
  `--qos`'s do.
- Code nit: the reconnect "succeeds once a replica accepts the session".
- decisions.md: a line under Approvals says the fixes of rounds 2 to 4 came after the approval;
  "Changed after approval" gains round 4 and the updated open question.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — e729522

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70c5 (incremental design-code), r70s5 (incremental design-security).

Round-4 findings: fixed, except where the new text was wrong (R5-S3, R5-S4, R5-S6) or relied on a
#75 check since replaced (R5-C1, R5-S5). The ledger-charge design and the lab subshell hold
against #72 and the code.

- **R5-C1 — Medium:** `csi_expand` still read the flags file, which #75 replaced because a file
  proves nothing (an older binary loops on the flag while the file holds it); a stale #75 commit
  was pinned. Evidence: #75 `design.md` (00e57e8, 4663835); the design's Lab and S9 row.
- **R5-C2 — Low:** Step 3's reads omitted the reports and the ledgers whose revisions the
  transaction compares. Evidence: #72 `design.md`, Charging and release.
- **R5-C3 — Low:** `TestControllerExpand` did not pass `ResourceExhausted` through.
- **R5-S1 — Medium:** Neither plan built growth's quota charge, so once #73 lands a resize gets
  around #73's quota; "a ResourceQuota until #73" was false; #73's keys were not in the 120 bound.
  Evidence: #73 `spec.md:87–88`, `intent.md:48`.
- **R5-S2 — Medium:** Only #70's plan assigned the ledger charge to #72, whose plan does not
  build it. Evidence: #72 `spec.md:54–59`, `intent.md:50`, `adr-proposal.md:66`.
- **R5-S3 — Medium:** Release (b) fails when a controller recreates the pod, and restarting the
  data plane hits other volumes. Evidence: `control/csi/node.go:58–77`;
  `dataplane/crates/flaminio-io/src/control.rs:192–212`.
- **R5-S4 — Medium:** Release (a)'s condition was per node, not per chunk; deleting the volume
  does not free the node; the prefix data survives. Evidence: `meta/service/service.go:539–577`.
- **R5-S5 — Low:** Same as R5-C1.
- **R5-S6 — Low:** The scope missed NodeUnstage's `umount`, which flushes writes under `n.mu`.
  Evidence: `control/csi/node.go:135–137`; `control/csi/mount.go:81–84`.
- **R5-S7 — Low:** The narrowing was not in the flag's usage or `meta-flags` help, and no one was
  told to remove it.
- **R5-S8 — Low:** `TestExpandChargesLedger` lacked slots alone, left-out nodes and a preferred
  node short of room.
- **R5-S9 — Nit:** The usage read as if removing the flag made every rollback safe.

Changes/resolution:
- R5-S3, R5-S4, R5-S7: resolved by cutting the cause. Growth is refused (`FailedPrecondition`,
  Metadata growth step 1) until it charges #72's ledger, so a growth that does not fit is refused
  before commit. The node-wide wait is stated once, in the design's Risks, for the one case left
  (a node's physical slots run out under its ledger), with no release beyond the operator's
  investigation; it stays found after approval and not accepted. Release paths (a) and (b) and
  the narrowing are gone. This changes the settled answer "Wait for #72? No"; the change is
  marked, not accepted, in the design's Risks, the spec's Settled questions, the ADR and
  decisions.md.
- R5-S1, R5-S2: one rule in the design's Charges: each step charges #72's ledger and #73's
  namespace and class quota in its transaction; whichever plan lands second adds each charge and
  its test (`TestExpandChargesLedger`, `TestExpandChargesQuota`). #73's keys count in the 120
  bound. Risks says what holds: until #73 lands, a Kubernetes ResourceQuota on
  `requests.storage`, which counts resizes; once both land, #73's quota.
- R5-S6: NodeUnstage's `umount` is in the scope (design Risks, S6, decisions.md).
- R5-C1, R5-S5: `csi_expand` follows #75's rule (4663835): it expands a probe volume through each
  cluster VM's own instance (`metactl`, `meta_endpoints`), failing naming any VM that refuses, and
  deletes the probe; it reads no flags file. The admin CLI gains `expand <volume> <size>`. The S9
  test: only w's instance refuses, the check fails naming w.
- R5-C2: step 3 reads the reports with `s.nodes()`, then the ledgers whose revisions the
  transaction compares.
- R5-C3: `TestControllerExpand` returns `ResourceExhausted` as is.
- R5-S8: `TestExpandChargesLedger` gains slots alone, left-out nodes and a preferred node short of
  room.
- R5-S9: the usage says "remove it before a metadata rollback; a data plane rollback after a
  growth is unsupported".
- decisions.md: a line under Approvals says rounds 2 to 5 came after the approval; round 5 is
  under Changed after approval with the two open items for the user.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 942d1eb

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70b6 (incremental design-code and design-security).

Round-5 findings: fixed or resolved by the cut, with gaps raised below (R6-S1, R6-S2, R6-C1,
R6-C2).

- **R6-B1 — High:** Step 1 refused every call until growth charged #72's ledger, so if #70 landed
  first its S2/S3 tests, the CLI test and `make csi-expand` could not pass, the intent's
  Done-when could not be met, and S9's advertising claimed what no configuration could do.
  Evidence: the design's step 1 and Tests; `intent.md:31`; `spec.md:93`.
- **R6-B2 — Medium:** Step 1 came before the flag check, so a flag-off refusal no longer named the
  flag, against the settled answer and S9. Evidence: `decisions.md:56–57`; `spec.md:94`.
- **R6-S1 — Medium:** "A volume without an owner charges nothing" let a tenant grow an unassigned
  volume past #73's quota; #73 charges it only at assignment, even past the limit. Evidence: #73
  `spec.md:172–175`, `design.md:103–104,161–166`, `contracts.md:283`.
- **R6-S2 — Medium:** `ExpandVolume` had no row in #73's role table, so once tenancy is active
  every resize fails, or any administrator name grows any volume. Evidence: #73 `contracts.md:31–51`,
  `design.md:45–55`; `meta/service/auth.go:64`.
- **R6-C1 — Low:** The fixed probe id is blocked by a pending tombstone; #75's rule now requires
  per-run ids and its trap rules; cleanup after a failed check was untested. Evidence:
  `meta/service/service.go:373–378`; #75 `design.md` at 02853e8 and 597b861.
- **R6-C2 — Low:** Step 3's read order placed #72's reads but not #73's.
- **R6-C3 — Nit:** The round-3 and round-4 entries in decisions.md did not mark what round 5
  removed.

Changes/resolution:
- R6-B1: #72 is an ordering prerequisite: this plan lands with or after #72 (as #72 does for
  #13), so it is the second lander of #72's "whichever of #70 and #72 lands second" and adds the
  ledger charge; growth never runs uncharged. The step-1 refusal is dropped, so the tests, the
  CLI test, the lab check, the advertising and the Done-when hold when the plan lands. Marked as
  changed after approval, not accepted, in the design's Risks, the spec, the ADR, the intent and
  decisions.md.
- R6-B2: step 1 checks the flag first again; its refusal names the flag.
- R6-S1: once #73's tenancy is active, a volume with no owner record does not grow
  (`FailedPrecondition` `unassigned`, nothing written, as #73's DeleteVolume refuses one); a
  system owner record charges nothing; the step compares `owners/<id>`. `TestExpandChargesQuota`
  gains both cases; the Risks text says what holds.
- R6-S2: the design's Contracts states #73's row once, added by whichever of #70 and #73 lands
  second, with `auth_test.go` cases for each role; spec S4 says the caller needs the role of the
  owner's kind once tenancy is active.
- R6-C1: probes `csi-expand-probe-<vm>-<T>-<P>` (T the unix time, P lab.sh's pid, checked with
  `valid_id`), per VM through its own instance, deleted by #75's `qos-check` trap rules; #75 cited
  at 597b861; the S9 test records w's probe deleted after a failed check.
- R6-C2: step 3 reads, after the ledgers, `owners/<id>` and, for a tenant owner, the policy and
  `quota/<ns>/<class>`, all revisions the transaction compares.
- R6-C3: the round-3 and round-4 entries say "removed in round 5".
- decisions.md: a line under Approvals says rounds 2 to 6 came after the approval; round 6 is
  under Changed after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 2511b40

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70b7 (incremental design-code and design-security).

Round-6 findings: fixed, with one window left (R7-S1), one check unplaced (R7-C1) and #72's text
uncorrected (R7-C2). The 120 bound holds with every key counted.

- **R7-S1 — Medium:** A step that read the store inactive compared none of #73's keys, so an
  activation and an assignment between its read and commit let growth commit uncharged on a
  tenant volume; the "absent" compare could not be reached and its test could not pass.
  Evidence: #73 `design.md:65–68,161–166`, `contracts.md:83,287`.
- **R7-C1 — Low:** Once tenancy is active, the owner-kind role check (`wrong-kind`) had no place
  in the steps, so `system-provisioner` could grow tenant volumes and a call authorized before
  activation could keep growing after it. Evidence: #73 `design.md:45–55,135–136`.
- **R7-C2 — Low:** #72's plan still has growth's second lander remove a step-1 refusal #70 no
  longer has. Evidence: #72 `design.md:89–90,172`, `decisions.md:147–149`.
- **R7-C3 — Nit:** #70 pinned #75 at 597b861 but quoted text first added in 9083e69.
- **R7-C4 — Nit:** The followups line still listed #72's charge as a follow-up, and decisions.md's
  round-5 entry did not mark its refusal as replaced.

Changes/resolution:
- R7-S1: every step's transaction compares `tenant/authorization` and `owners/<id>` at the
  revisions read, absent included, so an activation or an assignment between read and commit
  makes it read again; a call marked `inactiveRules` then answers `Aborted` `tenancy was
  activated during this call: retry`, committed steps kept (#73's race rule). Stated once, in step
  3 and the Quota bullet; spec S2 says such a call never commits a step uncharged.
  `TestExpandChargesQuota` reaches it with `interleaveKV` hooks after the reads of
  `tenant/authorization` and `owners/<id>`.
- R7-C1: step 3 refuses, once active, a context without the owner kind's role with
  `PermissionDenied` `wrong-kind`, nothing written; `TestExpandChargesQuota` tests the other role
  and no role on both stores; the S4 row says Authorize lets both roles through and the handler
  decides.
- R7-C2: decisions.md records the orchestrator's correction to #72's growth sentence and tests
  row (nothing to remove since #70's round 6).
- R7-C3: #75 is cited at 9083e69.
- R7-C4: the followups line names only #73's quota charge; the round-5 entry says "replaced in
  round 6".
- decisions.md: a line under Approvals says rounds 2 to 7 came after the approval; round 7 is
  under Changed after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — e1ad7c9

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70b8 (incremental design-code and design-security).

Round-7 findings: fixed in mechanism; the race's test, the assignment case and the no-role reason
were wrong (below). The 120 bound holds.

- **R8-B1 — Medium:** The compare R7-S1 added had no test that fails without it: `s.active`
  re-read activation after the step's reads, so the named hook ended the attempt before any
  commit; the check also ran after the reads, not before each attempt as #73's rule does.
  Evidence: #73 `design.md:40–41,51,66–67`; `meta/service/service_test.go:615–668`.
- **R8-B2 — Medium:** Spec S2, step 3 and the test gave three answers for a volume assigned during
  a call on an active store; `AssignOwner` needs the owner record absent, so the step always holds
  an absent read and answers `unassigned`. Evidence: #73 `design.md:161`, `contracts.md:49`.
- **R8-C1 — Low:** The test called a context with no role `wrong-kind`; #73's reason is `no-role`,
  active or not; the R7-C1 decision credited `wrong-kind` with the case the `Aborted` check
  handles. Evidence: #73 `contracts.md:281,298`, `design.md:54–55`.

Changes/resolution:
- R8-B1: step 3 checks `s.active` before each attempt, as #73's race rule (#73 `design.md:65–68`),
  and the transaction compares `tenant/authorization` and `owners/<id>`. The test calls the
  handler directly with the `inactiveRules` mark; an `interleaveKV` hook after the step's read of
  `owners/<id>` (the first Get of that key) activates tenancy, so the commit fails on the compare
  and the next attempt answers `Aborted`; without the compare the step commits uncharged and the
  test fails.
- R8-B2: one outcome in spec S2, step 3 and the test: on an active store, a step that reads no
  owner record answers `unassigned`, even if the volume is assigned before its commit; a later
  step or call charges the new owner. The Quota bullet no longer says an assignment makes the step
  read again. The test gains the `quota/<ns>/default` hook that makes the step read again and
  charge once.
- R8-C1: step 3 answers `PermissionDenied` `no-role` for a context with neither the mark nor a
  role, active or not; `wrong-kind` only for the other role; the test and the R7-C1 entry follow.
- decisions.md: a line under Approvals says rounds 2 to 8 came after the approval; round 8 is
  under Changed after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — c3df855

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70b9 (incremental design-code and design-security).

Round-8 findings: R8-B2 and R8-C1 fixed; R8-B1 partly fixed (the check runs before each attempt
and its test can fail, but a marked step that reads the record present still commits: R9-B1).

- **R9-B1 — Medium:** A call marked `inactiveRules` compared `tenant/authorization` at the
  revision read, so an activation after the race check and before the step's read of that key let
  the step commit uncharged on an active store, or grow a tenant volume with no role binding;
  #73's rule compares the record absent. Evidence: #73 `design.md:65–67`;
  `meta/service/service.go:276`; `meta/service/service_test.go:615–644`.
- **R9-C1 — Low:** The `no-role` check ran after the reads and the "already large enough" answer,
  so a context with no role got `NotFound` or a success, and `ensurePlacementGate` could write.
  Evidence: #73 `design.md:54–55`; #78 `design.md:31`.
- **N1 — Nit:** "a later step" and "before its commit" cannot happen: `unassigned` ends the call.
- **N2 — Nit:** spec.md:8 said the Tests list changed through round 8; round 8 did not change it.

Changes/resolution:
- R9-B1: a call marked `inactiveRules` compares `tenant/authorization` absent, as #73's race rule;
  a call with a role compares it at the revision read (step 3's Quota bullet, stated once). A new
  `TestExpandChargesQuota` case, with the mark and an `interleaveKV` hook after the step's read of
  `volumes/v`, activates tenancy: the commit fails, the next attempt answers `Aborted` with nothing
  written; a compare at the revision read would commit uncharged, so the case fails. decisions.md
  corrects its R7-S1 and R8-B1 entries.
- R9-C1: step 1 checks `no-role` right after the flag, before any read; the test adds a volume
  already at the requested size that still answers `no-role` with nothing written.
- N1: spec S2, step 3 and decisions.md say "even if the volume is assigned after the step's read;
  a later call reads the new owner and charges it".
- N2: the spec header lists the Tests list as changed in rounds 2 to 7, and S2 through round 9.
- decisions.md: a line under Approvals says rounds 2 to 9 came after the approval; round 9 is under
  Changed after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — 75fe538d1812c25173c91e4b2c3c2acdfc7858ff

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r70b10 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
