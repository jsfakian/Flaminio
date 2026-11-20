# #54 plan review, round 3 (incremental), design-code and design-security

Scope: `git diff 05fba15..55c05e7 -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. Where the changed lines rely on other material, I checked it against `meta/service/{auth,service}.go`, `pkg/captoken/captoken.go`, `flaminio-node/src/engine.rs`, `flaminio-io/src/control.rs`, `control/csi/{tombstones,dataplane}.go`, and the #15, #48, #73 and #76 plans. I tested dash's noclobber locally. Nothing else was run.

## Round-2 findings

- **R2-C1:** fixed. `node` is on both io RPCs, the auth rule is in place and the `auth_test` rows are named.
- **R2-C2:** fixed in the main. A residual on the `superseded` decision is R3-C2.
- **R2-C3:** fixed. The cluster UUID is cut, and `MaxLen` 401 adds up.
- **R2-C4:** fixed as specified. The `O_EXCL` premise underneath it is wrong, in R3-C4.
- **R2-C5:** fixed. 22 is proposed and deferred to the tag coordination; no plan claims 22.
- **R2-C6:** fixed. The Creating path is routed to #76's deletion, and `TestAbortRestoreCreating` covers it.
- **R2-C7:** fixed. It is a named gap, with a follow-up.
- **R2-C8:** fixed. The rollback rule is documented and the risk is named.
- **R2-S1:** fixed. The FLRT is the only target capability, and restore-start checks the phase. The engine's ordinary attach already compares GetVolume's epoch and target (`engine.rs:1616-1621`), so the design follows existing code. One gap remains, in R3-C3.
- **R2-S2:** fixed in the main. The sweep is gated on GetVolume, whose reads are linearizable (`meta/kv` has no serializable read). The name cannot be reused while the tombstone exists. A stale read therefore cannot end a live restore. The residual is R3-C2.
- **R2-S3:** fixed. The reader has its own user and directory.
- **R2-S4:** partly fixed. Dash's noclobber still follows some links: see R3-S1.
- **R2-S5:** fixed in the main. The residuals are R3-S2 and R3-S3.
- **R2-S6:** fixed (same fix as R2-C1).
- **R2-S7:** fixed. The error fields have their RPC, the helper sanitizes them, and the service bounds them.

## New findings

### R3-C1 (Medium): the new `status VOLUME` and `list` forms break #15's and #48's line grammars
- **Claim:**
  - Round 2 changed the answers to `status VOLUME` → `ok VOLUME restoring OP_UUID WRITER_GEN` and `list` → `VOLUME restoring OP_UUID WRITER_GEN` (contracts.md:210-211). Round 1 had `ok VOLUME restoring` and `VOLUME restoring`.
  - #15 fixes `list -> ok limit N [VOLUME STATE ...]`, as pairs, and `status VOLUME -> ok VOLUME STATE [/dev/ublkbN MAJ:MIN INSTANCE GEN]` (flaminio-15 design.md:353-354).
  - #48 states that "the arity of `status VOLUME` and the pairs of `list` do not change" (flaminio-48 design.md:696). #48's agent loop parses `list` every 100 ms (design.md:357).
  - A pair parser reads `OP_UUID` as a volume id in state `WRITER_GEN`. A `status VOLUME` parser that takes the third token as the device gets `OP_UUID` (#15 S9: "every script takes the path from the attach or status VOLUME").
  - Only plain `status` is tested (`TestOldParserPlainStatusWithRestore`).
- **Fix:**
  - Restore round 1's forms (`VOLUME restoring` in `list`; `ok VOLUME restoring` in `status VOLUME`).
  - Give the helper its own line for the restore attachments, e.g. `restore-list -> ok [VOLUME OP_UUID WRITER_GEN ...]`, with its 4096-byte bound.
  - Add tests: #15's and #48's `list` parsers with a restore entry.

### R3-C2 (Medium): `superseded` is ordered after the header read and the `writer_node` compare, so the cancel and moved-writer paths answer errors
- **Claim:**
  - ReportRestoreWriterClosed answers `superseded` "when the job is terminal or its writer generation is above the request's; a node other than `writer_node` is refused" (contracts.md:114-116). The auth paragraph runs the `writer_node` compare unconditionally (contracts.md:137-139), and the transaction compares the header (contracts.md:84).
  - **Moved writer:** node A's old generation reports with `node` A while `writer_node` is B. That is `FailedPrecondition`, not `superseded`.
  - **After an abort:** the header is deleted, so a header-first read (as `ChangeIsr`'s `readHeader`, service.go:770) answers NotFound.
  - In both cases restore-detach answers `error: closure not recorded`, and io keeps a closure that can never commit (contracts.md:201-205). The helper's ending loop (contracts.md:254-259) then fails on A and in `cancel`. The tests that rely on `ok superseded` (`moved_writer_old_attachment_ended`, `cancel_ends_local_attachment`) cannot pass as the contract is written.
- **Fix:**
  - Decide `superseded` from the job record alone, first: terminal, or the job's generation above the request's.
  - Then compare the header and `writer_node`, for the current generation only.
  - Add `meta/service` tests `TestWriterClosedSuperseded` covering three cases: aborted with the header gone, completed, and a later generation from another node.

### R3-C3 (Medium): a closure recorded in Incomplete survives finalization, so Ready can arrive with a live writer
- **Claim:**
  - Round 2 made restore-detach work "in any phase" (contracts.md:198). A current-generation closure can therefore be recorded in Incomplete.
  - BeginRestoreFinalize does not clear the closure (contracts.md:49), and restore-start accepts the same ticket again in Incomplete (contracts.md:174-178).
  - The sequence is: restore-detach of generation G in Incomplete (closure recorded), then restore-start of G again, then writes, restore-close, finalize and proofs. CompleteRestore then finds "the current generation's closure" (contracts.md:51), which belongs to the first attachment, while the second attachment is still live. Proofs carry no instance or `attach_gen` that could tell the two apart.
  - Replicas accept that FLRT until the first user attach. This breaks the ADR's "Ready only once the writer is closed".
  - The trigger is a root operator's or a buggy helper's line order, but the chaos phase replays restore-start only in Finalizing and after Ready (design.md:303-305).
- **Fix:**
  - Record a closure only in Finalizing, at the current finalization generation. In Incomplete, end the attachment and answer `ok not-recorded`. Alternatively, have BeginRestoreFinalize clear `closure`.
  - Add the case to the sim (replayed restore-start in Incomplete after a closure) and a bug `catches_closure_before_finalize`.

### R3-C4 (Low): `O_EXCL` does not detect an ordinary opener, so the round-2 flush argument is misleading
- **Claim:**
  - On Linux, `O_EXCL` without `O_CREAT` on a block device fails `EBUSY` only against another exclusive holder: a mount, another `O_EXCL` open, or a dm/md claim. It does not fail against a plain `O_RDWR` open, and blkid and udev probe with plain opens.
  - So "answering `device is open` while any process holds it open" (contracts.md:187; design.md:160) is false.
  - design.md:212-213's new claim, that the held `O_EXCL`, `fsync`ed descriptor "keeps a buffered writer from flushing after admission closed", is also false. A plain opener can still dirty pages, and periodic writeback flushes them.
  - The real guards are admission closure (late writes are refused) and the proofs (an earlier stray write mismatches the root). The retry rule is harmless.
- **Fix:**
  - Restate the check as "no exclusive holder (mount, another `O_EXCL`)", and name admission closure and the proofs as the guarantees.
  - `close_refused_while_open` must hold the device `O_EXCL` or mounted.

### R3-C5 (Low): the named risk for a moved-from node's attachment understates its effect
- **Claim:**
  - The risk names only a node "that holds no replica" keeping "a device and a target" (design.md:388-389). But a moved-from node that does hold a replica also keeps its attachment until a deletion's sweep.
  - On either node the attachment refuses an ordinary attach of the restored, Ready volume on that node. The engine answers "already attached" (engine.rs:641-646), as does #15's registry.
- **Fix:**
  - Correct the risk's wording.
  - Cheaper: the ordinary attach path, which already reads GetVolume (engine.rs:666-670), ends a restore attachment of the same volume whose restore is Ready or whose epoch or target is no longer its own, before it attaches. Test: `ordinary_attach_ends_superseded_restore_attachment`.

### R3-C6 (Low): resending an already-recorded closure is unspecified
- **Claim:**
  - A lost reply leaves io's closure "kept", and "a repeat resends" it (contracts.md:201-203).
  - Proofs define an idempotent repeat (contracts.md:149-150). ReportRestoreWriterClosed does not: a job whose closure already equals the request's may refuse it, leaving the helper stuck in Finalizing.
- **Fix:** an identical closure for the current generation is an idempotent success. Cover it in `TestWriterClosedSuperseded` or `restore_detach_resends_closure`.

### R3-S1 (Low): dash's noclobber follows a planted link to a non-regular file
- **Claim:**
  - Dash's `set -C` uses `O_CREAT|O_EXCL` only when `stat` fails. When the path's target exists and is not a regular file, it opens the target `O_WRONLY` through the link.
  - I tested this locally. `ln -s /dev/null DST` is written to, exit 0. Links to a regular file and dangling links are refused.
  - So "a planted link ... is refused and never followed" (design.md:353-356) is false for a link to a FIFO or a device.
  - The writer is the same user, so there is no escalation. Two harms remain: a link to `/dev/null` silently drops the secret, and a FIFO hands it to a planted listener.
- **Fix:**
  - Write to a fresh `mktemp` file in the 0700 directory, then `ln -T` it to DST (which neither follows nor overwrites) and remove the temporary file. Alternatively, use a small `O_CREAT|O_EXCL|O_NOFOLLOW` writer.
  - Extend `restore-secret-test.sh` with a link to `/dev/null` and one to a FIFO.

### R3-S2 (Low): the parent-death signal can be lost, or fire early
- **Claim:**
  - `PR_SET_PDEATHSIG` is "set between fork and exec" (contracts.md:246). The kernel clears it on any change of the effective or filesystem uid or gid. If it is set before the switch to `flaminio-restore-read`, it is gone.
  - It also fires when the forking thread exits, not the process. A spawn from a short-lived thread (for example tokio's blocking pool, whose threads retire when idle) kills a healthy reader.
- **Fix:**
  - Set it after `setgid`/`setuid`. Rust's `pre_exec` runs after `Command::uid`, so a `pre_exec` closure is in the right place; say so in the contract.
  - Spawn from a thread that lives as long as the helper.
  - Test that the reader survives the exit of the thread that spawned it.

### R3-S3 (Low): `RLIMIT_AS` counts reservations, so the named risk is misstated
- **Claim:**
  - The 2 GiB address-space limit (contracts.md:247) counts thread stacks and glibc's per-thread malloc arenas (64 MiB each, up to 8 × cores), not memory used.
  - A multi-threaded S3 reader on a many-core node can exceed it while using little memory.
  - The risk "a reader needing more than 2 GiB" (design.md:390-391) misstates the cause.
- **Fix:**
  - Bound the reader's threads (for example a fixed small runtime) and its arenas (`mallopt(M_ARENA_MAX)` in the reader, since the environment is empty).
  - Reword the risk.
  - Run `reader_memory_bounded` on the lab's core count.

## Nits
- `restore_no_write_after_ready` (design.md:307) says "no write at the restore's epochs ... after Finalizing began". A legal return to Incomplete at a new epoch would trip it. Scope it to the epoch that finalized.
- `cancel` racing the sweep: if `delete` ended the attachment first, restore-detach answers `no restore attachment`. Say that `cancel` treats this as done.
- The restore-close repeat "for a generation already closed answers `ok`" (contracts.md:189) should say what a repeat answers after the attachment has ended.
