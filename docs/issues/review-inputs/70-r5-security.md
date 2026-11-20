# Issue #70 plan review — round 5 (incremental), security focus (design-security)

Scope: `git diff 91caea0..e729522 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70`. HEAD 2c8856c changes only `report.md`. Line numbers are at
HEAD. Dependency plans read at: #72 7aae38e, #73 c69830e, #75 e907b39, #80 HEAD.

## Earlier findings

- R4-S1: the three asked changes are made: the restart release removes the pod first, the
  slot-table limit is stated, and `a_suffix_chunk_without_a_slot_waits` now frees a slot and sees
  the I/O complete (design.md:200). The new release text is wrong in places (R5-S3, R5-S4).
- R4-S2: fixed in this plan. The narrowing now waits for the charge (design.md:273, decisions.md:36).
  The charge is specified in the design (Capacity charge, :71–80), in S2 (spec.md:45–46) and in a
  test. But only this plan assigns the work to #72 (R5-S2), and #73 has the same gap (R5-S1).
- R4-S3: fixed. spec.md:5–7 matches `git diff a3dbb5c..e729522 -- spec.md`.
- R4-S4: fixed. `blkid -p` or `mount` is named and "in the last suffix chunk" is gone. One waiter
  is still missing (R5-S6).
- R4-S5: fixed. decisions.md:27 and adr-proposal.md:55 say "accepts the session".
- R4-S6: fixed. The new text matches the code: exit 2 (`flag.ExitOnError`, `main.go:84`) and
  `Restart=on-failure`, `RestartSec=2` (`meta.sh:34–35`). With systemd's default limit of 5 starts
  in 10 s, a restart every 2 s keeps looping. The rollback now turns the flag off first, and the
  usage says so.
- R4 code F1, F3 and the code nit: fixed. #75's flags file is kept by every `meta up`; S9's row
  lists the script test; the release says "succeeds once a replica accepts the session".
- R4 code F2: fixed as `(KEEP=1 csi_failover) || die` (design.md:223). The flag check it names is
  out of date against #75 (R5-S5).

## Findings

### R5-S1 — Medium

**Neither plan builds growth's quota charge, so once #73 lands, resizing a PVC gets around #73's
quota.**
- #70 says: "#73's quota is charged in the same transaction once #73 lands; this step does not
  build it" (design.md:79–80).
- #73 says growth is not its job: "Growth, snapshots and backups are charged by the issues that
  add them" (#73 spec.md:87–88), and "growth quota belongs to #70" (#73 intent.md:48).
- So with both landed, a tenant creates a volume within its namespace's quota and grows it to
  #80's largest size. #73's service-side check, which #73 calls the authority, never sees the
  growth.
- design.md:262 says "only a ResourceQuota bounds that until #73". That is false: #73 landing
  bounds nothing.
- The 120-operation bound (design.md:76–78, 1 + k + D operations, 2 + D compares) leaves out
  #73's usage keys, which would join the same transaction.

Fix:
- State as for #72: whichever of #70 and #73 lands second charges each step's added bytes to the
  owner's namespace and class quota in the step's transaction, by #73's rule. A step without quota
  answers `ResourceExhausted` and writes nothing. Delete releases the grown size.
- Add the case to `TestExpandChargesLedger`, or add a `TestExpandChargesQuota`.
- Count #73's keys in the 120 bound.
- Write "until growth charges #73's quota" in Risks.
- Record the change #73 needs in the batch log, as was done for #13 (`2a2bdc2`).

### R5-S2 — Medium

**The ledger charge is assigned to #72 by #70's plan only. #72's approved plan does not build
it, and its rule leaves no room for it.**
- #70 says: "Whichever of #70 and #72 lands second adds this, with its test" (design.md:79;
  decisions.md:147–150).
