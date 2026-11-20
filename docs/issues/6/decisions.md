# Issue #6 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fix, made after approval: see below.
- Plan review round 5 (2026-11-14) fix, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Work on this issue merged before this approval was recorded (pull request #25); the issue is
still open. Approved before its plan review cleared (status: pending).

## Settled at approval

None: `spec.md` and `design.md` have no Open questions section, no numbered question and no
choice labelled Proposed.

## Plan review round 1 (2026-11-14): changes made after approval

Round 1 of the 2026-11-12 batch's plan review (reviewed commit e0734f7; reviewers plan-reviewer
(design-code) and plan-reviewer (design-security)). Each change below was made after approval.
Where a finding offered options, the simplest safe one was taken. No change narrows the intent
or a spec rule.

- **R1-C1, R1-S5 (one root cause: the vendored libublk):** S1 no longer waits for a libublk
  release. It is one hunk in the vendored libublk 0.4.8 that #74 and #15 bring in, whichever
  lands first (`UBLK_F_NO_AUTO_PART_SCAN` joins `UBLK_DRV_F_ALL`), and lands with or after that
  plan; ublk-org/libublk-rs#72 stays offered upstream. S1's flag goes into #15's rewritten
  `add_device`, or today's if S1 lands first. Of R1-C1's two options this is the one that
  follows #74's and #15's approved, review-clear plans: the copy will exist and be maintained
  here, so the earlier reason (no patched copy) no longer holds. Intent, Dependency; spec, S1;
  design, S1, Follow-ups and Decisions.
- **R1-C2:** S1 is direct mode's. Under #74's `--privsep` the helper adds the device with fixed
  flags and the kernel already suppresses partition scans, re-reads included, so the node adds
  no flag and logs nothing; `partscan.sh` runs with `PRIVSEP=0`. Spec, S1; design, S1.
