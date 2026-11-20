# Issue #48: Live data-plane upgrade — specification

Steps 1 and 2 of the issue, for one node ([intent](intent.md)). Built on #12 and #15 as
planned: **S4 replaces part of #12's S4, and S5 replaces most of #15's S10.**
Changed after approval, plan review rounds 1 to 3 (2026-11-17): [decisions](decisions.md).

## Terms

- **Restart:** the node's data plane ends, however it ends (the **predecessor**), and a next
  one starts on the node (the **successor**).
- **Device, entry, instance, generation:** as in #15. **Record:** what the node keeps of each
  device outside the process. **Run mark:** #15's (S11): the last epoch at which a target of
  the volume started on this node, durable.
- **Recoverable device:** one that the kernel keeps when its server exits, and whose requests
  in flight it then issues again (S1).
- **Kept device:** a device whose server has exited and which the kernel holds: its id,
  `major:minor`, openers and mounts stay, and its requests wait.
- **Resume:** a successor serves a kept device again, as the target of its recorded volume,
  under the next epoch.
- **End a device:** delete it. Its waiting and later I/O fails, as after a crash today.
- **Quarantine:** a device whose record cannot prove it this node's is left as it is, for the
  operator (S5).
- **Acknowledged:** completed to the kernel by the data plane.
- **Pause:** on one device, from the last completion before the exit to the first after it.
  **The bound:** the longest a kept device waits for its resume (Q2).
- **The agent:** the holder of the node's agent identity; today, the CSI node plugin.

## The invariant

Across a restart a kept device is one block device that paused: only the delay tells its user
that the server changed.

- **An acknowledged write is there afterwards.** Every later read returns it, until it is
  overwritten (ARCHITECTURE §8.1).
- **A request unacknowledged at the exit gets no error from the restart.** The kernel issues it
  again, and only the successor's target answers it.
- **An unacknowledged write** may have reached every replica, some, or none. From the old
  epoch it takes effect, if at all, before the first operation of the new epoch on its blocks
  completes: reconciliation settles it. Issued again, it writes its data under the new epoch.
  So no read returns its value and a later one the older value.
- **Unacknowledged writes that overlap** end in either order, as concurrent writes do.
- **One target at a time.** The successor serves only once the predecessor's epoch is fenced
  on every ISR member, and acknowledges nothing from the predecessor's state.
- **A device serves one volume for its whole life:** the one it was added for.
- **No other target in between** (changed after approval, R1-S1). A kept device is resumed
  only if no target of its volume started anywhere since the device last served.

The bound and every time below are about availability. Safety comes from the new epoch and its
fence, whenever the resume comes.

## Behaviour

Changed after approval, plan review rounds 1 to 3 (2026-11-17): the marked rules below; why, in
[decisions](decisions.md).

**S1. Devices are added to be kept.** The kernel keeps every device the node adds when its
server exits, and then issues its requests in flight again. With zero copy too, and under
#74's helper too, which sets the flags itself (R1-C1). On a kernel that lacks either, or that
returns a device without either, devices are added as today, the node's log and status say so,
and a restart ends them (Q5).

