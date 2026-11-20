# Issue #6 plan review, round 2 (incremental), focuses: design-code and design-security

Scope: `git diff e0734f7..cb8f3c8 -- docs/issues/6/ ':!docs/issues/6/report.md'`, and only the code
and plans those lines depend on: `ublk.rs`, `control.rs`, `engine.rs`, `flaminio-node/tests/engine.rs`,
`flaminio-repl/src/sim`, libublk 0.4.8, #15 (e36882b), #17 (4154dfd), #74 (5a1423d).

## Earlier findings

- R1-S1 (High): fixed in the plan, by S6. The per-volume `ran` refuses only `claims.epoch <= ran`.
  The claims check (`engine.rs:1617`) runs first, so every epoch from a new `Meta.Attach` is
  above `ran`: CSI, failover back to this node, after a deposition (`engine.rs:1852` removes the
  target and keeps `ran`), and a volume created again (`service.go:461–465`). No lab script sends a
  token twice (`lab.sh:714–718`, `:825–827`, `:1907–1909`; `reconcile.sh:64`). S6 matches #15
  S11's mark: per volume, `<=`, raised at the start and not before `GetVolume`. #17 takes a new
  epoch for every maintenance start (its S4, S13). The simulator scenario fails with the bug and
  passes without it: the existing guard in `sim/mod.rs:818` lets a start through once the target
  is gone, and scripted runs start the controller at `cfg.chaos` (`:489`). See R2-C1 for where the
  check runs.
- R1-C1: fixed. Verified: `ctrl.rs:1107–1119`, `:2251`, `ublk_cmd.h:391`, and the generated
  `UBLK_F_NO_AUTO_PART_SCAN: u32 = 262144`.
- R1-C3: fixed. Two of the new tests are specified wrongly: R2-C2 and Nit 1.
- R1-S2: fixed.
- R1-C2: fixed. Direct mode is stated, but `partscan.sh` does not check it: R2-B1.
- R1-C4, R1-C5, R1-C6, R1-C7, R1-C8: fixed.
- R1-S3: fixed. The re-attach in the same check now meets S6: R2-B1.
- R1-S4, R1-S5: fixed.
- R1-C-N1, R1-C-N2, R1-C-N3, R1-S-N1: fixed.

### R2-C1 — Medium
**S6's bar lives in `flaminio-node`. The simulator checks a copy of it, and the injected bug
turns off only the copy.** The design puts `ran` in `Engine::start_target` (design.md:192–199).
The simulator gets its own per-node `ran` in its own `start_target` (design.md:217–220; today
`sim/mod.rs:815–840`, separate code from `engine.rs:1596–1683`). `sim::Bugs::restart_at_epoch`
skips "the node's refusal" (design.md:227), which is the simulator's copy. So no simulator run
exercises the comparison that ships. The simulator would not catch:
- `<` written for `<=`;
- a raise that is missing, or done before the claims check;
- a check placed after `targets.insert`.

Only the engine test would catch these. This goes against `AGENTS.md`'s Principles:
replication, membership and recovery logic lives in `flaminio-repl`, as sans-I/O state machines
the simulator checks, and a new safety rule gets an injected bug that the simulator must catch.
`ran` is a pure in-memory comparison, so nothing stops it living there.
**Fix:** make the bar a small sans-I/O type in `flaminio-repl`, for example
`RunMarks::admit(&mut self, volume, epoch) -> Result<(), Epoch>`, which checks and raises.
`Engine::start_target` and the simulator's `start_target` both call it. `restart_at_epoch` becomes
that type's test-only bug, as `target::Bugs` are. #15's durable mark can then load and store the
same type's state. Say so in "Later plans".

### R2-B1 — Low
**`partscan.sh` conflicts with S6, and has no positive control.** Evidence: design.md:165–177,
spec.md:178–183.
- **The token:** "detaches the volume, attaches it again" does not say to take a new token. After
  S6, sending the first attach's token again in the same run is refused
  (`epoch: a target of … already started`, design.md:192–199). A script that, like `resync.sh:34`,
  takes one token with `metactl attach` and reuses it would fail on every run.
- **No positive control:** nothing shows that the kernel would have created `p1` without the
  flag, so the check passes:
  - with a "prepared" table the kernel does not parse. With 4 KiB logical blocks, Linux scales the
    MBR's LBAs by 8;
  - when the scan has not finished by the time udev settles. Spec S2 says Linux 7.0 scans after
    `START_DEV` returns, and `udevadm settle` waits only for events already queued;
  - on a node that actually runs `--privsep`. Direct mode is stated, not checked.
- **An untested rule:** S1's "an explicit re-read (`blockdev --rereadpt`, `partprobe`) still does"
  (spec.md:15) has no test.

**Fix:**
- Attach the second time with a new `metactl attach` token.
- After the re-attach, poll for `p1` for a few seconds rather than checking once after the settle.
- Then run `blockdev --rereadpt` and require `/dev/ublkbNp1` to appear. That shows the table is
  valid and the node is in direct mode, and it tests S1's re-read clause. The `EXIT` trap still
  detaches and deletes the volume.

### R2-C2 — Low
**`take_attach`'s signature cannot call `attach_and_serve`.** design.md:265–268 gives
`take_attach(now, request: Control, serve: impl FnOnce(AttachRequest) -> Result<(), String>)`, with
`serve` meant to be `attach_and_serve` in the loop. But `attach_and_serve` needs the request's
`reply` sender, which `run_device` uses to send `ok /dev/ublkbN` (`ublk.rs:809`, `:888`, `:936–944`).
That sender is inside the `Control::Attach` that `take_attach` consumes (`ublk.rs:685–694`), and
`AttachRequest` has no `reply` field (`ublk.rs:824–830`). The loop's closure cannot capture it.
**Fix:** use `serve: impl FnOnce(AttachRequest, &mpsc::Sender<String>) -> Result<(), String>`, or
move `reply` into `AttachRequest`.

### R2-C3 — Low
**The new simulator check, scenario and injected bug are not planned into
`replication-protocol.md` §9.** §9 lists:
- the three things each run checks;
- the two scripted scenarios;
- the mutation table.

It also says "only a failure that breaks linearizability or convergence counts"
(`replication-protocol.md:350–392`). S6 adds:
- a fourth check, `two targets at epoch E`;
- a third scenario, `scenario_device_failure_then_retry`;
- an injected bug, `restart_at_epoch`, whose catch is of neither kind (design.md:217–230,
  :307–311).

The design plans only `docs/followups.md` §10 (design.md:111–119).
**Fix:** add a Docs bullet to S6. In §9, add the check, the scenario, and a row in the mutation
table ("A target restarts at its epoch on the same node (§3) | device-failure scenario"). Also
say that this scenario's catch is `two targets at epoch`.

## Nits

1. design.md:273–274, `the_steps_are_bounded_by_the_time_left`: "passed, zero and `now`". For
   a `by` already in the past, `(now + ADD_DEVICE_LIMIT).min(by)` (`ublk.rs:903`) returns `by`,
   not `now`. A faithful extraction would fail this test. Write "at `now`: zero and `now`", or
   state the clamp if one is intended.

Reviewed commit: 8fddde307a37fa7749dc3f4aaab5c2d844c6b0ca
