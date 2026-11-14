# Issue #75 — report

## Review rounds

### Plan review round 1 — 6597c44579ef672a037b04f500bb7831ccc918d9

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75c1 (design-code), r75s1 (design-security).

- **R1-C1 — Critical:** The control and data lanes broke the one ordered session that resync,
  copy reads and `Hello` rely on: `BeginResync` or a copy's `Read` could overtake a `Write`,
  leaving a stale block on a replica that joins the ISR. Evidence:
  `docs/replication-protocol.md:44,59-63,235-241,252-253`; `flaminio-repl/src/sim/mod.rs:235-237`.
- **R1-C2 — High:** The DRR scheduler and owned-return backpressure sat after the target emitted
  requests, so their delay counted against the 3 s request timeout and shrank the ISR; DRR also
  reordered a session's requests and ignored the target's own classes. Evidence:
  `flaminio-repl/src/target.rs:262-283,811-828`; `types.rs:60`; `replication-protocol.md:269-273`.
- **R1-C3 — High:** Lanes did not fit the one-connection-one-session transport (no session id
  in frames, `Hello` placement), a shared data lane let volumes delay each other, and ALPN
  `flaminio-repl/2` clashed with #72 and #61. Evidence: `flaminio-net/src/session.rs:1-11`,
  `frame.rs:1-4`.
- **R1-C4 — High:** The new logic that could lose a write or acknowledgement lived outside the
  simulator; no model change, check or `Bugs` field was named. Evidence:
  `flaminio-repl/src/sim/mod.rs:192-198`; draft `design.md:150-152,180`.
- **R1-C5 — High:** `SetVolumeQoS` had no path to a running target and the contract lacked
  `revision` and `first_epoch`. Evidence: `flaminio-node/src/engine.rs:668,1550,1782`;
  `meta.proto:117-127`.
- **R1-C6 — Medium:** Throttled I/O could "wait or return -EAGAIN" with no rule; `-EAGAIN` is an
  I/O error to a filesystem, no wake-up timer was defined and no minimum rate bounded the wait.
  Evidence: `flaminio-io/src/ublk.rs:1248-1281`; `engine.rs:912`.
- **R1-C7 — Medium:** The #73 ceilings S1 relied on do not exist in #73's contract, and the
  comparison with 0 as unlimited was undefined. Evidence: `73/docs/issues/73/design.md:147-148`.
- **R1-C8 — Medium:** Per-queue permits were unneeded (both paths take the engine mutex) and a
  fast read refused at finish could be charged twice with no rule. Evidence:
  `ublk.rs:1598-1600,1636-1638`; `engine.rs:2127-2157,2222-2226`.
- **R1-C9 — Medium:** Work added to every request had no stated cost or bound on the default
  path. Evidence: draft `design.md:29,45-47,66`.
- **R1-C10 — Medium:** Admission caps could refuse the `Hello` that fences. Evidence:
  `replication-protocol.md:47-50`; draft `design.md:50-51,63`.
- **R1-C11 — Medium:** S11 and "rejected rather than silently disabled" had no approach; an
  older data plane would serve a limited volume unlimited. Evidence: draft `spec.md:86-89`.
- **R1-C12 — Medium:** Tests were topics; `qos-v1.json` had no generator; `perf-check` was not
  wired; the lab section could not be run. Evidence: `Makefile:105-106,124-126`.
- **R1-C13 — Medium:** The release gate and lab lock rested on a missing `release.yml` and an
  unspecified lock. Evidence: `.github/workflows/`; `scripts/lab.sh`.
- **R1-C14 — Medium:** No first step could be built on today's code; the plan named no stage.
  Evidence: dependencies on #72, #73, #74, #76, #80, #81, #83, #84 beyond the issue's own.
- **R1-C15 — Low:** FLUSH and DISCARD had no cost rule. Evidence: `ublk.rs:1279,1454`.
- **R1-C16 — Low:** #76's create identity conflicted with a later `SetVolumeQoS`. Evidence:
  `76/docs/issues/76/spec.md:29-31`.
- **R1-C17 — Low:** The gate's decision function (aggregate, spread) was undefined. Evidence:
  draft `design.md:127-132`.
- **R1-C18 — Low:** The bound named background p99 where the foreground p99 was meant.
  Evidence: draft `design.md:113-114`.
- **R1-C nits — Nit:** `Net::send` line (`net.rs:91-93`); frame layout and byte order of
  `LaneOpen` (`frame.rs:1`); annotation names not listed (`controller.go:157-162`).
