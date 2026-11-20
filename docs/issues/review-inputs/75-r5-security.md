# Issue #75 plan review: round 5, design-security focus (incremental)

Scope: `git diff dc80bd5..40cdf3f -- docs/issues/75/ ':!docs/issues/75/report.md'` (the round-4
fix), read in `/tmp/flaminio-plans-2026-11-12/75` against that worktree's code.

## Earlier findings

- R4-S1: fixed at `make meta-flags`: values are checked for 0/1 in lab.sh before `ENV_VARS`, the
  sha256 gate covers every cluster VM, and `meta up` keeps the file. Later `meta up`s get around
  the gate, and `qos-check`'s precondition still has no test: see R5-S2.
- R4-S2: fixed. `AlreadyExists` comes first, then `FailedPrecondition`, and the case is in
  `TestCreateVolumeQoSOff`. It exposes nothing new. `CreateVolume` is admin-only
  (`auth.go:65-66`). `GetVolume` and `ListVolumes` are open to every authenticated caller
  (`auth.go:79-80`). The match is the CSI controller's existing comparison
  (`controller.go:239-247`).
- R4-S3: fixed: the usage text, `main.go`'s doc, and the `meta-flags` and `meta-up` help lines.
- R4 nit 1: fixed (spec S5, intent item 4).
- R4 nit 2: replaced, but the new remedy is too broad: see R5-S1.
- R4 nit 3: fixed (design.md:215-217).
- r75c4 #1: as R4-S1. r75c4 #2: fixed. r75c4 #3: fixed. `TestCreateQoSArgument` was added, and
  the fixture's interceptor chain (`main_test.go:78-84`) can carry the `qos`-dropping interceptor.

### R5-S1 — Medium

**The leftover remedy deletes every `pvc-<uid>` volume that has no PV, which can include a volume
someone still wants. It also promises a re-create that a tombstone blocks.**

Evidence:
- design.md:277-278 (Risks, Compatibility). Docs (design.md:252-256) put it in `docs/lab.md`.
- Volume ids are the provisioner's `pvc-<PVC uid>`; no prefix is set (`deploy/csi/driver.yaml:67-68`).
  "No PV" also matches two volumes that are not leftovers:
  - A volume kept on purpose after its PV was deleted. Patching a PV to `Retain` is the usual way
    to keep data, and the class's `reclaimPolicy: Delete` (`driver.yaml:233`) does not stop it.
    Deleting that volume is data loss.
  - A provision in flight: `CreateVolume` has returned but the provisioner has not written the PV
    yet. The PV is then created and bound to a deleted volume.
- "A Pending PVC's next retry then creates it" does not hold. The delete leaves a tombstone, and
  `createVolume` refuses the id until every node confirms (`service.go:372-377`;
  `docs/lab.md:207-213`). A chunk on fl-c waits for a manual `vol-confirm`. With the switch off,
  the retry is also refused (`FailedPrecondition`).

Fix:
- Tie the remedy to the PVC. For each Pending PVC of a QoS class whose events show the
  controller's lost-policy or "exists with other properties" error, delete only
  `pvc-<that PVC's uid>`.
- Before deleting, check that the volume has no PV and no target (never attached).
- Say that the PVC binds once the tombstone clears (the sweeps, or `vol-confirm` for fl-c) and
  the switch is on.

### R5-S2 — Low

**The build check guards only `meta-flags`. The kept file turns `--qos` back on at any later
`meta up` without the check, and `qos-check` trusts the file.**

Evidence:
- design.md:211-213: every caller of `meta up` keeps the flags. design.md:217-222: the sha256
  comparison runs only when `meta-flags` turns a flag on. design.md:223-225: "the file is what
  the process runs with".
- `cmd_meta up` builds the current checkout and installs it VM by VM on the selected VMs
  (`scripts/lab.sh:509-520`). The chaos run restarts w alone (`lab.sh:1407`).
- A shared-lab sequence that gets around the gate:
  1. Flags are set at build N.
  2. A checkout from before #75 runs `make meta-up`. Its `meta.sh` writes a unit with no
     `EnvironmentFile` (`scripts/guest/meta.sh:14-48`) and installs the old binary. The file
     stays.
  3. The current checkout runs `make meta-up VM=w` or `make chaos`, or is part-way through a full
     `make meta-up`. w then runs N with `--qos` while a and b run the older build. This is
     R3-S1's mixed state, reached with no comparison.
  4. `make qos-check` finds `FLAGS=--qos` in all three files and goes ahead. An older instance's
     `Attach` can then erase its volume's policy. The effect is lab-only: the check fails in a
     confusing way.
- Tests: `meta-flags-test.sh` has no `VM=a` case with b's binary differing. An implementation
  that compares only the selected VMs passes the listed case. There is also still no test of
  `qos-check`'s precondition, which R4-S1 asked for.

Fix:
- `qos-check` runs the same sha256 comparison on every cluster VM. It reads the running
  process's arguments (`/proc/<MainPID>/cmdline`) instead of the file.
- Pick one of these for `cmd_meta up`:
  - clear the flags file on any VM whose binary it changes, and keep it when the binary is
    unchanged (chaos, or `cluster-up` at the same build);
  - while a flag is on, refuse to install on a subset of VMs unless the other cluster VMs match
    the build.
- Say this in `docs/lab.md`, and add the two test cases.

### R5-S3 — Low

**#70's plan contradicts the "one mechanism shared with #70".**

Evidence:
- design.md:203-207 says #70 uses this mechanism and "the other adds its flag and variable".
- #70's plan at 294ded6 still has `meta.sh` writing `--volume-expansion` only with
  `VOLUME_EXPANSION=1`, and `make meta-up VOLUME_EXPANSION=1` as the second rollout. Its lab check
  reads the unit (`docs/issues/70/design.md:78-80,196-198`; `decisions.md:127-129`).
- That is the mechanism R4-S1 found unsafe: `meta up` installs and enables in one step, and
  `cluster-up` and chaos reset the flag.
- #70's round-4 review (F1) asks for the plumbing to be described, not for this mechanism to be
  adopted. Whichever plan lands first, the two describe incompatible `meta.sh` and Makefile
  interfaces.

Fix: record in decisions.md that #70 must adopt this section before either implementation
starts. Check #70's next revision against it: the path, the `FLAGS` line, `meta-flags`, and the
lab check reading what the process runs with.

## Nits

1. design.md:50-52 says "reads the header: an existing id answers `AlreadyExists`, as the claim
   would", which can be read two ways. `readHeader` hides headers that are being created or
   deleted (`service.go:286-289`). The claim answers `AlreadyExists` for any header and for a
   tombstone (`service.go:372-377,414-433`). Name `readHeader`, so that an id in flight or being
   deleted gets the retryable `FailedPrecondition`, and drop "as the claim would".

Reviewed commit: bb7fbb243410cadeeb9614dc6afdd701be76564d
