# #53 plan review, round 7 (incremental), design-code and design-security

Scope: `git diff 5567116..b9a76cd -- docs/issues/53/ ':!docs/issues/53/report.md'` (HEAD 908cf93 adds
only report.md). I compared words, not line breaks. I read the store's lifecycle and `down` text
around the changed lines (design.md:218-245, :280-300, :344-395) and contracts.md:218-240.

## Earlier findings
- R6-B1: partly fixed. A rerun now finishes the final deletes once the volume file is gone
  (design.md:361-363), and a script test is named (:386-387). The fix works only if the volume file
  is the first thing the final step deletes, and the design does not fix that order (R7-C1).
- R6-C1: fixed. With exactly one unit active, `start` and `down` refuse and name
  `make backup-lock-store-stop` (design.md:239-240). The forwarder-only case is script-tested (:385-386).
  `stop` stops both units, so the way out works.
- R6-C2: fixed. contracts.md:228-230 now splits the rule. `candidate_state: completed` appears under a
  qualified profile, and in `list` it marks an entry whose root passed `list`'s checks, with
  `immutable_guarantee: false`. `immutable_guarantee: true` stays limited to `backup`, `verify` and
  `restore`. This agrees with design.md:90 and :189.

Checked and not a finding: the resumable branch cannot delete a still-locked version. Every version
lives in the volume, and the branch runs only when the volume file is gone. Outside an interrupted
`down`, the condition holds in two other cases:
- **A never-upped fl-w.** `down` deletes nothing and exits 0.
- **An `up` that failed before it created the volume file.** `down` deletes the credentials that
  `up` left, and the next `up` would otherwise refuse them.

Neither case can hold a lock.

### R7-C1 — Low
**The resumable final step assumes the volume file is deleted first, but the design never says so. If another file goes first and the volume file's own removal fails, the R6-B1 dead end comes back.**
- **The text.** The final step stops both units, then deletes "its volume file, both credentials,
  the identity configuration and any `s3.json.new`", then removes the six worker-side files
  (design.md:355-360). This is a list, not an order. The resume condition is "neither unit is
  active and the volume file is gone" (:361-362).
- **The failure.** Suppose an implementation deletes `s3.json` or `owner.cred` before the volume
  file. Then the volume file's `rm` fails, or the ssh session drops. The volume file is still
  there, so the rerun does not resume:
  - `down` refuses, because the store is down, and names `start` (:342);
  - `start` refuses, because the identity configuration is missing (:239);
  - the owner steps need `owner.cred`.

  No lab command finishes the cleanup. That breaks "no file holding a secret left" (:390-392) and
  blocks the next `up`, which refuses existing credentials (:225).
- **The unmount.** `stop` unmounts the volume (:237), but the final step only says
  "`systemctl stop` of both units". The design does not say whether the step unmounts before it
  deletes the volume file, and the resume condition does not check the mount. A step that unlinks
  the volume file while it is still loop-mounted resumes with the mount still in place, and the
  next `up` mounts onto a busy `…/data`.
- **The test.** "A fake `rm` failing in `down`'s final step and a rerun leaving none of the files"
  (:386-387) does not say which `rm` fails. If the volume file's own `rm` fails, the rerun refuses
  and names `start`, so as written the test contradicts the design.

**Fix:** fix the order of the final step: stop both units, unmount the volume, delete the volume
file, then delete the other files, each where present. Make the resume condition "neither unit is
active, the volume is not mounted and the volume file is gone", and finish any missing unmount
there. Pin two script tests:
- the fake `rm` fails on a file after the volume file, and a rerun leaves none of the files;
- the fake `rm` fails on the volume file itself, the rerun names `start`, and `start` then
  `down` leave none of the files.

## Nits
- design.md:384: the test "`down` naming `start` with the store down" must now keep the volume file
  present, because with it gone `down` takes the resumable branch and exits 0. Say so in the test.

Reviewed commit: 908cf936c262308963470222832398ae232d4c40
