# #54 plan review, round 4 (incremental), design-code and design-security

Scope: `git diff 55c05e7..ca41362 -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. I checked the changed lines against `flaminio-node/src/engine.rs`, `flaminio-io/src/{control,ublk}.rs`, `meta/domain/domain.go`, and the #15 and #48 designs. I ran nothing. My local `ln -T` test was blocked, so the `linkat` behaviour below is reasoned, not run.

## Round-3 findings

- **R3-C1:** fixed. `status VOLUME` and `list` are back to round 1's forms, and `restore-list` is a separate line. There is a residual on the entry count (R4-C3).
- **R3-C2:** fixed. `superseded` is decided from the job record alone, before the header and `writer_node` compares.
- **R3-C3:** fixed. The slot argument holds:
  - restore-start reserves the slot before it reads GetVolume (contracts.md:188-190).
  - Size is never 0 (`domain.go:182`), so every finalization needs at least one proof.
  - After a resume at a new generation, the old attachment's closure is answered `superseded`.
  - The sim check and the injected bug are added.
- **R3-C4:** fixed.
- **R3-C5:** the risk is reworded correctly. The cheaper fix that was taken is wrong in two ways (R4-C1).
- **R3-C6:** fixed.
- **R3-S1:** fixed. `linkat` fails `EEXIST` on any existing name, whether a file, a dangling link or a link to `/dev/null` or a FIFO, and it never follows the name.
- **R3-S2:** fixed. Rust std runs `setgroups`/`setgid`/`setuid` before the `pre_exec` closures, and the kernel clears the parent-death signal on that switch, so the test can fail.
- **R3-S3:** fixed. No global allocator is set in `dataplane/`, so glibc's `mallopt` applies.

## New findings

### R4-C1 (Medium): the ordinary attach can end a live, current restore, and the named call path cannot end a device
- **Claim:**
  - **The race.** The new rule (contracts.md:246-251; design.md:155-157) ends the restore attachment when GetVolume shows "the restore Ready, or an epoch other than the attachment's write epoch, or another target". It compares a read issued earlier with whatever attachment exists when the answer arrives.
    - An attach that reads (E1, target B) while the helper's AcquireRestoreWriter (E2, this node) and restore-start then run ends the new E2 attachment, which is live.
    - If that attachment had already committed proofs in Finalizing, its closure is lost. The restore then needs a new generation and a full re-verify.
  - **The clauses gain nothing.** The epoch and target clauses only matter for a non-Ready volume, and the ordinary attach of a non-Ready volume is refused anyway (`attach_refuses_restoring`). Only the Ready clause serves R3-C5's purpose.
  - **The call path.** The plan routes this through `Input::Attach` "as today (`engine.rs:666-670`)". But:
    - `engine.rs:640-646` answers `already attached` before any GetVolume.
    - #15's registry refuses an existing entry in `reserve` before the engine is reached.
    - The engine cannot end io's ublk device, which "as restore-detach does" requires.
- **Fix:**
  - End a restore attachment only when GetVolume shows the restore Ready, or the volume gone, Deleting or of another operation (the sweep's `delete` rule).
  - Drop the epoch and target clauses.
  - Do it in io's socket/registry layer, after the token verifies, and only for the entry whose instance/generation was captured before the read.
  - Extend `ordinary_attach_ends_superseded_restore_attachment` with a restore-start that races the read; the live attachment must survive.

### R4-C2 (Medium): under #48, an io restart keeps the restore device rather than ending it
- **Claim:**
  - #48 asks every `add_device` for `UBLK_F_USER_RECOVERY` and `UBLK_F_USER_RECOVERY_REISSUE` (flaminio-48 design.md:50-55). Its scan keeps a recorded running device across a restart (design.md:150-163).
  - The restore device is a #15 registry entry (design.md:143-144), so it would be kept with its held requests. After the restart it shows as `kept`, not `restoring`, so round 3's #48 hand-off (contracts.md:239; design.md:434-435), "skip `restoring`", does not cover it.
  - #54's premise that "an io restart loses" the attachment and the helper replays (contracts.md:226; design.md:414) then fails. The helper's I/O hangs on the kept device.
  - Worst case: #48 resumes a kept entry on any attach at the run mark + 1 (flaminio-48 design.md:201-216). So an ordinary attach right after Ready, inside the kept lifetime, could reissue an old generation's held writes onto the verified image. That is the write after Ready the ADR forbids.
- **Fix:**
  - Add restore devices unrecoverable: no recovery flags, and no #48 block. A restart then ends them, which is #48's "without `UBLK_F_USER_RECOVERY`" row.
  - List this in the cross-plan changes for #48.
  - Add a test `restore_device_not_kept_across_restart`.

### R4-C3 (Low): it is unspecified whether a restore attachment counts against `--max-volumes`
- **Claim:**
  - "17 entries ... stay under 4096 bytes" (contracts.md:238) implies the restore entry sits outside #15's limit of 1 to 16. #15 states "`list` holds at most 16 entries".
  - #15 forces the limit to 1 under `--privsep` and before Linux 6.9, because a synchronous ublk delete blocks every add (flaminio-15 design.md:49-59). A restore device beside an ordinary one would break that gate.
- **Fix:**
  - Count the restore attachment in `reserve` under the limit.
  - `list` then stays at 16 entries, 2,236 bytes with a 9-byte state. Correct contracts.md:238.
  - Add "#15's longest state becomes 9 bytes" to the cross-plan list.

## Nits
- contracts.md:87 says an idempotent closure answer "reads the job only". By the step order (contracts.md:115-125), step 2 compares the header before step 3 finds the idempotent case.
- The step order does not say what a request whose writer generation is above the job's answers. State `FailedPrecondition`.
- `catches_closure_before_finalize` is caught only if the sim calls CompleteRestore while the replayed attachment is still live. Say so in the chaos phase (design.md:316-319).
- restore-close's `O_EXCL` probe: open it read-only, so its close does not set off udev's re-probe on close after write.
- Step 1 answers `superseded` to any node's `io` for any operation, before the `writer_node` compare. That is harmless, since an operation's terminal state is not secret, but say it.
