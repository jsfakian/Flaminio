# Plan review: issue #15, round 1, focus: design-security

Scope: everything in `docs/issues/15/` except `report.md`, at HEAD of `issue/15-volumes-per-node`.
Code checked in that worktree. Where the plan relies on #30, also checked on `main`, which has #30
merged and is the base the plan says it builds on. Cross-plan: #74
(`/tmp/flaminio-plans-2026-11-12/74`), #79 and #12.

### R1-S1 — High
**#15 contradicts #74's approved privilege separation, and never mentions it.** #15 puts these
operations in the I/O process:
- choosing ublk ids (a `/sys/class/ublk-char` scan, records, `--id`; design.md:72-81);
- `add_device` and `kill_dev`, and `DEL_DEV`/`DEL_DEV_ASYNC` (design.md:84-141);
- the held `O_RDONLY` open of `/dev/ublkbN` that every attach needs (design.md:88-89);
- the records under `/run/flaminio/ublk` (design.md:309-320);
- the startup cleanup, through `GET_DEV_INFO` and `DEL_DEV` (design.md:321-352).

Under #74's `--privsep`, which is the lab's default (`PRIVSEP=1`, #74 design.md:200), each of
these is forbidden:
- the I/O process never opens a disk, a ublk device path or `/dev/ublk-control` (#74 spec S2,
  spec.md:32);
