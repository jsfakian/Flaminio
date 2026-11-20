# Issue #75 plan review: round 3, design-security focus (incremental)

Scope: `git diff 83ae6dd..1a0a7ed -- docs/issues/75/ ':!docs/issues/75/report.md'` (de5d5ac, the
round-2 fix; 1a0a7ed, the approval), read in `/tmp/flaminio-plans-2026-11-12/75` against that
worktree's code.

Checked and found to hold:
- The CSI controller's new delete cannot remove a volume it did not create. `OK` comes only from
  the call whose claim compares the header at revision 0 (`service.go:414-433`). A retry, a second
  controller or a lost answer gets `AlreadyExists` or `NotFound`, and those never delete. While
  the stored policy lacks `qos`, every `AlreadyExists` comparison fails, so no concurrent call can
  adopt the volume before the delete.
- The burst clamp has no caller outside tests. Only `ublk.rs:666` builds `Input::Io`. Reads and
  writes larger than the buffer are refused with `EINVAL` (`ublk.rs:1280,1455`). The control socket
  sends no I/O (`control.rs:176,198`).
- Held requests after deposition: `drain_one`'s branch (`engine.rs:1841-1853`) runs before
  `targets.remove`. Their `Done` outputs reach the frontend before it kills the device
  (`ublk.rs:615-629`).
- The bound's figures (depth 128, one queue, 512 KiB, `--queues` 1-64; `node.rs:44-54,135,201-204`)
  hold. The engine does not know the depth or the queue count, as decisions.md says.
- `Attach` writes back the decoded header (`service.go:722-725`), and so do create and the
  deletion mark.
- #70 uses the same default-off flag pattern (`70/design.md:73,165`).

## Earlier findings

- R2-S1: fixed. Limited creates are refused until `--qos` is on. A dropped policy is deleted.
  Mixed instances are tested. The rollback residual is documented; see R3-S1 and R3-S2.
- R2-S2: fixed for the direct answer. A lost answer still leaves an orphan; see R3-S5.
- R2-S3: fixed. The bound is restated in spec S4, the design and the ADR, and the
  `hung_task_panic` requirement is documented.
- R2-C1: fixed (the clamp and `a_cost_above_the_burst_is_admitted_at_a_full_bucket`). See R3-S4
  on its wording and test.
- R2-C2: fixed. The refusal after `Attach` is stated as a limit, with
  `a_refused_volume_read_fails_the_attach`.
- R2-C3: fixed (`covers`, `covers_never_debits`).
- R2-C4: fixed. The call site is named; `release_held` drops a gone name; the test pumps afterwards.
- R2-C5: fixed. The timer starts at t0, before `Input::Attach`, and the k-th write may complete
  no sooner than t0 + k × 0.5 s.
- R2-C6: fixed (`uname -r`, the same device, an empty kernel list in the ADR).
- R2-C nit 1: rejected with a reason in decisions.md. The remaining text names #73 only to say
  that it plans no ceilings, which is acceptable.
- R2-C nits 2 and 3: fixed.

### R3-S1 — Low

**The switch's preconditions are only documented, and the rollback text understates what
breaking them does.**

Evidence:
- Nothing checks "every instance runs this version" when the switch goes on, or keeps an older
  instance out later (design.md:48-53, spec.md:58-60, adr-proposal.md:30-33). The lab's unit sets
  `--qos` (`scripts/guest/meta.sh:28`) on each VM in turn as it is upgraded (`scripts/lab.sh:514-519`).
  That is the template a production rollout would copy, and it turns the switch on during the
  rolling upgrade, not after it.
- The text ties "unsupported" to the switch, but the hazard is that a limited volume exists.
  Turning the switch off only refuses new creates, so it does not make a rollback safe.
- The text does not say the loss is permanent and silent. An older instance's `Attach`
  (`service.go:722-725`) drops `qos`, and upgrading again cannot restore it: the policy is
  immutable, no admin path sets it, the CSI compares only at create, and the refresh ignores
  `qos` (design.md:97).

Fix:
- In the switch bullet, the ADR and `docs/lab.md`, state:
  - Nothing enforces the order.
  - Once any limited volume exists, any older metadata instance serves it unlimited and erases
    its policy for good on its next attach, whatever the switch.
  - The only safe rollback deletes the limited volumes first.
- Add that an older binary exits on the unknown `--qos` flag (`meta/cmd/flaminio-meta/main.go:85`,
  `flag.ExitOnError`), so keeping `--qos` in the unit makes a binary-only rollback fail closed.
- Tell operators to add the flag in a second rollout, after every instance runs this version.

### R3-S2 — Low

