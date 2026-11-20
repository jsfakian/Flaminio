# #54 plan review, round 2 (incremental), design-code focus

Scope: `git diff 52df1de..05fba15 -- docs/issues/54/ ':!docs/issues/54/report.md'`, read as the revised contracts.md, design.md and decisions.md. I checked it against `meta/service/service.go`, `meta/domain/domain.go`, `flaminio-io/src/control.rs`, #52, #70, #75 and #80. I ran no tests.

## Round-1 findings

- R1-C1: fixed. The two-process split (a root helper, an unprivileged reader) and io's own calls to `ReportRestoreChunkProof` and `ReportRestoreWriterClosed` are specified, with `auth.go` rows. One gap remains, in R2-C1.
- R1-C2: fixed. `store_ca`, `store_identity`, the store client pair and the credential are all named, and the lab issues them.
- R1-C3: fixed. #52's `cluster` record is used, no UUID is supplied by the caller, and an archive from the same cluster is refused. `TestBeginRestoreClusterRecord` covers it.
- R1-C4: moved to tag 11. A collision risk remains, in R2-C5.
- R1-C5: fixed. #76 integration is spelled out as four points, with tests.
- R1-C6: fixed. `use` is Unknown in the first header, and the NodeStage tests are named.
- R1-C7: fixed. The `restore` epoch kind is named, and #17's maintenance attach refuses a non-Ready restore.
- R1-C8: fixed. A method table, a `system` owner and the tenant-name refusal are specified.
- R1-C9: fixed. The phase table is complete and abort needs no writer closure. A residual gap on the Creating state is in R2-C6.
- R1-C10: fixed. `EnableRestore` and `features/restore` give the enabling step.
- R1-C11: fixed. The ticket layout is explicit and its 417-byte maximum adds up. The path bound of 16 × 32 bytes matches #80's N ≤ 65,536 (#80 spec:11, design:13), and the 64 KiB control-line cap (`control.rs:77`) is enough. Records have keys, schemas and bounds.
- R1-C12: fixed. #53's output fields are in the reader's Header frame, and the tests extend #53's.
- R1-C13: fixed. Named tests, named simulator checks and six injected bugs are listed.
- R1-C14: mostly fixed. Commands, ports, units and check steps are named, and the ports do not collide with `lab.conf`. The first cluster's etcd uses :2379/:2380 on the WireGuard IP (`scripts/lab.sh:469-475`). A coverage gap remains, in R2-C8.
- R1-C15: fixed. Plain `status` skips a restore attachment, and plain `detach` and `delete` refuse one.
- R1-C16: fixed. The 38-byte bound is correct: 2 + 2 + 34.
- R1-C17: fixed in the main. `O_DIRECT`, `fdatasync` and the `O_EXCL` open in restore-close are specified. A small gap remains, in R2-C7.
- R1-C18: named as a risk, with the growth bounded at 2048 bytes per record and a follow-up to be filed. I accept it.

## New findings

### R2-C1: Medium. The io-called proof and closure RPCs carry no `node` field, but the auth rule needs one.
- contracts.md, "RPCs": the `ReportRestoreChunkProof(...)` and `ReportRestoreWriterClosed(...)` signatures have no `node` parameter.
- The paragraph after the method table says "the request's `node` must equal [the certificate's]; the transaction compares it with the job's `writer_node`."
- The two cannot both hold as written. The `auth_test` rows and the Rust metaclient message follow the signature.
- Fix: add `node` to both messages, or state that the transaction compares the certificate node and the request carries none. Add `auth_test` rows for a wrong-node io and for a non-writer io.