- ids belong to the helper, which "owns at most one device" (#74 spec S3; design.md:80-82);
- the I/O process never writes `/run/flaminio` (#74 decisions.md:100);
- there is no ledger: crash leftovers are the administrator's to delete (#74 decisions.md:102);
- `node.sh up` refuses `ATTACH` (#74 R2-S7). #15's S1 test and its S11 lab test both use
  `node.sh up ATTACH=`.

The I/O user has no supplementary groups, so with both plans built, the held-fd open and the
record write fail. Every attach fails then, the first one included. #74 has already recorded
that #15 must change its one-device rule (#74 design.md:344-346; ADR proposal, Consequences).
#15 takes on neither that change nor a refusal.

Fix: state #15's relation to #74. Either:
- (a) under `--privsep`, `--max-volumes` is 1 and #15's device thread, records and cleanup run
  in direct mode only, with `multi-vol`, `node-fault`, `bench-ab` and the S11 lab test run with
  `PRIVSEP=0`; or
- (b) specify the helper's extension here: one helper-chosen owned id per volume, bounded by the
  helper's own limit; the held block-device fd, rdev and diskseq taken by the helper; the records
  and the startup cleanup in the helper; stop and delete per owned id. Then amend #74's ADR
  proposal.

Either way, the device identity CSI acts on must come from root (see R1-S3).

### R1-S2 — Medium
**S11's "after a crash, an attach with the old token fails" has no durable bar when the target
holds no chunk of the volume.**
- The bar across restarts is "at or below the local replica's durable fence" (spec.md:144-147;
  design.md:370-373). `ran_epochs` lasts only one run.
- `Store::fence` writes a `Fence` record only for a volume that has chunks here. Otherwise it
  returns `Ok(())` and writes nothing (store.rs:1307-1333).
- `ReplicaCore::fenced` is 0 for a volume it does not know (replica.rs:162-164).
- A target need not hold a chunk: its local Hello covers only the chunks placed on it
  (engine.rs:1645-1653), and a volume without topology "is reachable from every node"
  (controller.go:253-254). #17's drains also move replicas off a target.

After a crash, the startup `--attach` token file, or any socket caller holding the old token,
restarts a target at the same epoch. Session numbers restart at 1 (target.rs:426), and that is
the collision replication-protocol.md:58-62 forbids. Even with local chunks, the rule needs the
local fence durable before any remote Hello at that epoch leaves the node. The design does not
state that order. The only test (design.md:503-504) has a local replica.

Fix: one of:
- make the bar durable for a target without chunks: an fsync'd per-volume "highest epoch run
  here" mark in the node's state, written before the target's first Hello;
- or narrow S11's claim and list the residual as an accepted risk.

Also state the local-fence-first order. Add an engine or lab test with a target that holds no
chunk, and the simulator check and injected bug that the Principles require for this rule.

### R1-S3 — Medium
**The device-lifetime contract never requires the device to be a ublk disk, which contradicts #74
S6.**
- The contract allows "a canonical /dev block-device path", calls the path diagnostic, and has
  CSI compare the opened fd only against the daemon's own reply (device-lifetime-contract.md:17-30).
- The daemon records whatever its open by path finds. It sets no `O_NOFOLLOW`, checks no
  `S_ISBLK`, and does not compare the result with `/sys/class/block/ublkbN/dev` (design.md:88-89).

#74 S6 requires `^/dev/ublkb(0|[1-9][0-9]{0,6})$`, with the sysfs dev of `ublkb<N>` equal to the
rdev (`checkUblkDevice`), plus a 4096-byte cap on answers (#74 spec.md:56-59; design.md:188-196).
#74 places that check in `device()` and in the raw-block `NodePublishVolume`, which #15 rewrites
(design.md:281-294). Under #74 the data plane's answers are untrusted. A compromised I/O process
can answer `/dev/sda 8:0 <sda's diskseq>`: that passes every comparison the contract lists, and a
raw-block bind would then hand a host disk to a pod. #79 is told to depend on this contract
alone.

Fix:
- In the contract, DEVICE must be `/dev/ublkb<N>`, and `/sys/class/block/ublkb<N>/dev` must equal
  both the reply's MAJ:MIN and the opened fd's rdev.
- The daemon opens with `O_NOFOLLOW`, and checks `S_ISBLK` and the sysfs dev before it publishes
  running.
- Add a contract test: a reply that names another block device, such as a loop device, is
  refused with no effects.

### R1-S4 — Low
**The key of `ran_epochs` is unspecified.** "An epoch already run" (design.md:363-373;
spec.md:144) reads as a set of epochs. Epochs are per volume, and fresh volumes share small
epoch numbers. A set keyed by epoch alone would let volume A's run refuse volume B's attach.
The only test is "an epoch already run is refused" (design.md:502).

Fix: key it by (volume, epoch). Test that B is accepted at an epoch A has run, and that A is
refused at that epoch again.

### R1-S5 — Low
**`list` can exceed #74's 4096-byte answer cap.**
- `--max-volumes` has no maximum (design.md:42-43).
- `list` prints every entry (design.md:256), and an entry is up to 138 bytes (a 128-byte name and
  a state).
- Above about 29 entries the answer passes #74's cap (#74 decisions R2-S5). NodeGetInfo then
  fails (spec S9).

Fix: bound `--max-volumes` at parse, or give NodeGetInfo a fixed-size `limit` answer. Test the
longest answer.

### R1-S6 — Low
**The trust model of the records is unspecified, yet root acts on them at start.**
- No mode or owner is given for `/run/flaminio/ublk`.
- No `O_NOFOLLOW`/`O_EXCL` for `.<id>.tmp`: only the final record is `O_EXCL`.
- No size bound or grammar for parsing a record or its name.
- `node.sh down` "deletes the recorded devices" without the startup cleanup's dead and
  dev_info match, or a decimal check before `del --id` (design.md:309-320, 354-356).

#12 shares `/run/flaminio` with the CSI node pod, and accepts admin-created hostPath PVs of it
(#12 design.md:205-210).

Fix: create the directory root-owned 0700 and check it with lstat. Open records and the tmp file
with `O_NOFOLLOW|O_CREAT|O_EXCL`. Cap record size. Names must be `^[0-9]{1,7}$`. `node.sh down`
uses the same matching cleanup. Test a symlinked `.tmp`.

### R1-S7 — Low
**S10's rule "dead and matching what it recorded; others never touched" has no test for its
matching branch.** The tests cover a live foreign device only (design.md:489). Nothing tests a
dead device at a recorded id whose dev_info (owner, flags, queues) differs, or a dead device with
no record.

Fix: add both lab-VM cases. Each must be left in place.

### R1-S8 — Low
**New lab values reach remote shell strings unvalidated and unquoted.** These are:
- `bench-ab`'s `A`/`B` revs and the `FLAMINIO_IO` path built from them (design.md:509-511), which
  `node.sh` runs with `sudo`;
- `DEV`, which is now taken from the data plane's answer (design.md:357, 408-409);
- `VOL2`/`OTHER`.

They pass through `ENV_VARS`, which `run_on_vm` and `run_detached` interpolate raw
(scripts/lib.sh:157, :175; `node_env` at lab.sh:668-671). AGENTS.md says ids reaching shell
commands are validated and quoted.

Fix:
- Resolve each rev with `git rev-parse --verify "$rev^{commit}"` and name its path by the hash.
- Check `DEV` against `^/dev/ublkb[0-9]+$`.
- Quote each new value with `printf %q`.
- Have `node.sh` check that `FLAMINIO_IO` lies under the bench root.

Add script tests in `scripts/tools`.

### R1-S9 — Low
**S11 is written against code before #30.** On `main`, which #15 builds on:
- the engine already checks the token's volume, node and epoch, but through `decode_unverified`
  (engine.rs:1610-1621, "Its signature is not checked here (#15)");
- `flaminio-node` enables `flaminio-net`'s `unverified` feature (Cargo.toml:16).

Intent §7 and design S11 describe the code before #30, and never name this path for removal.

Fix: S11 removes `decode_unverified` and the `unverified` feature, and takes `first_epoch` from
the verified claims. Restate intent §7 against `main`.

### R1-S10 — Low
**The accepted raw-block gap understates its impact.**
- The spec says the published `major:minor` "never serves another volume" (spec.md:124).
- The accepted risk is "the raw block gap" (decisions.md:152).

ublk registers only a char region for itself. Its disks take dynamic blkext minors (major 259),
from the same pool as NVMe: this host's `nvme0n1` is 259:0. So a stale bind in the gap can open
any blkext disk on the host, a host NVMe namespace included.

Fix: state that impact in the accepted risk, for the owner to re-accept. Have the S10 raw-block
lab test also assert that the stale bind opens no non-ublk disk.

### Nit
- **The limit error does not name its volume** (`error: limit: N volumes ...`, design.md:56),
  against spec.md:33. `limit` is a valid volume id, so `error: limit: epoch: ...` is ambiguous
  with that prefix. Use `error: VOLUME: limit: ...`, and match on the requested volume.

Reviewed commit: 976e7d54e7f7e7af712c1979507a73748903a519