**S2. What an exit leaves** (Q4). However the process ends (a signal, a kill, a panic, its
pod's deletion), every running recoverable device is kept, and no request fails because of the
exit. Under #74's helper, the helper ends with the process and the device stays (R1-C1). A
device that was starting, stopping or in the middle of a recovery is not resumed: S5 ends it.

**S3. The record** of a device: its volume and the volume's first epoch, its id, `major:minor`,
diskseq, size and parameters, the last epoch it served at, and when its data plane was last
alive. It is written before the device is added and removed after it is deleted (#15). It
outlives the process, and in #12 the pod; not a reboot, which ends every device. It holds no
token or key, and only root reads it. *Narrowed after approval (R1-C1, R1-S2; R2-B10):* under
`--privsep` root's record holds the device, its `major:minor` and diskseq included. What only
the I/O process knows, the volume and its lifetime, is in a note that the I/O user writes and
its successor reads: so the I/O user names the volume its successor serves on the device, and
nothing else reads the note (design, With #74's helper).

**S4. A planned exit** (Q6). On SIGTERM, while every attached device is recoverable (S1), the
data plane exits at once, without waiting for I/O in flight, and takes no attach after the
signal. Its pod's deletion then ends within seconds. *Replaced in #12's S4, for recoverable
devices:* a SIGTERM while attached remembered until the detach; a deletion while attached
waiting out the grace period; a crash while attached failing the volume's I/O. *Narrowed after
approval (R1-C14, R1-S4):* with an attached device that is not recoverable, #12's S4 stays
whole. The rest of #12's S4 stays, the lab commands' refusals and the pod's manifests included.

**S5. The successor's start.** Before it opens its store, and before its control socket
answers, the node goes through its records and the kernel's devices. *Replaces #15's S10,
except its last two bullets.*
- **A kept device that matches its record,** inside the bound, becomes an entry in a new
  state, **kept**, with its recorded lifetime, id and `major:minor`. It counts towards the
  limit (#15, S1).
- **A recorded device that cannot be resumed** is ended: dead, starting or stopping at the
  exit, in the middle of a recovery at the exit (R3-B1), past the bound, or held by the kernel
  with one of S1's two flags only (R2-B11).
- **A recorded device that differs from its record,** or whose record cannot be read, is
  **quarantined** (settled at approval, 2026-11-13; R1-C5): never deleted, never resumed,
  logged and listed in the node's status. If the kernel holds it, its requests wait until the
  operator deletes it (`flaminio-io del`). The bound does not apply to it.
- **A record without a device** is dropped. **A device without a record** is never touched.
- **The attach at startup** never resumes: its token is the old epoch's (#15, S11).

**S6. The resume.** Only an attach of its recorded volume resumes a kept device.
- **The token** (changed after approval, R1-S1, R1-C11) is for the volume's recorded first
  epoch, and for exactly the epoch after the volume's run mark on this node (#15, S11).
  - At or below the run mark, the attach is refused, and the device stays kept.
  - At any other epoch, an attach happened that this node did not see: the device is ended.
  - With another first epoch, the volume was made again under its name: the device is ended.

  The data plane checks this itself, whoever calls. The volume's size must be the device's.
- **The target starts as on any attach:** it fences and reconciles (replication-protocol §6),
  with #83's handoff where #83 is in. Then the kept device is served again. No second device
  is added for the volume.
- **The answer** names the same path; the status, the same `major:minor`, and the recorded
  instance and generation. *Changed after approval (R1-C3, R1-S7):* the device's lifetime
  (#15's contract) goes on, so that a stage made before the restart still unstages.
- **The requests the kernel held** go through the target as any I/O: each waits for its chunk.
- **An attach of another volume** never takes a kept device.
- **A resume that fails** before the kernel began to recover the device leaves it kept, for
  the next try or for S8. *Narrowed after approval (R1-C7):* once the kernel began the
  recovery, a failure ends the device, the successor's own exit included (R3-B1), and so does
  a kernel that refuses the recovery. A kernel that is still busy with the device refuses
  nothing: the device stays kept.

**S7. The agent resumes** (Q1, as recommended).
- When the node's data plane answers and lists kept devices, the agent takes a new epoch for
  each (`Attach` with `only_if_target`) and attaches it. Kubelet need not call.
- It tries again until the device is resumed, the metadata service refuses, or the bound; at a
  growing interval, with one `Attach` at most per try and none while it holds a token that the
  data plane has not used (R1-C15, R1-S9).
- It waits on no lock that a mount, a format or an unmount holds (R1-C2).
- One volume that cannot be resumed delays no other, beyond the wait #15's S1 names (attaches
  are taken one at a time).
- On a node outside Kubernetes, a lab command does the same, as the node's agent (S10).

**S8. Ending a kept device** (Q2, as recommended). It is ended, and its record removed, when:
- the metadata service answers that this node is not its volume's target, or that the volume
  is gone: the failover controller may have moved it meanwhile (§11.2);
- the bound has passed since its data plane was last alive. A data plane that resumed the
  device and ended before the deadline the device then had does not count (R1-S5, R2-B8): a
  crash loop ends at its first exit's bound, and a device served past that deadline starts a
  new bound at its next exit;
- its volume is detached (`NodeUnstageVolume`: by the device's whole lifetime once #79 is in,
  by name until then; or root's detach), during a resume too (R2-B9);
- a resume shows another target or another volume, or the kernel can no longer recover it
  (S6).

The successor ends it, from before its store is opened; under #74's helper, the helper; or the
agent when no data plane runs. Ending twice is harmless. The bound never cuts a resume under
way, which ends within #6's limits. Under #74's helper a resume is under way from the recovery
request that the kernel accepted: one that reaches the helper after the bound is refused, and
the device is ended. An ended device is never resumed: its volume attaches again as after a
crash today.

Settled at approval, 2026-11-13, and written here after it (R1-C5):
- no deadline is enforced while neither a data plane nor the agent runs: the agent's
  availability is a stated dependency;
- the bound covers the end of the service and of the waiting I/O. The kernel's id may last
  longer, while a mount holds it;
- a quarantined device (S5) is outside this rule.

**S9. The pause** (Q3, as recommended).
- **Promised:** under 2 s for a planned restart of the process, every ISR member of the volume
  reachable, on the lab's 16 GiB volume and kernel, under the helper and in direct mode.
- **Not promised:** a member unreachable adds the takeover wait (5.5 s, ADR 0005). On a pod
  node the pod's replacement adds its time, which is reported.
- **Logged for every resume:** when the process started, the store opened, the socket first
  answered, the new epoch came, the device was served again, and its first request completed.

**S10. The lab.** `make node-restart` restarts one node's data plane, by SIGTERM or on request
by SIGKILL, and leaves its devices. On a unit node it then resumes them, unless the CSI node
plugin runs there and does; on a pod node the agent does. A lab check does so under a pod
running verified fio, and reports the pause with S9's times. `make chaos`, `make node-fault`
and `make csi-failover` pass unchanged: a killed target's kept device is ended when its node
returns (S8).

**S11. Unchanged.**
- The replication protocol, the metadata service and its authorization, the on-disk format.
- #6's time limits. #15's limit, identity checks and control socket, but for the kept and
  resuming states and one status line.
- #12's ordered upgrade: `make node-upgrade` still evicts (step 3). The units' `down`: it ends
  the unit's device, and refuses while it is mounted.
- An older CSI node plugin resumes nothing: under it, kept devices end at the bound. The
  plugin is upgraded first (settled at approval: a rollout limit; R1-C5).
- A build without #48 ends no kept device: every volume is detached before a downgrade
  (R1-C9, R1-S8).

**S12. Docs.** ARCHITECTURE §5.1, §11.1 and §18: what a restart does now, and what waits for
steps 3 and 4. `docs/replication-protocol.md` §9: the new check. `docs/lab.md`: the command,
the check, the downgrade rule. Follow-ups: the lab's result.

## Tests

Each test's file and name: [design](design.md), Tests.

- **S1–S5, unit:** the flags asked for and returned, per kernel feature set; the record's round
  trip, order and permissions, and its refusals; SIGTERM while attached to recoverable and to
  unrecoverable devices, attaching and idle; each case of the start, the quarantine among
  them: nothing deleted, and listed in the status.
- **S6, S8, engine:** a second engine on the same store, after the first is dropped without a
  detach, attaches at the next epoch: acknowledged data is read back, writes in flight are
  written again, the old epoch is refused. Each refusal of S6 (a gap, another first epoch,
  another size, no handoff), each cause of S8, the bound against a resume under way, a detach
  during a resume, an attach of another volume, the crash loop.
- **The resume's steps, real kernel:** a successor killed between the kernel's two recovery
  commands, before and after its queues are ready; requests in flight issued again.
- **The invariant, simulator:** a target restarts on its node, and its client's unanswered
  operations are issued again to the new target. Histories stay linearizable under the rule
  for unanswered writes. A kept device met again after another node was the target is ended.
  An injected bug, a resume across such a gap, is caught.
- **S7, S8, unit (Go):** the agent against a fake data plane and metadata service: every kept
  device resumed with no kubelet call; an unmount blocked on a kept device, and the loop still
  resumes it, and a second volume too; the `Attach` calls counted; a refusal ends that
  lifetime only; the bound; a stuck volume beside a resumed one; an unstage after a resume.
  `io` still may not call `Attach`.
- **With #74's helper:** its requests, its record, a kept device taken over by the next
  helper, on a fake kernel and on a real one.
- **S10, script tests** (`make lint`), VMs faked: `node-restart` both ways, on a unit node and
  a pod node; never a `down`; `down` unchanged.
- **Lab,** Linux 7.0, with and without zero copy, under the helper and in direct mode:
  1. Verified fio in a pod on fl-a, its data plane restarted by SIGTERM, then by SIGKILL: no
     I/O error, the data verified, the same device and mount, the epoch one higher. The pause
     is under 2 s on the unit, and reported on a pod node, with S9's times.
  2. Two volumes on the node (#15), fio on both: both resumed.
  3. The volume moved to fl-b while fl-a's data plane is down: fl-a's device is ended, and
     fl-b serves the data. Moved to fl-b and back inside the bound: fl-a's device is ended
     (R1-S1). A data plane that does not return: its device ends at the bound.
  4. `make chaos`, `make node-fault`, `make csi-failover`.

## Open questions

- **Q1. Who takes the new epoch.** (a) The agent identity's holder, today the CSI node plugin:
  the authorization rule stays, and the plugin gains a loop quick enough for S9. (b) The data
  plane, with a new right for the `io` identity (§15). *Recommended: (a), as #12's decisions
  foresee. S7 follows it.*
  Settled at approval, 2026-11-13: (a), the agent identity's holder (see decisions.md, Settled
  at approval).
- **Q2. The bound, and who ends a device past it.** Without one, a data plane that does not
  return leaves the pod, and its unmount, waiting for good. *Recommended: 60 s, as the plugin's
  limit on a control command (an attach's own is 50 s); ended by the successor, or by the
  agent when none runs. S8 follows it.*
  Settled at approval, 2026-11-13: 60 s; ended by the successor, or by the agent when none runs
  (see decisions.md, Settled at approval).
- **Q3. Where the 2 s is promised.** *Recommended: as S9. If the pod's replacement alone
  passes 2 s, §16.2's target needs a handover (out of scope) or a new figure.*
  Settled at approval, 2026-11-13: as S9 (see decisions.md, Settled at approval).
- **Q4. Crashes too.** The kernel keeps a device whatever ended its server. *Recommended: a
  crash is resumed like an upgrade, and the bound ends a crash loop. S2 follows it.*
  Settled at approval, 2026-11-13: a crash is resumed like an upgrade; the bound ends a crash
  loop (see decisions.md, Settled at approval).
- **Q5. A kernel without the reissue.** ADR 0003 names `UBLK_F_USER_RECOVERY` only. (a) Add
  devices as today, logged. (b) Refuse to start. (c) Keep devices whose requests in flight
  fail. *Recommended: (a); S1 follows it. If Linux 6.6 lacks the reissue, what the baseline
  promises changes: a superseding ADR.*
  Settled at approval, 2026-11-13: (a), devices added as today, and logged (see decisions.md,
  Settled at approval).
- **Q6. A deletion of the data-plane pod while attached** no longer waits (S4). #12 made it
  wait so that no deletion failed a volume's I/O. *Recommended: exit at once. A deletion now
  pauses the I/O; step 3's guard is what stops one that would hurt.*
  Settled at approval, 2026-11-13: exit at once (see decisions.md, Settled at approval).