- **R1-S1 — High:** Same defect as R1-C1, plus an undefined heartbeat-to-lease binding across
  sessions sharing lanes and no simulator model. Evidence: `frame.rs:23-110`;
  `docs/adr/0005-replica-confirmed-leases.md:20-22`.
- **R1-S2 — High:** Tenant overrides were checked against ceilings no plan defines; 0 (unlimited)
  passed a numeric check; weight and burst had no ceiling. Evidence: `meta/service/auth.go:66`;
  `73/docs/issues/73/design.md:147-148`.
- **R1-S3 — High:** Shared node pools had no per-volume share, so one volume could make another
  fail its sessions and lose ISR members. Evidence: `types.rs:35-36,60`; `target.rs:778,811`.
- **R1-S4 — Medium:** `LaneOpen` was an under-specified authenticated input colliding with #72's
  v2, with no negative tests. Evidence: `session.rs:6-11,124-135`;
  `72/docs/issues/72/capacity-contracts.md:126-133`.
- **R1-S5 — Medium:** The destination could not classify or weight peer work without trusting
  the peer; per-peer control policing had no limit or action. Evidence: `frame.rs:23-110`.
- **R1-S6 — Medium:** Completing a throttled request with `-EAGAIN` is an I/O error to the
  tenant. Evidence: `ublk.rs:341,400,1249`; draft `spec.md:45`.
- **R1-S7 — Medium:** Destructive gate steps had no preflight guard; the shared lock had no
  location or recovery. Evidence: `.github/workflows/lab.yml:28-30`; `docs/ci.md:92-96`.
- **R1-S8 — Low:** How a live target learns a new policy revision was unstated. Evidence:
  `flaminio-repl/src/types.rs:41-43`.
- **R1-S9 — Low:** The release gate trusted `result.json` without provenance. Evidence:
  `.github/workflows/`.
- **R1-S nit — Nit:** "network-order" for `LaneOpen` against the little-endian codec. Evidence:
  `frame.rs:157-163,343`.

Changes/resolution:
- **R1-C14:** the plan now carries the issue's first step only: per-volume limits set by a
  StorageClass, enforced by the target's engine before the replication state machine, plus the
  lab check `make qos-check`. `intent.md` names the step and lists what stays open (the rest of
  step 1, step 2, steps 3-5); the spec has five rules S1-S5; the design was revised to that step
  in the shape of `docs/issues/14/design.md` (Files touched, Contracts, Tests, Lab, Docs, Risks,
  ADRs, Parallelizable?).
- **R1-C1, R1-C3, R1-S1, R1-S4, both frame nits:** resolved by the cut: no lanes, no `LaneOpen`,
  no ALPN; one ordered session per connection, protocol unchanged.
- **R1-C2, R1-S3:** resolved by the cut of DRR, pools and owned returns, and by placement: QoS
  holds client requests only before `Target::submit` and `Target::try_fast_read`, never what the
  target emits (spec invariant, S4). The bound after emission is zero; the engine test
  `held_writes_outlast_the_request_timeout` (300 ms timeout, writes held up to 2.5 s, ISR
  unchanged) fails if QoS delays an emitted request. The target's classes and #11's order do not
  change.
- **R1-C4:** no replication, membership or recovery logic changes, so no new simulator rule;
  the one hazard is covered by the engine test above (decisions.md).
- **R1-C5, R1-S8, R1-C16:** resolved by the cut of `SetVolumeQoS`: the policy is fixed at create
  and read once with the volume at attach (`start_target`); no revision or `first_epoch` needed.
  A create retry still matches; when #76 lands its create identity includes the policy.
- **R1-C6, R1-S6:** S4: a held request keeps its ublk tag and never completes with an error;
  released at its due instant by a timer on the gather timer's channel, with the 10 ms tick as
  backstop; floors (1 MiB/s, 1 MiB burst) bound the head's wait to 1 s; the kernel's timeout
  behaviour is checked by `make qos-check` (a wait past 30 s, no error).
- **R1-C7, R1-S2:** tenant overrides and weight cut: only StorageClass parameters set a policy,
  PVC annotations are not read, nothing depends on #73 (whose step provides no tenant QoS
  ceilings and does not plan them); per-namespace class restriction is not claimed by any plan.
- **R1-C8:** one charge point in the engine, under the mutex both paths already take; no
  permits. The fast path charges only on admission; a fast read refused at finish is charged
  again on resubmission, stated in S2 and tested.
- **R1-C9:** the design states the default path's cost (one `Option` test per request, one
  empty-set test per pump); measuring it is left open for the gate step (Risks).
