# #53 plan review, round 9 (incremental), design-code and design-security

Scope: `git diff 3c5a930..cc8ef2b -- docs/issues/53/ ':!docs/issues/53/report.md'`, compared by
words. cc8ef2b only rewraps the bullet. I read the store's `stop`/`start` lifecycle
(design.md:233-241) and `down` (design.md:343-368), which the changed lines rely on.

## Earlier findings
- R8-C1: partly fixed. Stopping at the first failure is now stated (design.md:362), and the test of
  a failed volume-file `rm` no longer contradicts itself (design.md:389-391). A failed-unmount test
  was added. But the recovery it asserts, `start` then `down`, needs `start` to work on a volume
  that is still mounted, and the design does not say that it does. That was the third part of the
  fix, and it is not taken (R9-C1).

### R9-C1 — Low
**After a failed unmount in `down`'s final step, the design sends the operator to `start`, but `start` "remounts it" onto a volume that is still loop-mounted, and nothing says that this succeeds.**
- design.md:362-364: "a failed unmount ... leaves every other file, so a rerun refuses naming
  `start`, and after `start` a full `down` runs again". decisions.md:463-465 says the same.
- design.md:237: `start` "remounts it and starts both again". It refuses only when the identity
  configuration is missing or the store runs. Nothing covers a volume that is already mounted.
  After the failed unmount the units are stopped and the volume is still mounted at `…/data`. A
  plain `mount -o loop` of the same file on the same target then fails with "already mounted"
  (util-linux), so `start` would exit non-zero. The operator is left with a `down` that names
  `start` and a `start` that fails: the dead end that R7-C1 and R8-C1 set out to close. The
  design also does not say whether `stop` retries the unmount when neither unit is active, so it
  is not a documented way out either.
- The new test (design.md:390-391, "a fake unmount failing ... after which `start` and `down`
  leave none of the files") passes only if the fake `mount` ignores the mount that is still in
  place. As written, the test cannot catch this.

**Fix:** say what `start` does with a volume already mounted at `…/data` from that file. Either it
skips the mount (checked with `findmnt` or `mountpoint`, and only when the source is that file),
or `stop` with neither unit active retries the unmount and `down`'s message names `stop` for this
case. Then give the test's fake mount state: a failed unmount leaves the volume mounted, and the
rerun passes through the step the design names.

Reviewed commit: cc8ef2be866a762a041c90ee24d8581179411084
