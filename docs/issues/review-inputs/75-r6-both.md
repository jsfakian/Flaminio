# Issue #75 plan review: round 6, design-code and design-security (incremental)

Scope: `git diff 40cdf3f..00e57e8 -- docs/issues/75/ ':!docs/issues/75/report.md'` (the round-5
fix to design.md and decisions.md), checked in `/tmp/flaminio-plans-2026-11-12/75` against that
worktree's code. HEAD e907b39 adds only report.md.

## Earlier findings

- r75c5 #1: fixed. `meta.sh running FLAG` reads the process, and `flags` dies on a unit without
  the line. Two gaps remain: the check's timing (R6-B1) and `flags`' own check (R6-B3).
- r75c5 #2: fixed (design.md:221-222; the Makefile help at design.md:270-273).
- r75c5 #3: fixed (design.md:266-268). The steps are in an unsafe order: R6-B5.
- r75c5 #4: fixed. The limit is stated (design.md:243-244).
- r75c5 nit 5: fixed (decisions.md:114-115).
- R5-S1: fixed in design.md:288-294. The condition is exact: `Target` is set only by `Attach`
  and is never cleared (`meta/domain/domain.go:217-220`), so "no target" means never attached.
  decisions.md still records the broad remedy: R6-B6.
- R5-S2: fixed in the mechanism. `qos-check` reads the cmdline, and the mixed cluster left by
  chaos or a partial `meta up` fails it. The test case R5-S2 asked for (`VM=a` with another VM
  differing) was not added: R6-B4.
- R5-S3: fixed. #70 no longer has `meta-up VOLUME_EXPANSION=1`. This revision opens a new
  divergence with #70: R6-B2.
- R5 nit 1: fixed (`readHeader`, service.go:275-291, cited correctly). Some wording is left:
  Nit 2.

### R6-B1 — Medium

**`meta.sh running` reads the service once. Right after a restart that read can fail on a
healthy VM. Against a crash-looping binary it can pass. So "all passing proves every instance is
new enough" does not hold as written.**

Evidence:
- The unit has no `Type=`, so it is `simple` (`scripts/guest/meta.sh:20-44`). For a simple
  service, `systemctl restart` returns once the process is forked, and the unit is already
  `active` with that `MainPID`.
- Before `flaminio-meta` is exec'd, systemd sets up the sandbox:
  - `DynamicUser`;
  - seven `LoadCredential`s;
  - `ProtectSystem=strict`, `PrivateTmp` and `PrivateDevices`;
  - `SystemCallFilter` (`meta.sh:21-44`).

  On the lab's Ubuntu 26.04 (`docs/lab.md:17`), this runs in systemd's executor process, under
  the same pid.
- `meta.sh flags` runs the check immediately after its restart (design.md:224). That
  `/proc/<MainPID>/cmdline` can still be the executor's, so `flags` dies on a healthy VM. It has
  already written the file there and not yet visited the later VMs.
- The reverse case: an older binary given `--qos` exits in `fs.Parse` (`flag.ExitOnError`,
  `meta/cmd/flaminio-meta/main.go:84,99`). A current binary can also exit after its flags are
  parsed, for example on `net.Listen` (`main.go:147-150`). With `Restart=on-failure` and
  `RestartSec=2` (`meta.sh:34-35`), either one loops. In each cycle it is `active`, with the flag
  in its cmdline, for the few milliseconds before it exits. A `qos-check` read in that window
  passes.

  This is how an older instance gets `--qos`: #70 lands the mechanism first, and a #70-era
  `meta up` runs while the file holds `--qos`. Then the claim at design.md:229-231 and
  decisions.md:153-154 fails.
- The test cases (design.md:238-239) read a static fake `PROC`, so they cannot show any of this.

Fix:
- `running` passes only when the service is `active` with the same `MainPID` and an unchanged
  `NRestarts` (`systemctl show -p MainPID,NRestarts,ActiveState`), and the flag is in that pid's
  cmdline, at two reads more than `RestartSec` apart (3 s).
- `flags` retries this for a bounded time, say 15 s, before it dies.
- Optionally, `meta.sh up` writes `Type=exec`, so the restart returns only after the exec.
- Test cases:
  - a fake `systemctl` whose `MainPID` changes between the reads fails;
  - one whose `NRestarts` grows between the reads fails;
  - a cmdline that is not yet the binary's, and becomes it within the bound, passes in `flags`.

### R6-B2 — Low

**#70's plan still has its lab check read the flags file "as `qos-check` does". This revision
changes the shared section to say that the file alone proves nothing.**

Evidence:
- design.md:226-228: "The file alone proves nothing". `qos-check` now runs `running --qos`.
- `/tmp/flaminio-plans-2026-11-12/70` at 2c8856c points to #75's section as of 40cdf3f
  (`docs/issues/70/design.md:90-91`; `decisions.md:157-158`).
