# Plan review: issue #48 (Live data-plane upgrade), round 1, focus design-code

Scope: `docs/issues/48/` (intent, spec, design, adr-proposal, decisions) in
`/home/jsfakian/Documents/src/flaminio-48`, against the code there, libublk 0.4.8
(`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/libublk-0.4.8`), the ublk UAPI
(`libublk-rs-sys-0.1.2/ublk_cmd.h`; headers 6.1, 6.5), a local kernel source
(`/home/jsfakian/Documents/src/eve-kernel`, 6.1.112) and the clear plans #6, #12, #15, #17, #74,
#79, #83, #84. No 6.6 or 7.0 kernel source was available offline: kernel behaviour is cited
from 6.1.112 and marked where 6.6 must be read.

Counts: High 3, Medium 8, Low 4, Nit 3.

### R1-C1 — High
**#74's hand-off is not taken: under `--privsep` every rule from S1 to S8 is false.** The plan
never names #74 or the helper.
- #74's helper deletes its owned device when its child exits, whatever the cause, and then exits
  (#74 design, Owned devices, outcome 4, line 114). So "however the process ends, every running
  device is kept" (S2) fails in the lab's default mode (#74 spec line 63: the lab node runs under
  the helper by default). S9's promised 2 s is measured "on the unit", which is that mode.
