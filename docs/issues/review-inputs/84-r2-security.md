# Issue #84 — plan review round 2 (incremental), focus: design-security

Diff reviewed: `git diff 733065c..166c016 -- docs/issues/84/ ':!docs/issues/84/report.md'`
(checkout `/tmp/flaminio-plans-2026-11-12/84`). Nothing was run; the code was read only.

## Earlier findings

- Review84-R1-C1: fixed. `node_params` and the four ordered rules of `classify_node_request`
  match `ublk.rs:946-962`, `1220-1281`, `1374-1456` and the pinned header
  (`libublk-rs-sys-0.1.2/ublk_cmd.h:455-505`); discard stays unadvertised. One gap left in rule 4
  (R2-S6).
- Review84-R1-C2: fixed. The two-queue FLUSH test, the hypothesis and the drain are cut; S3 rests
  on S2; the oracle is the limits plus the request trace, with two controls.
- Review84-R1-C3: fixed in substance (CLI, exit codes, result schema, run-owned volumes, guards,
  admission). The new check has the gaps below (R2-S1 to R2-S5, R2-S7).

## Findings

### R2-S1 — Medium

**The restore can reformat NODE's data disk.** The design says the node is restarted "never with
`FORMAT=1`" and that the check "formats nothing", but the restore restarts it "with the caller's
environment, as `make node-up` does". That environment carries `FORMAT`: `node_env` passes
`FORMAT=${FORMAT:-0}` and `node.sh up` then adds `--format --force`. Only the two mode restarts
pin `FORMAT=0`. With `FORMAT=1` exported, or given to `make block-contracts`, the third restart
erases every replica on NODE (an RF-1 volume there is lost). No test covers it.

- Evidence: `design.md:174-175` against `design.md:178-180`; `kernel-check.md:17`;
  `scripts/lab.sh:679`; `Makefile:243-244`; `scripts/guest/node.sh:47`.
- Fix: the restore pins `FORMAT=0`, as `delete_volume` does (`scripts/lab.sh:1750`), or the check
  refuses a caller's `FORMAT=1` before changing anything. Add a script test: with `FORMAT=1` in
  the caller's environment, no recorded `node up` call carries `FORMAT=1`.

### R2-S2 — Medium

**The guest script accepts the volume and the device from its caller, unvalidated.** Spec S7 and
the Security risk say the check "accepts no volume or device" and "cannot be pointed at another
volume". The guest CLI takes `VOLUME`, `DEV`, `SIZE` and `RUN` from its environment, and its
guards never require `VOLUME` to be the run's: `status` answering `ok <VOLUME> <DEV>` holds for
any attached volume. Run by hand, or by a host-side bug, it writes 17 MiB to any attached,
unmounted 1 GiB volume that reads zeros there (a new PVC). `RUN` is not validated either and
becomes a path root creates and removes under tracefs. The host puts all five values into
`ENV_VARS`, which reaches the remote shell unquoted; `DEV` comes from the node's answer. That is
against the principle "Ids reaching shell commands or socket lines are validated and quoted".
`SIZE` has no stated use: the guard fixes 1073741824.

- Evidence: `kernel-check.md:22-25,56-59`; `spec.md:58-59`; `design.md:205-207`;
  `scripts/lib.sh:157`.
- Fix: the guest takes `RUN` and `MODE` only. It requires `RUN` to match
  `^bc-[0-9]+-[0-9a-f]{8}$` and `MODE` to be `copy` or `zero-copy`, reads the device from its own
  `status` answer, and requires that answer to be exactly `ok <RUN>-c|-z /dev/ublkb<n>`. The host
  validates the run id before each use. Add script tests: a `RUN` outside the pattern, and
  `status` naming a volume that is not `<RUN>-c`/`-z`, write nothing.

### R2-S3 — Low

**Cleanup deletes a volume the run did not create.** Each id is appended to the run's file before
its create; "an id that exists fails the run"; cleanup deletes every id in the file that matches
the pattern. So when `create` answers `AlreadyExists`, the existing volume's id is in the file
and cleanup deletes it: the adoption the rule forbids, on exactly the path the rule names.

