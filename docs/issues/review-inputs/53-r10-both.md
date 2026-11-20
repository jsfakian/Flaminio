# #53 plan review, round 10 (incremental), design-code and design-security

Scope: `git diff cc8ef2b..ae66112 -- docs/issues/53/ ':!docs/issues/53/report.md'`, compared by
words. I read the store's lifecycle (design.md:233-243), `down` (design.md:343-368) and the
script test (design.md:369-399), which the changed lines rely on.

## Earlier findings
- R9-C1: fixed. `start` now mounts the volume unless it is already mounted at `…/data`
  (`mountpoint -q`, design.md:237-238). After a failed unmount, the rerun of `down` refuses naming
  `start`: the units are stopped and the volume file is present. `start` passes its own checks,
  because the identity configuration is deleted only after the volume file. It skips the mount and
  starts both units, so the full `down` that follows can run. The test now gives the fake mount
  state ("`start` mounting nothing, the volume still mounted", design.md:391-392), so it fails if
  `start` remounts. decisions.md:472-478 records the same fix.

## What the revision broke
Nothing of Low or worse.

## Nits
- design.md:370-371: the script test's list of fakes does not name `mountpoint`, which the new
  failed-unmount case needs faked. It also leaves out the mount, unmount and `rm` that the
  test already fakes. Add them to the list, or say they go through the fake `sudo`.

Reviewed commit: 14236dcf78d40d4e130edc536b3c34b75364fbab
