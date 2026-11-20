# Issue #70 plan review — round 3 (incremental), security focus (design-security)

Scope: `git diff 4057eb9..82b7bec -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70`. That covers the approval (a3dbb5c) and the round-2 fix
(82b7bec). HEAD a56309e changes only `report.md`. Line numbers are at HEAD.

## Earlier findings

- R2-S1: partly fixed. Fixed: S6 now says what the attach does; the design's preflight account
  is corrected; the NodeExpand context-end text is corrected; the three tests are added
  (`a_suffix_chunk_without_a_slot_waits`, the `GrowExt4` fake that ignores cancellation, the old
  fence in the full-replica test). Not fixed: the wait is recorded as user-accepted when the user
  never saw it (R3-S1), and the stated scope misses where the wait starts (R3-S2).
- R2-S2: fixed. The test list adds three cases, each with no tool run: a record not `ready`,
  `device-info` changing between its replies, and a stage mounted from another device.
- R2-S3: partly fixed. Fixed: what the start deletes is stated, the block run comes first, and
  the 6Gi and added-file check is added. Not fixed: the restore trap and that check do not
  survive the call to `csi_failover` (R3-S3).
- R2-S4: fixed (`auth.go:20`).
- R2-S5: fixed (`engine.rs` is in Files touched, and the refresh is stated once).
- R2-C1 to R2-C6 and the code nits: fixed as the diff states. Spot-checked: `driver.yaml:71–76`,
  `rbac.yaml:69–90`, `engine.rs:1490–1500`, `:1955`, `sim/mod.rs:1049`, `:1262`, `:1352`.

## Findings

### R3-S1 — Medium

**The plan presents a risk the user never saw as accepted.** The risk is a node-wide CSI wait
with no deadline that crosses tenants.

What the user accepted:
- The approval (a3dbb5c) accepted one risk: "a node out of chunk slots at the attach after a
  growth refuses the session; its prefix chunks lose that member or stall at minISR"
  (`decisions.md:14–17`; the a3dbb5c design Risks).
- `decisions.md:5–7` itself says the approval came "before an incremental review" of the fixes.

What 82b7bec then did:
- It added the R2-S1 consequence: I/O waits with no deadline, and NodeExpand holds `n.mu`, so the
  node's CSI calls wait, other tenants' volumes included once #15 lands.
- It filed that consequence under the accepted risk:
  - `design.md:221` labels it "Availability, accepted (decisions)";
  - `decisions.md:77–81` says "add it to the accepted availability risk above";
  - `decisions.md:8–10` lists the round-2 fixes under **Approvals**, citing a "standing
    instruction … to continue with the recommendations and approve everything".
- It edited the ADR after its acceptance:
  - `adr-proposal.md:48–49` adds "a chunk with no other replica waits, and NodeExpand with it";
  - `:3` keeps "Accepted with the plan, 2026-11-13";
  - `:5` records no revision after round 1;
  - `:54–55` now accepts "the chunk-slot availability risk above", which includes the new sentence.
- `spec.md:4` marks the spec "Accepted 2026-11-13", although S6 (`:69–71`) and S7 (`:78–79`)
  gained these sentences afterwards.

An instruction to keep going with the recommendations is not acceptance of a cross-tenant denial
of service that was first written afterwards.

Fix:
- Move the round-2 entry out of **Approvals** into a section such as "Changed after approval,
  awaiting the user".
- Keep the risk the user approved word for word, and list the node-wide wait separately, marked
  "not yet accepted".
- Make it an open question for the user, with the alternatives and R3-S2's full scope:
  - a daemon status field;
  - releasing `n.mu` at context end and leaving the child, guarded per volume;
  - keeping the flag off in multi-tenant clusters until #72.
- In the ADR, mark the Consequences sentence and the date line as added after acceptance, or
  restore the accepted text until the user accepts the new one.

### R3-S2 — Medium

**The risk statement says NodeExpand waits. For ext4, the wait starts earlier, at NodeStage, and
the plan does not say how the node is released.**

