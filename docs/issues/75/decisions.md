# Issue #75 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status:
needs_incremental_review).

- **Changes made after approval:** the plan review's round-3 fixes (2026-11-13), listed under
  Round 3 below.
- **Changes made after approval:** the plan review's round-4 fixes (2026-11-13), listed under
  Round 4 below.
- **Changes made after approval:** the plan review's round-5 fixes (2026-11-13), listed under
  Round 5 below.
- **Changes made after approval:** the plan review's round-6 fixes (2026-11-13), listed under
  Round 6 below.
- **Changes made after approval:** the plan review's round-7 fixes (2026-11-13), listed under
  Round 7 below.
- **Changes made after approval:** the plan review's round-8 fixes (2026-11-13), listed under
  Round 8 below.

## Settled at approval

- **Q1, the limits' semantics** (`spec.md`, Open questions; `design.md`, Risks, Open;
  `adr-proposal.md`, Consequences and open choices): as proposed. The limits count client READ
  and WRITE requests and bytes, not 4 KiB units; reads and writes share them; each target starts
  with empty buckets; the policy is fixed at create.
- **The ADR's decision** (`adr-proposal.md`, Decision proposed for review; `design.md`, ADRs):
  accepted as proposed, with the plan.
- **The kernel's timeout re-arm** (`design.md`, Risks, Open; `adr-proposal.md`, Consequences and
  open choices): as the plan states. It is assumed for kernels from 6.6 and confirmed per kernel
  by `make qos-check`; the ADR's Confirmation lists each kernel once a run passes on it.