- **R1-S1 (High): the startup retry could start a second target at one epoch.** A defect of the
  shipped code (#25, on main). New spec rule S6: an attach at an epoch at or below the highest
  epoch at which a target of that volume started in this run is refused (`epoch: …`), and the
  startup retry stops at that refusal. Of the two choices the orchestrator gave, a refusal and
  not a fresh token: the data plane cannot take a new epoch itself without a new metadata call
  path and its own `auth.go` rule, while the refusal is a map and one check. The check is in
  `start_target`, where every target starts, not only in the retry thread (R1-S1's example, a
  typed error from `attach_and_serve`): it covers every attach in the run, the control socket's
  with an old token included, and it is what the simulator models and what #15's run mark makes
  durable. So R1-S1's test "no second Attach is sent" becomes "one refused attach, then the
  retry stops". Tests: the engine's `a_target_never_starts_twice_at_one_epoch`, the retry's
  `the_startup_attach_stops_when_it_must`, and a simulator check (`two targets at epoch E`, the
  scripted step `DeviceFails`) with the injected bug `sim::Bugs::restart_at_epoch`. Until #15,
  the bar lasts one run of the process: a node restarted with the old token can still start a
  target at that epoch, as before #25; #15's run mark closes that. Spec, S2 and S6; design, S6,
  Tests, Decisions, Risks.
- **R1-C3, R1-S2 (one root cause: rules without tests):** the spec and design name the tests on
  main (`an_attach_takes_the_callers_shorter_limit`, `an_attach_dropped_without_an_answer_says_so`,
  `TestControlSocketTellsTheDataPlaneItsTimeLeft`, `TestControlSocketFallsBackForAnOlderDataPlane`
  and the others), and correct two claims: the Go default test asserts equality only, and the
  Rust limits test reads `DefaultControlTimeout` from `dataplane.go` and covers `DELETE_LIMIT`.
  New tests: the main loop's decision to begin a request moves into `take_attach`, tested so that
  a request past its steps is answered and never begun (no `Input::Attach`); the step bounds as
  pure functions; the startup retry with its interval and limit as parameters; and "already
  attaching" in the engine tests. Spec, Tests; design, Tests.
- **R1-C4:** S2 states the startup retry (5 s, 5 minutes, its stops), "already attaching", the
  fallback to `attach VOLUME TOKEN` for a data plane older than #25, and the "dropped the attach
  without an answer" reply, each with its test (R1-C3). Spec, S2.
- **R1-C5:** the worked example is corrected: with 90 s of kubelet's 2 minutes left, the plugin
  sends 59 s and the socket takes 50 s. The fourth word cuts the limit only for a caller with
  less than 51 s left. Design, Time limits.
- **R1-C6 and R1-S-N1 (the security Nit; one root cause: the generation check's reason):**
  without the generation, a late view at the old epoch would fail the new attach
  (`start_target` refuses a token whose epoch is not the view's, since #30), not start a target
  at the old epoch. The generation keeps a late answer from failing or filling in the next
  attach; the token check bars an old-epoch target. Design, S3 and Tests.
- **R1-C7:** a status line in the spec and the design: S2–S4 shipped in #25 (3b5560b), S1 and S6
  open; the replaced values are marked "before #25", and the code is described as on main.
- **R1-C8:** a "Later plans" section in the design states each change #74 and #15 make to this
  design's statements: `START_LIMIT` enforced under `--privsep` and spent again on #15's udev
  wait; the startup retry's stop per volume under #15; #15's id walk in place of the retry at
  the same id; and #15's run mark and error form in place of S6's map and answer.
- **R1-S3:** `partscan.sh` works on a scratch volume it creates and deletes from an `EXIT` trap,
  so no volume with data is overwritten and nothing needs restoring (the simplest of R1-S3's
  options: the save and restore are cut, not guarded); its write uses `dd conv=fsync`. Spec,
  Tests; design, S1.
- **R1-S4:** the intent's Out of scope names whole-device probing (LVM event activation, md
  incremental assembly on a raw block volume's header) as a follow-up, not filed yet: a udev rule
  for `ublkb*` and an LVM `global_filter`. #74's settled Q4 leaves the same residual to a later
  plan of #74.
- **R1-C-N1:** S1 logs once per process (a `static` `OnceLock`, filled at the first add), not
  once per add. Spec, S1; design, S1.
- **R1-C-N2:** `partscan.sh` learns whether the kernel advertises the flag from the node's log of
  its current run (S1's line), and says so and fails if it does not. Spec, Tests; design, S1.
- **R1-C-N3:** the timed attach is `reconcile.sh`'s own timing (its `device … ready` line), under
  a second with nothing failing (S5). The stopped-replica runs stay measurements, not checks: S5
  covers an attach with nothing failing. Spec, Tests; design, Tests.

## Plan review round 2 (2026-11-14): changes made after approval

Round 2 of the 2026-11-12 batch's plan review (reviewed commit 8fddde3; reviewer plan-reviewer
(design-code and design-security, one agent)). Each change below was made after approval, with
the reviewer's recommended fix. No change narrows the intent or a spec rule.

- **R2-C1 (Medium):** S6's comparison moves into a sans-I/O type in `flaminio-repl`,
  `RunEpochs<K>` (`run_epochs.rs`, new), whose `admit(volume, epoch)` refuses an epoch at or
  below the volume's highest and otherwise raises it. `Engine::start_target` and the simulator's
  `start_target` both call it. The injected bug `restart_at_epoch` becomes that type's test-only
  `Bugs`, which `sim::Bugs::run_epochs` sets, so the simulator breaks the code that ships rather
  than a copy. A new "Files touched" section names it; "Later plans" says #15's durable mark can
  load and store the same type's state. Design, S6, Tests, Files touched, Later plans,
  Decisions; spec, Tests.
- **R2-B1 (Low):** `partscan.sh` takes a new token with `metactl attach` before it attaches the
  volume again (S6 refuses the first one in the same run), polls for `p1` for 5 s rather than
  checking once after `udevadm settle`, and then requires `blockdev --rereadpt` to create `p1`.
  That positive control shows that the kernel parses the table and that the node runs in direct
  mode, and it tests S1's re-read clause. Design, S1; spec, Tests.
- **R2-C2 (Low):** `take_attach`'s `serve` takes the reply sender too:
  `FnOnce(AttachRequest, &mpsc::Sender<String>)`. Of the two options, passing the sender keeps
  `AttachRequest` as it is, and `take_attach` still holds the sender to answer `serve`'s error;
  moved into the request, it would go with it. Design, Tests.
- **R2-C3 (Low):** S6 gets a Docs bullet for `docs/replication-protocol.md` §9: the fourth check
  (`two targets at epoch E`), the third scenario, a row in the mutation table, and a sentence
  that this scenario's catch is `two targets at epoch`, neither linearizability nor convergence.
  Design, S6.
- **R2-N1 (Nit):** the bounds test expects `steps_by` itself, not `now`, for adding the device
  once `steps_by` has passed, as `(now + ADD_DEVICE_LIMIT).min(by)` returns `by`. Design, Tests.

## Plan review round 3 (2026-11-14): changes made after approval

Round 3 of the 2026-11-12 batch's plan review (reviewed commit 8fde090; reviewer plan-reviewer
(design-code and design-security, one agent)). Each change below was made after approval, with
the reviewer's recommended fix. No change narrows the intent or a spec rule.

- **R3-B1 (Low):** `partscan.sh`'s positive control runs `udevadm settle`, then retries
  `blockdev --rereadpt` for 5 s, as the re-read fails with `EBUSY` while udev's probe holds the
  device. Its failure names the likely causes: "the re-read created no p1: the table is invalid,
  the node runs --privsep, or the device was busy". The control no longer claims to show direct
  mode. Until #74 lands, the lab runs only direct mode (no script has a `--privsep` setting, so
  the `PRIVSEP=0` the plan named is dropped); once #74 lands, the check reads `--privsep` from
  the node unit's command line and fails up front, naming the mode. Design, S1; spec, Tests.
- **R3-N1 (Nit):** S6's Docs bullet for `docs/replication-protocol.md` §9 also rewords the
  scenarios' intro (three scenarios, not only the two split-brain cases) and the mutation
  checks' counting rule (`two targets at epoch` counts for that scenario's bug). Design, S6.

## Plan review round 4 (2026-11-14): changes made after approval

Round 4 of this batch's plan review (reviewed commit `8fb5882`; reviewer: plan-reviewer
(design-code and design-security, one agent)).

- **R4-N1 (Nit):** once #74 lands, `partscan.sh` matches `--privsep` in the node unit's
  `ExecStart` as a whole word (`grep -qw -- --privsep`); the flag name is confirmed when #74 lands.

## Plan review round 5 (2026-11-14): changes made after approval

Round 5 of this batch's plan review (reviewed commit `f9974eb`; reviewer: plan-reviewer
(design-code and design-security, one agent)).

- **R5-N1 (Nit):** `grep -w` treats `-` as a non-word character, so `--privsep=0` or
  `--privsep-x` would also match. The check now matches an argument that is exactly `--privsep`:
  `grep -qE -- '(^| )--privsep( |;|$)'` on `ExecStart`'s value.
