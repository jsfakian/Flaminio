# Issue #48: Live data-plane upgrade — intent

[Issue #48](https://github.com/jsfakian/Flaminio/issues/48); ARCHITECTURE §5.1, §11.1, §16.2,
§18. This change is the issue's steps 1 and 2 together, for one node: a data plane that
restarts on a node serving a pod keeps its devices, and the pod's I/O pauses, then goes on. The
two steps cannot be checked apart: a device that survives is of no use until a target serves it
again. Steps 3 and 4 come later: issue #48 stays open after this change.

Built on #12 (the data plane in a pod) and #15 (several volumes per node), as planned. Both
left this out by name (#12 decisions: "Running through its own node's restart needs ublk user
recovery"; #15 decisions: "A restart ends every device. There is no user recovery"). #74's
helper leaves recovery to this change as well: with it, recovery commands go through the
helper (#74 intent, Which step). Changed after approval, plan review rounds 1 to 3 (2026-11-17):
see [decisions](decisions.md).

## Why

ARCHITECTURE describes user recovery as how the data plane restarts. The code has none of it.

1. **The devices end with the process.** `add_device` creates the ublk device with the
   zero-copy flags or none (`flaminio-io/src/ublk.rs:902-934`): never `UBLK_F_USER_RECOVERY`.
   Nothing in `dataplane/crates` names it. Without the flag, the kernel fails the device's I/O
   when its server exits. A filesystem mounted on it gets I/O errors, and stays on a dead
   device.
2. **A planned stop is refused, or is a crash.** `flaminio-io` handles no signal: SIGTERM ends
   it as SIGKILL does. The lab's `node.sh down` refuses while the device is mounted ("unmount
   it first, or delete the pod that uses it", `scripts/guest/node.sh:24-27`), then deletes the
   device. The unit runs with `Restart=no`. #12 keeps this: a SIGTERM while attached waits for
   the detach, and the ordered upgrade evicts the node's pods first (#12 spec, S4, S5).
3. **The new process knows no volume.** What is attached is in memory only (`Attached`,
   `ublk.rs:682`). After a restart `status` answers `ok none`.
4. **Nothing attaches again.** The CSI node plugin starts a target only inside
   `NodeStageVolume` and `NodePublishVolume` (`device`, `control/csi/node.go:58-79`), and kubelet
   calls neither for a pod that is running. So after a crash the pod's I/O fails until the pod
   is deleted (#12 spec, S4).
5. **The new process cannot take its epoch.** A target never restarts under the epoch it had:
   its session numbers start from 1 again (replication-protocol §3). A new epoch is `Attach`
   with `only_if_target`, which only the node's agent identity may call
   (`meta/service/auth.go:66-70`). The data plane holds the `io` identity alone (#12 spec, S2).
   Today the CSI node plugin holds the agent's.
6. **The protocol's part exists.** A new epoch, fencing and reconciliation are what every
   attach does (replication-protocol §6). After a restart the local replica reports every block
   of its chunks as uncertain: its uncertain set is volatile. Digests make that cheap. After
   both replicas of a 16 GiB volume restarted, the first read waited 0.13–0.17 s, and the whole
   volume was reconciled in about 1 s (follow-ups §2, §10;
   [results](../../benchmarks/2026-11-04-reconcile-order.md)). The control socket answers an
   attach in under 0.1 s (follow-ups §10).
7. **libublk is not the limit.** libublk 0.4.8 accepts `UBLK_F_USER_RECOVERY`,
   `UBLK_F_USER_RECOVERY_REISSUE` and `UBLK_F_QUIESCE` (`UBLK_DRV_F_ALL`, its `src/ctrl.rs:1107`),
   recovers a device (`UBLK_DEV_F_RECOVER_DEV`, `start_user_recover`), and has an example that
   does so (`examples/ramdisk.rs`). To handle:
   - a control made with `UBLK_DEV_F_ADD_DEV` deletes its device when dropped, unless disowned
     (`src/ctrl.rs:1085-1096`, `disown`);
   - a recovery needs no JSON from before the exit (corrected, R1-C13, R1-S11): libublk tries
     to reload it from `/run/ublksrvd`, prints a failure and goes on (`src/ctrl.rs:1170–1173`),
     and writes it anew. So #12's `emptyDir` there stays;
   - its `start_user_recover` waits up to 30 s on `EBUSY` and cannot be cut
     (`src/ctrl.rs:2606–2624`).
8. **What the kernel does with requests** (the kernel's ublk documentation):
   - with `UBLK_F_USER_RECOVERY` the device and its id stay. Requests not yet handed to the
     server are requeued. Requests already handed over are **aborted**: the pod sees errors;
   - with `UBLK_F_USER_RECOVERY_REISSUE` as well, those are requeued too, and issued again to
     the new process: "for backends who tolerate double-write". "No error" needs this flag;
   - I/O waits for as long as there is no server. The kernel sets no limit.
9. **The baseline.** ADR 0003 names `UBLK_F_USER_RECOVERY` for Linux 6.6. It does not name
   `UBLK_F_USER_RECOVERY_REISSUE`: by the header's order both predate 6.6, which the design
   confirms against 6.6's source. `UBLK_F_QUIESCE` is later than 6.6; the lab runs 7.0. This
   step must not need it.
10. **A write issued twice is within the rules.** A write the old process sent but never
    acknowledged may be on some replicas. Reconciliation makes them agree on either value
    (ARCHITECTURE §11.4). The kernel then issues the same request again, under the new epoch.
    An acknowledged write was durable on the whole ISR, and the old target, fenced, completes
    nothing more (§8.4).
11. **No measurement exists.** No lab check restarts the data plane on the node that serves a
    volume and keeps the device. Unmeasured, and all inside the pause:
    - the store's recovery on open, after a stop under load;
    - the replacement of the data-plane pod (#12): its deletion, the successor's start;
    - reconciliation, when the local replica reports every block and several volumes are
      attached (#15).
12. **The 2 s target has a condition.** A new target serves at once only when every ISR member
    answers. Otherwise it waits out the old lease, 5.5 s (ADR 0005, replication-protocol §6).

## What we want

1. Every device is created recoverable. When the data plane exits, however it exits, its
   devices stay, with their ids and `major:minor`, and their mounts.
2. The next data plane on the node recovers them, each for the volume it served, under a new
   epoch, after fencing and reconciliation. Never under the old epoch, and never a device for
   another volume.
3. The pod sees a pause, not an error. Requests in flight at the exit are issued again. An
   acknowledged write is there afterwards; an unacknowledged one is written by the reissue.
4. A planned restart while attached (SIGTERM, the pod's deletion) exits at once and leaves the
   devices. This replaces #12's rules for a SIGTERM and a deletion while attached (S4), and
   #15's "every device ends with the process" (S10). Narrowed after approval (spec, S4): only
   while every attached device is recoverable; with one that is not, #12's rules stay.
5. The pause is bounded. A device whose volume is no longer this node's, or whose data plane
   does not return in time, is ended: its I/O fails, as after a crash today, and nothing waits
   on it for good. One exception, settled at approval (spec, S5): a device that its record
   cannot prove this node's is left for the operator, not deleted.
6. A lab check, by an operator: verified fio in a pod, the node's data plane restarted (stopped,
   and killed), no I/O error, the data verified, the epoch higher, and the pause recorded with
   its parts. Under 2 s with every ISR member reachable, on the lab's 16 GiB volume.

## Decisions the spec will ask for

- **Who takes the new epoch.** The holder of the agent identity (the CSI node plugin today), as
  #12's decisions foresee, or the data plane itself, with a new right for the `io` identity.
  Recommended: the agent identity; the authorization rule stays.
- **The bound of the pause,** and who ends a device past it. Without one, a data plane that
  does not come back leaves the pod waiting for good, and its unmount too. Recommended: about
  the plugin's limit on a control command (60 s; an attach's own limit is 50 s), ended by the
  next data plane, or by the agent identity's holder when none runs.
- **Where the 2 s is promised.** Recommended: the process's restart on a node, every ISR member
  reachable, measured in the lab. The pod's replacement time is reported, not promised, until
  measured.
- **Crashes too.** The flag is the device's: the kernel cannot tell a crash from an upgrade.
  Recommended: both recover; a crash loop is ended by the bound.
- **Two more, found writing the spec** (its Q5, Q6): what a kernel without the reissue gets,
  and that a deletion of the data-plane pod while attached no longer waits (#12, S4).

## Out of scope

- **Step 3: the storage disruption guard** (§10.4): a PodDisruptionBudget per storage node.
  Until then `make node-upgrade` keeps its checks and its evictions (#12 spec, S5).
- **Step 4: versions** (§18). The replication protocol carries no version: `Hello` has none
  (`flaminio-net/src/frame.rs:29-39`), and only the token has one (1 or 2). Negotiation per
  session, N-1 with N, and `finalize` for on-disk features are later.
- **The upgrade from N-1 to N of every lab node under live I/O:** the issue's last criterion,
  after steps 3 and 4.
- **A handover without a gap:** the new process started before the old one exits. #12 forbids
  two data planes on a node.
- **`UBLK_F_QUIESCE`:** a planned stop that drains first. Not on the baseline kernel.
- **A shorter reconciliation after a restart:** the uncertain set persisted at checkpoints
  (follow-ups §2, step 2). Wanted if the measurements say so.
- **The replicas the node holds for other nodes' targets:** they leave the ISR and rejoin by
  resync, as today (§11.1).
- **The replication protocol, the metadata service's transitions, the on-disk format:**
  unchanged.
- **Privilege separation itself** (§5.1, #74): this change adds no privilege boundary. It
  works under #74's helper, whose part in a recovery it designs (design, With #74's helper).
- **The NVMe/TCP fallback frontend.**