- #70's `csi_expand` "reads `/etc/flaminio/meta-flags` on every VM ... as #75's `qos-check`
  does" (`docs/issues/70/design.md:208-210`; `decisions.md:159-160`). Its test case refuses
  "when one VM's file lacks the flag" (`docs/issues/70/design.md:202`).
- decisions.md:163-164 calls R5-S3 handled, with "no change here". Yet this round changes the
  section #70 depends on. Whichever plan lands first, the two describe different checks for the
  same mechanism.

Fix:
- Record in decisions.md (and in the shared section) that every lab check of a flag uses
  `meta.sh running FLAG` on every cluster VM. `csi_expand` runs `running --volume-expansion`.
- Have #70 update its 40cdf3f pin, its `csi_expand` text and its test case ("refused when one
  VM's `running --volume-expansion` fails").

### R6-B3 — Low

**`meta.sh flags`' check after its restart is not specified for a flag being cleared, and no
test covers the check at all.**

Evidence:
- design.md:222-225: `flags` "runs the check below on itself". That check, `running FLAG`
  (design.md:226-228), tests that a flag is present. Neither text covers `QOS=0`, or
  `QOS=1 VOLUME_EXPANSION=0` given together. Checking `running --qos` after a clear fails by
  design. Checking nothing leaves a failed restart, the case design.md:228 names, undetected.
- The `flags` test cases (design.md:237-238) are: sets, clears, keeps, and "dies before writing
  on a unit without the line". None has the restarted process without the set flag. An
  implementation that never calls the check passes them all.

Fix:
- Specify it per given variable: `1` means `running` passes; `0` means the service is active and
  its cmdline lacks the flag.
- Add two cases:
  - a fake cmdline without the set flag after `flags QOS=1`: exits non-zero;
  - a fake cmdline still holding `--qos` after `flags QOS=0`: exits non-zero.

### R6-B4 — Low

**The two "every cluster VM" test cases cannot tell every cluster VM from a smaller set.**

Evidence:
- `vms_of all` is `a b`; `cluster` is `a b w` (`scripts/lib.sh:44-45`). `qos-check`'s `NODE` is
  fl-a or fl-b (design.md:248).
- "`QOS=1` refused ... with one VM's binary differing" (design.md:241-242) does not say which VM
  is selected or which differs. An implementation that compares only the selected VMs, or only
  `all`, passes when the differing VM is among them. R5-S2 asked for `VM=a` with another VM
  differing; that case was not added.
- "the precondition refused when one VM's `running --qos` fails" (design.md:242-243) does not
  name the VM. An implementation that checks only `NODE`, or `all`, passes unless that VM is w.

Fix:
- `make meta-flags QOS=1 VM=a` with only w's binary differing: refused, and no file written on a.
- The `qos-check` precondition with only w failing `running --qos`: refused.

### R6-B5 — Low

**The lab rollback deletes the limited volumes while the switch is still on.**

Evidence:
- design.md:267-268: "delete the limited volumes, then `make meta-flags QOS=0`". Until the
  second step, the current CSI controller and the metadata service still create limited volumes.
  A Pending or new PVC of a QoS class gets one; external-provisioner retries Pending PVCs.
- The older instances then serve such a volume unlimited and erase its policy
  (design.md:56-58), which is the case this warning exists to prevent.
- Turning the switch off first loses nothing. The `GetVolume` check and the returned `qos` do
  not depend on the switch (design.md:50), so existing limited volumes are still served with
  their limits.

Fix: order it as follows.
1. `make meta-flags QOS=0`.
2. Delete the QoS StorageClasses and the limited volumes.
3. Check out the older tree.

### R6-B6 — Low

**decisions.md still records round 4's broad remedy as a decision, without a mark.**

Evidence:
- decisions.md:145-146: "the leftover remedy deletes a `pvc-<uid>` volume that has no PV (R4
  nit 2)". This is the remedy R5-S1 showed to delete retained volumes and to race provisions in
  flight.
- Round 5 drops it (decisions.md:156-157), but only round 3's superseded line was marked
  (decisions.md:114-115).
- decisions.md is the settled record. An implementer writing docs/lab.md's "leftovers of
  Compatibility" (design.md:268) can take it from there.

Fix: append "(Replaced in round 5: no remedy; Compatibility states the identifying condition.)"

## Nits

1. design.md:235-236: "its two paths overridable (`META_UNIT`, `META_FLAGS_FILE`, `PROC`)"
   lists three.
2. design.md:51-53: "anything else ... gets `FailedPrecondition`" also covers `readHeader`'s
   etcd and decode errors (`service.go:276-284`). An implementation that follows it literally
   reports an etcd outage as the switch being off. Say instead: `NotFound` (absent, being
   created, or being deleted) gets `FailedPrecondition`; any other error is returned.

Reviewed commit: e907b39f5277329aeb6ec3f3e8668b80cad32377