Why the wait starts at NodeStage:
- On a filesystem stage that is not yet mounted, NodeStage runs `blkid -p` under `n.mu`
  (`control/csi/node.go:89–91`, `:108`; `mount.go:37–40`). #79 keeps a blkid probe after the
  device pin, under Node.mu (#79 `design.md:47`, `:91–92`).
- Low-level probing looks for RAID signatures (the comment at `mount.go:38–39` says so), which
  read the last sectors of the device. Those sectors are in the last suffix chunk.
- Preflight refuses the volume's whole session (`design.md:135–138`), so prefix chunks on that
  node wait too (`design.md:179`, chunk 1). Even the superblock read can wait.
- So the restaged pod hangs in NodeStage, under `n.mu`, before kubelet ever calls NodeExpand.
- The comparison "a first stage … waits the same way today (mkfs under n.mu)" (`design.md:224`)
  also names the wrong tool: today's wait is in `blkid -p` (`node.go:108`), before mkfs.

Why nothing in Kubernetes can release the node:
- The volume's own NodeUnstage takes `n.mu` first (`node.go:135–136`). Neither the tenant nor
  kubelet can free the node: the pod cannot terminate and the node cannot drain.
- The wait ends only when a refusing node has slots for every chunk of the volume placed on it
  (the target reconnects, `target.rs:829–836`, `:1221`), or when the data plane is stopped.
- Neither S6, Risks nor the docs plan states that recovery procedure.

Who can trigger it:
- A tenant who can patch a PVC can make it certain: ask for #80's cap. Only a Kubernetes
  ResourceQuota bounds that request until #73.
- The only test of the wait is NodeExpand's `GrowExt4` fake (`design.md:180`).

Fix:
- State in S6/S7 and Risks that an ext4 NodeStage (blkid) waits as well as NodeExpand, under
  `n.mu`.
- State the release path for operators (free slots on the refusing nodes, which the per-node
  tombstone sweep does outside `n.mu`, `tombstones.go:13`; or stop the data plane) in
  `docs/followups.md` and the operator note.
- Correct "(mkfs under n.mu)".
- Add a `TestNodeStage` case whose `FSType` fake ignores cancellation, or carry the mitigation
  chosen under R3-S1.

### R3-S3 — Low

**The lab sequence still cannot keep its restore promise or run its after-failover check.**

- `design.md:199–201` calls `csi_failover` from `csi_expand`. `csi_failover` replaces the shell's
  global EXIT trap (`lab.sh:1158`) and then clears it (`:1184`).
  - While it runs, a failure does not uncordon or delete the namespace.
  - After it returns, `csi_expand` has no trap left. That breaks "deletes the namespace whatever
    happens" (`design.md:201`).
- Unless `KEEP=1`, `csi_failover` deletes `flaminio-test` at its end (`lab.sh:1200–1203`). This
  happens before `csi_expand`'s check that "the PVC is still 6Gi and the added file verifies"
  (`design.md:200`). So a default run cannot pass.
- `deploy/lab/csi-expand-block.yaml`'s namespace is not named. If it is not `flaminio-test`, a
  block volume kept by a failed `KEEP=1` run is not deleted at the start
  (`design.md:189–190`), and it holds a node's single volume slot.

Fix:
- Run `(KEEP=1 csi_failover)` in a subshell, then the check, then `csi_expand`'s own cleanup;
  or re-arm `csi_expand`'s trap after every inner trap.
- Put the block pod in `flaminio-test`, or delete its namespace at the start too.

## Checked without finding (as asked)

**Refresh.** A stale, deposed or malicious same-epoch metadata reply cannot install a larger or
changed geometry:
- `TargetRt` keeps the attached geometry, which only ever passes the comparison.
- The core applies only the attached prefix (`take(len)`), with `isr_ver` guarding prefix records
  (`target.rs:2510–2518`).
- A same-epoch reply cannot legitimately be smaller.
- A higher epoch deposes (#80).
- The ublk size is fixed at creation.

**Replica.** A replica's size rises only from a Hello its fence accepts:
- A stale Hello gets `Stale`.
- A replica refused by preflight keeps the old fence, and a deposed target's size is never larger.
- The Hello's size is asserted by the token holder (`token.rs:15–24` binds no geometry), as it
  already is for creation.

## Nits

- **R3-S4 — Nit.** `spec.md:70–71` says I/O waits "until one can create it". A waiting prefix
  chunk exists already: it waits until one of its replicas accepts the session, which needs slots
  for every chunk of the volume placed there. Reword.

Reviewed commit: a56309eafb476ebf79a0239535c1796bb2502764
