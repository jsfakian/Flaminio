# Issue #6 plan review, round 1, focus: design-code

Scope: docs/issues/6/{intent,spec,design,decisions}.md at HEAD. S2–S4 were merged in PR #25
(3b5560b, in HEAD), and the code matches the spec's rules: `control.rs:30-37,117-161`,
`ublk.rs:685-705,782-897,902-934`, `engine.rs:415-422,640-683,1452-1508,1856-1858`,
`dataplane.go:28-115`. Only S1 is still to do. The findings are drift in the plan, gaps in its
test list, and contradictions with #15 and #74.

### R1-C1 — Medium
**S1's blocker and its decision are contradicted by #74 and #15, which vendor libublk 0.4.8.**
#6 defers S1 because "we do not carry a patched copy of libublk" and waits for a libublk release.
#74's settled Q2 vendors libublk 0.4.8 in `dataplane/vendor/libublk/` (`[patch.crates-io]`),
maintained by this project. #15 vendors it too, whichever of the two lands first. With that copy,
the blocker is one hunk: add `UBLK_F_NO_AUTO_PART_SCAN` to `UBLK_DRV_F_ALL` (libublk 0.4.8
`ctrl.rs:1107-1119`, checked at `:2251`). The constant already exists in the bindings
(`libublk-rs-sys` `ublk_cmd.h:391`). The issue's "Done when" needs step 1, so the plan's reason
for keeping #6 open no longer holds.
Evidence: design.md:131-132 and :179-181 ("S1 waits for upstream"); intent.md:51-58; spec.md:5;
#74 decisions.md:28-33 and :49-52, design.md:227-228; #15 design.md:152-159.
Fix: choose one and record it in decisions.md:
- S1 lands as a hunk in the vendored copy once #74 or #15 brings it in, and #72 is still offered
  upstream; or
- S1 still waits for the release despite the copy, with the reason.
Then update the intent's Dependency section, design S1 and Decisions, and followups §10.