- **Long waits and the hung-task detector** (`design.md`, Risks, Open; `adr-proposal.md`,
  Consequences and open choices): as the plan states. Nodes serving limited volumes must not set
  `kernel.hung_task_panic=1`, which is documented; an attach is not refused for it.
  Changed after approval (round 3, R3-S8): widened to every node that may attach a limited
  volume. Settled 2026-11-13 (user's instruction): the widening is accepted (see below).
- **A fast read refused at finish is charged twice** (`design.md`, Risks, Open; `spec.md`, S2):
  as the plan states.
- **Measuring the default path's cost** (`design.md`, Risks, Open): no recommendation in the
  plan. Settled 2026-11-13 (user's instruction): argued in this step, measured by the issue's
  benchmark steps (see below).

## Settled at the user's instruction of 2026-11-13

- **The `hung_task_panic` requirement widened after approval (round 3, R3-S8):** no node that may
  attach a limited volume (any node its pod may run on) sets `kernel.hung_task_panic=1`, as the
  plan now states. A change after approval, accepted under this instruction.
- **Measuring the default path's cost:** this step ships on the argued cost (design, "Default
  path": one `Option` test per request, one empty-set test per pump), with no measurement
  campaign of its own (R1-C18); the cost is measured by the issue's benchmark steps (intent, "What
  stays open", steps 3-5). Recommended by Claude, no recommendation in the plan: the plan already
  keeps measurements out of this step, the unlimited path adds no lock, allocation or metadata
  call, and a guessed number would not be evidence.

## Settled choices

- **The plan carries the issue's first step only** (R1-C14): per-volume limits set by a
  StorageClass, buildable on today's code. Background scheduling, bounded queues and safety
  priority, measurements, gates and comparisons stay open in the issue ([intent](intent.md)),
  each for its own plan; nothing in this plan guards them.
- **One ordered session per connection; no control and data lanes** (R1-C1, R1-C3, R1-S1,
  R1-S4, the frame nits): the replication protocol and the simulator do not change.
- **QoS acts before the target's state machine, and nowhere else** (R1-C2, R1-S3, R1-C4): held
  requests are not emitted, so `request_timeout` never runs for them. No scheduler, credit
  pools, owned returns or admission caps in this step (R1-C9, R1-C10, R1-S5), so no new
  replication rule needs a simulator check; S4's engine test catches a delay after emission.
- **Limits count client requests and bytes, not 4 KiB units** (raised while fixing R1-C8 and
  R1-C15): with 4 KiB logical blocks (`flaminio-io/src/ublk.rs:187`) a 4 KiB unit count is the
  byte count / 4096, so the draft's two limits were one. FLUSH and DISCARD cost nothing
  (R1-C15). No weight: it served the cut fair scheduler.
- **One charge point: the engine, under the mutex both paths take** (R1-C8): no permits. A fast
  read is charged once admitted; one refused after its reads is charged again on resubmission.
- **Waits hold the ublk tag and never complete with an error** (R1-C6, R1-S6), released by a
  timer at the head's due instant with the 10 ms tick as backstop; the floors (1 MiB/s, a 1 MiB
  burst) bound the head's wait to 1 s.
- **No tenant overrides** (R1-C7, R1-S2): only StorageClass parameters set a policy; PVC
  annotations are not read; nothing depends on #73, whose step provides no tenant QoS ceilings
  and does not plan them.
- **The policy is fixed at create and read with the volume at attach** (R1-C5, R1-S8, R1-C16):
  no `SetVolumeQoS`, no revision, no `first_epoch` binding; a retry of the create still matches.
- **Mixed versions fail closed** (R1-C11): `GetVolumeRequest.enforces_qos`, the CSI check of the
  stored policy, and the older CSI's refusal of unknown parameters. No feature switch.
- **One validity rule, written once in the design's Contracts, tested by the same table in Go
  and Rust; no shared vector file** (R1-C12).
- **Proto tag 40** for the new fields, above the sibling plans' proposals.
- **No gates, release workflow, lab lock or benchmark harness in this step** (R1-C13, R1-C17,
  R1-S7, R1-S9); no measurement campaign (R1-C18). The later step's metric is the foreground
  p99 with each background producer running.
- **The lab check is `make qos-check`** (R1-C12): one limited volume, fixed settings, pass
  criteria and cleanup in the design.

### Round 2

- **One switch guards against older metadata instances** (R2-S1, R2-C2): `flaminio-meta serve
  --qos`, off by default; while off, a limited create is refused. The operator turns it on, on
  every instance, once every metadata instance and data plane runs this version; rolling back
  is then unsupported. As #70's flag; no separate policy record (a separate key would outlive an
  older instance's delete and could be inherited by a volume created again under the name).
- **An older data plane is refused, after the target moved** (R2-C2): stated as the limit, with
  its test; nothing steers scheduling, drain or failover away from such a node.
- **The CSI controller deletes a volume its create just made when the stored policy differs**
  (R2-S2); never on `AlreadyExists`, where the volume may hold data.
- **A cost above a limited burst is charged as the burst** (R2-C1): only a caller other than
  the frontend can send one.
- **`Gate::covers` refills and tests without debiting; `release` returns one request at a time**
  (R2-C3, R2-C4), so a deposition during release leaves the rest in the gate for the deposition
  branch of `drain_one` to fail.
- **Timing tests measure from before `Input::Attach`** (R2-C5): an exact lower bound, no tolerance.
- **The wait bound is `depth × queues × max(1/iops, io_buf/bytes_per_second)` plus blk-mq;
  nodes serving limited volumes must not set `kernel.hung_task_panic=1`** (R2-S3), documented
  rather than refused at attach: the engine does not know depth or queue count.
- **Kernel versions are recorded by `make qos-check`** (R2-C6); the ADR lists none until a run
  passes.
- **Does not apply: r75c2's Nit that decisions.md and intent.md still name #73.** The
  coordinator's note meant references that relied on #73; naming #73 to say its step provides no
  tenant QoS ceilings is the intended text.

### Round 3 (after approval)

- **The switch gates `CreateVolume` only** (R3-S2, r75c3 #3): `GetVolume`'s check and the
  returned policy never depend on it; tested with the switch off on a store holding a limited
  volume.
- **The switch's order is documented, not enforced** (R3-S1): the plan says so, says that an
  older instance erases a limited volume's policy for good whatever the switch, that the only
  safe rollback deletes the limited volumes first, and that operators add `--qos` in a second
  rollout. An older binary exits on the unknown flag, so a binary-only rollback that keeps it
  fails closed. The lab unit writes `--qos` only with `QOS=1` (`make meta-up QOS=1`), and
  `make qos-check` refuses until every instance has it. (Replaced in round 4 by `make
  meta-flags`, and the check in round 5.)
- **metactl compares the returned policy, as the CSI controller does** (R3-S3, r75c3 #4); on a
  mismatch it exits non-zero naming the volume and deletes nothing.
- **The lost-answer leftover is stated, not handled** (R3-S5): a retry that ends in
  `AlreadyExists` never deletes (the volume may hold data); an administrator deletes the
  never-attached volume; tested, with the failed-delete case (r75c3 #2).
- **"Effective burst" names the burst or one second of the rate** (R3-S4); the bound is on what is
  admitted, tested with 0 bursts; the cross-chunk test uses a burst of 2 (r75c3 #1).
- **The `hung_task_panic` requirement covers every node that may attach a limited volume**
  (R3-S8), not only the node serving it; this widens the answer settled at approval.
- **Approval consistency** (R3-S6, r75c3 nit 5): settled questions are marked in place in the
  spec, the design and the ADR proposal; its status says it binds this plan's implementation
  from the approval on.

### Round 4 (after approval)

- **One lab mechanism for optional `flaminio-meta serve` flags** (r75c4 #1, R4-S1, R4-S3 nit 3),
  shared with #70 (`--volume-expansion`): design.md, "Lab flags of the metadata service". Flags
  live in `/etc/flaminio/meta-flags`, which `meta up` never writes, so `cluster-up` and the chaos
  restart keep them; `make meta-flags` validates each value to 0 or 1 in lab.sh, refuses to turn a
  flag on unless every cluster VM runs the build, and rewrites only that file. Whichever of #70
  and #75 lands first adds it. This replaces round 3's `make meta-up QOS=1`. (The build check
  was cut in round 6.)
- **With the switch off, an existing id answers `AlreadyExists` before the refusal, and the
  refusal is `FailedPrecondition`** (R4-S2): a retry after a lost answer matches, and the CSI
  controller's mapping keeps it retryable.
- **The flag's usage, `main.go`'s doc and the Makefile help carry the rollback warning** (R4-S3).
- **The `hung_task_panic` widening is a change after approval** (r75c4 #2), marked as such in
  Settled at approval, the design and the ADR proposal.
- **metactl's policy argument has its own test; `TestCreateQoSNotStored` drops `qos` with an
  interceptor in the fixture** (r75c4 #3).
- **S5 and the intent say operators ensure the version order** (R4 nit 1); **the leftover remedy
  deletes a `pvc-<uid>` volume that has no PV** (R4 nit 2). (Replaced in round 5: no remedy;
  Compatibility states the identifying condition.)

### Round 5 (after approval)

- **The lab checks what the process runs with, not the flags file** (r75c5 #1, R5-S2):
  `meta.sh running FLAG` reads the service's `/proc/<pid>/cmdline`; `make qos-check` requires it
  for `--qos` on every cluster VM, and `meta.sh flags` runs it after its restart and dies on a
  unit that does not read the file. An older binary exits on `--qos`, so all passing proves every
  instance is new enough; a later `meta up` that leaves a mixed cluster is refused by the same
  check. No further guard on `meta up`. (Replaced in round 6 by the probe.)
- **The leftover is stated, with the one condition that identifies it; no remedy** (R5-S1):
  deleting a volume is the administrator's decision.
- **The build check covers the metadata binaries only** (r75c5 #2), said in the design and the
  `meta-flags` help (the build check was cut in round 6); **docs/lab.md gives the lab rollback
  for the flags file** (r75c5 #3); **the script test's limit is stated** (r75c5 #4).
- **`readHeader` named** (R5 nit 1); **round 3's `make meta-up QOS=1` marked replaced** (r75c5
  nit 5).
- **R5-S3 is handled in #70's plan** (its round-4 fix replaces `make meta-up VOLUME_EXPANSION=1`
  with a reference to design.md, "Lab flags of the metadata service"); no change here.

### Round 6 (after approval)

- **One lab check that proves the property directly; no other guard on the flags** (R6-B1, with
  R6-B3 and R6-B4): for each cluster VM, `make qos-check` creates a small limited volume through
  that VM's own metadata instance and deletes it; an instance without `--qos`, or older, fails
  it, naming the VM. This replaces `meta.sh running`, the process inspection, the build check of
  `make meta-flags` and the "dies on a unit that does not read the file" guard. The flags file
  and `make meta-flags` stay as the way to set a flag and prove nothing. R6-B3 (the check after
  `flags`) is resolved by that cut; R6-B4's case, only w failing, is a test of the probe.
- **The rule for every lab check of a `serve` flag** (R6-B2), in design.md, "Lab flags of the
  metadata service": do the flagged operation through each cluster VM's own instance, then undo
  it. #70's `csi_expand` follows it (an expansion through each instance); the orchestrator
  carries the change to #70.
- **The lab rollback turns the switch off first** (R6-B5): `make meta-flags QOS=0`, then delete
  the QoS StorageClasses and the limited volumes, then check out the older tree.
- **Round 4's remedy marked replaced** (R6-B6); **the overridable paths listed as two** (nit 1);
  **only `readHeader`'s `NotFound` becomes `FailedPrecondition`; other errors are returned**
  (nit 2).

### Round 7 (after approval)

- **The probe's trap is exact** (R7-B1): it deletes every id the run passed to `create`, through
  the first cluster instance that answers, treats `NotFound` as deleted, and is cleared once the
  probes are gone; two test cases need it.
- **`make meta-flags` waits for each VM's instance to answer before the next** (R7-B2), with the
  existing `wait_for`; a liveness wait, not a flag check, so round 6's decision stands.
- **The shared rule requires probe ids of the run's own** (R7-C1), as a deleted id is held by its
  tombstone; #70's probe follows it (the orchestrator carries it).
- **Store errors go through `errStatus` (`Unavailable`)** (nit 1); **the probe's failure message
  carries metactl's own output** (nit 2).

### Round 8 (after approval)

- **The trap reads metactl's output** (R8-1): `code = NotFound` counts as deleted, any other
  failure tries the next instance, and an id no instance deletes is printed as left behind.
- **`META_WAIT` is defined (default 60) and the wait is wrapped** (R8-2): `(wait_for …) || die
  "<vm> did not answer; undo: …"`, since `wait_for` dies itself; the test stubs `wait_for`.
- **Probe ids carry lab.sh's pid as well as the time** (R8-3): `<prefix>-<vm>-<unix time>-<pid>`,
  so two runs in the same second neither collide nor delete each other's probes; the shared
  rule says so for #70.
