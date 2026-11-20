# #53 plan review, round 8 (incremental), design-code and design-security

Scope: `git diff 908cf93..3c5a930 -- docs/issues/53/ ':!docs/issues/53/report.md'`, compared by
words, not line breaks. I read `down` and the store's lifecycle around the changed lines
(design.md:218-243, :343-396).

## Earlier findings
- R7-C1: fixed in the rule. The final step now stops both units, unmounts the volume and deletes
  the volume file first, then the other files (design.md:355-357). The resume condition is still
  "neither unit active and the volume file gone" (:362-363), with no mount check. That is sound only
  if a failed unmount or delete stops `down` at once. The decision says so (decisions.md:452-454),
  but the design's rule does not, and the new test text contradicts itself (R8-C1).
- Nit (`down` naming `start` with the store down): fixed. The test now keeps the volume file
  present (design.md:386).

### R8-C1 — Low
**The fix rests on the final step stopping at its first failure, but the design never says so, and the new test of a failed volume-file `rm` asks for a rerun that leaves "none of the files" right after saying that rerun names `start`.**
- **Stopping at the first failure.** design.md:355-361 lists the order but never says that a failed
  unmount or `rm` exits 1 before the next one runs. Only decisions.md:452-454 says it ("a failed
  `rm` of the volume file leaves every other file"), and the test's "every other file kept"
  (design.md:389) implies it. The case it matters for: an implementation that logs a failure and
  carries on. If the unmount fails, the volume file can still be unlinked while it is loop-mounted
  (Linux allows this), so the rerun resumes with the mount still in place. That is R7-C1's mount
  case, which the order alone does not close. If the volume file's `rm` fails, carrying on deletes
  `s3.json` and `owner.cred`, and that is R7-C1's dead end again.
- **The test text.** design.md:388-390 says: "a fake `rm` of the volume file failing (the rerun
  names `start`, every other file kept) … each rerun leaving none of the files". In the first case
  the rerun refuses, because the store is down and the volume file is present (:343-344), so it
  cannot leave none of the files. R7-C1's fix said that `start` then `down` clean up there. As
  written, the test cannot pass, or an implementer will bend `down` to make it pass.
- **The unmount.** No test covers a failed unmount. After one, the units are stopped, the volume is
  still mounted and its file is present. `down` names `start`, and `start` remounts (:238) onto a
  volume that is already mounted.

**Fix:**
- In design.md:355, say that the final step runs in that order and that a failed unmount or delete
  exits 1 at once, keeping every later file.
- Reword the test: "a fake `rm` of the volume file failing (every other file kept; the rerun names
  `start`, and `start` then `down` leave none of the files), and a fake `rm` of `owner.cred` failing
  after it (a rerun takes the resume path and leaves none)".
- Add a fake `umount` failing in `down`'s final step: `down` exits 1 with the volume file present.
  Also say what then recovers it: `stop` retries the unmount, or `start` accepts a volume that is
  already mounted.

Reviewed commit: 3c5a930a36eda482740237305cd680dea3de24fb
