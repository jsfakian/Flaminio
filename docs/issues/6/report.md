# Issue #6 — report

## Review rounds

### Plan review round 1 — e0734f7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-S1 — High:** the startup `--attach` retry resends the same token after a failure that
  comes once the target has started, so a second target starts at one epoch, against
  replication-protocol §3. A defect of the shipped code (#25). Evidence: `ublk.rs:747–768` (same
  token), `:888–896` (detach after a device failure), `engine.rs:1617` (the check passes),
  `target.rs:484–486` (sessions from 1 again).
- **R1-C1 — Medium:** S1's "waits for upstream" is contradicted by #74 and #15, which vendor
  libublk 0.4.8. Evidence: design.md:131–132, :179–181; intent.md:51–58; #74 decisions.md:28–33,
  :49–52; #15 design.md:152–159.
- **R1-C3 — Medium:** several rules have no named test, and the test list drifted from the code.
  Evidence: `ublk.rs:731–769`, `:802–808`, `:858`, `:903`; `engine.rs:649–655`;
  `control.rs:234–296`; `dataplane_test.go:87–158`.
- **R1-S2 — Medium:** the rules that keep a stale request from running have no test. Evidence:
  `control.rs:220–231` checks only the deadline; `ublk.rs:799–806`; `engine.rs:649–655`.
- **R1-C2 — Low:** S1 is wrong or unspecified under #74's `--privsep`. Evidence: spec.md:5–11;
  design.md:123–129; #74 spec.md:22, :40, design.md:57–58, :286, :358–359.
- **R1-C4 — Low:** the startup retry, "already attaching", the older-data-plane fallback and the
  "dropped without an answer" reply have no spec rule. Evidence: design.md:58–63, :77–83;
  spec.md:33–42.
- **R1-C5 — Low:** the per-request deadline's worked example has the wrong arithmetic. Evidence:
  design.md:63–64; `dataplane.go:56–62`, `:108`; `control.rs:153–160`.
- **R1-C6 — Low:** the generation check's stated failure mode is no longer true since #30.
  Evidence: design.md:157–158; `engine.rs:1610–1622`.
- **R1-C7 — Low:** the spec and design still describe S2–S4 as future work. Evidence:
  spec.md:20–22, :47–49; design.md:31, :107–108, :131.
- **R1-C8 — Low:** some statements are contradicted by #74 and #15, unmentioned. Evidence:
  design.md:49–51, :74–81; spec.md:26–27; #74 design.md:165–168; #15 design.md:125–152, :165,
  :211, spec.md:174–175.
- **R1-S3 — Low:** the S1 lab check can leave a volume's first block overwritten. Evidence: spec
  Tests "S1, once it lands"; design "Lab check".
- **R1-S4 — Low:** whole-device probing (LVM, md) is another route for the intent's threat, not
  named. Evidence: intent "Why"; no udev rule or LVM filter for `ublkb*` in the repository.
- **R1-S5 — Low:** #6's reason for waiting on upstream conflicts with #15's plan. Evidence: design
  Decisions; #15 design, S2 and Risks.
- **R1-C-N1 — Nit:** "logs once" does not say once per what. Evidence: spec.md:7–8,
  design.md:124–125.
- **R1-C-N2 — Nit:** the S1 lab check does not say how it learns that the kernel advertises the
  flag. Evidence: spec.md:89–92, design.md:126–129.
- **R1-C-N3 — Nit:** "a timed attach, as in the design's measurements" is not runnable. Evidence:
  spec.md:86–88, design.md:163.
- **R1-S-N1 — Nit:** the design gives the wrong reason for the generation check. Evidence: design
  S3 and Tests; `engine.rs:1617–1623`.

Changes/resolution:
- R1-S1: new spec rule S6. The engine refuses an attach at an epoch at or below the highest one at
  which a target of that volume started in this run (`start_target`, a per-volume map), and the
  startup retry stops at that refusal. A refusal, not a fresh token (the data plane cannot take a
  new epoch without a new call path and `auth.go` rule); in the engine, not only the retry
  thread, so every attach in the run is covered. Engine test, retry test, and a simulator check
  (`two targets at epoch E`, scripted `DeviceFails`) with injected bug
  `sim::Bugs::restart_at_epoch`. Until #15 the bar lasts one run; #15's run mark makes it durable.
- R1-C1, R1-S5: S1 is a hunk in the vendored libublk 0.4.8 (`UBLK_F_NO_AUTO_PART_SCAN` in
  `UBLK_DRV_F_ALL`), landing with or after #74 or #15; #72 stays offered upstream; the flag goes
  into #15's rewritten `add_device`. Intent Dependency, spec S1, design S1, Follow-ups, Decisions.
- R1-C2: S1 is direct mode's; under `--privsep` the kernel already suppresses scans and re-reads,
  so no flag and no log; `partscan.sh` runs with `PRIVSEP=0`.
- R1-C3, R1-S2: the tests on main are named, the Go equality claim and the Rust limits test's
  source corrected; new tests for the skip (`take_attach`: answered, never begun, no
  `Input::Attach`), the step bounds, the startup retry and "already attaching".
- R1-C4: S2 states the four behaviours, each with its test.
- R1-C5: example corrected (59 s sent, 50 s taken); the fourth word cuts the limit only below 51 s
  left.
- R1-C6, R1-S-N1: the generation keeps a late answer from failing or filling in the next attach;
  the token check bars an old-epoch target.