**Nothing says or tests that GetVolume's refusal and the returned policy apply when the switch
is off.**

Evidence:
- The switch defaults to off (design.md:48-50). A metadata instance added with default flags, or
  restarted without the flag, serves the existing limited volumes.
- The ADR's "the metadata service refuses limited volumes until its `--qos` switch is on"
  (adr-proposal.md:30) reads as if the switch gates more than create.
- `TestGetVolumeQoSNeedsAnEnforcingDataPlane` (design.md:177) needs a limited volume, which only a
  service with the switch on can create (design.md:173).
- So an implementation that skips the check, or leaves `qos` out of `volumePB`, while the switch
  is off passes every listed test. That instance then serves limited volumes unlimited to older
  data planes, and also to new ones if `qos` is left out.

Fix:
- State that the switch gates `CreateVolume` only, and say "refuses limited creates" in the ADR.
- Extend the test: create with the switch on, then check that a service on the same store with
  the switch off still refuses an undeclared `io` caller and still returns `qos`.

### R3-S3 — Low

**`metactl create` with a policy, sent to an older instance, makes an unlimited volume and
exits 0.**

Evidence:
- design.md:67-68 says "Before the switch every limited create fails". The CSI controller checks
  the returned policy, but metactl only refuses bad input (design.md:64-66).
- An older instance ignores tag 40 and answers `OK`.
- metactl prints the response with unpopulated fields left out (`main.go:501`), so nothing shows
  that the policy was dropped.

Fix: metactl compares the returned policy with the one requested, as the CSI controller does,
and fails with a message naming the volume (deleting it, or telling the admin to). Test this
beside `TestControllerQoSMixedInstances`.

### R3-S4 — Low

**The clamp's wording and the bound's test would let a clamp that under-charges pass.**

Evidence:
- design.md:82-83 says "each clamped to its bucket's burst". The Bucket has no burst field, only
  `cap = burst × 10⁶`, where a burst of 0 means the rate (design.md:79).
- Clamped against the policy field instead, the clamp is `min(cost, 0)` under the default burst
  of 0, and that dimension becomes free.
- S2 now bounds what is charged, not what is admitted (spec.md:38-40). Under that bug,
  `charges_never_exceed_rate_times_window_plus_burst` (design.md:174) passes trivially.
- No listed unit test names the bytes dimension at a burst of 0. The lab check (1 MiB/s, burst 0)
  would be the first to catch it.

Fix:
- Write "clamped to the bucket's cap (one second of the rate when the burst is 0)".
- State S2's bound on the requests and bytes admitted, for costs no larger than the cap.
- Make the property test sum the real counts and lengths of the admitted requests, with policies
  whose burst is 0 among its cases.

### R3-S5 — Low

**A create whose answer is lost still leaves an orphan while instances are mixed.**

Evidence:
- `metaCall` retries `CreateVolume` on the next instance after `DeadlineExceeded` or `Unavailable`
  (`control/csi/meta.go:72-79`, 10 s a try).
- Suppose the first try committed on an older instance, which dropped `qos`. The retry answers
  `AlreadyExists`, the comparison fails on `qos`, and by design nothing is deleted
  (design.md:62-63). The PVC stays Pending, and once it is deleted nothing deletes the volume.
  Before this change, such a retry matched and succeeded.
- design.md:216-217 names only the failed delete as a leftover.
  `TestControllerQoSMixedInstances` checks "no volume is left" only for the direct answer.

Fix: list this leftover in Compatibility and in the upgrade note in `docs/lab.md`: an admin
deletes volumes that `metactl volumes` shows without a PVC. A tenant cannot repeat it, so stating
it is enough.

### R3-S6 — Nit

**Q1 still reads as open after the approval.**

spec.md:84-87 and design.md:218-219 still call Q1's semantics Proposed and open.
adr-proposal.md:47 says Q1 "stays open until the design is approved". adr-proposal.md:5 says
"no ... approval is assigned", beside line 3's "Accepted". decisions.md settles all of these.

Fix: mark Q1 settled in all three places, and reword line 5.

### R3-S7 — Nit

**intent.md:39 says "an older data plane then refuses one".** It is the other way round: the
metadata service refuses the older data plane (design.md:54-55).

### R3-S8 — Nit

**"A node serving limited volumes must not set `kernel.hung_task_panic=1`" covers too few nodes**
(adr-proposal.md:55-56, design.md:200-201). The target moves at attach, failover and reschedule
(design.md:70-72), so the rule must cover every node that may attach a limited volume.

Reviewed commit: 1a0a7ed3f5b9aeb1964d18f300afea9b2b848f01
