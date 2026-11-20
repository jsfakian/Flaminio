# Issue #70, plan review round 5 (incremental), design-code

Reviewed: git diff 91caea0..e729522 (without report.md).

## Earlier findings
- R4 F1 (how VOLUME_EXPANSION reaches meta.sh, other `meta up` callers): fixed by delegation to #75's "Lab flags" section (file kept by `meta up`, `make meta-flags`, `QOS` and `VOLUME_EXPANSION` compose, `VOLUME_EXPANSION=2` refused). Depends on #75's text; see R5-C1.
- R4 F2 (csi_expand mechanism; `|| die`): `|| die "csi-expand: failover failed"` fixed. The check mechanism is named, but it is the one #75 is dropping: see R5-C1.
- R4 F3 (no test for refusal / flag script): fixed in form (S9 row lists the script test and a `csi_expand` check with fake VMs), but it tests the file read: see R5-C1.
- R4 nit (reconnect "is accepted"): fixed ("succeeds once a replica accepts the session").
- R4 security findings: not in my focus.

Citations verified: meta.sh:34 `Restart=on-failure`; node.go:124 Mount and mount.go:69-77; main.go:84; Makefile:301-308 and 331-332; node.go:108/135-136. `vms_of` (lib.sh:38) and `run_on_vm` exist.

## Findings

### R5-C1 — Medium (cross-plan: #75's text is being revised)
#70 still relies on reading the flags file. design.md Lab: "reads `/etc/flaminio/meta-flags` on every VM ... as #75's `qos-check` does, and refuses unless `FLAGS` holds `--volume-expansion`"; Tests S9: "`csi_expand`'s flag check, with fake VMs, refusing when one VM's file lacks the flag". #75's committed design (00e57e8, "The check, `meta.sh running FLAG`") says the file alone proves nothing and checks the running process's `/proc/<pid>/cmdline`. The file is exactly what a rollback leaves behind: an older binary with the flag in the file loops on exit status 2 (design.md:85) while the file still holds `--volume-expansion`, so the check would pass on a metadata service that is not running. design.md:91 also pins "(40cdf3f)", an older #75 commit than the one now committed.
Fix: say `csi_expand` runs #75's `meta.sh running --volume-expansion` on every VM of `vms_of cluster` and refuses unless all pass (as `qos-check`); change the S9 test to a fake VM whose `running` fails (inactive, or flag absent from the fake `PROC/<pid>/cmdline`) while its file has the flag; drop the pinned commit or point to the current one.

### R5-C2 — Low
Growth step's reads omit the ledgers and reports. design.md Metadata growth step 3 lists the attempt's reads as gate, header, `s.nodes()`; the Capacity charge bullet says the transaction compares each used node's ledger revision and that candidates need a valid report and a readable ledger. #72's createVolume reads `nodes()` with the reports and then the ledgers (#72 design, Charging and release). Without the read order named, an implementer may read ledgers after the compare set is fixed or skip the report validity check.
Fix: in step 3 add "once #72 lands: the reports with `s.nodes()`, then the ledgers", and say the ledger read is the one whose revisions the Txn compares.

### R5-C3 — Low
`TestControllerExpand` (Tests, S1/S4 row) lists NotFound and Unimplemented but not `ResourceExhausted`, the new code ExpandVolume can now answer (design CSI step 5, "any other code as returned"). The pass-through of that code to the sidecar is the one visible effect of the capacity charge in the CSI layer and nothing would fail if it were mapped to Internal.
Fix: add "ResourceExhausted returned as is" to that case's list.

Reviewed commit: 2c8856c995f9f6d66d75b2fd086dcb993d2887c5 (diff 91caea0..e729522)