- The helper adds devices with `ctrl_flags` 0 (#74 design, Requests, `add`; test `fixed_flags`),
  so no recovery flag. The I/O process has no `/dev/ublk-control`, so `UblkCtrl::get_features`
  (design, Kernel capability) cannot run there.
- The helper's pin keeps `UB_STATE_OPEN` set, and `START_USER_RECOVERY` answers `EBUSY` while it
  is (6.1.112 `ublk_drv.c:1933-1937`; #74 decisions R3-C2; #74 intent lines 33-37: "#48 must
  close the pin before recovery and take a new one after").
- #74's intent names `flaminio-io end-kept` as a second root path outside its boundary; the
  design keeps it (Agent, deadline and races; Contracts).
- Under `--privsep` there are no records, no directory lock and one id (#15 design, S10 "direct
  mode only"; With #74's helper). `END_USER_RECOVERY` needs the child's pid, as #74's
  `start_dev_for` gives `START_DEV`.

Fix: add a section "With #74's helper". Either (a) define the helper's part: recovery flags at
`add`, a helper that outlives the I/O process and keeps the device at the child's exit, requests
`recover` (close the pin, `START_USER_RECOVERY`, a new pin, the descriptor) and `end-recovery`
with the child's pid, the feature probe and the record in the helper, and `end-kept` as a helper
request; or (b) state that recovery is direct mode's only, that `--privsep` devices are added
unrecoverable (S1's fallback, shown in status), that `make node-restart` and the lab check need
`PRIVSEP=0`, and that #74's follow-up owns the rest. Tests for whichever is chosen.

### R1-C2 — High
**The agent's loop takes the plugin's lock, which a stage or unstage holds while it waits on the
kept device's own I/O.** Design, Agent: the loop runs "sharing #15's per-volume operation
serialization". #15 has none: "the plugin's one node lock stays" (#15 design, S9, Locking), and
that is `n.mu` (`control/csi/node.go:28`), held across `mkfs`, `mount` and `umount`
(`node.go:89-126`, `:135-139`, `:167-202`, `:210-214`). A `NodeUnstageVolume`, `NodeUnpublishVolume` or
`NodeStageVolume` running when the data plane restarts blocks in the kernel on the paused
device; the loop waits for the lock; the device is resumed by nobody; at 60 s the successor ends
it and the pod gets I/O errors. With one node lock this stops the resume of every kept volume on
the node; with a per-volume lock, of that volume. A pod terminating during a node's upgrade is
the ordinary case.

Fix: the resume must not wait on any lock held across a mount, format or unmount. State what
the loop holds (nothing across the two calls, or a lock no I/O-bearing step holds), and let the
data plane's registry settle a resume against a detach (generation, entry lock). Add a Go test:
an unmount blocked on a kept device, and the loop still resumes it; and the same with a second
volume.

### R1-C3 — High
**A resumed device has a new lifetime tuple, and nothing says what #15's contract and #79 do
with it.** S6: after a resume the status gives "this run's instance and a new generation";
design, Contracts: status keeps "its identity tuple". #15's tuple is
`(volume, first_epoch, boot_uuid, instance, generation, rdev, diskseq)`
(`device-lifetime-contract.md`), and #79 unstages with `detach-device` and "the record's entire
tuple"; a tuple that differs is `stale attachment`, "untouched" (#79 design lines 131-134). So
after any resume, the stage's ownership record names a lifetime that no longer exists and the
volume cannot be unstaged. Also unspecified:
- what `device-info VOLUME` and `detach-device` answer for a Kept and a Resuming entry (the
  contract's states are `starting`, `running|stopping`, parsed with exact arity);
- S8 ends a kept device by "`detach VOLUME` ... without a token", which #79 forbids from the
  plugin ("name-only detach is not allowed", #79 S7);
- the successor's held device fd (#15 S2: held from ready until the detach): a Kept entry has
  none, and the design never reopens it;
- #15's raw-block rule reopens the plugin's fds only where "instance and generation still
  match" (#15 design, S9).

Fix: decide and state the tuple of a resumed device and how a stage made before the restart
follows it (for example: `device-info` reports the kept entry with its recorded lifetime and a
resume rebinds it through a named contract step that #79's record can verify by rdev, diskseq,
boot and FirstEpoch). Give the lines for Kept and Resuming, the conditional detach of a Kept
entry, and when the successor reopens the device. Tests: unstage after a resume; `detach-device`
with the pre-restart tuple.

### R1-C4 — Medium
**#83's fresh-process handoff is not followed.** #83 (clear) requires every target start to
carry a handoff bound to the daemon's challenge: the agent reads `attach-challenge`, sends
`AttachRequest.target_nonce` and passes `HANDOFF`; "Legacy attach VOLUME TOKEN, startup
--attach/--token, automatic restart and sync/fallback paths must refuse missing handoff"
(`docs/issues/83/fresh-process-contract.md`). The design's loop is "call
Attach(only_if_target=true) as agent, then socket attach", and Contracts says "No metadata RPC
... changes". Once #83 lands, every resume is refused. The design's one #83 line ("No automatic
recovery after a #83 clock quarantine is implied") does not say how the loop tells such a
restart from any other.

Fix: state that the loop, and S10's lab command on a unit node, use #83's sequence where it has
landed (whichever lands second), and that a daemon restarted after a quarantine is resumed or
not, with the mechanism.

### R1-C5 — Medium
**The spec was not updated with the three settled clarifications, and contradicts the design.**
- S5: "A recorded device that cannot be resumed is ended: ... differing from its record". Design
  and decisions: it is quarantined, never deleted. Its requests then wait with no limit (intent,
  fact 8), against intent, want 5 ("nothing waits on it for good") and S8.
- S8 promises the bound; design: none while neither enforcer runs, and "S8's bound covers ending
  service/waiting I/O, not guaranteed disappearance".
- The design still says these "require explicit spec clarification" (Root records; Lab,
  documentation, risks), and the ADR's Consequences that they "remain open; this draft accepts
  neither policy nor risk".

Fix: write the three into S5, S8 and S11, say what a quarantined device's I/O does and how the
operator ends it (the command), and remove the "needs clarification" sentences. A test for the
quarantine: no delete, listed in status.

### R1-C6 — Medium
**The start's classification ignores the kernel's device state, and libublk's `EBUSY` wait is
outside #6's limits.** Design, Root records: "matching recoverable/Running device becomes Kept".
- A Running record with a `UBLK_S_DEV_DEAD` device exists: Running is persisted "before first
  successful serving", so a crash before `START_DEV` returns leaves one; S5 says "dead" is
  ended. Recovery there answers `EBUSY` (6.1.112 `ublk_drv.c:1933-1934`: state must be
  QUIESCED).
- A device still `LIVE` is a predecessor not yet reaped: on 6.1.112 quiescing waits for the
  monitor work, every 5 s (`ublk_drv.c:137`, `:1015-1044`), unless a new request arrives. 6.6
  must be read; the design verified only its UAPI.
- `UblkCtrl::start_user_recover` retries `EBUSY` every 100 ms for 30 s (`ctrl.rs:2606-2624`) and
  cannot be cut. The design states this as a fact and gives no approach. A resume then needs
  30 s where an attach has `ADD_DEVICE_LIMIT`, 10 s, inside a 50 s limit
  (`control.rs:33-35`), so "runs under #6's existing limits" does not hold.

Fix: classify by `GET_DEV_INFO`'s state: QUIESCED kept; DEAD ended; LIVE waited for, with a
bound, then left kept. Bound the `EBUSY` wait by the attach's deadline: a hunk in the vendored
libublk (#74, #15 and #6 already carry one), or the design's own loop. Tests for each state and
for the deadline.

### R1-C7 — Medium
**No step-by-step crash argument for the resume itself.** The crash tests cover the record
("O_EXCL, ADD return, JSON write, rename, Running, heartbeat, Stopping and DEL reply loss"),
none between `START_USER_RECOVERY`, the queues' fetches, `END_USER_RECOVERY` and the durable
Running update. On 6.1.112 a successor that dies after `START_USER_RECOVERY` and before a queue
is ready leaves `nr_queues_ready == 0`, and every later `START_USER_RECOVERY` answers `EINVAL`
(`ublk_drv.c:1918-1919`, `:1943`): the device can no longer be recovered, against S6 ("A resume
that fails leaves the device kept, for the next try"). `END_USER_RECOVERY` unquiesces the queue
at once (`:1979-1983`), so "durable Running update" must precede it; with `run_target` that is
before `start_dev`, not in #15's ready callback, which runs after.

Fix: a table of the resume's steps: what is durable and what the kernel holds after each, and
what the next successor does. Read 6.6 and 7.0 for the `nr_queues_ready` rule; if it holds, S6
says that such a device is ended. Name the crash tests and the real-kernel test (a successor
killed between the two commands).

### R1-C8 — Medium
**The resume's epoch rule is a second copy of #6's and #15's, outside `flaminio-repl`, and its
injected bug duplicates theirs.** Design: the resume checks "epoch>last_served_epoch", a field
of a `/run` record, in `flaminio-io`. The rule "no second target at an epoch" is `RunEpochs`
(`flaminio-repl/src/run_epochs.rs`, #6 design, S6), made durable by #15's run mark, with the
simulator check `two targets at epoch E` and the bug `run_epochs::Bugs::restart_at_epoch`, which
#15 extends across crashes. S6 cites #15's S11 but names "the record's" epoch. The design's
mutation `resume_keeps_epoch` is the same bug, or a bug in a copy the simulator does not run
(#6's R2-C1). AGENTS.md: recovery logic lives in `flaminio-repl`.

Fix: the resume's admission is the run mark's `admit`; `last_served_epoch` is information only.
The simulator scenario (a restart, unanswered operations issued again) runs with
`restart_at_epoch`. If #48 adds a rule of its own (a request issued again is served only once
its chunk is reconciled), name its check and its own bug.

### R1-C9 — Medium
**The contracts are named, not written.** Design, Contracts: status and list gain "kept/resuming,
recoverable, last_epoch and expiry fields"; the record becomes "v2"; a helper `flaminio-io
end-kept VOLUME RECORD_NONCE`. Missing:
- the grammar of each changed line against #15's (`status VOLUME -> ok VOLUME STATE [/dev/ublkbN
  MAJ:MIN INSTANCE GEN]`, `list -> ok limit N [VOLUME STATE ...]`), the fit in #74's 4096-byte
  cap with 16 entries, and what a #15 plugin does with a state it does not know;
- the attach's error lines for a kept volume (each S6 refusal);
- `end-kept`'s output and exit status;
- the record's v2 grammar. #15's record has no version field (#15 design, S10: "a fixed
  grammar", decimal fields). What a #48 node does with a #15 record, and a #15 node (a
  rollback) with a v2 record: #15 logs and leaves a record it cannot parse, and deletes only
  devices without `UBLK_F_USER_RECOVERY`, so a kept device then waits for good;
- which thread writes the once-a-second "alive" update and takes the record lock: not a queue
  thread nor #15's engine thread (AGENTS.md: the I/O path takes no locks).
The design's last section says the Go loop and the Rust side "can proceed against
record/status/helper contracts".

Fix: write them, as #15's S8 block and its contract file do, with the compatibility both ways.

### R1-C10 — Medium
**S9 and S10 have no approach, the tests have no names, and the lab plan is not runnable.**
- `make node-restart` (S10): no function, script, variables (how SIGKILL is asked for), or how a
  unit node gets its new epoch "as the node's agent" (the lab attaches with the admin identity:
  `metactl ... attach`, `scripts/lab.sh:624-629`). `node.sh up` cannot be the restart: it runs
  `down` first (`scripts/guest/node.sh:44`, when the unit is active; always, under #74 and #12), and `down` dies on a
  mount and deletes the device (`node.sh:23-30`); S11 keeps `down` as it is.
- S9's log: where each of the six times is taken; the first completion is on the I/O path.
- How the pause is measured (the spec's "last completion before the exit to the first after").
- Tests: the table gives descriptions. No test has a file and a name but the mutation, and two
  rules have none at all: "an attach of another volume never takes a kept device" (S6) and
  "its volume is detached" (S8), also for a detach during a resume.
- Lab: prose ("Later explicit authorization: ..."), with no commands, pass criteria or restore
  steps. The clear plans give all three (#74, #15, #12).

Fix: a section for S9 and S10; a test per rule with file and case name; the lab as numbered
commands with the expected output and the restore.

### R1-C11 — Medium
**The record names its volume by name only.** A volume deleted and made again under its name
(#30) is another volume: the token carries `first_epoch` (`flaminio-net/src/token.rs:23`), and
#15's tuple has FirstEpoch for this. The v2 record has no FirstEpoch (design, Root records), so
an attach of the new volume resumes the old volume's kept device, with its mounts and its size,
against the invariant "A device serves one volume for its whole life" and want 2.

Fix: record FirstEpoch and the size; a resume requires the token's `first_epoch` and the
volume's size to equal them, else the device is ended. A test.

### R1-C12 — Low
**"#12 plans a single data-plane process lock" (design, facts; Kernel capability: "#12's process
lock") is not in #12.** #12 has the pod-node guard and `hostPort` 7411 (#12 decisions line 69).
The lock is #15's `flock` on `/run/flaminio/ublk`, direct mode only, held for the node's life,
which `flaminio-io cleanup` also takes (#15 design, S10, One owner at a time). The design adds
"separate stable lockfiles" and a "record lock" without saying how they relate to it: a helper
that runs only when no node runs needs only the directory's lock.

Fix: name #15's lock; state the one locking scheme, and what `end-kept` does when a successor
starts under it.

### R1-C13 — Low
**libublk's recovery does not need its JSON.** Intent, fact 7: a recovery "reloads the device's
JSON from `/run/ublksrvd`, which must outlive the process". The reload's failure is printed and
ignored (`ctrl.rs:1170-1174`), the parameters are not sent again for a QUIESCED device
(`ctrl.rs:2007-2011`), and only a caller's `get_target_from_json` reads it. Flaminio's own
record holds what it needs. So the hostPath for `/run/ublksrvd`, the validation of the JSON and
"Libublk JSON must be durable before advertised recovery readiness" are not the simplest change,
and add a host directory that libublk creates 0777 and writes through `File::create`
(`ctrl.rs:549-558`).

Fix: keep #12's `emptyDir`, and correct the fact; or say what needs the file.

### R1-C14 — Low
**S4 is unconditional, S1 has a fallback.** With `recoverable=false`, "exits at once, attached or
not", the removed preStop wait and a 10 s grace fail the volume's I/O on every planned deletion,
which #12's S4 prevented. The design's "retain existing nonrecoverable behavior" does not say
whether that includes #12's wait. Both flags date from 6.1 (headers 6.1, 6.5), so the fallback
is off ADR 0003's baseline; it still ships.

Fix: say which SIGTERM rule a node with unrecoverable devices follows (#12's), with its test, or
that the fallback gives up #12's S4.

### R1-C15 — Low
**The retry of a failed resume has no pace.** The loop runs every 100 ms and "New epoch on each
retry is permitted": a resume that keeps failing takes up to 600 epochs per volume in 60 s, each
a write of the volume's header and a new fence on every member.

Fix: a backoff per volume, stated and tested (S7's "tries again").

## Nits
- **N1.** Spec Q2 and intent: "60 s, as an attach's limit". The attach's limit is 50 s
  (`control.rs:33-35`, `:295`); 60 s is the plugin's `DefaultControlTimeout`
  (`control/csi/dataplane.go:41`).
- **N2.** `adr-proposal.md`: "Propose ...", "Proposed bound", "this draft accepts neither policy
  nor risk" beside "Accepted with the plan". Reword the body as the decision taken.
- **N3.** S7: "beyond the waits #15's S5 names". The wait that applies is #15's S1 (attaches
  taken one at a time on the main thread, up to `METADATA_LIMIT` each).

Reviewed commit: f7db82449509baca36ec3565ab96c97981a64250
