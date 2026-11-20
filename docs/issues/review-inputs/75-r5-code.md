# Plan review #75, round 5, design-code (diff dc80bd5..40cdf3f)

Round-4 findings: #1 (QOS plumbing, reset by meta up) fixed in design, with the gap in finding 1 below.
#2 (hung_task_panic widening) fixed: marked as a change after approval in decisions, design, ADR.
#3 (metactl argument test, NotStored fixture) fixed: TestCreateQoSArgument; the interceptor fits
the fixture (main_test.go:79-85 already chains `count` before Authorize). Switch-off create:
`FailedPrecondition` maps to retryable ResourceExhausted (controller.go:248-249), consistent.
The mechanism is feasible: unit text `EnvironmentFile=-...` + `$FLAGS` (written `\$FLAGS` in the
unquoted heredoc), ExecStart word splitting, `ENV_VARS` unquoted (lib.sh:157, hence the 0/1
validation), `cmd_meta` and `sha256` (lab.sh:1636) extractable like lab-deletion-test.sh.

1. Medium - "the file is what the process runs with" is not guaranteed, and qos-check trusts it.
   (a) A VM whose unit was written by an older meta.sh has no `EnvironmentFile`; `meta.sh flags`
   writes the file and restarts, the flag is silently ignored. The build check does not catch it
   when the binary is unchanged (e.g. #70 lands first and #75's binary hash equals what is
   installed after a `meta up` of a prior commit, or a rerun on a VM whose binary was updated but
   not its unit). (b) `meta.sh flags` writes the file, then the restart can fail (VM by VM, no
   rollback): the file says `--qos`, the process does not run with it, and `qos-check`, which reads
   the file, passes or proceeds against an instance that refuses limited creates. Fix: `qos-check`
   (and `meta flags` as its last step) reads the running process: `sudo tr '\0' ' ' </proc/$(systemctl
   show -p MainPID --value flaminio-meta)/cmdline` contains `--qos`, with the service active;
   `meta.sh flags` dies if the unit lacks `EnvironmentFile` ("run make meta-up first"). Add a
   script-test case for each (stub systemctl `show`, a unit without the line).

2. Low - The "every VM runs the current build" check proves only the metadata binary on the
   cluster VMs equals the workstation checkout's build. The data planes (the other half of the
   precondition the Makefile help and flag usage state) are not checked, and the help text reads as
   if both are. Say in the design that the check covers metadata instances only and that data planes
   remain the operator's responsibility (as S5 already says).

3. Low - Rollback remedy for the lab is missing. A rollback by `meta up` from an older checkout
   leaves `meta-flags` with `--qos`; the older binary exits on the unknown flag (fail closed) and
   the older lab.sh has no `meta flags` to clear it. docs/lab.md (listed under Docs) should say to
   delete limited volumes and then `sudo rm /etc/flaminio/meta-flags` (or run `meta flags QOS=0`
   before checking out the old tree).

4. Low - The script test checks the unit's text (`EnvironmentFile`, `$FLAGS`), not systemd's
   parsing of a multi-flag line (`FLAGS=--qos --volume-expansion`) or an empty value. If systemd
   is present, `systemd-analyze verify` on the generated unit is cheap; otherwise name this as
   covered only by the lab (hence finding 1's cmdline check is the real proof).

5. Nit - decisions.md:111 (round 3) still says `make meta-up QOS=1`; the round-4 entry says it is
   replaced, fine, but mark the line "replaced in round 4" to avoid misreading.