- **R1-C10, R1-S5:** resolved by the cut: no admission caps, no destination scheduling.
- **R1-C11:** S5: older CSI refuses the parameters; the CSI controller checks the stored policy;
  `GetVolumeRequest.enforces_qos` lets the metadata service refuse a limited volume to an older
  data plane. Each has a test.
- **R1-C12:** one test line per rule with file, case and level; the validity rule is written
  once in Contracts and tested by the same table in Go and Rust (no vector file); `perf-check`
  cut; the Lab section gives target, settings, pass criteria and cleanup.
- **R1-C13, R1-C17, R1-S7, R1-S9:** resolved by the cut: no gates, release workflow, lock or
  harness in this step; `make qos-check` formats nothing and validates `NODE`.
- **R1-C15:** S2: FLUSH and DISCARD cost nothing (answered without the engine).
- **R1-C18:** resolved by the cut of the campaigns; the intent names the later step's metric
  (foreground p99 with each producer running).
- **R1-C nit (`Net::send`):** cited as `net.rs:91-93` in the intent. **Annotation nit:** the four
  StorageClass parameter names are listed exactly; no annotations.
- **Beyond the findings (raised while fixing R1-C8 and R1-C15):** the limits now count client
  requests and bytes; with 4 KiB logical blocks the draft's 4 KiB units were the byte count / 4096,
  so its two limits were one (decisions.md, ADR proposal).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 83ae6ddbbbf2dccb7601593e764a745609c95880

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75c2 (incremental design-code), r75s2 (incremental design-security).

- **R2-S1 — High:** A metadata instance older than the change, serving beside newer ones
  (rolling upgrade, CSI first, rollback), serves a limited volume unlimited and its `Attach`
  drops `qos` from the header for good; S5 treated the metadata service as one version and no
  test covered mixed instances. Evidence: `flaminio-metaclient/src/lib.rs:6-7`;
  `control/csi/meta.go:64-79`; `meta/service/service.go:275-284,709-724`.
- **R2-C2 — Medium:** The data plane's refusal comes after `Attach` moved the target and
  advanced the epoch; nothing keeps failover or drain from choosing that node again. Evidence:
  `meta/service/auth.go` (agent `Attach`); `flaminio-node/src/engine.rs:660-675`.
- **R2-C1 — Medium:** A request larger than the bucket's cap is never admitted and blocks the
  FIFO; `Input::Io` is public and `Engine::submit` checks only alignment and range. Evidence:
  `flaminio-io/src/node.rs:222`; `engine.rs:1694-1709`.
- **R2-C3 — Low:** The fast path needs a check that does not debit; the Gate had none.
  Evidence: draft design "Fast path".
- **R2-C4 — Low:** Where the gate is drained on deposition, and how `held` and `qos_timer`
  tolerate a removed volume, was unstated. Evidence: `engine.rs:1841-1853`.
- **R2-C5 — Low:** The S4 test's bound, (k − 1) × 0.5 s, passes with a free first request.
  Evidence: draft design Tests.
- **R2-C6 — Low:** The kernel's timeout re-arm is the only guard against blk-mq's timeout and
  is checked on one kernel; the ADR's Confirmation lists no versions. Evidence: draft design
  Risks.
- **R2-S2 — Low:** Each create against an older metadata service left an orphaned volume that
  nothing deletes. Evidence: `control/csi/controller.go:239-247`; `meta/domain/domain.go:50-51`.
- **R2-S3 — Low:** The wait bound held only at the default depth and queue count, omitted
  blk-mq waiters, and the hung-task effect on the node was unstated. Evidence:
  `flaminio-io/src/node.rs:44-54,171,201-204`.
- **R2-C nit 1 — Nit:** decisions.md and intent.md still name #73.
- **R2-C nit 2 — Nit:** S4's 1 s holds per head only.
- **R2-C nit 3 — Nit:** "pods do not start" should read "attach fails".

