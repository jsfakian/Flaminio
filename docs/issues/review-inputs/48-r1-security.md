# Plan review: issue #48 (Live data-plane upgrade), round 1, focus design-security

Scope: `docs/issues/48/` (intent, spec, design, adr-proposal, decisions) in
`/home/jsfakian/Documents/src/flaminio-48`, against the code of that checkout, libublk 0.4.8
(`~/.cargo/registry/src/*/libublk-0.4.8`), and the clear plans #6, #12, #15, #74, #79, #83.
Paths below are relative to the checkout unless they start with `#<n>` (that plan's folder on
`origin/main`, or the main checkout for #6).

Counts: High 2, Medium 6, Low 4, Nit 1.

### R1-S1 — High

**A kept device is resumed after another target served the volume, or for another volume of the
same name.** The resume checks only that the node is the target now and that the token's epoch is
above the record's (spec S6, S8; design, "Resume state machine": "epoch>last_served_epoch and
durable local fence, and current metadata target/epoch"). Neither proves that this node stayed the
target since the device last served.
- **Moved away and back.** `Attach` with `only_if_target` compares only the current target
  (`meta/service/service.go:714`), and `Header.Attach` just adds one to the epoch
  (`meta/domain/domain.go:217-220`). Node A at epoch 5 exits; the volume is attached on B (6), B
  acknowledges writes; it is attached on A again (7) inside the 60 s. The agent's
  `only_if_target` call (8), or the administrator's own token at 7, resumes A's kept device: the
  kernel reissues the old mount's held writes over B's acknowledged data, and the old mount's cache
  is stale. Today the device dies with the process, so this cannot happen. The lab's own check 3
  (spec, Tests, Lab 3) moves the volume to fl-b while fl-a is down; moving it back before fl-a
  returns is the same commands.
- **Made again under its name.** The record (design, "Root records": volume, boot_id, nonce,
  instance/generation, phase, dev_info, epochs) holds no `FirstEpoch`, though the token carries it
  (`pkg/captoken/captoken.go:49-51`, `meta/service/service.go:743-745`) and #15's lifetime tuple has it. A volume
  deleted and created again with this node as target inside the bound resumes the old volume's
  device: the old consumer reads and writes the new volume. This breaks the spec's own invariant
  "A device serves one volume for its whole life".

Fix: put `first_epoch` in the record and refuse a resume whose token differs. Add a continuity
rule the data plane itself enforces (the administrator's attach resumes too): for example, resume
only at exactly the last epoch this node's daemon durably saw for the volume plus one, the record
updated before each try; any gap means an attach it did not see, and the device is ended. State
what a lost `Attach` reply then costs. It is a new safety rule: a simulator check (a target
elsewhere between exit and resume; a volume made again) and an injected bug, an engine test, and a
lab case (move to fl-b and back inside the bound: fl-a's device is ended).

### R1-S2 — High

**#74's hand-offs are not taken; under `--privsep` nothing of the design can work, and
`end-kept` is the root path #74 rules out.** The batch ledger queues two items for #48
(`docs/issues/batch-2026-11-12.md:226`, `:250`): recovery commands go through #74's helper, which
must outlive an I/O restart, and the helper's pin is closed before START_USER_RECOVERY and taken
again after. The plan names neither #74, the helper, the pin nor `--privsep`; intent's Out of scope
still says "Privilege separation (§5.1)".
- #74 intent, "Which step": "a second root path to it (such as #48's draft `flaminio-io end-kept`)
  would be outside this boundary". Design, "Agent, deadline and races", keeps exactly that helper.
- Under `--privsep` the helper fixes the add flags to 0 (#74 spec S3; design, Requests, `add`), so
  S1's flags cannot be asked for; the helper exits with its child and deletes its owned device (#74
  S5, outcome 4), and a unit stop signals both (outcome 5), so S2 ("however the process ends ...
  every running device is kept") is false there; the I/O process cannot open
  `/run/flaminio/ublk` (#15 design, S10 "One owner at a time", and "With #74's helper"), so S3's
  record, its once-a-second stamp and the flock do not exist; it holds no `/dev/ublk-control`, so
  the design's `UblkCtrlBuilder RECOVER_DEV`, `start_user_recover` and DEL cannot run in it.
- #74's `node.sh up` "runs `down` first, always", and `down` deletes `/dev/ublkc$id` (#74 design,
  lab node): the unit path of S10 would delete the kept device unless the design says how
  `node-restart` avoids it.
- #74 makes `PRIVSEP=1` the lab node's default (#74 S7), so the spec's "under 2 s on the unit" is
  measured only with `PRIVSEP=0`, which the plan does not say.

Fix: decide and write it. Either (a) recovery is direct mode's until a later step of #74: under
`--privsep` status says `recoverable=false`, S2 and S4 keep #12's and #74's behaviour there, the
lab check runs with `PRIVSEP=0`, `end-kept` exists only in direct mode, and the hand-off to #74
(helper requests `recover` and `end`, a helper that outlives its child, pin closed before
START_USER_RECOVERY and retaken, records written by the helper) is recorded in the ledger; or (b)
design those helper requests here, with #74's bounds and tests. In both, drop a root
`flaminio-io end-kept` beside a running helper, and state `node-restart`'s script path.

### R1-S3 — Medium

**The recovery loop omits #83's fresh-process handshake.** #83's contract (clear) requires every
target start to carry a handoff bound to the daemon's process challenge: `attach-challenge` on
the socket, `AttachRequest.target_nonce = 6`, the `FLCH` envelope as the HANDOFF operand; "Legacy
attach VOLUME TOKEN, startup --attach/--token, automatic restart and sync/fallback paths must
refuse missing handoff", and "CSI `Node.device`, metactl/admin scripts and every startup target
path use this sequence" (#83 `fresh-process-contract.md`). Design, "Agent, deadline and races",
has "call Attach(only_if_target=true) as agent, then socket attach", and S10's unit command is
unspecified. With #83 in, every resume is refused and every kept device ends at the bound. #79's
strict plugin likewise allows no name-only attach or detach (#79 spec S1, S7), while design,
"Contracts", ends a kept device by `detach VOLUME`.

Fix: state that the loop, the lab command and the engine test use #83's sequence (challenge, nonce,
handoff) and #79's operation forms where those plans are in, whichever lands second; add the
refusal of a resume without a handoff to the S6 tests.

### R1-S4 — Medium

**S4 exits at once even when the attached devices cannot be kept.** Spec S4: "On SIGTERM the data
plane exits at once, attached or not"; design removes #12's preStop wait and cuts the grace to
10 s. S1 and Q5 (a) keep nonrecoverable devices on a kernel without both flags, and the design
adds two more such cases: a zero-copy combination the kernel refuses ("test the real returned
features/flags"), and `--privsep` (R1-S2). There a SIGTERM or a pod deletion now fails every
attached volume's I/O at once, where #12's S4 waited for the detach; the plan replaces that rule
without the recovery that justified it.

Fix: S4 applies only while every entry is recoverable; otherwise #12's S4 (SIGTERM remembered,
the preStop wait, the long grace) stays. Add the unit and pod-fixture cases.

### R1-S5 — Medium

**The bound does not end a crash loop that reaches Running.** Q4 and decisions.md: "the bound
ends a crash loop". Design, "Root records": the alive stamp is refreshed "for actual running
ownership", and only "Kept/retrying/crash-loop state cannot reset its deadline". A successor that
resumes, then dies, restarts the 60 s each time. With `UBLK_F_USER_RECOVERY_REISSUE` the kernel
hands the same requests to every successor, so a request that makes the data plane panic ("A
panic ends the node", #15 S5) is replayed without end: one tenant's I/O keeps the node's data
plane, every other volume on it and its replicas' ISR membership cycling. Today that request dies
with the device.

Fix: keep in the record a resume count or first-kept time that a resume does not clear, and end a
device after N resumes inside a window (or one whose server died within T of its resume); reset it
only after the reissued requests completed and it served for T. Test it (engine and record tests).

### R1-S6 — Medium

**Who ends a kept device when no data plane serves is not designed, and a live but stuck
successor is never overruled.** Design, "Agent, deadline and races" and "Contracts".
- `flaminio-io end-kept VOLUME RECORD_NONCE` is run by "the agent", the CSI node plugin. Its
  image holds only the Go binary (`scripts/dev/csi-image.sh:11-14`); the design names no image
  change, no rule for a helper binary older or newer than the record's writer, and not how the Go
  plugin learns the volume and nonce with no socket to ask (a second parser of root's records in
  Go, or a helper `list`).
- Locks: "the same lock", "record lock", "active-owner lock" and "separate stable lockfiles" are
  not related to #15's one flock on the records directory, which the node holds for its life and
  which makes a second taker refuse (#15 design, S10, R2-B6). A sweeper that takes it makes a
  starting data plane refuse to start; one that does not take it races the start-up scan.
- A successor that holds the lock but never enforces (store recovery that hangs before the scan;
  the scan's place relative to the store's open is not stated; a deadlock later) blocks the helper
  ("never kills a running/resuming different generation"): the device waits for good, which S8 and
  intent 5 exclude. The settled dependency covers "neither runs", not this.

Fix: specify the ender: the binary and its image, the mounts, the nonce's grammar, the exit codes,
the lock protocol against #15's flock, and how the plugin finds kept records. Put the scan and a
deadline timer before the store's open, and say what the agent does when the owner lock is held,
the socket does not answer and the bound has passed. Tests for each.

### R1-S7 — Medium

**The resumed device's lifetime tuple changes under #15's contract and #79's ownership.** Spec
S6: the status gives "this run's instance and a new generation" for the same `major:minor`. #15's
contract (revision 5, pinned by #79) defines a device's lifetime as
`(volume, first_epoch, boot_uuid, instance, generation, rdev, diskseq)`; `detach-device` answers
`stale attachment` on any differing field, #79 detaches only by that complete tuple (#79 spec S7)
and never adopts a live mount from an obsolete record (S2), and #15's plugin reopens raw-block fds
only "for the publishes whose instance and generation still match" (#15 design, S9). So after a
resume the staged volume's unstage, publish and conditional detach are refused, and S8's "its
volume is detached (`NodeUnstageVolume`...)" cannot happen under #79. `device-info` has no answer
for `kept` or `resuming` either, and its parser takes no unknown state.

Fix: define the tuple across a resume (which fields persist, and how a plugin holding the old
tuple proves the device is the same: boot, rdev, diskseq, first_epoch, the record's nonce), the
`device-info` and `detach-device` answers for kept and resuming entries, and record the needed
revision of #15's contract and #79's rebinding as ledger hand-offs. Go tests with a staged volume
across a resume.

### R1-S8 — Medium

**The new inputs have no grammar or bound.** Design, "Contracts and files", lists field names
only, yet its last section lets the Go loop and the Rust recovery "proceed against
record/status/helper contracts".
- Record v2: #15's record is a fixed decimal grammar of at most 4096 bytes with names
  `^(0|[1-9][0-9]{0,6})$` (#15 design, S10). v2 adds boot_id, nonce, phase, two epochs and a
  timestamp with no order, encoding or size, and no rule for a v1 record met by a v2 node (it must
  keep #15's rule: dead, matching, deleted) or a v2 record met by a pre-#48 node (#15's start
  waits 5 s on a device that is "not yet dead" and keeps it: a rollback leaves kept devices
  waiting with no bound).
- Socket: `list` is `ok limit N [VOLUME STATE ...]` in pairs, sized so that 16 entries fit #74's
  4096-byte cap (#15 design, S8; #74 S6). The added "recoverable, last_epoch and expiry" fields
  have no position or width, so neither the cap nor #15's plugin's parse can be checked.
- `end-kept`'s NONCE and the lab command's arguments reach command lines with no stated grammar.

Fix: write the three grammars as #15 does (fields, order, widths, the longest answer against 4096
bytes), the v1 and rollback rules (S11 and docs: detach before a downgrade), and parser tests with
over-long, unknown-state and wrong-arity input on both sides.

### R1-S9 — Low

**The loop can mint epochs without limit.** Design: a 100 ms loop, "Availability errors retry",
"New epoch on each retry is permitted". Every `Attach` is an etcd write. A socket attach that
fails fast (the limit, a resume refusal, a store error) gives 10 epochs a second for each kept
volume, up to 16, for 60 s, driven by answers of a process #74 treats as possibly compromised.
Fix: one `Attach` per volume per back-off step (state the steps), none while an earlier token is
unused, and a test that counts the calls.

### R1-S10 — Low

**The resume's epoch checks do not name #15's run mark, and the injected bug's target is
unclear.** Design lists "epoch>last_served_epoch and durable local fence"; the bar that outlives
the process and that the simulator checks is the run mark through `RunEpochs::admit`, durable
before the first `Hello` (#15 S11; #6 design, S6). `last_served_epoch` lives in `/run` and is
written later, so it is the weaker bar. If the resume uses `RunEpochs`, `resume_keeps_epoch` is
#6's `restart_at_epoch` under another name; if it uses its own comparison in `flaminio-io`, the
rule lives outside `flaminio-repl` and the mutation turns off a copy. Fix: say that a resume passes
#15's token check and `MarkEpoch` unchanged, what `last_served_epoch` adds, and which shipped
comparison the mutation disables.

### R1-S11 — Low

**The host-mounted `/run/ublksrvd` is not needed and adds a root-trusted input.** Intent, fact 7:
a recovery reloads the JSON, "which must outlive the process". libublk only logs a failed reload
and goes on (`ctrl.rs:1170-1173`, `:1185-1188`); the device's facts come from GET_DEV_INFO. The
design therefore replaces #12's `emptyDir` with a hostPath, requires the JSON "durable before
advertised recovery readiness", and must then defend a directory libublk creates 0777
(`ctrl.rs:547-552`) and a file it writes through `File::create`, which follows links (`:555`).
Every lab node that ran a unit already has that directory at 0777; whether the node refuses to
start or repairs it is not said. Fix: keep the `emptyDir`, take every parameter from the record
and the kernel; or state the upgrade rule for an existing 0777 directory and test it.

### R1-S12 — Low

**"A resume ends within #6's limits" does not hold with libublk's wrapper.** Spec S8, design: the
bound never cuts a resume, which #6 bounds. `UblkCtrl::start_user_recover` retries EBUSY for 30 s
with no way to stop it (`ctrl.rs:2606-2624`; the inner call is private), against #6's 10 s device
step and 50 s attach (`flaminio-io/src/ublk.rs:697-703`). The design cites the 30 s but not how it
is cut. Fix: name the mechanism (the vendored copy #74 already plans, or a thread treated as #6's
late start) and the test.

## Nits

- design.md:12-14, :32: "#12 plans a single data-plane process lock" and "#12's process lock".
  #12 plans none; the one-owner lock is #15's flock on the records directory (#15 design, S10,
  R2-B6), in direct mode only, beside the disk's `O_EXCL`.

Reviewed commit: f7db82449509baca36ec3565ab96c97981a64250
