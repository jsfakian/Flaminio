# Issue #75 plan review: round 4, design-security focus (incremental)

Scope: `git diff 1a0a7ed..dc80bd5 -- docs/issues/75/ ':!docs/issues/75/report.md'` (the round-3
fix), read in `/tmp/flaminio-plans-2026-11-12/75` against that worktree's code.

## Earlier findings

- R3-S1: fixed. The text now says that nothing enforces the order and that an older instance
  erases the policy for good. It also says that the only safe rollback deletes the limited volumes
  first, and it covers the second rollout and the `--qos` exit. `main.go:84` is
  `flag.ExitOnError`, as claimed. Only `Attach` (`service.go:724-725`) rewrites a live header, so
  "on its next attach" is exact. The new lab path has a gap; see R4-S1.
- R3-S2: fixed (the switch gates creates only; `TestGetVolumeQoSSwitchOff`).
- R3-S3: fixed (metactl compares, exits 1 through `main.go:77-80`, `TestCreateQoSNotStored`).
- R3-S4: fixed. "Effective burst" is defined. The bound now counts what is admitted, and the
  property test includes policies with a 0 burst. With S1's rule, `b ≥ 1` for I/Os and
  `b ≥ 1 MiB` for bytes, so no 0 field can make a dimension free.
- R3-S5: fixed for the CSI controller's retry; see R4-S2 for a third leftover, and Nit 2.
- R3-S6, R3-S7, R3-S8: fixed.
- r75c3 #1 (burst 2 cross-chunk test), #2 (delete-fails and no-delete tests), #3 (switch-off
  reads), #4 (metactl readback), nit 5 (ADR status): fixed.

### R4-S1 — Low

**The lab's "second rollout" also does the first one, and nothing checks the order.**

Evidence:
- design.md:56-58 calls `make meta-up QOS=1` the second rollout.
- `cmd_meta up` builds the binary, installs it and rewrites the unit on each VM in turn
  (`scripts/lab.sh:509-520`). Run on a cluster that is not yet upgraded, `QOS=1` enables the switch
  VM by VM while the other VMs still run the older binary. This is R3-S1's hazard, back on the
  lab's opt-in path.
- The other callers of `cmd_meta up` rewrite the unit without `--qos`: `make cluster-up`
  (`Makefile:308`) and `make chaos`'s restart of w (`lab.sh:1403-1407`). Either one silently undoes
  the second rollout. This fails closed, but the plan does not say so.
- No test covers `QOS`'s refusal of other values or `make qos-check`'s precondition. The pattern
  for such tests exists: fake VMs in `scripts/tools/lab-deletion-test.sh`, run by `make lint`.

Fix:
- With `QOS=1`, `cmd_meta` refuses before writing any unit unless every cluster VM's
  `/usr/local/bin/flaminio-meta` already matches the build. It can use the sha256 comparison of
  `cmd_vol_confirm` (`lab.sh:1820-1822`).
- State in `docs/lab.md` that `cluster-up` and `chaos` turn the switch off.
- Add both guards to a `scripts/tools` test.

### R4-S2 — Low

**The switch's refusal pre-empts `AlreadyExists`, which gives a third leftover during the
documented second rollout.**

Evidence:
- The switch is checked at create (design.md:48-50). Validation in `CreateVolume` runs before the
  claim that answers `AlreadyExists` (`service.go:348-364,414-433`), so a switch-off refusal would
  naturally come first. The design does not specify the order.
- Scenario: a CSI create commits on an instance whose switch is on, and its answer is lost.
  `metaCall` then tries the next instance (`control/csi/meta.go:72-79`). If that instance is
  current but has the switch off, which happens during the second rollout or after `make chaos`
  (R4-S1), it refuses instead of answering `AlreadyExists`.
- The refusal's code is not specified. design.md:173-174 reads as `InvalidArgument`, which the
  external-provisioner treats as final. If the PVC is then deleted, nothing deletes the limited
  volume.
- `FailedPrecondition` would avoid that, because the controller maps it to the retryable
  `ResourceExhausted` (`controller.go:248-249`).
- Compatibility (design.md:234-236) lists only two leftovers, both "while instances are mixed".

Fix:
- With the switch off and a limited policy, `CreateVolume` reads the header first and answers
  `AlreadyExists` for an existing id. The CSI comparison then matches against a current instance,
  and the retry succeeds.
- Specify the refusal's code.
- Add the case to `TestCreateVolumeQoSOff`.

### R4-S3 — Low

**The switch's warning is missing from the text an operator reads where the switch is set.**

Evidence:
- The warning is in the spec, the design, the ADR and (as planned) `docs/lab.md`.
- The design does not specify:
  - the `--qos` usage string printed by `flaminio-meta serve -h`;
  - `main.go`'s command doc, which already explains `--insecure` (`main.go:19-23`);
  - `meta-up`'s Makefile help line (`Makefile:331`). design.md:220 adds only `qos-check`'s line.
- An operator outside the lab sets the switch from that help. AGENTS.md also asks for the Makefile
  help to match the real commands.

Fix: give the flag's usage text, for example: "allow limited volumes: only once every metadata
instance and data plane runs this version; afterwards an older instance erases their limits, so
delete them before any rollback". Name `QOS=1` and the second rollout in `meta-up`'s help.

## Nits

1. **Spec S5's lead sentence and intent item 4 overstate the guarantee.** Both say "Limited
   volumes exist only once every metadata instance and every data plane runs this version" as if
   something ensured it (spec.md:58-59, intent.md:38). The next bullet says that nothing enforces
   it. Write "are meant to exist only once ...; operators ensure it".
2. **The leftover remedy cannot be followed as written.**
   - `metactl volumes` lists volume ids only (`main.go:424-425`), so it cannot show that a volume
     has no PVC (design.md:235-236).
   - The PVC stays Pending until a user deletes it.
   - Better: once every instance runs this version, delete the volume whose id (`pvc-<uid>`) has no
     PV. The Pending PVC's next retry then creates the volume with its policy.
3. **"`QOS` (default 0, else refused)" does not say where the value is refused** (design.md:57).
   `ENV_VARS` reaches the remote shell unquoted (`scripts/lib.sh:157`), so the refusal belongs in
   `lab.sh`, before the value is added to `ENV_VARS`, and not in `meta.sh`.

Reviewed commit: fd83341ca05feccee3c2149cbfd1e15258945257
