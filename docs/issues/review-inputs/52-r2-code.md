# #52 plan review, round 2, focus design-code

Scope: `git diff c6efcd7..0d26cc2 -- docs/issues/52/ ':!docs/issues/52/report.md'`, checked against the
worktree code, origin/main scripts, and the clear plans #12, #13, #16, #71, #73 (origin/main) and #51
(flaminio-51).

## Earlier findings
- R1-C1: fixed (role rows, worker rule, insecure behaviour, whichever-lands-second, TestSnapshotReadRoles).
- R1-C2: fixed (ListClosingSnapshotReads poll); its ack path has a new gap, R2-C3.
- R1-C3: fixed in part. The reader model, the two checks, six mutations and the vectors are planned, but two mutations cannot be caught by the named checks, and the simulator has no deletion, re-creation or forgetting: R2-C1.
- R1-C4: fixed (records, per-call table within 120, `pending`, header untouched, collection); new details in R2-C6, R2-C7.
- R1-C5: fixed (intent after #73's admission, its compares, the unparseable header, `FailedPrecondition`, refused-deletion test).
- R1-C6: fixed (encrypted volumes refused; S8 deferred).
- R1-C7: fixed (7413 is free in every clear plan; `--reader-listen`; address from `Node.address`); the lab's node start is not updated, R2-C4.
- R1-C8: fixed (`plan_view_read`, first-epoch refusal, failover test).
- R1-C9: fixed (cluster record, conditional and race-safe; size at the grant; 4096; #61's fields).
- R1-C10: fixed (#13's `forgotten` mark); one residual case, R2-C8.
- R1-C11: fixed (follow-up). R1-C12: fixed. R1-C13: fixed (grant file, Close outcome/root, admin commands).
- R1-C14: fixed in part: targets, scripts and ordered steps named, but two steps cannot run as written, R2-C5.
- R1-C15: fixed. R1-C16: fixed. Nit (u32le): fixed.

### R2-C1 — Medium
**Two of the six injected bugs cannot be caught by the two named checks, and the simulator cannot yet drive deletion, re-creation or forgetting.**
- Evidence: design.md:176-183. `close_without_notify` means closing never reaches a source. The source then never acks, the grant never closes and no pin goes, so neither "no read permit after closure" nor "no pin goes while a source is unacked" fails. The run is only stuck.
- `grant_under_closing` is not caught either. A grant admitted during closing raises `open`, and the deletion keeps answering `FailedPrecondition` until it closes (contracts.md:184-188). A source on a closing grant raises `pending` and is acked at the next poll. In both cases safety holds and only progress suffers.
- So `catches_reader_close_without_notify` and `catches_reader_grant_under_closing` (design.md:226) would fail as specified.
- The simulator's chaos list has crashes, partitions, outages, failovers and moves (`flaminio-repl/src/sim/mod.rs:7-14`). It has no volume deletion, re-creation or node forgetting. `meta.rs` has no deletion and no forgotten set. Yet the model is "driven ... beside ... delete and re-create" and checks exclusion (design.md:173-176). Those events are never named as new chaos actions.
- Fix:
  - Add a settle-phase check, beside the existing liveness checks (`sim/mod.rs:500-533`): once the run is healed, every closing grant closes, and every requested deletion completes within the settle bound.
  - Add a direct check that no grant or source record is created after its volume's closing intent.
  - Name the new chaos actions: delete a volume, re-create it at a new first epoch, forget a node (with #13's set in `meta.rs`).

### R2-C2 — Medium
**A deletion's closing intent can never be undone, so a volume whose deletion stops after the intent can never be backed up again. The plan's risk text says the opposite.**
- Evidence: contracts.md:89 and 93 say `closing` is "set by a deletion", and nothing clears it. AuthorizeSnapshotRead refuses "the volume closing" (contracts.md:114).
- design.md:289-290 and decisions.md (Risks) instead say that after a failed deletion "its backups must be authorized again". Authorization is in fact refused for good.
- This is realistic. An administrator runs `delete`, gets `reader grant <op> waits for node <n>` (contracts.md:185) and does not retry. Or, under #73, the volume becomes protected between the intent and the mark, so the mark is refused. In both cases the volume keeps `closing` for its whole life.
- Fix: one of these.
  - Let an administrator's `snapshot-read reopen VOLUME` clear `closing`, conditional on the header not being marked deleting.
  - Or have AuthorizeSnapshotRead clear `closing` when the header is not deleting and `open` = 0. A later deletion's mark still compares the reader record, so it stays safe.
  - Then correct the risk text, and test that an abandoned deletion is followed by a new grant.

### R2-C3 — Low
**The acks are not bounded on the data plane's single metadata-client thread, so they can delay the target's ChangeIsr.**
- Evidence: `MetaClient` runs one call at a time on its thread (`flaminio-metaclient/src/lib.rs:45-46`), and the engine's ChangeIsr goes through it (`flaminio-node/src/engine.rs:89-91`).
- The plan bounds the poll to "one call outstanding" (contracts.md:169). It then has the engine send one AckSnapshotReadClosed per listed source, up to 1024 per page (contracts.md:131-133, 169-171), with no bound.
- A replica failure during that burst queues the ISR shrink, which writes wait for, behind up to 1024 transactions.
- Fix: the poll and its acks share #13's sweep rule: one reader call outstanding, none queued behind it (#13 design, The sweep). Or acks are batched per grant, ≤ 100 records per transaction, as the exclusion batch. Add a test that a ChangeIsr waits behind at most one reader call.

### R2-C4 — Low
**No node is actually started with `--reader-listen`.**
- Evidence: the lab starts nodes through `scripts/guest/node.sh`, whose argument list has no reader flag (`node.sh:41-54`). The environment line at `lab.sh:679` passes `LISTEN` only.
- #12 (clear) runs the DaemonSet's constant command, `--listen "$HOST_IP:7411"`, with `hostPort` 7411 only (#12 design.md:96-105).
- Neither appears in Files touched (design.md:203-205) or in a whichever-lands-second note. `lab.conf` alone does not start the listener, so step (2) of `backup.sh` has no reader to dial.
- Fix:
  - Add `READER_LISTEN` (WG:`$READER_PORT`) to `lab.sh:679` and `node.sh`, with a `node-scripts-test.sh` case.
  - Add a hand-off to #12: `--reader-listen "$HOST_IP:7413"` and `hostPort` 7413, added by whichever lands second.

### R2-C5 — Low
**Two lab steps cannot run as written.**
- **The store's TLS pair.** `scripts/guest/backup-store.sh up` "makes the store's TLS server pair in the VM with `pki.sh` from the lab CA" (design.md:251-252). `pki.sh` only makes a key and CSR and installs files (`scripts/guest/pki.sh:2-5`). The CA is on the workstation and signs through `ca_tool` in `lab.sh`'s `issue` (`lab.sh:548-553`), so a guest script cannot sign the pair.
- **The restore.** Step (7) restores "onto a new volume `bk-<stamp>-r`" (design.md:270-271). The worker and repository key are on fl-w, which runs no data node and so has no ublk device. The step says neither where the device is nor how the bytes reach it without the key leaving fl-w (design.md:256-257).
- Fix:
  - Have `lab.sh backup-store up` issue the server pair from the host with `issue "$VM_W" backup-store ... --server --ip`, or add a line to #16's `identities VM`, before it runs the guest script.
  - For step (7), restore to a file on fl-w, then copy that file (data only, no secret) to the new volume's device on fl-a, and compare SHA-256s.

### R2-C6 — Low
**Two details of collection contradict the code or the incarnation rule.**
- **The deletion prefix.** Collection deletes `snapread/<v>/<fe>/` after the header goes, and "DeleteVolume's header-absent branch and a creation's claim do the same" (contracts.md:102-104). Neither branch has a first epoch, since the header is absent (`service.go:527-534`, `:441`). #51 deletes the whole `snapshots/<volume>/` there (#51 contracts:141). Line 191, "earlier incarnations keep none past their deletion", relies on it.
- **The ordering.** "The oldest closed one (by create revision)" (contracts.md:100-101): `kv.Entry` carries only the revision of the last change (`meta/kv/kv.go:11-16`), and ADR 0002 keeps the interface narrow.
- Fix:
  - Delete `snapread/<v>/` on all three paths.
  - Order by the record's `Rev`, which is stable once closed, or by a `created` field in the grant record.

### R2-C7 — Low
**ListClosingSnapshotReads cannot answer from the read it names.**
- Evidence: contracts.md:131-133 calls it "a linearizable read of `snapsrc/<node>/`", returning the records "whose grant is closing". A source record holds `{device_uuid, store_generation, isr_version}` (contracts.md:91); the state is in `snapread/<v>/<fe>/grants/<op>`.
- The join's reads and their bound are unstated. `kv.List` also has no limit (`kv.go:39-40`), so each page lists the node's whole prefix.
- Fix: say that the handler Gets each distinct grant on the page once (≤ 1024 Gets), in the same linearizable read. State the cost of listing per page, and add a paging test with grants in mixed states.

### R2-C8 — Low
**A forgotten node that is still running never closes its reader sessions.**
- Evidence: "every reader method refuses" a forgotten node (contracts.md:174-175), the poll included. Its sources are excluded and the pins go (contracts.md:176-177).
- #13 does not stop the node's data plane: a forgotten node only gets `FailedPrecondition` on its calls (#13 design.md:233-235). So its open sessions keep serving reads after pin release and deletion, which goes against S3's "invalidate source read admission".
- Fix: a `FailedPrecondition` "node N is forgotten" answer to the poll makes the node close every reader session and refuse new ones. Test it in `flaminio-node`.

### R2-C9 — Low
**The grant lifecycle has two Rust copies, so the vectors may not come from the model the simulator checks.**
- Evidence: design.md:173-175 puts "grant states ... closing ... the deletion barrier" in `src/reader.rs`, driven by the simulator. design.md:184-186 says `meta.rs` "gains the same lifecycle", and `meta_vectors.rs` emits from `Metadata` (`examples/meta_vectors.rs:15`).
- With two copies, R1-C3's link between the Go code and the simulator-checked model is lost.
- Fix: keep the metadata side once, in `meta.rs`, driven by the simulator and emitted by the vectors. `reader.rs` then holds only the node and worker sides.

## Nits
- spec.md:44: "The worker takes its identity from its certificate" should say the node takes the worker's identity from the worker's certificate (contracts.md:222).
- design.md:266-267: `lab.sh node-fault` stops `$VM_B`'s node, a replica, not fl-a's target (`lab.sh:746-753`); say "as `node-fault` does for fl-b".

Reviewed commit: ca86b81d5e88f4024700f5e62733010f7c63dd05