### R2-C2: Medium. The restore attachment cannot be ended reliably after a helper crash or an abort.
- `restore-detach` needs the `CLOSURE_NONCE` that `restore-close` returned (contracts.md, control lines).
- A helper that crashed after restore-close has lost the nonce. Design.md "Admission": on a same-node resume, "the helper first ends that node's older restore attachment with restore-close and restore-detach". The contract does not say whether a repeated restore-close returns the same nonce. If it does not, that resume is stuck, because plain `detach` is refused for a restore attachment.
- `restore-volume cancel` is only AbortRestore. `DeleteVolume` today does not look at attachments (`service.go:516-585`), so after an abort the local ublk device and target stay attached on the node, and nothing in the plan ends them.
- Fix: make restore-close idempotent, returning the existing nonce for the same generation. Define `restore-detach` for an aborted job, either with the nonce or by a force form that checks the job generation, and have `cancel` and `resume` call it. Add tests: `resume_after_helper_crash_ends_old_attachment` and `cancel_ends_local_attachment`.

### R2-C3: Low. The ticket's cluster UUID has no checker.
- `RestoreTicketV1` binds `[16] cluster UUID` (contracts.md). Disks carry a NIL cluster UUID (`flaminio-io/src/cli.rs:83`, design.md facts), and the node learns the home UUID from nowhere.
- Only the signature protects the ticket, which is fine because the key is shared. The binding therefore adds nothing at the node.
- Fix: say so, or have the node compare it with the UUID in its first Hello or config. Otherwise drop the claim of "cluster/home incarnation" binding.

### R2-C4: Low. The same-node restore-close `O_EXCL` open can fail for reasons other than a writer.
- Udev and blkid may briefly hold a fresh `/dev/ublkbN` open.
- The contract answers `error: device is open` with no retry rule. The check also only protects restore-close time: if the `O_EXCL` fd is closed before the drain, the page-cache flush happens at last close, after write admission has closed.
- Fix: the helper's `fdatasync`/close must precede restore-close, which it does. Also say that restore-close holds the `O_EXCL` fd across the drain and `fsync`s it before closing admission. Say the helper retries a bounded number of times on "device is open".

### R2-C5: Low. The claim that tag 11 is the "lowest `Volume` number no plan claims" conflicts with #75's design.
- #75 design.md:175 says "11-14 for #70" among the sibling plans' `Volume` proposals. The #70 plan files do not repeat it, so it is unfrozen.
- A late renumber is a `make proto` change for whichever lands second.
- Fix: record in decisions.md that #75 notes 11-14 as #70's proposal. Take a number outside it (for example 22), or have the plans coordinate before `make proto`.

### R2-C6: Low. Abort of a restore still in Creating is underspecified.
- AbortRestore "compares the job's operation and first epoch" (contracts.md, RPCs). `first_epoch` is 0 until #76's finalization (Records).
- The design routes the Creating case through "#76's deletion of an incomplete operation", but gives AbortRestore no rule for it. The `restores/<op>` job exists from admission, so its outcome and generation after such a deletion are not defined.
- Fix: say whether AbortRestore on a Creating root delegates to #76's deletion and writes the job `aborted` in the same transaction. Add a test `TestAbortRestoreCreating`.

### R2-C7: Low. Restore-check covers only one replica.
- The destination is "one-node" (design.md, Lab), so `--replicas` is 1.
- Per-member proofs, a member failure in Finalizing and the return to Incomplete are exercised only in the simulator. Step 3's "every ISR member" is one member.
- Fix: state this as a named gap with a follow-up, or have `restore-dest up` take a second disk on fl-c as a second node, if the lab allows.

### R2-C8: Low (downgrade). Rollback after `EnableRestore`.
- `features/restore` is written once and never deleted. After it, a metadata binary rolled back to one without this change would drop `restore` when it rewrites an Incomplete header (`service.go:708`, `:722-724`).
- Fix: document in the upgrade notes that a rollback requires first aborting or completing every restore. Optionally have the rollback procedure check for non-Ready restore headers.

## Nits
- contracts.md "Header and Volume": "9–10 #79, 15 #13, 16–17 #17" disagrees with #75's note ("9-10 for #13 and #18"). Cite one source.
- design.md, Cancel: "A plain DeleteVolume of a non-Ready restore does the same". State that DeleteVolume reads the job key from `header.restore.operation`.
