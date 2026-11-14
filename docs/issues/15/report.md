# Issue #15 — report

## Review rounds

### Plan review round 1 — 976e7d5

Scope: correctness against code and security of the plan, approved 2026-11-13; fixes are
changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High:** a failed `ADD_DEV` at a chosen id deletes whatever device holds that id
  (libublk's drop), and before 6.9 blocks on it; the id plan relied on skipping `EEXIST`.
  Evidence: libublk 0.4.8 `ctrl.rs:1258`, `:1164–1167`, `:1085–1096`, `:1626–1635`;
  design.md:79–80, :342–351.
- **R1-C2 — High:** outputs routed after the engine's lock is let go can deliver a read's `Done`
  before its `Piece`. Evidence: design.md:28–35, :176–180; `ublk.rs:557–606`, `:1341–1353`;
  engine.rs:1612–1630.
- **R1-C3 — Medium:** the plugin side of S9 had no boundary with #79, depended on #79 in a cycle,
  left Q5/U5 against the contract, and gave no fallback for an older daemon. Evidence:
  spec.md:116–129; design.md:281–304; decisions.md:120–124; #79 design.md:30, :142–144.
- **R1-C4 — Medium:** a reserved attach skipped for its deadline never ends its entry. Evidence:
  `ublk.rs:775–781`; `control.rs:137–144`; design.md:58–66.
- **R1-C5 — Medium:** the zero-copy fallback aborted the node on one volume's failed attach,
  against S5. Evidence: design.md:111–114; spec.md:72–73.
- **R1-C6 — Medium:** S5's space case, S2's stopping log and S7's 50 s bound had no named test.
- **R1-C7 — Low:** the stop flag alone does not end `event_loop` on a device that never started.
  Evidence: `ublk.rs:1670`; design.md:115–122.
- **R1-C8 — Low:** `next_wakeup` left out #75's QoS release timer. Evidence: design.md:168–175;
  #75 design.md:123–128.
- **R1-C9 — Low:** the limit error did not start with its volume. Evidence: design.md:55–56;
  spec.md:33.
- **R1-C10 — Low:** one node-wide `Mutex<Routes>` on every submit and completion. Evidence:
  design.md:29–31; `ublk.rs:517`.
- **R1-C11 — Low:** `flaminio-io del` always used `DEL_DEV_ASYNC`, which 6.6–6.8 lack. Evidence:
  design.md:352; ADR 0003.
- **Nits (code):** spec S4 named S11 for S12; decisions still said "to be approved" for the
  limit; the design did not say where the commit settings go.
- **R1-S1 — High:** #15 put in the I/O process what #74's approved `--privsep` forbids: ids,
  `ADD_DEV`/`DEL_DEV`, the held `/dev/ublkbN` open, `/run/flaminio` records, the cleanup.
  Evidence: design.md:72–141, :309–352; #74 spec S2, S3, decisions R1-C7, R1-C8, R2-S7, R5-C1.
- **R1-S2 — Medium:** S11's bar after a crash was not durable for a target holding no chunk of
  the volume. Evidence: `store.rs:1307–1333`; `replica.rs:162–164`; engine.rs:1645–1653;
  replication-protocol.md:58–62.
- **R1-S3 — Medium:** the contract did not require DEVICE to be a ublk disk the kernel names,
  against #74 S6. Evidence: device-lifetime-contract.md:17–30; design.md:88–89.
- **R1-S4 — Low:** `ran_epochs`' key was unspecified. Evidence: design.md:363–373.
- **R1-S5 — Low:** `list` could exceed #74's 4096-byte answer cap. Evidence: design.md:42–43,
  :256.
- **R1-S6 — Low:** no trust model for the records root acts on. Evidence: design.md:309–320,
  :354–356.
- **R1-S7 — Low:** S10's matching branch had no test. Evidence: design.md:489.
- **R1-S8 — Low:** new lab values reached remote shell strings unvalidated. Evidence:
  `scripts/lib.sh:157`, `:175`; design.md:357, :408–409, :509–511.
- **R1-S9 — Low:** S11 and intent 7 described code before #30. Evidence: main's
  engine.rs:1610–1621; `flaminio-node/Cargo.toml:16`.
- **R1-S10 — Low:** the accepted raw block gap understated its impact (blkext minors shared with
  NVMe). Evidence: spec.md:124; decisions.md:152.
- **R1-S11 — Nit:** the limit error did not name its volume (same root as R1-C9).

Changes/resolution:
- R1-S1: under `--privsep` the node takes one volume (limit 1, larger refused at parse); ids, the
  held fd, records and cleanup are direct mode's; add/start/stop/delete go through #74's helper;
  `device-info` reads sysfs; the lab's multi-volume and `ATTACH` checks run with `PRIVSEP=0`;
  whichever of #15 and #74 lands second adds the parse refusal and the run mark's directory.
  Recorded as a narrowing (decisions.md).
- R1-S5: `--max-volumes` 1 to 16; recorded as a narrowing.
- R1-C1: the vendored libublk with #74's failed-add hunk (whichever lands first vendors it); any
  errno moves to the next id; lab test with a foreign device at the next id.
- R1-C2: I/O outputs routed under the engine's lock; new S2 rule (a read's data before its end)
  and a test that fails if routing moves after the unlock.