Changes/resolution:
- **R2-S1:** one switch, stated once in the design ("The switch, the one guard against older
  instances"): `flaminio-meta serve --qos`, off by default; while off, a limited create is
  refused. The operator turns it on, on every instance, once every metadata instance and data
  plane runs this version; rolling back is then unsupported (as #70's flag). Spec S5, intent
  and ADR proposal updated. Mixed-instance tests: `TestCreateVolumeQoSOff`, and
  `TestControllerQoSMixedInstances` (a refusing and a dropping instance: the create fails both
  ways, no volume left).
- **R2-C2:** stated as the limit in "Versions": the attach fails after the target moved; drain
  moves replicas only, failover lets Kubernetes reschedule, nothing steers the pod away; the
  volume has no serving target until a capable node attaches it. Test:
  `a_refused_volume_read_fails_the_attach` with the service test.
- **R2-C1:** a cost above a limited burst is charged as the burst (spec S2, design "The gate");
  test `a_cost_above_the_burst_is_admitted_at_a_full_bucket`.
- **R2-C3:** `Gate::covers` refills and tests without debiting; the fast path uses it, then
  `take`; tests `covers_never_debits` and `fast_reads_share_the_bucket`.
- **R2-C4:** `release` returns one request at a time; `release_held` drops a name whose target is
  gone; the deposition branch of `drain_one` drains the gate before `targets.remove`; the
  deposition test pumps after.
- **R2-C5:** S3 and S4 tests take t0 before `Input::Attach` and assert the k-th write no sooner
  than t0 + k × 0.5 s (first write t0 + 1 s at 1 IOPS).
- **R2-C6:** `make qos-check` also checks the device stayed the same and prints `uname -r`; the
  ADR's Confirmation lists checked kernels: none yet.
- **R2-S2:** on `OK` with a missing or different policy, the CSI controller deletes the volume
  its create made, then fails; never on `AlreadyExists`. A failed delete is stated in
  Compatibility.
- **R2-S3:** the bound is `depth × queues × max(1/iops, io_buf/bytes_per_second)` plus blk-mq
  waiters (spec S4, design Waits, ADR); `docs/lab.md` and the ADR require
  `kernel.hung_task_panic` off on nodes serving limited volumes.
- **R2-C nit 1:** does not apply (decisions.md): naming #73 to say its step provides no
  ceilings is the intended text.
- **R2-C nit 2:** S4 now says "after its predecessor's admission".
- **R2-C nit 3:** Compatibility now says the attach fails after the target moved.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 1a0a7ed3f5b9aeb1964d18f300afea9b2b848f01

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75c3 (incremental design-code), r75s3 (incremental design-security).

- **r75c3 #1 — Medium:** With round 2's clamp, `a_request_across_chunks_is_charged_once` at burst
  1 cannot tell a single charge from a double one. Evidence: design Tests S2.
- **r75c3 #2 — Low:** Two CSI rules had no test: a failed delete returns its error, and nothing
  is deleted on `AlreadyExists`. Evidence: design CSI bullet; `TestControllerQoS`.
- **r75c3 #3 / R3-S2 — Low:** Nothing said or tested that `GetVolume`'s refusal and the returned
  policy apply with the switch off; the ADR read as if the switch gated reads. Evidence:
  design switch bullet; `adr-proposal.md:30`.
- **r75c3 #4 / R3-S3 — Low:** `metactl create` against an older instance silently makes an
  unlimited volume and exits 0. Evidence: `meta/cmd/flaminio-meta/main.go:501`.
- **r75c3 nit 5 / R3-S6 — Nit:** The ADR's status against AGENTS.md's binding rule, and Q1 still
  called open or Proposed after the approval. Evidence: `spec.md:84-87`; `design.md:218-219`;
  `adr-proposal.md:5,47`.
- **R3-S1 — Low:** The switch's order is only documented, the rollback text understated the
  permanent, silent erasure, and the lab unit turned `--qos` on mid-rollout. Evidence:
  `scripts/guest/meta.sh:28`; `scripts/lab.sh:514-519`; `service.go:722-725`.
- **R3-S4 — Low:** "Clamped to its bucket's burst" could read as the policy field (0 = free), and
  the bound's test summed charges, not what was admitted. Evidence: design "The gate";
  `spec.md:38-40`.
- **R3-S5 — Low:** A create whose answer is lost still leaves an orphan while instances are
  mixed. Evidence: `control/csi/meta.go:72-79`.
- **R3-S7 — Nit:** `intent.md:39` reversed who refuses whom.
- **R3-S8 — Nit:** The `hung_task_panic` rule covered only nodes serving limited volumes, not
  every node that may attach one.

Changes/resolution:
- **r75c3 #1:** the test now uses 1 IOPS with burst 2: once full, a request over two chunks and
  then a one-chunk request are both admitted with no QoS timer armed, and a third is held; a
  double charge would hold the second.
- **r75c3 #2:** `TestControllerQoS` asserts the volume stays and no `DeleteVolume` is sent on
  `AlreadyExists`; `TestControllerQoSMixedInstances` adds a failing delete whose error is
  returned.
- **r75c3 #3 / R3-S2:** the switch gates `CreateVolume` only (design, spec S5, ADR "refuses
  limited creates"); new `TestGetVolumeQoSSwitchOff` (created with the switch on, served with it
  off: `qos` returned, an undeclared `io` caller refused).
- **r75c3 #4 / R3-S3:** metactl compares the returned policy like the CSI controller; on a
  mismatch it deletes nothing and exits 1 naming the volume (exact message in Contracts);
  `TestCreateQoSNotStored` in `main_test.go`.
- **r75c3 nit 5 / R3-S6:** the ADR's status says it binds this plan's implementation from the
  approval on, numbered and moved into `docs/adr/` with the implementation; its scope line no
  longer says no approval. "Settled at approval" lines added in place in spec (Q1), design
  (Risks, ADRs) and the ADR proposal (Decision, Consequences); measuring the default path's cost
  is marked still open.
- **R3-S1:** the switch bullet, spec S5, the ADR and `docs/lab.md` now say nothing enforces the
  order, that an older instance erases a limited volume's policy for good whatever the switch,
  that the only safe rollback deletes the limited volumes first, that an older binary exits on
  the unknown flag, and that operators add `--qos` in a second rollout. Lab: `meta.sh` writes
  `--qos` only with `QOS=1`, `make meta-up QOS=1` is the second rollout, and `make qos-check`
  refuses until every instance has it.
- **R3-S4:** "effective burst" is defined in the spec and used in the gate; S2 bounds what is
  admitted; the property test sums real counts and lengths with 0-burst policies among its cases.
- **R3-S5:** stated, not handled: Compatibility and `docs/lab.md` list the lost-answer leftover
  (and the failed-delete one); an administrator deletes the never-attached volume.
  `TestControllerQoSMixedInstances` covers it (retry `AlreadyExists`, volume stays, no delete).
- **R3-S7:** intent now says the metadata service refuses the older data plane.
- **R3-S8:** the rule now covers every node that may attach a limited volume (design Docs and
  Risks, ADR).
- decisions.md records the round-3 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — dc80bd5 (reviewed at fd83341, which adds only report.md)

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75c4 (incremental design-code), r75s4 (incremental design-security).

- **r75c4 #1 / R4-S1 — Medium:** The lab's `QOS` plumbing was incomplete. `meta-up` did not
  pass it and `cmd_meta` had fixed `ENV_VARS`. `cluster-up` and `make chaos`'s restart of w
  rewrite the unit without `--qos`. `make meta-up QOS=1` also reinstalls binaries VM by VM, so
  it turns the switch on mid-rollout. Nothing checked the order, and the `qos-check`
  precondition was not tested. Evidence: `Makefile:301-308,331-332`;
  `scripts/lab.sh:509-525,1403-1407`.
- **R4-S2 — Low:** With the switch off, the refusal pre-empted `AlreadyExists`, and its code was
  unspecified. A retry after a lost answer could then fail for good and leave a third leftover.
  Evidence: `meta/service/service.go:348-364,414-433`; `control/csi/meta.go:72-79`;
  `control/csi/controller.go:248-249`.
- **R4-S3 — Low:** The rollback warning was missing from the flag's usage, `main.go`'s doc and
  the Makefile help. Evidence: `meta/cmd/flaminio-meta/main.go:19-23`; `Makefile:331`.
- **r75c4 #2 — Low:** The `hung_task_panic` answer was widened after approval but listed as
  settled at approval. Evidence: decisions.md, Settled at approval; design.md Risks.
- **r75c4 #3 — Low:** `metactl`'s ninth argument had no test of its own, and how
  `TestCreateQoSNotStored` builds an instance that drops `qos` was unsaid. Evidence:
  `meta/cmd/flaminio-meta/main_test.go:74`.
- **R4 nit 1 — Nit:** S5's lead sentence and intent item 4 read as if something ensured the
  version order. Evidence: `spec.md:58-59`; `intent.md:38`.
- **R4 nit 2 — Nit:** The leftover remedy could not be followed: `metactl volumes` lists ids
  only. Evidence: `main.go:424-425`.
- **R4 nit 3 — Nit:** Where a bad `QOS` value is refused was unsaid; `ENV_VARS` reaches the
  remote shell unquoted. Evidence: `scripts/lib.sh:157`.

Changes/resolution:
- **r75c4 #1, R4-S1, R4 nit 3:** one lab mechanism for every default-off `flaminio-meta serve`
  flag, shared with #70 (`--volume-expansion`), in design.md, section "Lab flags of the metadata
  service (S5, shared with #70)":
  - The flags live in `/etc/flaminio/meta-flags`, read by the unit through `EnvironmentFile`
    and `$FLAGS`. `meta up` never writes the file, so `cluster-up` and the chaos restart keep
    the flags.
  - `make meta-flags` checks in lab.sh that each value is 0 or 1 before anything reaches a
    remote shell. It refuses to turn a flag on unless every cluster VM runs the build (sha256,
    as `vol-confirm` does), and rewrites only the file, without reinstalling.
  - `make qos-check` reads the file on every cluster VM.
  - `scripts/tools/meta-flags-test.sh` tests the generated unit and the refusals.
  - Whichever of #70 and #75 lands first adds the mechanism. It replaces round 3's
    `make meta-up QOS=1`.
- **R4-S2:** with the switch off, a limited create first reads the header: an existing id
  answers `AlreadyExists`, so a retry matches. Otherwise it answers `FailedPrecondition`, which
  the CSI controller maps to the retryable `ResourceExhausted`. This is in Contracts, spec S5
  and `TestCreateVolumeQoSOff`.
- **R4-S3:** the flag's usage text and `main.go`'s doc carry the rollback warning, and so do
  the Makefile help lines of `meta-flags` and `meta-up`.
- **r75c4 #2:** the widening is recorded as a change after approval in decisions.md (Settled at
  approval), in the design's Risks and in the ADR proposal.
- **r75c4 #3:** new `TestCreateQoSArgument`: valid input passes through; wrong count,
  non-decimal and above-u64 values are refused with no call. `TestCreateQoSNotStored` drops
  `qos` with an interceptor in the fixture.
- **R4 nit 1:** S5 and the intent now say limited volumes are meant to exist only once every
  component runs this version, and that operators ensure it.
- **R4 nit 2:** the remedy is now: once every instance runs this version, delete each
  `pvc-<uid>` volume that has no PV; a Pending PVC's next retry then creates it with its policy.
- decisions.md records the round-4 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 40cdf3f (reviewed at bb7fbb2, which adds only report.md)

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75c5 (incremental design-code), r75s5 (incremental design-security).

- **r75c5 #1 — Medium:** "The file is what the process runs with" is not guaranteed. A unit
  written by an older `meta.sh` does not read the file, and a failed restart leaves the file
  ahead of the process; `qos-check` trusted the file. Evidence: `scripts/guest/meta.sh:14-48`.
- **R5-S1 — Medium:** The leftover remedy (delete every `pvc-<uid>` volume with no PV) would hit
  retained volumes and provisions in flight. It also promised a re-create that a tombstone
  blocks. Evidence: `deploy/csi/driver.yaml:67-68,233`; `meta/service/service.go:372-377`.
- **R5-S2 — Low:** The build check guarded only `meta-flags`. The kept file turns `--qos` back on
  at a later partial `meta up` or the chaos restart on a mixed cluster, and `qos-check` trusted
  the file; its precondition had no test. Evidence: `scripts/lab.sh:509-520,1407`.
- **r75c5 #2 — Low:** The build check covers the metadata binaries only, yet the help read as
  if it covered the data planes too.
- **r75c5 #3 — Low:** The lab had no rollback remedy for a leftover flags file.
- **r75c5 #4 — Low:** The script test checks the unit's text, not systemd's parsing of `FLAGS`.
- **R5-S3 — Low:** #70's plan still described `make meta-up VOLUME_EXPANSION=1`. Evidence:
  `70/docs/issues/70/design.md:78-80,196-198`.
- **R5 nit 1 — Nit:** "reads the header ... as the claim would" was ambiguous; `readHeader`
  hides headers being created or deleted. Evidence: `service.go:286-289`.
- **r75c5 nit 5 — Nit:** decisions.md's round-3 `make meta-up QOS=1` was not marked replaced.

Changes/resolution:
- **r75c5 #1, R5-S2:** one check, `meta.sh running FLAG`. It requires the service to be active
  and its `/proc/<pid>/cmdline` to hold the flag.
  - `make qos-check` requires `running --qos` on every cluster VM.
  - `meta.sh flags` runs the check after its restart, and dies before writing on a unit that
    does not read the file.
  - An older binary exits on `--qos`, so every VM passing proves every instance is new enough.
    A mixed cluster left by a later `meta up` or the chaos restart fails the check. In the lab
    only `qos-check` creates limited volumes, so no further guard on `meta up` is needed.
  - New script-test cases: a unit without the line, `running` with a fake `PROC`, and the
    precondition refused for one failing VM.
- **R5-S1:** the remedy is dropped. Compatibility now gives the one condition that identifies
  each leftover: the `pvc-<uid>` of a Pending PVC of a QoS class showing the controller's error,
  with no `qos` and no target. Deleting it is the administrator's decision. The PVC binds only
  after the tombstone clears and with the switch on.
- **r75c5 #2:** the design and the `meta-flags` help say the build check covers the metadata
  binaries only.
- **r75c5 #3:** docs/lab.md gives the lab rollback: delete the limited volumes, then
  `make meta-flags QOS=0`, before going back to an older checkout.
- **r75c5 #4:** the design says the script test does not cover systemd's parsing of `FLAGS`;
  only `running`, on the lab, proves it.
- **R5-S3:** recorded in decisions.md as handled in #70's round-4 fix; no change here.
- **R5 nit 1:** the design names `readHeader`. A visible header answers `AlreadyExists`;
  anything else, including a header being created or deleted, gets `FailedPrecondition`.
- **r75c5 nit 5:** the round-3 line is marked replaced.
- decisions.md records the round-5 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 00e57e8 (reviewed at e907b39, which adds only report.md)

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75b6 (incremental, design-code and design-security).

- **R6-B1 — Medium:** `meta.sh running` read the service once. Right after a restart, that read
  could fail on a healthy VM (the executor's pid). Against a crash-looping binary it could pass.
  So "all passing proves every instance is new enough" did not hold. Evidence:
  `scripts/guest/meta.sh:20-44`; `meta/cmd/flaminio-meta/main.go:84,99,147-150`.
- **R6-B2 — Low:** #70's `csi_expand` still read the flags file "as qos-check does", while the
  shared section now said the file proves nothing. Evidence: `70/docs/issues/70/design.md:202,208-210`.
- **R6-B3 — Low:** `meta.sh flags`' check after its restart was unspecified for a cleared flag,
  and untested.
- **R6-B4 — Low:** The "every cluster VM" test cases could not tell every cluster VM from a
  smaller set. Evidence: `scripts/lib.sh:44-45`.
- **R6-B5 — Low:** The lab rollback deleted the limited volumes while the switch was still on.
- **R6-B6 — Low:** decisions.md still recorded round 4's broad remedy without a mark.
- **R6 nit 1 — Nit:** "two paths overridable" listed three.
- **R6 nit 2 — Nit:** "anything else gets `FailedPrecondition`" also turned `readHeader`'s etcd
  and decode errors into it. Evidence: `meta/service/service.go:276-284`.

Changes/resolution:
- **R6-B1:** the lab flag machinery is cut down to what a check can prove directly. `make
  qos-check` first creates a small limited volume through each cluster VM's own metadata
  instance (`metactl VM SERVER`, the endpoints `meta_endpoints` lists) and deletes it. A probe
  passes only on exit 0 with the printed `qos` matching. An instance without `--qos`, an older
  instance or an older client fails it, naming the VM. `meta.sh running`, the process
  inspection, `make meta-flags`' build check and its unit guard are cut. The flags file and
  `make meta-flags` stay only as the way to set a flag, and the design says they prove nothing.
- **R6-B2:** the shared section ("Lab flags of the metadata service") states the rule for every
  lab check of a `serve` flag: do the flagged operation through each cluster VM's own instance,
  then undo it. #70's version is an expansion through each instance, refused where the flag is
  off. The change to #70 goes through the orchestrator.
- **R6-B3:** resolved by the cut; nothing checks a VM after `flags` any more.
- **R6-B4:** the probe's test cases name the VM: only w refusing fails naming w; only w answering
  without `qos` fails naming w.
- **R6-B5:** the rollback order is now `make meta-flags QOS=0`, then delete the QoS
  StorageClasses and the limited volumes, then check out the older tree.
- **R6-B6:** round 4's remedy is marked replaced in decisions.md (as are the round-5 entries the
  cut replaces).
- **R6 nit 1:** two overridable paths, `META_UNIT` and `META_FLAGS_FILE`.
- **R6 nit 2:** only `readHeader`'s `NotFound` (absent, being created or being deleted) becomes
  `FailedPrecondition`; any other error is returned unchanged, and `TestCreateVolumeQoSOff`
  checks a store error.
- decisions.md records the round-6 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 4663835 (reviewed at ca90950, which adds only report.md)

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75b7 (incremental, design-code and design-security).

- **R7-B1 — Low:** The trap is the probe's only cleanup on failure. Which ids it deletes and
  through which instance was unsaid, and no test case needed it. Evidence: design "Probe";
  `meta/cmd/flaminio-meta/main.go:345`; `meta/service/service.go:583`.
- **R7-B2 — Low:** `make meta-flags` restarted VM after VM without waiting. Against a binary older
  than the flag, every instance would end up crash-looping while the command reported success.
  Evidence: `scripts/guest/meta.sh:20-44`; `main.go:84,99`.
- **R7-C1 — Low:** The shared rule left out that each probe needs a fresh id: a deleted id is held
  by its tombstone. Evidence: `service.go:372-379`; `control/csi/tombstones.go:9,33-49`.
- **R7 nit 1 — Nit:** "Returned as it is" read as skipping `errStatus`, which turns a store error
  into `Unavailable`. Evidence: `service.go:133-152`; `control/csi/meta.go:62-82`.
- **R7 nit 2 — Nit:** The probe's fixed failure message hid metactl's own error.

Changes/resolution:
- **R7-B1:** the design now specifies the trap. It is set before the first create. It deletes
  every id the run passed to `create`, whatever that create returned, through the first cluster
  instance that answers, and counts `NotFound` as deleted. It is cleared (`trap - EXIT`) once
  every probe is deleted. New test cases: only w answering without `qos` records a `delete` of
  w's probe; a's inline delete failing fails the check, and the trap deletes a's probe.
- **R7-B2:** after each VM, `make meta-flags` waits up to `META_WAIT` (60 s) for that VM's
  instance to answer `metactl "$VM_A" "$vm" nodes`, using the existing `wait_for`. Otherwise it
  stops before the next VM, naming it and the undo. This is a liveness wait, not a flag check.
  New test case: the first VM never answering stops the command with no call to the second VM.
- **R7-C1:** the shared rule now requires probe ids unique to the run,
  `<prefix>-<vm>-<unix time>`, checked with `valid_id`. They are deleted by the same trap rules.
- **R7 nit 1:** other errors go through `errStatus`, so a store error is `Unavailable`;
  `TestCreateVolumeQoSOff` asserts it.
- **R7 nit 2:** the failure message is followed by metactl's own output.
- decisions.md records the round-7 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — 02853e8 (reviewed at ea73f03, which adds only report.md)

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75b8 (incremental, design-code and design-security).

- **R8-1 — Low:** How the trap tells `NotFound` from other errors, and what it does when no
  instance answers, was unspecified. metactl exits 1 for every error and prints `rpc error:
  code = …`. Evidence: `meta/cmd/flaminio-meta/main.go:77-79`.
- **R8-2 — Low:** `META_WAIT` was not defined anywhere. Also, `wait_for` dies itself, so it
  cannot name the undo, and it polls every 5 s. Evidence: `scripts/lib.sh:211-219`;
  `scripts/lab.sh:643`.
- **R8-3 — Low (r75b8's R8-4):** Two runs in the same second collide on `qos-probe-<vm>-<T>`, and
  the failed run's trap then deletes the other run's probe. Evidence: `service.go:372-379`.

Changes/resolution:
- **R8-1:** the trap tries the cluster instances in order for each id. Output holding
  `code = NotFound` counts as deleted; any other failure tries the next instance. An id that no
  instance deletes is printed as left behind. New test cases: `Unavailable` from a's instance
  deletes through b's; no instance answering prints the ids left.
- **R8-2:** `META_WAIT` (seconds, default 60) is defined in lab.sh beside the flag variables.
  The wait is written as `(wait_for "metadata on $vm" "$META_WAIT" metactl "$VM_A" "$vm" nodes)
  || die "$vm did not answer; undo: …"`, so the undo is in the message. The test stubs
  `wait_for`.
- **R8-3:** probe ids are now `<prefix>-<vm>-<unix time>-<pid of lab.sh>` (`$$`, still passing
  `valid_id`). The shared rule says so, for #70 as well.
- decisions.md records the round-8 choices and, under Approvals, that these fixes came after
  the approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 597b861eda502c3845843d08e12b1797ba5ddf4d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75b9 (incremental design-code and design-security).

r75b9 confirmed all three round-8 findings fixed against the code and reported nothing of Low or worse.

- **R9-N1 — Nit:** metactl prints its errors on stderr, through ssh; the design said only "output", so the trap and the probe must capture `2>&1` for the `code = NotFound` match to work. Evidence: meta/cmd/flaminio-meta/main.go:77–79; design.md, "The trap".

Changes/resolution (one sentence of design.md, made by the orchestrator; a change after the approval): the trap's sentence now says metactl prints on stderr and that the trap and the probe capture it with `2>&1` through ssh.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — 9083e69231a9f41808501d07a61a4cd0a7af745a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r75b10 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
