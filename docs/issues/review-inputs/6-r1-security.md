# Issue #6 — plan review, round 1, focus: design-security

Scope: docs/issues/6/ (decisions.md, intent.md, spec.md, design.md), checked against the code at
HEAD (the step-2 code from #25 is in) and against #15's plan (flaminio-15, e36882b).

### R1-S1 — High
**The startup `--attach` retry starts a second target under an epoch that already had one.**
The design ("Time limits", bullet "The attach at startup") resends the same token every 5 s for
up to 5 minutes after *any* failure. The code does that (`ublk.rs:747–768`, the same `token` each
time). A failure can come after the target has started: `start_target` has run, the target has
sent its `Hello`s and begun reconciling (digests and repairs at `(E, seq)`), and then
`run_device` fails. Causes include `add_device` failing for 10 s (ublk_drv not loaded,
`--zero-copy` on a kernel without `UBLK_F_AUTO_BUF_REG`), libublk's 3 s wait for `/dev/ublkcN`,
or `START_DEV` failing. `attach_and_serve` then detaches the target (`ublk.rs:888–896`). The
retry's `GetVolume` still shows epoch E, so `start_target`'s check passes
(`engine.rs:1617`, `claims.epoch == v.epoch`), and a new `Target` starts at E with
`next_session` 1 and `next_seq` 0 (`target.rs:484–486`). That can happen up to about 20 times in
the 5 minutes. It breaks the binding rule "One target per epoch, for its whole life … a target
that restarts, even on the same node, must not reuse its epoch" (`replication-protocol.md:66–73`).
The node plugin states the same rule (`node.go:53–56`, "A target never restarts under the epoch
it had"). Replicas would treat the new target's sessions and versions as the old one's
(`replica.rs:174–186`).
The spec has no rule for this retry, and neither document has a test or a simulator check for
it. The CSI path is safe because each NodeStage takes a new epoch (`node.go:70`).
#15's run mark refuses such a retry once #15 lands (#15 S11: "So does a startup retry after its
device failed"). #6's own plan, and the code at HEAD, have the gap until then.
**Fix:** retry only failures that came before `Attached`: the metadata service did not answer,
or the engine sent `AttachFailed`. Stop for good once a target has started at the token's epoch,
for example with a typed error from `attach_and_serve`. Add the retry to the spec (S2) with
this restriction. Add a test where the device fails after `Attached` and no second `Attach` is
sent, and say that #15's run mark later enforces the same rule durably.

### R1-S2 — Medium
**The rules that keep a stale request from running have no test.** Spec S2 says "The node skips
a request it would begin with no time left … and answers with that reason". The design says why:
otherwise such a request would "run after the next detach, with a token nobody uses any more"
(design, "Skipping"). Neither the spec's nor the design's test list checks that the main thread
skips such a request. The control-socket test only checks that the request carries a passed
deadline (`control.rs:220–231`), not that the node never begins it (`ublk.rs:799–806`). Other
rules with no test:
- the steps bounded by `deadline − START_LIMIT` (the wait in `attach_and_serve` and
  `add_device`'s `by`);
- the engine's "already attaching" refusal (design, "One attach of a volume at a time";
  `engine.rs:649–655`);
- the startup retry (R1-S1).
**Fix:** move the main loop's decision whether to begin a request (`steps_by`, the skip) into a
function, and test it: a request queued past `deadline − START_LIMIT` gets the "time ran out"
answer, and no `Input::Attach` reaches the engine. Add an engine test where a second `Attach`
of a volume still attaching gets `AttachFailed("already attaching")`, and the first attach still
completes.

### R1-S3 — Low
**The S1 lab check can leave a volume's first block overwritten.** As planned, `partscan.sh`
saves the first 4 KiB, writes an MBR over it with `dd`, detaches, attaches again, checks, and
only then writes the 4 KiB back (spec Tests "S1, once it lands"; design "Lab check"). Any failure
in between leaves the MBR in place: a failed attach, a `p1` that appears, or the script
exiting. On XFS, whose superblock is at offset 0, that corrupts the filesystem. Nothing says
the writes are flushed before the detach.
**Fix:** run the check on a scratch volume it creates, or refuse one that is mounted or in use.
Restore the saved block from an `EXIT` trap. Write both times with `conv=fsync` (buffered:
the device's logical block is 4 KiB).

### R1-S4 — Low
**S1 covers only partitions. The intent's threat has another route, which the plan does not
name.** The intent motivates S1 with tenant-written metadata that reaches the host ("a partition
table that a tenant writes … shows up on the host … and udev probes them"). With the flag on,
udev still probes the whole device. A tenant's LVM PV header or md superblock at the start of a
raw-block volume can then be auto-activated or assembled by the host (LVM event activation,
`mdadm --incremental`). That holds the ublk device open, which blocks the node's detach. The
repo has no udev rule or LVM filter for `ublkb*`.
**Fix:** name whole-device probing in the intent's "Out of scope" as a follow-up issue: a udev
rule for `ublkb*` (for example `ENV{SYSTEMD_READY}="0"` and skipping the LVM/md rules) and an
LVM `global_filter` that rejects `/dev/ublkb.*`.

### R1-S5 — Low
**#6's reason for waiting on upstream conflicts with #15's plan.** #6's Decisions say "S1 waits
for upstream. We do not carry a patched copy of libublk (about 11 000 lines)". #15's design has
whichever of #15 and #74 lands first vendor libublk 0.4.8 into `dataplane/vendor/libublk`
(`[patch.crates-io]`), with hunks kept until upstream takes them (#15 design, S2 "A failed
`ADD_DEV` deletes nothing", and Risks "The vendored libublk"). After that, #6's reason no longer
holds, and S1, a tenant-isolation measure, waits for nothing. #15 also rewrites `add_device`,
where #6's S1 change goes.
**Fix:** say in #6's Decisions that once a vendored libublk exists, S1 adds ublk-org/libublk-rs#72's
hunk to it, or record why not. Say that S1's `ctrl_flags` change goes into #15's `add_device`.

## Nits
- **The design gives the wrong reason for the generation check** (design, "A generation for each
  attach", and Tests, engine bullet 2: "without the generation check they would start a target
  at E"). At HEAD, `start_target` refuses a token whose epoch is not the volume's
  (`engine.rs:1617–1623`). Without the generation, the old answers would make the new attach
  fail with `AttachFailed`; they would not start a target at E. The test still fails without the
  change. But the text credits the generation with barring an old-epoch target, when the token
  check does that. Reword it: the generation keeps a late answer from failing or filling in the
  next attach.

Reviewed commit: e0734f703ee3d372afec9314054b917658e2b06c
