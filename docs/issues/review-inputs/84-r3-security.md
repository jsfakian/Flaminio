# Issue #84 — plan review round 3 (incremental), focus: design-security

Diff reviewed: `git diff 166c016..d94baaa -- docs/issues/84/ ':!docs/issues/84/report.md'`
(checkout `/tmp/flaminio-plans-2026-11-12/84`; HEAD adds only `report.md`). The code was read
only. One local command was run: `timeout 5 sh -c 'ps -o pgid= -p $$'`, to confirm R3-S1.

## Earlier findings

- R2-S1: fixed. The check restarts no node; the only start left is `delete_volume`'s, pinned to
  `FORMAT=0` (`scripts/lab.sh:1748,1750`). The other node settings still pass: R3-S3.
- R2-S2: fixed. The guest takes a validated `RUN` alone; the device comes from a `status` answer
  that must be exactly `ok <RUN> /dev/ublkb<n>` (`control.rs:104-107`); fixtures added.
- R2-S3: fixed. `AlreadyExists` removes the record; no detach or delete names that id.
- R2-S4: fixed. No restart is left to guard; a foreign volume fails the run as `guard`. The
  other answers after a failed attach are not specified: R3-S4.
- R2-S5: fixed in substance (record, `leftover`, step timeouts, guest trap, `cleanup` verb). The
  new verb's kill and waits have gaps: R3-S1, R3-S2.
- R2-S6: fixed. `checked_add`, with the table row.
- R2-S7: fixed. `refused`/`nodes-not-alive`; Risks and fixtures agree.
- R2 Nit: fixed. Step 6 has the identity guards and no zero guard.

## Findings

### R3-S1 — Medium

**The recorded process group is neither all of the run nor only the run.** `cleanup` "ends a
`run` of `RUN` that still runs (its recorded process group) and waits for it".

- Not all of it: every step runs under `timeout 60`, and GNU `timeout` moves itself and its
  command into a new process group unless `--foreground` (confirmed here: the shell's group was
  460124, the command's 460126). A signal to the recorded group reaches the script, not the
  `dd`s. In step 5 the two `dd` are background jobs: the script's trap runs at once, the group
  empties, `cleanup` returns 0, and the host detaches and deletes the volume while both still
  write, for up to 60 s. With KILL this holds in every step. The decision's "so no writer
  outlives the volume's detach" does not follow, and the two fixtures (TERM during step 5;
  `cleanup` while a `run` still runs) assert only that the instance and the directory are gone
  and that "that `run` has ended".
- Not only it: the script is not said to start its own group. Under `run_on_vm` it has the ssh
  session's; under `scripts/tools/block-contracts-test.sh` it has the test's, which is
  `make lint`'s (`Makefile:93-94`), so that fixture signals the whole lint run. A directory left
  by a killed `run` keeps a group id that `cleanup`, as root, signals later whoever holds it by
  then; the earlier-record path calls `cleanup` for old ids.

- Evidence: `kernel-check.md:41-44,89-91,132-133`; `decisions.md:48-52`; `scripts/lib.sh:157`.
- Fix: run the guest `run` as a transient unit named from the validated id
  (`systemd-run --unit=flaminio-bc-<RUN>`, as `node.sh:65` and `lib.sh:171` do); `cleanup` stops
  that unit and waits until it is inactive. That kills by cgroup, with no id to record or reuse.
  Or keep the group: `setsid` at the start, `timeout --foreground`, and `cleanup` checks that
  the leader is that run (its `RUN` in `/proc/<pid>/environ`) before any signal. Fixtures: a
  step's child in another process group has ended before `cleanup` returns; a directory whose
  recorded id belongs to an unrelated live process gets no signal.

### R3-S2 — Low

**The two new waits have no bound.** `timeout 60` without `-k` sends TERM once and then waits
for its child; a `dd` blocked in direct I/O on a device whose node hangs, the thing under test,
does not end. So "the script ends on its own even after ssh is lost" does not hold, `cleanup`
"waits for it" for ever, and the host's trap hangs with it, with no result and no `fail`/`cleanup`.

- Evidence: `kernel-check.md:42-43,89-91`.
- Fix: `timeout -k 10 60`; `cleanup` waits a stated time (longer than a step), then exits
  non-zero, which the host already reports as `fail`/`cleanup` with the record kept. Fixture: a
  step that ignores TERM.

### R3-S3 — Low

**The cleanup can start a node, with the caller's settings.** The procedure and the design say
the check "starts, stops and formats no node" and "reads no node setting (`FORMAT`, `ZERO_COPY`,
`QUEUES`, …) from its caller"; the Lab section says "it restarts and formats nothing". Its
cleanup calls `delete_volume`, which starts every node it finds stopped through `cmd_node up`
and `node_env`: `FORMAT=0` is pinned, but `ZERO_COPY`, `NO_VERIFY`, `QUEUES` and the rest come
from the check's environment. A node that crashes during the check, the realistic case, comes
back in another mode than the operator started it in (the Lab line's `A && B` gives its
settings to `node-up` only), or without verification under an exported `NO_VERIFY=1`. The
fixture row asserts both "the check itself calls no `node.sh up`" and a `node.sh up`.

- Evidence: `kernel-check.md:9-11` against `36-38,123`; `design.md:101-103,192-194`;
  `scripts/lab.sh:677-680,1742-1751`.
- Fix: before `delete_volume`, the cleanup requires every data node to answer; a stopped one is
  `fail`/`cleanup`, record kept, the node named, nothing started. Or keep the start and say so
  in the three places, with the settings it carries, and have the fixture assert them.

### R3-S4 — Low

**After a failed attach, three answers of `attached` have no result.** Only another volume's id
(`guard`) and `none` (`node-refused`) are specified. The run's own id is possible: an attach
answered "no answer within" may still bring the device up (`control.rs:139-141`). So are
`invalid` and exit 3 (the node died in the attach). An implementer has to guess whether the
guest `run` starts, and what is reported.

- Evidence: `kernel-check.md:31-34,85-86,130-131`.
- Fix: any answer but `none` is `fail`/`guard`, with `found` only for a valid other id; no
  guest `run`; cleanup as usual. Add the three fixture rows.

### R3-S5 — Low

**A write step can create `/dev/ublkb<n>` as a regular file.** The identity guard checks the
path, then `dd` opens it by name. The guest now outlives ssh by design, and a failed guest
`cleanup` tells the operator to run `VOL=<run> make vol-delete`, which detaches under a live
`run`. A detach between the guard and the open makes `dd` create the file; `node.sh attach`
then refuses every attach on NODE until it is removed, the hazard `node.sh:82-87` describes.
The window is small; the fix is one flag.

- Evidence: `kernel-check.md:39,43,47-49,59-62,135`; `scripts/guest/node.sh:82-87`.
- Fix: every `dd` that writes the device carries `conv=nocreat`. Fixture: the path is absent
  when a step writes; no file is created.

## Nits

- `kernel-check.md:29-30`: "Any other record refuses, naming its id and `VOL=<id> make
  vol-delete`" also covers a record whose name is outside the pattern, and would print an
  unvalidated name in a deletion command (`wbench.pending` gives `VOL=wbench make vol-delete`).
  Say that such a record is named by its file path only, with no `VOL=` hint, and add that to
  the fixture row at line 138.

Reviewed commit: 8913c756970e304b0b9ca9d66bfb237b430eab19