- R1-C3: S9 states what #15 ships (device check through the contract, checks of new and existing
  mounts, legacy one-volume path until #79) and what #79 ships; "#79 enabled" is the build with
  #79; Q5/U5 superseded by the contract; contract's tool rule restated.
- R1-C4: the reservation is a guard; a skipped request is answered and its entry ends; test.
- R1-C5: a second vendored-libublk hunk waits for queue registration under
  `UBLK_F_AUTO_BUF_REG`; no watchdog, no abort; spec S5 names the case.
- R1-C6: tests for S5 space and its log, S2 stopping log, S7 4 TiB bound.
- R1-C7: `kill_dev` ends pending FETCHes, and `event_loop` breaks on the stop flag.
- R1-C8: `next_wakeup` covers #75's QoS release.
- R1-C9, R1-S11: `error: VOLUME: limit:`, matched with the requested volume.
- R1-C10: routes under the engine's existing lock; no lock added.
- R1-C11: `del` takes the release rule; tested on 6.8 and 7.0.
- R1-S2: durable per-volume run mark written before any `Hello`, replacing `ran_epochs`;
  simulator check and injected bug `volatile_run_mark`; engine test with no local chunk.
- R1-S3: contract and daemon require `/dev/ublkb<N>` with sysfs `dev` matching; loop-device test.
- R1-S4: resolved by the run mark's per-volume key; test.
- R1-S6: records' directory, opens, names and size fixed; `node.sh down` via `flaminio-io
  cleanup`; host tests.
- R1-S7: lab tests of the matching branch.
- R1-S8: revisions by hash, values validated and quoted, `FLAMINIO_IO` under the bench root;
  script tests.
- R1-S9: S11 and intent 7 restated against main; `decode_unverified` and its feature go.
- R1-S10: impact restated in Accepted risks; the owner's re-acceptance is pending; lab check
  added.
- Nits: fixed as listed.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — fe35c40

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 1's changes; fixes are changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 1's findings were all found fixed; the items below are new.

- **R2-B1 — Medium:** a leftover run-mark `.tmp` (a crash before the rename, or a failed write)
  makes every later attach of that volume on the node fail with `EEXIST`. Evidence:
  design.md:463–478; spec.md:163–171.
- **R2-B2 — Low:** no test can fail if the mark's fsyncs are dropped; no test of a failed write or
  a malformed mark; the new `epochs/` directory is not made durable in its parent; the refusal
  test names the engine, which never reads marks. Evidence: design.md:459–479, :699–702.
- **R2-B3 — Low:** #15's `checkUblkDevice(path)` is not #74's function, and the plans give
  different codes for the same refusal. Evidence: design.md:343–345, :360–361; #74
  design.md:188–192.
- **R2-B4 — Low:** the daemon's own device check has no test. Evidence: design.md:122–125.
- **R2-B5 — Low:** the routing-order test deadlocks against the fix as worded. Evidence:
  design.md:596–600; `ublk.rs:1275–1277`.
- **R2-B6 — Low:** `flaminio-io cleanup` can delete a device mid-attach beside a live node; its
  wait before 6.9 is unstated and `node.sh down` lost its mount guard; no script test; the
  any-errno id walk has no pace and burns ids on unrelated failures. Evidence: design.md:103–107,
  :399–402; `node.sh:22–25`; `ublk.rs:896–899`.
- **R2-B7 — Low:** #79 pins the contract at `8af7aeb`, which round 1 changed, and the contract has
  no revision marker. Evidence: device-lifetime-contract.md:5–8; #79 design.md:30–35.
- **R2-B8 — Low:** under `--privsep` the device thread (`run_target`'s callback) and S5 are not
  reconciled with #74: no `run_target` there, a fatal `start` limit, and no test of the sysfs
  `device-info`. Evidence: design.md:500–529; spec.md:84–86; #74 design.md:165–179.
- **R2-N1 — Nit:** #75's QoS timer described as today's code.
- **R2-N2 — Nit:** S9's raw block rule reads as unconditional under `--privsep`.
- **R2-N3 — Nit:** record names accept leading zeros, unlike #74's grammar for the same ids.

Changes/resolution:
- R2-B1: the leftover `.tmp` is unlinked before each exclusive create and on any failed write,
  and stray ones at start; tests with a leftover `.tmp` and a read-only state directory.
- R2-B2: a `MarkFs` trait and a test of the write, file `fsync`, rename and directory `fsync`, in
  order, before `Input::Marked`; created directories `fsync`ed in their parent; `flaminio-io`
  tests for a failed write and a malformed mark; the across-restart refusal test moved to
  `flaminio-io`.
- R2-B3: #74's `checkUblkDevice(path)` kept as #74 defines it; #15's comparison of the reply's
  MAJ:MIN with sysfs and with `fstat`, and its diskseq, stated apart; one code, #74's
  `codes.Internal`, for a device that fails the check (kept, as it fits a node fault; #74 needs
  no change); a mount in place on another device stays `FailedPrecondition`; tests assert both.
- R2-B4: the open step takes the path and sysfs root; a test with a symlink, a character device
  and a mismatched rdev fails each attach and keeps no fd.
- R2-B5: the hook releases after a fixed 200 ms and never waits for the queue thread.
- R2-B6: an exclusive `flock` on the records directory for the node's life, which `cleanup` must
  take (it refuses while held); `cleanup` waits up to 5 s for its `DEL_DEV` threads before 6.9;
  `node.sh down` keeps its mount guard for `$dev` and every recorded device; script test of
  `down`; the id walk moves on only when `ublkc<N>` exists, one add per 250 ms until
  `add_device`'s deadline.
- R2-B7: a revision marker in the contract (revision 3, this round); decisions record that #79
  must re-pin and add #15's comparison to its client.
- R2-B8: the device thread's `--privsep` variant (#74's attach steps) stated, for the second of
  #15 and #74 to land, with tests including the sysfs `device-info`; #74's fatal `start` limit
  listed as an S5 exception under `--privsep`.
- R2-N1, R2-N2, R2-N3: fixed as reported.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 28711dd

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 2's changes; fixes are changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 2's findings were all found fixed; the items below are new.

- **R3-B1 — Medium:** the id walk stalls on an id held by a deleted but still-open device:
  `ublkc<N>` is gone, so the walk retries that id until the 10 s deadline, and every attach on
  the node fails until the device is closed (likely on 6.9+, where `DEL_DEV_ASYNC` drops the
  record at once). Evidence: design.md:97–112, :184, :445–448; #74 design.md:84–85, :133–134.
- **R3-C1 — Low:** the new `node.sh down` test keeps `del --id` before the stop for a build
  without records, against #74's test in the same file ("`down` stops the unit before `del`"),
  and the branch has no detection and no need. Evidence: design.md:817–820; #74 design.md:209–211,
  :290.
- **R3-C2 — Low:** the plugin's comparison has no test seam, so its three mismatch cases cannot
  each be reached, and the sysfs case passes with the comparison deleted. Evidence:
  design.md:731–736.
- **R3-C3 — Low:** the `--privsep` device thread leaves out direct mode's udev wait before it
  answers, and does not state its steps after the queue join; `delete` through the helper can
  block with no bound before the entry is stopped. Evidence: design.md:126, :177–190, :564–570;
  #74 outcome 3.
- **R3-N1 — Nit:** the records `flock` is not limited to direct mode.
- **R3-N2 — Nit:** "Any failure of steps 2–4 is `codes.Internal`" includes a changed second
  `device-info`, which is a state change. Evidence: design.md:362, :377–378.
- **R3-N3 — Nit:** `MarkFs` lists four steps, but the tests also drive the directory creation,
  the `.tmp` unlink and the exclusive create through it. Evidence: design.md:509–511, :785–789.

Changes/resolution:
- R3-B1: an id is held when `GET_DEV_INFO` at it answers (a live device or a deleted one still
  open); a held id is skipped at once; an id not held is tried at most 4 times, then skipped; the
  next attach starts above the last id tried. Tests: the walk on a fake kernel; on the lab's 7.0,
  a deleted but open device at the next id, and the attach takes N + 1 within its deadline.
- R3-C1: following #74, `node.sh down` stops before it deletes; the branch for a build without
  records is dropped (main's own `node.sh` keeps its `del --id`), and the test checks the mount
  guard, the stop, then `flaminio-io cleanup`.
- R3-C2: the comparison takes the device path and the sysfs root; the test (a block node named
  `ublkb<N>` with a loop device's rdev, a fake sysfs `dev`) reaches each case, each
  `codes.Internal` with no probe, and fails if the sysfs comparison is removed.
- R3-C3: the variant waits for `/dev/ublkb<DEV>` by `stat` before answering; after the join it
  purges the router, removes the arrivals counter, and ends or stops the entry under its lock,
  then calls `delete`, whose wait is bounded only by the device's last close, as #74 accepts.
  Tests for a late device node and a blocking `delete`.
- R3-N1: the `flock` is direct mode's only.
- R3-N2: `codes.Internal` covers only the device checks (steps 2 and 3, and after mounting); a
  changed second reply is answered as that state is when first seen. Design, spec and contract
  changed; the contract is now revision 4.
- R3-N3: `MarkFs` lists every operation the tests drive.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — baee5c8

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 3's changes; fixes are changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 3's findings were all found fixed; the items below are new.

- **R4-B1 — Low:** `node.sh down`'s new text and test name no mode, and collide with #74's
  assertion in the shared test file: under `PRIVSEP=1` the cleanup finds nothing, and #74's `del`
  of `/dev/ublkc<ID>` must stay. Evidence: design.md S10 Scripts, Tests; #74 design.md:139–142,
  :290.
- **R4-B2 — Low:** an unheld id skipped after 4 tries lets a failure unrelated to the id burn
  about 40 ids per failed attach, with no rule for the kernel's id range (2^20), so a node that
  keeps failing can exhaust it. Evidence: design.md:103–113, :224.
- **R4-B3 — Low:** the `--privsep` failure path still ends the entry, and answers the attach,
  only after an unbounded `delete`. Evidence: design.md:587–589, :209–210.
- **R4-B4 — Low:** the claim that `GET_DEV_INFO` answers for a deleted, open device is tested
  only on 7.0 with `DEL_DEV_ASYNC`; 6.6–6.8 delete synchronously. Evidence: design.md:696–698;
  ADR 0003.
- **R4-N1 — Nit:** "answered as that state is when first seen" cannot hold for "none" or
  "another lifetime". Evidence: spec.md:142–145; design.md step 4; the contract.

Changes/resolution:
- R4-B1: `down` stated per mode: mount guard and stop in both, then `flaminio-io cleanup` in
  direct mode and #74's `del` under `PRIVSEP=1`; the test asserts each order; the second plan to
  land turns #74's direct-mode assertion into `cleanup` and keeps its `PRIVSEP=1` case.
- R4-B2: the cap of 4 tries is cut: an unheld id is tried until the deadline and skipped only as
  the attach fails (one id per failed attach); one attach skips at most 40 ids, all held but the
  last; no id above 1048575 is tried, and once none is left every attach is refused until a
  restart (no wrap, which would reuse ids in a run). Spec S2; fake-kernel test.
- R4-B3: the failure path stops, joins, drops the `UblkDev`, then answers the attach, sends
  `Detach`, purges the router, removes the counter and ends the entry, and only then sends
  `delete`; tested with a blocking `delete`.
- R4-B4: a 6.8 test (synchronous `DEL_DEV` on the test's thread, the device held open; the attach
  takes N + 1). Checked against the 6.1 source available (`eve-kernel` 6.1.112): `GET_DEV_INFO`
  answers by `idr_find` until the release, but a synchronous `DEL_DEV` holds `ublk_ctl_mutex`
  across its wait and `ADD_DEV` takes that mutex; for 6.6–6.8 this is unchecked, stated under
  Risks, and the 6.8 test settles it.
- R4-N1: reworded to "the state's code: stopping `FailedPrecondition`; none, starting or another
  lifetime `Unavailable`" in spec, design and contract (now revision 5).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — dea201b

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 4's changes; fixes are changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 4's findings were all found fixed; the items below are new.

- **R5-1 — Low:** "at most 40 ids skipped per attach" contradicted "a held id moves to the next
  at once": only unheld retries were paced. Evidence: design.md:104–112, :690–692.
- **R5-2 — Low:** the ceiling test missed its edges (a held skip at 1048575, `first_id` above
  1048575 at start, a successful add at 1048575), and the spec named no code for the refusal.
  Evidence: design.md:696–698; spec S2.
- **R5-3 — Low:** the 6.8 test was the only check for a possible rule violation (a synchronous
  `DEL_DEV` holding `ublk_ctl_mutex`, blocking every `ADD_DEV`, as in the 6.1 source), with no
  outcome stated if it failed. Evidence: design.md Risks, :722–724; 6.1 `ublk_drv.c:1758–1781`,
  `:1673`.
- **R5-N1 — Nit:** contract revision 5 was named by commit message, not hash. Evidence:
  device-lifetime-contract.md:10–16.

Changes/resolution:
- R5-1: every `ADD_DEV` is paced, one per 250 ms, held id or not (simpler than two bounds: one
  rule, one bound to test); one attach tries at most 40 adds and skips at most 40 ids.
- R5-2: the refusal is `error: VOLUME: limit: no ublk id left in this run; restart the node`,
  mapped to `ResourceExhausted` by the plugin's existing `limit:` rule (spec S2); tests for the
  three edges beside the failing add, each refused with the ceiling's text.
- R5-3 (the coordinator's decision, under the user's "otherwise figure it out"): the 6.8 test is
  a gate before #15 ships, recorded in the report. Pass: nothing changes, and the risk stays
  named for the unchecked 6.6 and 6.7. Fail: before 6.9, the default limit is 1 and a larger one
  is refused at start (spec S1, a kernel-conditional narrowing, as S5 cannot hold there with
  several volumes); 6.9 and later unchanged. #74's reading of 6.8.12 does not settle it.
- R5-N1: revision 5 is `95a1dba`, which #79 pins; this round changed only that line.
- Format (noted by the reviewer): the orphaned bullet in the design's Order rejoined its list.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 5b20c61

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 5's changes (diff head `d39e372`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 5's findings were all found fixed; the items below are new.

- **R6-1 — Low:** "at most 40 adds and 40 skipped ids per attach" is off by one against the loop
  it copies, which checks the deadline after a failed add and before the wait, so a 41st add runs
  at about 10 s; and it was unsaid whether the ceiling's refusal after a held skip waits the
  pace. Evidence: design.md:111–112, :125–128, :134–135, :719–720; `ublk.rs:891–899`.
- **R6-2 — Low:** spec S1 did not carry the failing gate's outcome in full (the refusal at start,
  its error, an unparsable release counting as before 6.9), and nothing said that branch's tests
  exist only on that outcome. Evidence: spec.md:30–32; design.md:56–60, :708–709.

Changes/resolution:
- R6-1: the loop is kept as today: at most 41 adds per attach (40 waits, the last at about
  10 s), at most 41 ids skipped; the S2 test says 41. The ceiling is checked when the next id is
  chosen, so its refusal after a held skip at 1048575 comes at once, without the wait; tested.
- R6-2: spec S1 states the outcome in full; that branch and its tests are written only if the
  gate fails (spec S1, design S1).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — cd787d9

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 6's changes (diff `5b20c61..2cb1f9d`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 6's findings were both found fixed; the items below are new.

- **R7-L1 — Low:** the ceiling's refusal "at once" was not tied to the loop's order: design.md:111
  still said a held id moves on "at the next paced attempt", so a next id chosen after the wait
  would make the refusal wait 250 ms, against R6-1. Evidence: design.md:111, :134.
- **R7-L2 — Low:** when the held add at 1048575 is also the one that ends the deadline (the
  41st), "fails with the last error" and "never the last add's error" disagreed. Evidence:
  design.md:125, :140.

Changes/resolution:
- R7-L1: the order after a failed add is pinned: choose the next id (the next if held, the same
  if not), check the ceiling, then the deadline, then wait 250 ms and add. "At the next paced
  attempt" is gone. The S2 test asserts on a fake clock that no wait passes between the held add
  at 1048575 and the refusal.
- R7-L2: the ceiling check comes before the deadline's, so the ceiling's text wins; a new S2
  case covers the held add at 1048575 that is also the 41st.

Commits: `fc755c0` (the fixes), then `7ba974a` and `ce26a4f`, which only rewrap lines of the
design that the fix left over 100 columns.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — fef9ba1

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- Both round-7 findings fixed (R7-L1 the pinned order; R7-L2 the ceiling text wins); the rewrap
  commits changed no wording.
- **R8-L1 — Low:** the S2 id test (design.md:732–733) still said a held id "moves to the next id at
  the next attempt". Evidence: design.md:124–131 pins "straight after the failed add".

Changes/resolution: R8-L1 reworded to "straight after the failed add"; recorded in decisions.md as
round 8, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 3592050cc4da3cacf4c9a759692907534fbcccb3

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
