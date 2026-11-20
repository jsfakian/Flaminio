# #54 plan review, round 5 (incremental), design-code and design-security

Scope: `git diff ca41362..fcce15b -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. I checked the changed lines against #15's design (S1, S9) and its `device-lifetime-contract.md`, against #48's design (the probe, the scan, "With #74's helper"), and against #74's helper request table. I ran nothing.

## Round-4 findings

- **R4-C1:** fixed. Round 3's ending rule is cut, and #15's `reserve` refuses with `is restoring here`. No legal flow after Ready gets stuck on the writer node, because Ready needs the closure and restore-detach ends the device before it reports one. A stopping entry there gets #15's `is stopping here` (`FailedPrecondition`), which kubelet retries. One problem remains: CSI does not reach `reserve` first (R5-C1).
- **R4-C2:** fixed in direct mode only. Under `--privsep` with #48, the restore device is still kept (R5-C2).
- **R4-C3:** fixed. Two small leftovers: a retry gap (R5-C3) and a wrong test number (Nit).

## New findings

### R5-C1 (High): CSI asks `device-info` before it attaches, and the plan never says what that answers for a restore entry
- **Claim:**
  - The plan rests on two things: `status VOLUME` answering `restoring` "never an ordinary device for device()'s shortcut" (design.md:117-118), and `reserve` refusing CSI's attach (contracts.md:258-260).
  - Under #15, though, CSI's `device()` first asks `device-info VOLUME` and calls `Attach` only on `ok none` (flaminio-15 design.md:396-397). `device-info` has only the states none, starting, running and stopping, with the device tuple (device-lifetime-contract.md:19-23).
  - The restore attachment is a #15 entry (contracts.md:192-194, 246). Unless the plan says otherwise, `device-info` answers it `ok VOLUME running … /dev/ublkbN MAJ:MIN DISKSEQ`.
  - The plan names a case where this happens: after Ready, on a node the writer moved away from. There CSI passes the device check and mounts the private, fenced restore device as the user's volume, which the design forbids ("not a user-readable attachment"). At best the result is I/O errors.
  - The same gap applies to #79's `detach-device`, which is not covered by the plain `detach` refusal.
  - If instead the answer is `ok none`, CSI's `Attach` gets `error: VOLUME is restoring here`, and #15's map of attach errors has no entry for it (flaminio-15 design.md:398-404).
- **Fix:**
  - State that `device-info VOLUME` of a restore entry answers `ok none`. This keeps #15's revision-5 grammar, which #79 pins.
  - State that `detach-device` against it answers `ok absent` and changes nothing.
  - Map `is restoring here` to `FailedPrecondition` in the plugin, as `stopping` is.
  - List all three as #15/#79 hand-offs.
  - Tests: `device_info_hides_restore` (io), and `TestDeviceRestoringHereFailedPrecondition` (CSI) for a Ready volume on a node the writer moved away from.

### R5-C2 (Medium): under `--privsep`, the "unrecoverable" add cannot be made, so R4-C2's hazard remains
- **Claim:**
  - Under #48 with #74's helper, the helper's `add` uses fixed `ctrl_flags`: both recovery flags whenever the kernel has them. "The child still names no flag" (flaminio-48 design.md:442-444).
  - #74's request is `add QUEUES DEPTH IO_BUF SIZE`, with no flag field (74 design.md:173, 254).
  - Root's record, including its `recovery 2` block, is written by the helper, not by io (flaminio-48 design.md:445-464). At the child's exit, the helper keeps any recoverable device whose record says `running` (flaminio-48 design.md:491).
  - So contracts.md:261-267 ("asking for neither flag … no #48 block") and the ADR line (adr-proposal.md:37-39) cannot hold under `--privsep`. The plan does consider restores under `--privsep` (contracts.md:249).
  - A restore device killed in Incomplete is then kept for #48's bound. If another node's writer reaches Ready inside that window, an ordinary attach of the volume there resumes the kept device and reissues the held restore writes onto the verified image. That is the same failure R4-C2 described.
  - `restore_device_not_kept_across_restart` tests direct mode only.
- **Fix:**
  - Add a hand-off to #48 and #74: an unrecoverable form of the helper's `add` request (asking for fewer flags is safe for the child to choose). The helper writes no #48 block for it and deletes such a device at the child's exit. #48 writes no `kept/<ID>` note for a restore entry.
  - Add a `--privsep` variant of the test.
  - Correct the test's expected `recovery` answer: under `--privsep` it answers `unknown` until an ordinary add (flaminio-48 design.md:63-65), not `on`.

### R5-C3 (Low): the helper retries only `is stopping here`, but it can also get `limit:` or `restore slot taken` after it detaches another volume's attachment
- **Claim:**
  - The helper restore-detaches a terminal or superseded job's attachment of any volume before restore-start (contracts.md:313-317), and restore-detach can answer while #15 is still ending the entry (contracts.md:319-320).
  - If the detached attachment is of another volume, #15's stopping entry still counts under the limit (flaminio-15 design.md:61-63). At limit 1 (the `--privsep` default), restore-start therefore answers `limit:`. When the restore slot is freed is unspecified, so at other limits it may answer `restore slot taken`.
  - The helper does not retry either, so it exits 1 after AcquireRestoreWriter has already taken the epoch.
  - The 10 × 500 ms retry is also shorter than #15's own 30 s detach wait (flaminio-15 design.md:372-374).
- **Fix:**
  - State that the restore slot is freed when the #15 entry ends.
  - Have the helper retry restore-start's `limit:` and `restore slot taken` only after its own restore-detach, for up to 30 s.
  - Extend `start_after_detach_retries_stopping` with another volume at `--max-volumes 1`.

## Nits
- `restore_counts_under_limit` expects "16 entries of 128-byte names, one `restoring`, list in 2,236 bytes". With one `restoring` and 15 eight-byte states the list is 2,221 bytes. 2,236 is the all-`restoring` bound, which the single restore slot makes unreachable (design.md:306; contracts.md:248).
- `resume_of_completed_job_ends_attachment` and decisions.md:279 expect `ok superseded`. An attachment on a node the writer moved away from, in Incomplete (the usual case), has no proof, so io answers `ok not-recorded` and sends no RPC (contracts.md:227-228). Test both answers.
- The restore entry's #15 states are not named. It is `restoring` in `list`, yet restore-start expects `is stopping here` after a detach, and the plan says nothing of a starting phase. Say that it passes through #15's starting and stopping.
- contracts.md:264, "the next start deletes it by #15's rule": #15 deletes only a dead device and leaves any other (flaminio-48 design.md:130-131). Safety does not depend on it, because a device without recovery flags cannot be resumed. Say "ends or leaves; never resumes".
