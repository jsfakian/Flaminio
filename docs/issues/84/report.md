# Issue #84 — report

## Review rounds

### Plan review round 1 — 733065c45b3805b3353ba2a2c2a05225e83d96fa

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84c1 (design-code), r84s1 (design-security).

- **Review84-R1-C1 — Medium:** Exact op/flag/range/registration contract and discard advertisement unspecified Evidence: design.md18–25,79–89; ublk.rs950–966 BASIC-only params.
- **Review84-R1-C2 — Medium:** No usable cacheless kernel-consumed FLUSH/FUA observation oracle Evidence: design.md36–46,92–93; Linux writeback_cache_control and v6.6 blk-flush.c.
- **Review84-R1-C3 — Medium:** Matrix/helper path CLI/result schema deferred; need bounded exact run-owned disposable guards and #83 admission Evidence: design.md51–63,83–84,116–120.

Changes/resolution: Only summaries of the findings were kept; each was re-derived from the cited lines and the code, and all three apply.
C1: the design now fixes `node_params` (today's values, `UBLK_PARAM_TYPE_BASIC` alone) and `classify_node_request` with four ordered rules: registration failure EIO, flag bits outside six named ones EOPNOTSUPP, op, range EINVAL. Discard stays unadvertised and its arm stays a hint; spec S5 says so.
C2: on a cacheless device the kernel consumes FLUSH and FUA, so the two-queue kernel FLUSH test, the "hypothesis" and the follow-up drain are cut. Spec S3 rests FLUSH and FUA on S2 alone. The kernel oracle is the advertised limits plus a `block:block_rq_issue` trace that shows the check's writes and no flush, FUA or discard at the device, with the store's flush on the data disk as a control. The existing `ack_before_shrink` and `late_join` mutations remain the caught early completions; no new safety rule.
C3: the Rust helper and the lab fault matrix are cut. The appendix kernel-check.md fixes the shell check's CLI, exit codes, result JSON and reasons; it takes no volume or device, creates and deletes its own run-named volumes, guards the device before each write step, and reports `unsupported` where the node's own admission (including #83's) refuses. Script tests cover each refusal and failure.
The design's Risks keep three open questions: unknown flag bits, the measured matrix, and where the check runs once #83 lands. No test or lab check was run; no approval or accepted risk is inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 166c0167157527e2ff4b62283b90434b59342603

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84c2 (incremental design-code), r84s2 (incremental design-security).

r84c2: no comments, Nits included. r84s2 confirmed R1-C1, R1-C2 and R1-C3 fixed, and reported:

- **Review84-R2-S1 — Medium:** The restore restart inherits the caller's `FORMAT`; `FORMAT=1` would reformat NODE's data disk. Evidence: design.md:174-175 against 178-180; kernel-check.md:17; scripts/lab.sh:679; Makefile:243-244; scripts/guest/node.sh:47.
- **Review84-R2-S2 — Medium:** The guest script accepts `VOLUME`, `DEV`, `SIZE` and `RUN` from its caller, unvalidated, and its guards do not require the run's volume. Evidence: kernel-check.md:22-25,56-59; spec.md:58-59; design.md:205-207; scripts/lib.sh:157.
- **Review84-R2-S3 — Low:** Cleanup deletes a volume the run did not create when create answers `AlreadyExists`. Evidence: kernel-check.md:9-13; meta/service/service.go:142.
- **Review84-R2-S4 — Low:** "Nothing is attached on NODE" is checked once; the three restarts tear down a device attached meanwhile, reported as `unsupported`. Evidence: kernel-check.md:14-21; design.md:103,178-180; scripts/guest/node.sh:29,44; control.rs:95; scripts/lab.sh:1796.
- **Review84-R2-S5 — Low:** An interrupted run has no cleanup path, on the host or in the VM. Evidence: design.md:178-182; kernel-check.md:11-13,19,39-40,49,56.
- **Review84-R2-S6 — Low:** The classifier's "offset + length exceeds `size`" can overflow. Evidence: design.md:57-60,141-143; engine.rs:1702.
- **Review84-R2-S7 — Low:** Under #83 the check answers `refused`, not `unsupported` as the design, Risks and fixture say. Evidence: kernel-check.md:14-16 against 19-21,77; design.md:106-107,164,203-204.
- **Review84-R2-N1 — Nit:** Whether step 6 (`blkdiscard`) is a guarded step is not said. Evidence: kernel-check.md:22-25,35.

Changes/resolution: each finding was checked against the cited lines and the code; all eight apply.
S1, S4: the check no longer restarts any node. One run measures the mode and queue count NODE's node already runs with, read from its process, with one volume; both modes take two runs, with the operator's existing `make node-up` between them, written with `FORMAT=0` in the Lab section. The check reads no node setting from its caller. A volume found attached, at the precheck or at the attach, is left alone and fails the run as `guard`, named in the result. Fixtures: the caller's `FORMAT=1` yields no `node.sh up` from the check and none with `FORMAT=1`; a foreign attachment.
S2: the guest takes `RUN` alone, checked against `^bc-[0-9]+-[0-9a-f]{8}$`, and reads the device from the node's `status` answer, which must be exactly `ok <RUN> /dev/ublkb<n>`. `VOLUME`, `DEV`, `SIZE` and `MODE` are cut. The host validates the run id before each use and reads what is attached through a guest verb that prints only `none`, a valid id or `invalid`. Fixtures: a `RUN` outside the pattern, `status` naming another volume.
S3: the run's record is one file per run; a create answered `AlreadyExists` removes it, so no detach or delete names that id. Fixture added.
S5: the record is removed only after a complete cleanup; a later run refuses with the new reason `leftover`, naming the id and `VOL=<id> make vol-delete`, and clears a record only once its volume is gone. The guest bounds each step with `timeout` in the VM, cleans up in its own trap, and gains a `cleanup` verb the host always calls first; it ends a `run` still going before anything is removed. Fixtures for each.
S6: rule 4 uses `offset.checked_add(len)`; the table gains the row `start_sector = (u64::MAX >> 9) - 7`, `nr_sectors = 8` → `Refuse(EINVAL)`.
S7: a node not running before the check is `refused`/`nodes-not-alive`; `node-refused` is kept for a running node whose attach fails with nothing else attached. The summary, the Risks entry and the fixtures say so.
N1: step 6 has the identity guards and no zero guard; a fixture covers it.
The fixtures moved from the design's Tests into a table in kernel-check.md. The spec is unchanged: its S7 already said what the check must not accept. No test or lab check was run; no approval or accepted risk is inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — d94baaae66bb9122e9961b0ec9a0c4e20f8af119

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84c3 (incremental design-code), r84s3 (incremental design-security).

Both reviewed HEAD 8913c756970e304b0b9ca9d66bfb237b430eab19, which adds only report.md to that commit. Both confirmed every round-2 finding fixed, and reported:

- **Review84-R3-C1 — Medium:** A leftover record whose volume answers `NotFound` is cleared with only the guest `cleanup`, though `delete_volume` may have failed after the metadata delete. Evidence: kernel-check.md "An earlier record" and its two fixtures; scripts/lab.sh:1733-1790.
- **Review84-R3-C2 — Low:** How the host script calls `delete_volume` is not stated; lab.sh runs its commands when sourced. Evidence: scripts/checks/resync.sh:10; scripts/tools/lab-deletion-test.sh:21; scripts/lab.sh:727.
- **Review84-R3-C3 — Low:** `make node-up && make block-contracts` runs the check before the node is alive in the metadata service, so the first run can be `refused`. Evidence: scripts/lab.sh:1890-1896.
- **Review84-R3-C4 — Low:** No fixture for a well-formed record whose volume still exists, nor for a `metactl get` error other than `NotFound`. Evidence: kernel-check.md fixture table.
- **Review84-R3-S1 — Medium:** The recorded process group is neither all of the run (`timeout` starts its own group; step 5's writers are background jobs) nor only the run (the script starts no group of its own; a stale id is signalled as root). Evidence: kernel-check.md:41-44,89-91,132-133; decisions.md:48-52; scripts/lib.sh:157.
- **Review84-R3-S2 — Low:** `timeout 60` without `-k` and `cleanup`'s wait have no bound. Evidence: kernel-check.md:42-43,89-91.
- **Review84-R3-S3 — Low:** The cleanup's `delete_volume` can start a node with the caller's `ZERO_COPY`, `NO_VERIFY`, `QUEUES`, against "starts no node". Evidence: kernel-check.md:9-11 against 36-38,123; design.md:101-103,192-194; scripts/lab.sh:677-680,1742-1751.
- **Review84-R3-S4 — Low:** After a failed attach, the answers "the run's own id", `invalid` and exit 3 of `attached` have no result. Evidence: kernel-check.md:31-34,85-86,130-131; control.rs:139-141.
- **Review84-R3-S5 — Low:** A write step can create `/dev/ublkb<n>` as a regular file after a detach under a live `run`. Evidence: kernel-check.md:39,43,47-49,59-62,135; scripts/guest/node.sh:82-87.
- **Review84-R3-N1 — Nit:** A record with a name outside the pattern is printed in a `VOL=<id> make vol-delete` hint. Evidence: kernel-check.md:29-30,138.

Changes/resolution: each finding was checked against the cited lines and the code; all ten apply to the round-2 text. The round-2 machinery they are in is cut, not guarded.
The check now changes three things, in order: it creates one volume, attaches it on NODE and writes under 32 MiB. It deletes and detaches nothing, starts, stops and formats no node, signals no process and keeps no record. It ends by printing `VOL=<run> make vol-delete`, the operator's existing command; the Lab section lists it after each run.
C1, C4, N1: there is no record and no clearing. The precheck refuses (`leftover`) while `metactl volumes` or `metactl tombstones` lists an id with the check's pattern, or while NODE has the guest's fixed directory or trace instance; a listing that fails is `refused`/`metadata`. Only an id matching the pattern is printed in a deletion command. Fixtures: the id in `volumes`, the id only in `tombstones`, a failing listing, ids outside the pattern ignored.
C2, S3: the check never calls `delete_volume`: the host script extracts `metactl` alone from lab.sh. Every fixture asserts no delete, detach, node start or stop, including with `FORMAT=1 ZERO_COPY=1 QUEUES=4 NO_VERIFY=1` in the caller's environment. The Lab section says what the operator's `vol-delete` can start.
C3: the precheck asks `metactl nodes` every 2 s for at most 60 s. Fixtures: alive at the third listing; never alive.
S1, S2: the guest `cleanup` verb and the recorded process group are cut. Every command that opens the device is a foreground child under `timeout -k 10 60`; step 5's two writers run one after the other; the trap removes the instance and the directory once the current command has ended and signals nothing. The bound is 370 s unless the kernel cannot kill a child, which leaves the directory and makes the next run `leftover`. A guest `run` that returns no result is `fail`/`interrupted`, with the wait printed. Fixtures: a command that ignores TERM; TERM during step 5; the 300 s deadline.
S4: after a failed `node.sh attach`, `none` is `unsupported`/`node-refused`; any other answer, the run's own id and no answer included, is `fail`/`guard` with no guest `run`. Fixture rows for each.
S5: every `dd` that writes the device has `conv=nocreat`. Fixture: the path absent at a write step, no file created.
Also: the attach token is checked against `^[A-Za-z0-9+/=]+$` before it reaches a remote command. Reasons `cleanup` is cut; `metadata`, `setup` and `interrupted` are added. The spec is unchanged. No test or lab check was run; no approval or accepted risk is inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 317ef0964a3b55437141f2408e92cff14cf98863

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84c4 (incremental design-code), r84s4 (incremental design-security).

Both reviewed HEAD 87a60b3e84e95a7170708ed27b11fa24872ef11f, which adds only report.md to that commit. Both confirmed the round-3 findings fixed, R3-S2 in part (R4-S1), and reported:

- **Review84-R4-C1 — Low:** Two fixtures need elapsed time (the 60 s nodes-alive wait, the 300 s deadline) but the stub list has no clock. Evidence: kernel-check.md "Script tests".
- **Review84-R4-C2 — Nit:** "keeps no state of its own" against the result file the check writes. Evidence: kernel-check.md Procedure and Result.
- **Review84-R4-S1 — Low:** The guest's 370 s bound does not follow (a step runs several bounded commands; the `status` call waits up to 180 s outside `timeout`; `timeout` before `sudo` cannot signal its child), and the operator is told to rely on it. Evidence: kernel-check.md:55-59,97-98,165,167; decisions.md:64-67; scripts/guest/common.sh:22-30.
- **Review84-R4-S Nit 1 — Nit:** "once no `block-contracts.sh` runs there" names a process that never exists under that name. Evidence: kernel-check.md:98-99; scripts/lib.sh:126,157.
- **Review84-R4-S Nit 2 — Nit:** The trap removes the trace instance and the directory, also in a second `run` that found the directory present. Evidence: kernel-check.md:60-61 against 160.

Changes/resolution: each finding was checked against the cited lines and the code; all five apply. Wrong statements are corrected or removed; no mechanism is added.
C1: both scripts take the time from `date +%s` and wait with `sleep`; the script tests stub both, with a `sleep` that advances the stub `date`.
C2: "keeps no state that any run reads", in the appendix and the design.
S1: the 370 s bound is dropped, from the appendix, the decisions and the operator's instructions. The appendix states what is bounded: each command that opens the device, written once as `sudo timeout -k 10 60 <command>`, and each `status` call, by the socket's 180 s; the run as a whole has no stated bound. After `interrupted` the operator waits until `/run/flaminio-bc` is gone on NODE; a directory that stays is a guest `leftover`. Fixture rows: the recorded call has `timeout` after `sudo`; the interrupted message carries no number of seconds.
S Nit 1: the guest `leftover` is cleared by hand only once `sudo grep -lsaz '^RUN=bc-' /proc/[0-9]*/environ` (a live guest script) and `pgrep -f /dev/ublkb` (one of its children) print nothing.
S Nit 2: the trap is set once the `mkdir` has succeeded; a `run` that finds the directory removes nothing, and its fixture row asserts the directory and the trace instance are left.
The spec is unchanged. No test or lab check was run; no approval or accepted risk is inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — e342b46b01f22ceb7ee81af4321e982efa8839dd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84c5 (incremental design-code), r84s5 (incremental design-security).

Both reviewed HEAD 4a0b4f8b7d857963ed7a18e65a0c9d871eb66258, which adds only report.md to that commit. Both confirmed every round-4 finding fixed and reported nothing of Low or worse:

- **Review84-R5-S1 — Nit:** Step 9 says the check prints "the wait and the two guest paths" after `interrupted` and a guest `leftover`, without the two liveness commands that must print nothing first; no fixture asserts that text. Evidence: kernel-check.md:43-44 against 104-108; 166; design.md:197-198.
- **Review84-R5-C1 — Nit:** `pgrep -f /dev/ublkb` can also match an unrelated process whose command line contains that string; harmless. Evidence: kernel-check.md "by hand" paragraph.

Changes/resolution: both apply; wording only, in kernel-check.md.
S1: step 9 keeps the result and the restore line. A sentence after it says what else is printed and when: for `interrupted` (step 8) the wait, and for it and for a guest `state` answering `leftover` (step 4 or 7) the two commands that must print nothing, then the two paths. This also corrects step 9 placing a guest `leftover` "from step 5 on": that refusal is the precheck's step 4. Three fixture rows assert the order: `state` answering `leftover` at the precheck, the same after a failed attach, and `interrupted`.
C1: the "by hand" paragraph says that `pgrep -f` can match an unrelated command line containing the path, and that such a match only makes the operator look at it before going on.
No test or lab check was run; no approval or accepted risk is inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — edb8d8d7d9c2c4209e7c38a5f9d914d480d1d2d7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r84b6 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