- R1-C7: status lines; S2–S4 shipped in #25 (3b5560b), old values marked "before #25".
- R1-C8: design "Later plans" names #74's and #15's changes (START_LIMIT, the retry's stop, the
  id walk, the run mark and error form).
- R1-S3: `partscan.sh` uses a scratch volume deleted from an `EXIT` trap; save and restore cut;
  `dd conv=fsync`.
- R1-S4: intent Out of scope names whole-device probing as a follow-up (not filed), beside #74's
  settled Q4.
- R1-C-N1: once per process (`OnceLock`). R1-C-N2: the node's log of its current run.
  R1-C-N3: `reconcile.sh`'s `device … ready` timing.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made
after approval and are recorded in decisions.md. No further approval is inferred.

### Plan review round 2 — 8fddde3

Scope: correctness against code and security of the plan's round-1 changes; no runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-C1 — Medium:** S6's bar lived in `flaminio-node`, and the simulator checked a copy of it,
  so the injected bug turned off only the copy. Evidence: design.md:192–199, :217–227;
  `sim/mod.rs:815–840` against `engine.rs:1596–1683`; AGENTS.md, Principles.
- **R2-B1 — Low:** `partscan.sh` reused the first token on re-attach, which S6 refuses, and had no
  positive control; S1's re-read clause had no test. Evidence: design.md:165–177, spec.md:178–183,
  spec.md:15.
- **R2-C2 — Low:** `take_attach`'s `serve` could not call `attach_and_serve`, which needs the
  reply sender. Evidence: design.md:265–268; `ublk.rs:685–694`, `:809`, `:824–830`, `:888`,
  `:936–944`.
- **R2-C3 — Low:** the new simulator check, scenario and injected bug were not planned into
  `replication-protocol.md` §9. Evidence: `replication-protocol.md:350–392`; design.md:111–119,
  :217–230, :307–311.
- **R2-N1 — Nit:** the bounds test expected `now` for a `by` already past; `(now +
  ADD_DEVICE_LIMIT).min(by)` returns `by`. Evidence: design.md:273–274; `ublk.rs:903`.

Changes/resolution:
- R2-C1: S6's comparison is a sans-I/O type in `flaminio-repl`, `RunEpochs<K>`
  (`run_epochs.rs`), with `admit(volume, epoch)`; the engine and the simulator both call it, and
  `restart_at_epoch` is its test-only bug, set through `sim::Bugs::run_epochs`. New "Files
  touched" section; "Later plans" says #15's mark can store the same type's state.
- R2-B1: a new `metactl attach` token for the re-attach; a 5 s poll for `p1`; `blockdev
  --rereadpt` must then create `p1` (positive control, direct mode, S1's re-read clause).
- R2-C2: `serve` takes `(AttachRequest, &mpsc::Sender<String>)`; `take_attach` keeps the sender
  to answer errors.
- R2-C3: S6's Docs bullet plans §9's fourth check, third scenario, mutation-table row, and the
  note that this catch is `two targets at epoch`.
- R2-N1: the test expects `steps_by` itself once it has passed.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made
after approval and are recorded in decisions.md. No further approval is inferred.

### Plan review round 3 — 8fde090

Scope: correctness against code and security of the plan's round-2 changes; no runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R3-B1 — Low:** `partscan.sh` did not verify direct mode, and its positive control could fail
  for other reasons: `blockdev --rereadpt` returns `EBUSY` while udev's probe holds the device,
  and a failure gave no cause. Evidence: design.md "Lab check", spec.md S1 test; no `privsep` in
  `scripts/`.
- **R3-N1 — Nit:** the §9 Docs bullet did not reword the scenarios' intro ("the two split-brain
  cases") or the mutation checks' counting rule (linearizability or convergence only), which the
  new scenario and its catch do not fit. Evidence: design.md, S6 Docs bullet;
  `replication-protocol.md` §9.

Changes/resolution:
- R3-B1: the control runs `udevadm settle`, then retries `blockdev --rereadpt` for 5 s; on failure
  it says "the re-read created no p1: the table is invalid, the node runs --privsep, or the device
  was busy". It no longer claims to show direct mode. Until #74 lands the lab runs only direct
  mode (`PRIVSEP=0` dropped); once #74 lands, the check reads `--privsep` from the node unit's
  command line and fails up front, naming the mode.
- R3-N1: the Docs bullet also rewords §9's scenarios' intro (three scenarios) and the counting
  rule (`two targets at epoch` counts for that scenario's bug).

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made
after approval and are recorded in decisions.md. No further approval is inferred.

### Plan review round 4 — 8fb5882

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- R3-B1 and R3-N1 fixed; nothing at Low or above opened. `flaminio-node` is the lab's unit and
  `systemctl show -p ExecStart --value` is already parsed this way by `confirm.sh` and `reconcile.sh`.
- **R4-N1 — Nit:** the design did not say to match `--privsep` as a whole word. Evidence:
  design.md:169.

Changes/resolution: the check uses `grep -qw -- --privsep` on `ExecStart`'s value, the flag name to
be confirmed when #74 lands; recorded in decisions.md, round 4, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — f9974eb

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- R4-N1 fixed; no defects.
- **R5-N1 — Nit:** `grep -w` treats `-` as a non-word character, so `--privsep=0` or `--privsep-x`
  would also match. Evidence: design.md:169–170.

Changes/resolution: the check matches an argument that is exactly `--privsep`
(`grep -qE -- '(^| )--privsep( |;|$)'`); recorded in decisions.md, round 5, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 4642aaa788ffb5c9ea653b8f4aef48732233e0fb

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
