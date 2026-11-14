# Issue #84: draft review choices

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Flag bits the frontend does not know** ([spec](spec.md), Open questions;
  [design](design.md), One frontend contract, rule 2, and Risks, Unknown flag bits fail closed):
  the plan's choice. They are refused with `EOPNOTSUPP`, accepting that a kernel which sets a
  new hint bit fails that I/O until the table grows.
- **Evidence on the minimum kernel and a physical backing stack** ([spec](spec.md), Open
  questions; [design](design.md), Risks, Matrix): no recommendation in the plan. Settled
  2026-11-13 (user's instruction): required before #84 closes, gathered by the same kernel check
  (see below). The plan measures one cell (the lab's kernel, pinned libublk, both
  buffer modes, two queues, virtual disks).
- **When and where the kernel check runs once #83 lands** ([design](design.md), Risks, #83;
  Choices below, Review84-R1-C3): no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): on the lab VMs before #83 lands, then only on a host #83 admits (see below).
  #83's profile
  refuses the node on the lab VMs, so the check must run before #83 lands or on a host #83 admits.
- **Not settled here:** ADR 0010, which records this contract, stays Proposed ([spec](spec.md),
  Open questions; [design](design.md), ADRs); accepting it is a separate ADR step.

## Settled at the user's instruction of 2026-11-13

- **Evidence on the minimum kernel and a physical backing stack:** required before #84 closes.
  The kernel check ([kernel-check.md](kernel-check.md), both runs) also passes on ADR 0003's
  minimum kernel (6.6) and on a physical backing stack (one host may give both), each recorded as
  another cell of the same matrix; until then the published contract claims only the measured
  cells, and the issue stays open as the intent says. Recommended by Claude, no recommendation in
  the plan: ADR 0003 supports 6.6 and #83 admits only physical hosts, so the one virtual cell
  covers no supported production deployment, the plan forbids claiming unmeasured cells or
  fabricating an accepted risk, and the check already records `kernel` and `backing`, so no new
  mechanism is needed.
- **When and where the kernel check runs once #83 lands:** before #83 lands, on the lab VMs as
  the design states; once it has landed, only on a host #83's profile admits, with no flag or
  bypass of its admission; where no such host runs a node, the check answers
  `refused`/`nodes-not-alive` and the result waits for one. Recommended by Claude, no
  recommendation in the plan: the plan already forbids bypassing the node's admission, so this
  keeps #83's clock contract whole and uses the existing lab for the first cell.

## Choices

- **Review84-R1-C1:** one pure classifier decides every node request, in a fixed order:
  registration failure (EIO), flag bits (EOPNOTSUPP outside FAILFAST×3, META, FUA, SWAP), op,
  then range (EINVAL). Refusing a flag bit the frontend does not know is a change from today,
  kept as an open question in the design's Risks.
- **Review84-R1-C1:** discard stays unadvertised, as the code is today (`UBLK_PARAM_TYPE_BASIC`
  alone): the kernel refuses discard and write-zeroes itself. The handler arm stays a no-op
  hint for a discard that arrives anyway, so the frontend's behaviour does not change.
  Advertising discard, even as a hint, is left out: it would make filesystems send it.
- **Review84-R1-C2:** FLUSH gets no ordering rule, drain or sequence counter. On the cacheless
  device the kernel consumes FLUSH and FUA, so no kernel test can drive them into a handler;
  delivered or not, they are safe by S2 alone. The spec's barrier clause and the design's
  "hypothesis to test" and follow-up drain are cut. The kernel evidence is narrowed to what can
  be observed: the advertised limits, and a request trace showing the writes and no flush or
  FUA at the device. The early-completion bug the simulator must catch is the existing pair
  `ack_before_shrink` and `late_join`: no new safety rule, so no new mutation.
- **Review84-R1-C2:** the trace has two controls: the count of traced writes, and the store's
  own flush seen on the data disk when that disk advertises a write cache.
- **Review84-R1-C3:** the Rust helper is cut. The check is shell (`dd`, `blkdiscard`, sysfs,
  tracefs), its CLI and result schema are fixed in [kernel-check.md](kernel-check.md), and it
  takes no volume, device, size or offset: it creates and guards its own volume.