- Evidence: `kernel-check.md:9-13`; `meta/service/service.go:142`.
- Fix: a create refused as `AlreadyExists` strikes its id from the file (or the file records the
  outcome and cleanup skips `exists`); an unknown outcome stays deletable. Add a script test:
  create answers `AlreadyExists`, the run fails, no `delete` call names that id.

### R2-S4 — Low

**"Nothing is attached on NODE" is checked once; the three restarts are unguarded.** `node.sh up`
on a running node calls `down`, which deletes ublk device 1 whatever it serves. The run lasts
about 10 minutes and `delete_volume` needs the CSI node plugins running for the tombstones, so a
CSI attach can land on NODE after the precheck or between the modes. The next restart, or the
restore, then tears that device down under its user. In that case the check's own attach is
refused ("one volume at a time") and is reported as `unsupported`/`node-refused`, which hides it.

- Evidence: `kernel-check.md:14-21`; `design.md:103,178-180`; `scripts/guest/node.sh:29,44`;
  `dataplane/crates/flaminio-io/src/control.rs:95`; `scripts/lab.sh:1796`.
- Fix: before each restart, the restore included, `status` must answer `ok none` after the run's
  own detach; otherwise `fail`/`guard`, no restart, and the result names the volume found. An
  attach refused because another volume is attached is `fail`/`guard`, not `unsupported`. Add a
  script test for a foreign attachment appearing between the modes.

### R2-S5 — Low

**An interrupted run has no cleanup path.** "On every exit path" covers a shell exit only. After
a killed host script or a lost connection, the run's volumes stay (one attached), NODE's node
stays in the check's mode, and the next run answers `refused`/`attached` without naming them:
nothing reads an earlier `<run>.volumes`, and the CLI takes no argument. In the VM, the guest CLI
has only `run`, and where `timeout 300` runs is not said: after a timeout or a dropped ssh the
host cannot remove the trace instance or the temporary file.

- Evidence: `design.md:178-182`; `kernel-check.md:11-13,19,39-40,49,56`.
- Fix: run `timeout` in the VM around a guest script whose own exit trap removes the instance and
  the temporary directory, and add a guest `cleanup` verb (validated `RUN`) that the host's
  restore always calls. Remove `<run>.volumes` only after a complete cleanup; at start, refuse
  (a new reason, such as `leftover`) while an earlier file remains, naming its ids and
  `VOL=<id> make vol-delete`. Add script tests for both.

### R2-S6 — Low

**Rule 4's "offset + length exceeds `size`" can overflow.** `start_sector` up to `u64::MAX >> 9`
gives an aligned offset up to 2^64 − 4096; adding any length wraps. A wrapping sum passes the
comparison and returns `Read`/`Write` for an offset far past the device (a debug build panics the
queue thread instead). The engine's check uses `checked_add`; the classifier's is not specified,
and the table's "sector overflow" row covers only the shift.

- Evidence: `design.md:57-60,141-143`; `dataplane/crates/flaminio-node/src/engine.rs:1702`.
- Fix: state the test as `offset.checked_add(len)` (or `len > size - offset` after
  `offset <= size`), and add a table row: `start_sector = (u64::MAX >> 9) - 7`, `nr_sectors = 8`
  gives `Refuse(EINVAL)`.

### R2-S7 — Low

**Under #83 the check answers `refused`, not `unsupported`.** The precheck requires NODE's socket
to answer `ok none` and both nodes to be alive before anything changes. A node that #83's
admission refuses never runs, so the run ends at the precheck with exit 2. The design, the Risks
entry and the test list say that case gives `unsupported`/`node-refused`. Neither is a pass, but
the reason, the exit code and the test fixture disagree with the procedure.

- Evidence: `kernel-check.md:14-16` against `kernel-check.md:19-21,77`;
  `design.md:106-107,164,203-204`.
- Fix: say that a node not running before the check is `refused`/`nodes-not-alive`, and keep
  `node-refused` for a node that ran before but does not start or attach in the check's mode.
  Correct the Risks entry and the fixture.

## Nits

- `kernel-check.md:22-25,35`: say whether step 6 (`blkdiscard`) is a guarded step. The identity
  guards should apply; the "reads as zeros" clause cannot, since step 3 wrote that range.

Reviewed commit: 166c0167157527e2ff4b62283b90434b59342603
