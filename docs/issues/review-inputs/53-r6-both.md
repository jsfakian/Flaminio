# #53 plan review, round 6 (incremental), design-code and design-security

Scope: `git diff 386c947..6b406e7 -- docs/issues/53/ ':!docs/issues/53/report.md'` (HEAD 5567116 adds
only report.md). I checked the transient units against `scripts/guest/{ublk,replica,node,probe}.sh`
on origin/main (`reset-failed`, then `sudo systemd-run --quiet --unit=… --property=Restart=no`,
with `systemctl is-active` and `systemctl stop`). They match. I also checked #52's lab store
(flaminio-52 design.md:334-346).

## Earlier findings
- R5-B1: fixed. The processes run as transient units under `User=flaminio-backup-store`. The PID is read
  from `systemctl show -p MainPID --value` at each check, and `0` refuses. `stop` is `systemctl stop`.
  A reboot leaves nothing stale. The fakes are `systemd-run` and `systemctl` (design.md:256-268,
  :365-373). The new "up/runs" states leave one gap, R6-C1.
- R5-S1: fixed. `list` reports `immutable_guarantee: false`, top-level and for each entry (spec.md:51-55,
  contracts.md:88-89, :224-232). The forged-root case is in `guarantee_only_for_covered_archive`, and
  `list_checks_root_only` asserts `false`. The narrowing is recorded and leaves "Done when" alone. The
  sentence's rewrite breaks the scope of `completed` (R6-C2).
- R5-C1: fixed. `down` removes the six worker-side files by name, and both script tests assert it
  (design.md:355-358, :381-386). If `down` fails partway through its own final step, the cleanup
  cannot be finished (R6-B1).
- R5-S2: fixed. `s3.json.new` is named, created exclusive and no-follow as `flaminio-backup-store` 0600.
  A leftover one is removed first, and the final delete includes it. Both are tested
  (design.md:344-347, :354, :383-385).
- Nit (mount point): fixed. The volume mounts at `…/data`, and `s3.json` sits outside it (design.md:222, :231).
- Nit ("respectively"): fixed (contracts.md:275).

### R6-B1 — Low
**If `down` fails during its final deletes, the cleanup cannot be finished, so secrets and the files that block the next cycle stay.**
- **When `down` runs.** It refuses unless the store is up (design.md:342). It reaches its last step
  only after stopping the store. That step then deletes the volume file, both credentials, `s3.json`
  and `s3.json.new`, and removes the six worker-side files (:353-358).
- **The failure.** Any failure or interruption inside this step stops the cleanup partway, for
  example a failed `rm` or a dropped ssh session. "A refused or failed `down` keeps them" (:358) does
  not cover it, because by then the earlier deletes have already happened.
- **Why a rerun cannot help.** A rerun of `down` refuses because the store is down, and names
  `start`. `start` refuses because `s3.json` is missing (:239), or it cannot remount a deleted volume
  file. So no lab command reaches the remaining deletes.
- **What stays.** Possibly `owner.cred`, `lock.key` and `nolock.key`, which breaks the post-condition
  "no file holding a secret left" (:385-386).
- **What the next cycle hits.** The next `up` refuses an existing credential, since both are created
  exclusive (:225). Its check refuses `lock.key` and the evidence files. That is the R5-C1 cycle
  again, now through a failure path.

**Fix:** make the final step resumable. When the volume file is gone and neither unit is active,
`down` skips the owner steps and finishes the remaining deletes, each where present, then exits 0.
Alternatively, delete the volume file last and accept a rerun on that state. Script-test it: a fake
`rm` fails mid-step, and a rerun leaves none of the files.

### R6-C1 — Low
**When only one of the two units is active, the store is neither up nor down, and the command `down` names refuses.**
- **The states.** The store is "up" when both units are active, and it "runs" when either is
  (design.md:235-236). With `Restart=no`, `weed` can exit while the forwarder keeps running, for
  example after an OOM kill or a crash.
- **What each command does in that state:**
  - `down` refuses, because the store is not up, and names `make backup-lock-store-start` (:342);
  - `start` refuses, because the store runs (:239);
  - the renewal's restart is skipped, because the store is not up (:289).
- **The only way out.** `stop`, then `start`. Nothing names it, and no test covers the partial
  state. The script tests cover `start` refusing with either unit active (:374) and `down` with the
  store fully down (:380), not `down` with one unit active.

**Fix:** when exactly one unit is active, `down` and `start` refuse naming
`make backup-lock-store-stop` first, or `start` stops the leftover unit before starting both.
Script-test `down` and `start` with only the forwarder's unit active.

### R6-C2 — Low
**contracts.md now says that only `backup`, `verify` and `restore` outputs say `completed`, but `list` must still say it.**
- **The changed sentence.** contracts.md:228-230 now reads: "Only a qualified profile's outputs say
  `immutable_guarantee: true` and `completed` … `backup`'s root …, and the archive `verify` checked
  or `restore` read …". The `list` clause it used to carry is gone.
- **The effect.** The sentence covers `completed` as well as the guarantee, so it now rules out
  `list` ever reporting `candidate_state: completed`.
- **What still expects `completed` in `list`:**
  - design.md:90 ("cannot list as a completed immutable archive");
  - the test at design.md:189 ("in `list` … a `completed` entry whose root passed among them");
  - intent outcome 4, under which a backup is listed once covered.
- **The risk.** An implementer who follows contracts.md would never list an archive as `completed`.

**Fix:** split the sentence. `immutable_guarantee: true` appears only in `backup`, `verify` and
`restore`, as now. `candidate_state: completed` appears only under a qualified profile, for a
completed archive. In `list`, that means an entry whose root passed `list`'s checks, with
`immutable_guarantee: false`.

Reviewed commit: 55671160e46e094f6f77ddfa8f77578bfcb8edb8