### R1-C2 — Low
**S1 is wrong, or unspecified, under #74's `--privsep`.** In that mode:
- the root helper adds the device with fixed flags (`ctrl_flags` 0, test `fixed_flags`);
- the I/O process never runs `add_device`;
- the kernel suppresses partition scanning for an unprivileged server and leaves the suppression
  on, so `blockdev --rereadpt` fails (#74's `partition_scan_suppressed` test).

So, under `--privsep`:
- S1's "An explicit re-read (`blockdev --rereadpt`, `partprobe`) still does" is false;
- the design's feature check in `add_device`, and its "the kernel scans the partitions" log, do
  not apply;
- `partscan.sh` passes whether the flag is set or not.

Evidence: spec.md:5-11; design.md:123-129; #74 spec.md:22 and :40, design.md:20, :57-58, :286
and :358-359.
Fix: limit S1 to direct mode. State that under `--privsep` the kernel already suppresses scans
and re-reads, so the helper adds no flag and nothing is logged. Run `partscan.sh` in direct mode.

### R1-C3 — Medium
**Several rules have no named test, and the test list has drifted from the code.**
No test, in the plan or in the code:
- the skip of a request past `deadline − START_LIMIT` (`ublk.rs:802-808`);
- the step bounds, `min(METADATA_LIMIT, steps_by − now)` (`ublk.rs:858`) and `add_device`'s
  `min(now + ADD_DEVICE_LIMIT, by)` (`ublk.rs:903`);
- the startup `--attach` retry: every 5 s, for 5 minutes, stopping once a volume is attached
  (`ublk.rs:731-769`);
- "already attaching" (`engine.rs:649-655`).

Tests the code has but the plan does not name:
- `an_attach_takes_the_callers_shorter_limit` and `an_attach_dropped_without_an_answer_says_so`
  (`control.rs:234-251`);
- `TestControlSocketTellsTheDataPlaneItsTimeLeft` and
  `TestControlSocketFallsBackForAnOlderDataPlane` (`dataplane_test.go:102-158`).

Where the plan and the code disagree:
- spec.md:70-72 says the Go test asserts that the default is above 50 s. It asserts only
  equality (`dataplane_test.go:87-100`).
- design.md:149-150 compares against a literal 60 s. The real check reads `dataplane.go`
  (`control.rs:256-296`), and also covers `DELETE_LIMIT`.

Fix: name the existing tests, and correct the two claims. Add unit tests for the rest:
- the skip and bound arithmetic, as pure functions of `deadline` and `now`;
- the startup retry, with its interval as a parameter (as #17's `startup_attempt` does);
- "already attaching", in `flaminio-node/tests/engine.rs`.

### R1-C4 — Low
**Design behaviour with no spec rule.** The spec does not mention:
- the startup `--attach` retry (5 s, 5 minutes, its stop conditions);
- "already attaching";
- the fallback to `attach VOLUME TOKEN` for an older data plane, and so the compatibility of
  the new fourth word;
- the "dropped the attach without an answer" reply (`control.rs:145-147`).

All of these are visible to callers or operators.
Evidence: design.md:58-63, :77-83; spec.md:33-42.
Fix: add these to S2 as rules, each with its test (R1-C3).

### R1-C5 — Low
**The design's worked example of the per-request deadline gets the arithmetic wrong.** It says a
NodeStage that spent 30 s in `Meta.Attach` "gets an attach bounded by the 20 s it has". In fact
90 s of kubelet's 2 minutes are left. The plugin sends `min(60 s, 90 s) − 1 s` = 59 s
(`dataplane.go:56-62`, `:108`), and the socket takes `min(59 s, 50 s)` = 50 s
(`control.rs:153-160`). Under kubelet's deadline with three endpoints, the fourth word never
cuts the limit below 50 s. It does so only when the caller has less than 51 s left.
Evidence: design.md:63-64.
Fix: correct the example, or say when the fourth word actually shortens the limit.

### R1-C6 — Low
**A stated failure mode of the generation check is no longer true.** design.md:157-158 says
that, without the generation check, the old answers "would start a target at E". Since #30,
`start_target` refuses a token whose epoch differs from the view's (`engine.rs:1610-1622`). The
new attach's E + 1 token with the old view at E therefore gives `AttachFailed` ("the token is
for v epoch 2 … the volume is at 1"), not a target at E. The test
(`flaminio-node/tests/engine.rs:1269-1300`) still fails without the check, because it asserts
no `AttachFailed`.
Fix: say that without the check the old view fails the new attach.

### R1-C7 — Low
**The spec and design still describe S2–S4 as future work.** The following are false at HEAD:
- spec.md:20-22 ("Today it waits up to 120 s");
- spec.md:47-49 ("Today such an answer starts a target…");
- design.md:31 ("Today it waits up to 60 s");
- design.md:107-108 ("Today nothing removes it");
- design.md:131 ("not in this pull request").

Only decisions.md mentions PR #25. Someone implementing S1 cannot tell from the spec or design
what has shipped.
Fix: add a status line to the spec and the design: S2–S4 merged in #25 (3b5560b), S1 open.
Keep the old values as "before #25".

### R1-C8 — Low
**Some of #6's statements are contradicted by #74 and #15, and #6 does not mention it.**
1. `START_LIMIT` "is not enforced", and a slow start is said to leave a running device that a
   retry finds through `status` (design.md:49-51, :74-76). Under `--privsep`, a `start` reply
   later than `START_LIMIT` ends the I/O process (#74 design.md:165-168). #15 also spends
   `START_LIMIT` again on the udev wait in the ready callback, and stops the device when it runs
   out (#15 design.md:165, :211).
2. The startup retry runs "while the node has no other volume" (design.md:77-81). Under #15 it
   runs until *that* volume has an entry or an epoch refusal; another volume's entry does not
   stop it (#15 spec.md:174-175, design.md:726-727).
3. Adding the device is "retried while a previous device with the same id goes away … unchanged"
   (spec.md:26-27, design.md:34-35). #15 removes the retry at the same id and walks ids instead
   (#15 design.md:125-152).

Fix: a short "Later plans" note in the design that names these three changes, so that S1's
`add_device` work starts from #15's and #74's structure.

## Nits
- spec.md:7-8 and design.md:124-125: "logs once" does not say once per what. Name the mechanism:
  once per process (a `static` `Once`), not per add. Under #15 there are many adds.
- spec.md:89-92 and design.md:126-129: the S1 lab check depends on "while the kernel advertises
  the flag", but neither says how the script learns this (the node's log line, for example), and
  the design's `partscan.sh` drops the condition.
- spec.md:86-88 and design.md:163: "A timed attach, as in the design's measurements" is not
  runnable as written: no script or commands are named. Name them, or point to `reconcile.sh`'s
  timing.

Reviewed commit: e0734f703ee3d372afec9314054b917658e2b06c