- #72's spec, at 7aae38e, says "exactly these transactions add charges": `CreateVolume`'s claim
  and `MoveReplica` (#72 spec.md:54–59).
- #72's design and its file list name no growth path. Its intent and ADR expect #70 to do it
  (#72 intent.md:50, adr-proposal.md:66).
- If #72 lands second and follows its own plan, ExpandVolume places copies that are never
  charged. That breaks #72's invariant (#72 spec.md:31–36). The case where a growth overfills a
  node's slot table can then commit again.
- The README narrowing waits for a charge that no plan schedules in that order. This is R4-S2's
  defect again, moved into #72's plan.

Fix: record in the batch log the change #72 needs: S3 gains ExpandVolume's step, and the design
and tests gain #70's Capacity charge, if #72 lands second. Alternatively, make #70 the only owner:
whichever lands second, #70's implementation adds the charge. Then the README narrowing names
that change.

### R5-S3 — Medium

**Release (b) fails in the common case, and it does more damage than it needs to.** The text:
"Scale the workload to 0 or delete the pod, then restart the data plane on the target's node"
(design.md:267–269; decisions.md:33).
- **Deleting the pod is not enough when a controller owns it.** StatefulSets are the common case,
  and the lab's own workload is one (design.md:214). The controller recreates the pod. Its
  NodeStage attaches again under a new epoch (`node.go:58–77`), on this node or another, and
  waits again under that node's `n.mu`. This is the restart-first failure the text warns about,
  and it can move the wait to a second node.
- **Restarting the data plane hits other volumes.** It drops every replica session the node
  holds, so other tenants' volumes lose that member (an ISR change, or a stall at minISR). With
  #15 it also fails every volume attached there. A narrower tool exists: the control socket's
  `detach <volume>` "End[s] the target first, so pending I/O fails cleanly"
  (`flaminio-io/src/control.rs:192–212`). The lab already uses it as `node.sh detach`
  (`lab.sh:1083–1088`).
- **The volume can still block any node.** After the release it is still unservable. Any later
  stage of it, on any node, waits again, and the tenant controls when that happens by scaling up.
  The text does not say the workload must stay at 0 until (a) works or the volume is deleted.

Fix: write (b) as: scale the owning workload to 0 (a pod its controller recreates stages again),
then detach the volume on the target's node through the control socket. Keep the data-plane
restart only as the last resort, with its cost stated. Say the workload stays at 0 until (a)
works.

### R5-S4 — Medium

**Release (a) gives the wrong condition, and "can only be deleted" claims more than is true.**
The text: "This helps only if each refusing node's slot table can hold the volume's chunks placed
on it; otherwise those chunks are never served again, and the volume can only be deleted"
(design.md:265–267; decisions.md:31–32). I wrote that condition in R4-S1; it was too strong.
- **The condition is per chunk, not per node.** A waiting chunk is served once one of its
  replicas accepts the session (S6, spec.md:75–76; design.md:158–160). At RF ≥ 2 on nodes of
  different sizes, a chunk whose other replica node can hold its chunks is served. The text
  calls it lost and tells the operator to delete a volume that could have been saved.
- **Deleting the volume does not free the node.** DeleteVolume never touches the waiting target
  (`service.go:539–577`), and the PVC cannot be deleted while a pod uses it. (b) is still needed.
- **The prefix data is still there.** Its chunks still serve: raw block I/O to them completes
  (design.md:260), and the ext4 filesystem lies wholly in the prefix until `resize2fs` runs. So
  "can only be deleted" announces a data loss the design does not force. The data can be copied
  out first.

Fix: "it helps once every waiting chunk has a replica on a node whose slot table can hold all of
the volume's chunks placed there. A chunk with no such replica is never served again: copy the
prefix out, then delete the volume. Either way, (b) frees the node."

### R5-S5 — Low

**`csi_expand`'s flag check copies #75's old method, which #75 has since replaced because it
proves nothing.**
- The design says `csi_expand` "reads `/etc/flaminio/meta-flags` … as #75's `qos-check` does, and
  refuses unless `FLAGS` holds `--volume-expansion`" (design.md:209–211). The S9 test refuses
  "when one VM's file lacks the flag" (:202).
- #75's round 5 (00e57e8) replaced that check with `meta.sh running FLAG`, which reads the
  service's `/proc/<pid>/cmdline`. #75's reason: "The file alone proves nothing (a unit that does
  not read it, a failed restart)" (#75 design.md:226–233).
- #70's own rollback case shows the problem. An older binary loops on the flag (design.md:83–86),
  yet the file check passes on that VM.

Fix: require `meta.sh running --volume-expansion` on every VM of `vms_of cluster`. Make the S9
test fail when one VM's fake cmdline lacks the flag or the service is inactive.

### R5-S6 — Low

**The scope still misses NodeUnstage's `umount`, which can wait under `n.mu` itself.**
- The scope lists `blkid -p`, `mount` and `resize2fs` (design.md:255–258). It names NodeUnstage
  only as a call blocked behind `n.mu`.
- Suppose stage and resize pass, because the waiting chunk is one none of them read. Every
  refusing node holds at least one suffix chunk, and this can happen when the last chunk is
  served.
- The pod's writes, or a journal checkpoint, then dirty blocks in the waiting chunk. NodeUnstage's
  `umount` (`node.go:137`, `mount.go:81–84`) flushes them and waits, holding `n.mu`
  (`node.go:135`).
- Scaling to 0, the first step of release (b), then starts that very wait.

Fix: add "NodeUnstage's `umount`, which flushes the filesystem's writes" to the scope in the
design, decisions.md:28–30 and S6. Say that release (b) frees it too.

### R5-S7 — Low

**The narrowing is not where the operator turns the flag on, and no one is told to remove it.**
- The narrowing is in the README note and in `docs/followups.md` (design.md:233–238).
- It is not in the flag's usage (design.md:88–90) or in the `meta-flags` help (:239–240). Those
  are the two texts an operator sees when turning the flag on, and #75's pattern makes them the
  warning.
- "Whichever … lands second adds this" (design.md:79) covers the code and the test. It does not
  cover removing the narrowing from the README and followups, so an operator cannot tell whether
  their build charges the ledger.

Fix: add "where tenants share data nodes, only once growth charges the capacity ledger
(docs/followups.md)" to the usage and to the `meta-flags` help. Make the second lander remove
that text, the README narrowing and the followups row.

### R5-S8 — Low

**`TestExpandChargesLedger` leaves out the abuse paths that the "slot-table case never commits"
claim relies on** (design.md:196).
- **Slots.** No case refuses a step for slots while its bytes fit. The node-wide wait is about
  slots.
- **Left-out nodes.** No case covers nodes left out of placement: a ledger being initialized,
  absent or unreadable, or a node with no valid report. A step that places there charges
  nothing, which breaks #72's invariant.
- **Preferred node.** No case covers a preferred node whose room is less than the step's chunks.

Fix: add the three cases. Each must fail if growth skips the rule.

## Nits

- **R5-S9 — Nit.** The usage ends "so remove it before any rollback" (design.md:90). That reads as
  if removing the flag makes every rollback safe. A data plane rollback after a growth stays
  unsupported either way. Say "remove it before a metadata rollback; a data plane rollback after
  a growth is unsupported".

Reviewed commit: 2c8856c995f9f6d66d75b2fd086dcb993d2887c5