- **Review84-R1-C3:** the lab check injects no fault. Replica failure, restart under load,
  device failure and power loss stay with the engine, store and chaos tests.
- **Review84-R1-C3:** the check passes the node no flag of its own and never bypasses its
  admission. When and where it runs once #83 lands: Settled 2026-11-13 (user's instruction):
  on the lab VMs before #83 lands, then only on a host #83 admits (see Settled at the user's
  instruction of 2026-11-13). What
  it answers where the node does not run changed in round 2 (R2-S7, below).
- **Review84-R2-S1, R2-S4:** the check restarts no node. One run measures the mode and queue
  count the node already runs with, read from its process, and creates one volume. Both modes
  need two runs, with the operator's existing `make node-up` between them, pinned to
  `FORMAT=0` in the Lab section. This removes the three restarts, the restore restart and
  every node setting the check passed, instead of guarding each. A volume found attached is
  never torn down: it fails the run as `guard`, and is named.
- **Review84-R2-S2:** the guest script takes `RUN` alone, validated, and reads the device from
  the node's `status` answer, which must name the run's volume. `VOLUME`, `DEV`, `SIZE` and
  `MODE` are cut. The host learns what is attached through a guest verb that prints only a
  valid id, `none` or `invalid`.
- **Review84-R2-S3:** a volume whose create answered `AlreadyExists` is never the run's: a leak
  is preferred to a deletion. How that is kept changed in round 3: the check deletes nothing.
- **Review84-R2-S5:** a later run refuses (`leftover`) while an earlier one's remains exist,
  and the guest bounds each step with `timeout` and cleans up in its own trap. The record
  file, the host cleanup and the guest `cleanup` verb of round 2 are cut in round 3.
- **Review84-R2-S6:** the classifier's range rule uses `checked_add`, as the engine does.
- **Review84-R2-S7:** a node that is not running before the check is
  `refused`/`nodes-not-alive`: the check does not start it. `unsupported`/`node-refused` is
  kept for a running node whose attach fails with nothing else attached.
- **Review84-R2-N1:** step 6 (`blkdiscard`) has the identity guards and no zero guard.
- **Review84-R3-S1, R3-S2, R3-S3, R3-C1, R3-C2:** the check has no cleanup. It creates,
  attaches and writes; it deletes and detaches nothing, starts no node, signals no process and
  keeps no record. Every run ends by printing `VOL=<run> make vol-delete`, the operator's
  existing command. This cuts the guest `cleanup` verb and its process-group kill, the host's
  `delete_volume` call, the record file and its clearing. A run costs the operator one command
  more; nothing the check does can delete, kill or start the wrong thing.
- **Review84-R3-C1, R3-C4, R3-N1:** leftovers are found where they are, not in a local record:
  an id with the check's pattern in `metactl volumes` or `metactl tombstones`, or the guest's
  fixed directory or trace instance on NODE. Each refuses the next run. Only an id matching
  the pattern is ever printed in a deletion command.
- **Review84-R3-S1, R3-S2:** in the guest, every command that opens the device is a foreground
  child under `timeout -k 10 60`; step 5's two writers run one after the other, so there is no
  background job. (Round 4 dropped the 370 s bound first written here.)
- **Review84-R3-C3:** the precheck waits up to 60 s for both nodes to be alive.
- **Review84-R3-S4:** after a failed attach only `none` is `unsupported`; every other answer is
  `fail`/`guard`, and the guest `run` does not start.
- **Review84-R3-S5:** every write to the device has `conv=nocreat`.
- **Review84-R4-S1:** the guest run as a whole has no stated bound, and the operator is told no
  number of seconds. What is bounded is each device command (`sudo timeout -k 10 60`) and each
  `status` call (the socket's 180 s). After an interrupted run the operator waits until the
  guest's directory is gone; a directory that stays is a `leftover`, cleared by hand only once
  two stated commands show no live guest process. No mechanism is added.
- **Review84-R4-C1:** both scripts take the time from `date +%s` and wait with `sleep`, which
  the script tests stub.
- **Review84-R4-C2, R4-S N1, R4-S N2:** wording only: "no state that any run reads"; the live
  guest is found by its environment and its children, not by a script name; the trap is set
  only after the `mkdir` succeeded, so a second run removes nothing.
