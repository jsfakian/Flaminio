# #53 plan review, round 5 (incremental), design-code and design-security

Scope: `git diff afb3fc5..1091928 -- docs/issues/53/ ':!docs/issues/53/report.md'` (HEAD 386c947 adds
only report.md). I checked it against the guest scripts' daemon idiom on this checkout
(`scripts/guest/node.sh:65`, `ublk.sh:24`, `replica.sh:18`, `probe.sh:68`, all `sudo systemd-run
--unit`) and against #52's clear plan (flaminio-52 at ae97cf5, design.md:334-359).

## Earlier findings
- R4-B1: not fixed in mechanism. The presence wait, its bound and its tests are added, but the PID is still a wrapper's (R5-B1).
- R4-B2: fixed. The three-version distinction is in the validation profile (contracts.md:90-93), and `shipped_absence_answers_distinct` covers the excluded codes.
- R4-C1: fixed. There is a policy for `flaminio-nolock`, a no-lock code mapped to exit 3 with `NoSuchBucket` exiting 1, an exit-3 list entry, a test, and a writer for `nolock.conf`. Its "refusing an existing one" adds to R5-C1.
- R4-S1: fixed for the failing outputs, `repo init`/`repo check` and `candidate_state` under an unknown id. The `list` case is now inconsistent (R5-S1).
- R4-C2: fixed. The word list is extended (design.md:183), and the Makefile help and docs/ci.md are in the update list (design.md:383-384).
- R4-B3: fixed. The configuration has a path, an owner, mode 0600 and an exclusive create, `down` rewrites and then deletes it, and the contradiction is gone. The rewrite's crash case is in R5-S2.
- Nits: all three fixed (contracts.md:23, :222-223, design.md:241).

### R5-B1 — Low
**`exec` under `setpriv` still records a wrapper's PID. Recorded PIDs also go stale across a restart.**
- **Who starts the processes.** The guest scripts run as the ssh user and get root through `sudo`.
  The design itself uses `sudo ss` (design.md:252) and `sudo install` (:299). `setpriv` needs root to
  change the uid, so the script runs `sudo setpriv … -- weed …`. Started in the background, `$!` is
  then sudo's PID. On Ubuntu, sudo forks for its PAM session, and `use_pty` adds a monitor process.
  This is the same reason decisions.md:122-124 gives for rejecting `runuser`. The wait at
  design.md:251-254 then finds no listener under that PID and always refuses. The script test fakes
  both `sudo` and `setpriv` (design.md:344-352), so it cannot catch this.
- **Restarts.** `start` refuses when "the store runs" (design.md:235), and `stop`, `down` and the
  renewal restart (:278-280) all act on the recorded PIDs. After `make vm-restart VM=w` those PIDs
  may belong to unrelated processes, which a root `kill` would hit. `ssh … 'cmd &'` also keeps the
  session open until the background process closes stdout.

**Fix:** use the scripts' own idiom. Start each process with `sudo systemd-run --unit=flaminio-backup-lock-{store,fwd}
-p User=flaminio-backup-store`. Take the PID that `ss` filters on from `systemctl show -p MainPID
--value` (the exec'd process). "Runs" means the unit is active, and `stop` is `systemctl stop`.
Transient units do not survive a reboot. Script-test the PID read from the faked `systemctl`.

### R5-S1 — Low
**`list` reports `immutable_guarantee: true` for an archive whose coverage it never checked.**
- **What the rule says.** The guarantee is true "only for a completed, covered archive, per
  archive in `list`" (spec.md:51-53; contracts.md:226-229).
- **What `list` checks.** Only each root's retention, and it always reports `graph_checked: false`
  (contracts.md:84-88). Page, block and descriptor versions are checked only by `verify` and
  `restore` (:89-91). So `list`'s `true` claims coverage it did not see. Also, "(`graph_checked`
  says which)" cannot pick anything out in `list`, where that field is always false.
- **The abuse case.** A holder of the uploader credential and the repository key (a compromised
  cluster, which is the threat) can PUT an authentic root with COMPLIANCE retention past E. Its
  page or block versions can carry short or no retention. `list` marks that root `true`, while
  `verify` would refuse it. `guarantee_only_for_covered_archive` (design.md:183) has no such case.

**Fix:** in `list`, `immutable_guarantee` is `false` per archive unless the graph was checked. As
specified, that means always `false`, with the guide pointing to `verify`. Otherwise, rename `list`'s
per-archive field to a root-only claim such as `root_retained`. Add the forged-root case to the test:
a root retained past E over a block version short of E is `false` in `list` and refused by `verify`.

### R5-C1 — Low
**The worker-side lab files outlive `down`, so a second lab cycle refuses.**
- **What `down` deletes.** Only the volume file, both credentials and the identity configuration
  (design.md:340-341).
- **What stays.** `lock.conf`, `lock.key` (a secret), `lock.evidence.json`, `nolock.conf`,
  `nolock.key` and `nolock.evidence.json` all stay in `/var/lib/flaminio-backup`.
- **What then refuses.** After `down` and a new `up`:
  - the owner steps' evidence install refuses an existing destination (:299-300);
  - `repo init` refuses the existing `lock.key` (:283-284);
  - the check's new `nolock.conf` write refuses the existing file (:286-288, this round).
- **The test.** The "no file holding either secret" test (:371-373) does not cover `lock.key`.

**Fix:** `down`, after its deletes succeed, also removes these six files. Alternatively the check's
step 9 removes them. Extend the after-`down` script test to assert that none remains.

### R5-S2 — Low
**`down`'s atomic rewrite of `s3.json` names no temporary file, so a crash can leave a copy of the owner's secret.**
- **The gap.** The rewrite is "by atomic rename, same owner and mode" (design.md:333-334). A crash
  between writing the temporary file and the rename leaves that file, which holds the owner's
  secret. The final delete names only `s3.json` and the credentials (:340-341), so a later full
  `down` keeps that file. That breaks the stated post-condition "no file holding either secret left"
  (:371-373).
- **Unspecified behaviour.** Whether a rerun refuses the leftover file or overwrites it is
  unspecified.

**Fix:** name the temporary file (for example `s3.json.new`), created exclusive and no-follow,
`flaminio-backup-store` 0600. A rerun of `down` removes a leftover one first, and the final delete
includes it. Script-test a leftover `s3.json.new` gone after a full `down`.

## Nits
- design.md:229, "outside the volume": no mount point is named, and `s3.json` sits in the directory the volume lives under. Name the mount point (for example `…/data`).
- contracts.md:91-92, "one with a retention or a hold": say "respectively" (a retention for GetObjectRetention, a hold for GetObjectLegalHold).

Reviewed commit: 386c947a363cb26956ed2d76bbf99e0a766132ab
