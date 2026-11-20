# Issue #70 plan review — round 2 (incremental), security focus

Scope: `git diff 7a5a04e..4057eb9 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70`, checked against that worktree's code and the #78, #79 and #80
drafts it relies on. Line numbers are at 4057eb9. HEAD has since moved to a3dbb5c, which only
records the approval and settles the open questions. I read it, and it does not change these
findings: the accepted availability risk covers prefix chunks only (see R2-S1).

## Earlier findings

- R1-S1: fixed. XFS is cut, the controller refuses XFS, and NodeExpand never runs mkfs.
- R1-S2: partly fixed. The case of one full replica is stated (S6) and tested. The case where
  every replica of a suffix chunk is full is not, and the stated path is wrong once #80 lands
  (R2-S1).
- R1-S3: fixed. There is no pending operation, and Attach is never refused.
- R1-S4: fixed. No header field is added, the flag is off by default, and a test checks that the
  header's keys are unchanged.
- R1-S5: fixed. Capability labels are cut, and the rollout order is the gate.
- R1-S6: fixed. The use-state protocol is cut, and a running target ignores appended chunks.
- R1-S7: fixed. Each step compares #78's gate, which drain marks and registrations advance.
  `TestExpandRaces` covers it.
- R1-S8: fixed. The known size rises only in the Fence arm. Tests cover a stale larger Hello and
  a smaller Hello.
- R1-S9, R1-S10, R1-S11, R1-S12, R1-S13: fixed by the cut.
- R1-S14: fixed. The prerequisite commits match the heads of the #18, #77, #78, #79, #80 and #15
  worktrees.
- R1-S15, R1-S16: fixed.

### R2-S1 — Medium

**When every replica of a suffix chunk lacks a slot, the attach still succeeds at the larger
size, and NodeExpand hangs past its deadline while holding the node plugin's lock. S6's account
of the refusal is also wrong once #80's preflight runs first.**

How it happens:
- The engine reports `Attached` as soon as `start_target` returns, before any chunk is fenced
  (`flaminio-node/src/engine.rs:1490–1500`). The device is created at that size.
- A chunk leaves Fencing only once one of its members is fenced (`flaminio-repl/src/target.rs:2218–2252`).
- Client operations are queued whatever the chunk's phase, and fail only when the target is
  deposed (`target.rs:747–766`). They are blocked per chunk (`target.rs:2159–2180`), and no I/O
  deadline exists.

So when every replica node of a suffix chunk refuses the session (S6, `spec.md:67–69`), the
prefix is served and NodeStage mounts. Then:
- NodeExpand's `resize2fs` calls the kernel's online resize, which reads the new last block
  (`ext4_resize_fs`) and waits uninterruptibly.
- The design says "Context end: the child is killed, `DeadlineExceeded`" (`design.md:107`). That
  cannot happen while the child waits in the kernel.
- NodeExpand runs under `n.mu` (`design.md:93`; `control/csi/node.go:27–28`; #79 keeps `Node.mu`
  per RPC), so every later CSI node call on that node waits. With #15, that includes other
  tenants' volumes.
- A raw block pod's I/O to the suffix region also waits indefinitely.

Who can cause it, and what was accepted:
- Every suffix chunk prefers the volume's last target (`design.md:64–65`), so a PVC owner picks
  one of the replica nodes through pod placement.
- The accepted risk (decisions.md at HEAD; design Risks) covers prefix chunks that lose a member
  or stall at minISR. It does not cover this case.

Separately, `design.md:128–129` says the Fence arm fails the session "as today (`:252–254`)".
But `design.md:125` runs #80's preflight first, and that preflight refuses on chunk slots before
the core fences (#80 `design.md:93–98`). Today the core advances the fence in memory before
creation fails (`flaminio-repl/src/replica.rs:168–184`). So a full replica stays fenced at the
old epoch, like an unreachable one. That is safe under ADR 0005, because the new target fences
another ISR member before it serves the chunk. But the design does not say so, and
`a_full_replica_refuses_the_grown_volume` does not check it.

Fix:
- State in S6 what an attach does when no replica of a suffix chunk can be created.
- Keep NodeExpand from hanging the node. Either check before running the tool that the target
  serves every chunk (a daemon status field, which is a contract change), or document the hang
  and add it to the accepted risk.
- Correct `design.md:128–129` to describe the preflight refusal and its fence consequence.
- Tests:
  - an engine test where every replica of one suffix chunk is out of slots: prefix I/O
    completes, suffix I/O waits, and no operation fails;
  - a `TestNodeExpand` case whose `GrowExt4` fake ignores cancellation;
  - in the full-replica test, a check that the old epoch's fence is unchanged on that node.

### R2-S2 — Low

**NodeExpand's device-identity refusals are untested.** Two steps keep a root `resize2fs` off
the wrong device, for example a device the daemon recreated after a restart:
- step 3 refuses a pin mismatch (`design.md:99–100`);
- step 5 refuses a stage mounted from another device (`design.md:104–105`).

`TestNodeExpand` (`design.md:169`) lists block, ext4, device too small, readonly, another fs,
capability mismatch, unowned path, tool failure and repeat. It has no case for:
- `device-info` changing between its two replies;
- a stage whose mount is on another device;
- a record outside phase `ready`.

Fix: add those three cases, each asserting that no tool runs.

### R2-S3 — Low

**The lab section leaves out what it deletes, its restore is partial, and the sequence cannot
run as written.**
- **What it deletes:** the ext4 run reuses `deploy/lab/csi-test.yaml` (namespace
  `flaminio-test`). Its precedent, `csi_test`, first detaches the lab volume on every VM and
  deletes any existing `flaminio-test` namespace, volume included (`scripts/lab.sh:1083–1088`,
  `1105–1106`). The section does not say whether `csi-expand` does the same. If it does, it
  deletes a volume kept with `KEEP=1` by an earlier `csi-test` or `csi-failover`.
- **Restore:** it names only the uncordon trap (`design.md:185`). A failure after "scale to 0"
  leaves the StatefulSet at zero replicas and the namespace in place.
- **Sequence:** `KEEP=1` is needed for the failover step, and it keeps both volumes. A node
  serves one volume at a time (`control/csi/node.go:31–37`), and the lab has two data VMs.
  - The block run cannot move to "the other node" while the kept ext4 pod holds that node.
  - `csi-failover` cannot bring the ext4 pod onto the node that the block pod holds.
- **Failover check:** if `data-0` is gone, `csi-failover` creates a fresh 4Gi volume
  (`lab.sh:1144–1147`). It checks only the 256 MiB blob (`lab.sh:1179`). It can therefore pass
  without testing the grown volume.

Fix:
- Run the block check first and always delete its namespace.
- Then run the ext4 check with `KEEP=1`, then `csi-failover`.
- State what is deleted at the start.
- Have the failover step check the PVC's 6Gi capacity and the added file.

## Nits

- **R2-S4 — Nit.** `design.md:137` puts `Options` in `service.go`, but it is declared in
  `meta/service/auth.go:20`.
- **R2-S5 — Nit.** `design.md:116–118` calls the engine refresh "unchanged", and Files touched
  (`design.md:139`) omits `flaminio-node/src/engine.rs`. But #80 puts its refresh guard there and
  forbids loosening it implicitly (#80 `design.md:79–86`). Passing a larger same-epoch view is a
  change to `engine.rs`.

Reviewed commit: 4057eb9ba1b5af7d487de4bc5bbd500e24d89734 (the briefed diff end; HEAD
a3dbb5cb10927500176d9cc05602dccd1cae6b57 adds only the approval record)
