# Issue #75 plan review: round 7, design-code and design-security (incremental)

Scope: `git diff 00e57e8..4663835 -- docs/issues/75/ ':!docs/issues/75/report.md'` (the round-6
fix to design.md and decisions.md). I checked it in `/tmp/flaminio-plans-2026-11-12/75` against
that worktree's code. HEAD ca90950 adds only report.md.

These hold against the code:
- `metactl VM SERVER` dials only `$(vmvar SERVER WG):$META_PORT` (`scripts/lab.sh:529-535`;
  `grpc.NewClient(*server)`, `meta/cmd/flaminio-meta/main.go:339`). lab.sh already reaches w's
  instance this way (`lab.sh:643`).
- The probe's arguments pass `Spec.Validate` (`meta/domain/domain.go:176-196`). An empty failure
  domain and an empty policy parse (`domain.go:119-142`). The quota is the ninth argument.
- `qos-probe-<vm>-<unix time>` matches `idPattern` (`domain.go:87`). No other lab or CSI volume
  uses that prefix: CSI names are `pvc-<uid>`, and `VOL` defaults to `vol1`.
- `readHeader` returns `ErrNotFound` for a header that is absent, being created or being deleted.
  It returns the raw error for a store or decode failure (`meta/service/service.go:275-291`).
- The rollback order is right: `GetVolume` and the returned `qos` do not depend on the switch.

## Earlier findings

- R6-B1: fixed. The probe exercises each instance's own handler. A binary that exits in `fs.Parse`
  never answers, so it can no longer pass. A residual is in R7-B2.
- R6-B2: fixed here (design.md:221-227; decisions.md:179-182). #70's side is in its uncommitted
  worktree (`70/docs/issues/70/design.md:221-225`). It opens R7-C1.
- R6-B3: fixed, by the cut. Nothing checks a VM after `flags`.
- R6-B4: fixed (design.md:233-234). An implementation that probes only `all` or only `NODE`
  fails "only w refusing".
- R6-B5: fixed (design.md:265-267; decisions.md:183-184).
- R6-B6: fixed (decisions.md:149-150).
- Nit 1: fixed (design.md:229-230).
- Nit 2: fixed in substance (design.md:51-54), with a test (design.md:199). Some wording is left:
  Nit 1 below.

### R7-B1 — Low

**The trap is the probe's only cleanup when it fails. The design leaves open which ids it deletes
and through which instance, and no test case needs the trap.**

Evidence:
- design.md:246: "A trap deletes every probe on exit".
- Probes that exist after a failure:
  - An older instance creates an unlimited `qos-probe-fl-w-T`. The new metactl then exits 1 and
    deletes nothing (design.md:77-78).
  - A create whose answer is lost after 30 s (`main.go:345`) can still have committed.
  - An inline delete can fail: `Aborted` "too much contention" (`service.go:583`), or the instance
    stops.

  If the trap tracks only creates that exited 0, it leaks every one of these. If it deletes
  through the failing VM's instance, it fails whenever that instance is the one that is down. If
  it lists `qos-probe-*`, it can delete another run's or an administrator's volumes.
- None of the test cases (design.md:233-234) exercises the trap:
  - "Only w answering without `qos` fails naming w" asserts the failure, not that w's probe is
    gone.
  - In "all passing deletes every probe", the inline deletes do the work.

  An implementation with no trap passes all three cases.

Fix:
- Specify the trap:
  - it deletes every id this run passed to `create`, whatever that create returned;
  - it deletes through `$VM_A`'s instance, or the first instance that answers;
  - it treats `NotFound` as done;
  - it is cleared (`trap - EXIT`) once the probes are deleted, as lab.sh does at 843 and 1125.
- Test cases:
  - "only w answering without `qos`": fails naming w, and a `delete` of w's probe is recorded;
  - "a's inline delete fails": the check fails, and the trap deletes a's probe.

### R7-B2 — Low

**`make meta-flags` now restarts one VM after another without waiting for any instance to come
back. Against a binary older than the flag, every metadata instance ends up crash-looping, and
the command still reports success.**

Evidence:
- design.md:217-220: "VM by VM, `meta.sh flags` rewrites the file and restarts the service ...
  Nothing here checks anything".
- The unit has no `Type=`, so it is `simple` (`scripts/guest/meta.sh:20-44`). Its
  `systemctl restart` returns 0 once the process is forked.
- An older binary given `--qos` exits in `fs.Parse` (`flag.ExitOnError`, `main.go:84,99`).
  `Restart=on-failure` with `RestartSec=2` (`meta.sh:34-35`) then loops it.
- So `make meta-flags QOS=1` run before `make meta-up` takes down fl-a, then fl-b, then fl-w.
  The design names this order (#70 landed first, #75's tree checked out) and says nothing
  enforces it (design.md:55-56).
- Round 5's build check refused this before any write. The cut removes the only stop between
  VMs. The help text warns only that "a rollback then erases limits" (design.md:270-273).
- A smaller effect: `qos-check` run straight after `meta-flags` probes an instance that may still
  be starting. It then fails a healthy VM with "does not take limited volumes". lab.sh's own idiom
  after a restart is `wait_for ... metactl "$VM_A" "$VM_W" get` (`lab.sh:643`).

Fix:
- After each VM's `meta.sh flags`, lab.sh waits a bounded time (`wait_for`, 60 s) for that VM's
  instance to answer a read, for example `metactl "$VM_A" "$vm" nodes`.
- If it does not answer, lab.sh stops before the next VM, naming the VM and the undo
  (`make meta-flags QOS=0 VM=…`, or `make meta-up`).
- This is a liveness wait, not a flag check, so round 6's decision stands.
- Test case: the first VM's fake instance never answers. `meta flags` dies naming that VM, and no
  call reaches the second VM.

### R7-C1 — Low

**The shared rule leaves out the one constraint that #75's probe meets and #70's does not: each
probe needs a fresh id. A deleted id stays taken until its tombstone is confirmed.**

Evidence:
- `createVolume` refuses an id that still has a tombstone, with `ErrExists` "still has a deletion
  tombstone" (`service.go:372-377`).
- A probe's tombstone clears only after the replica node's CSI sweep confirms it. That sweep runs
  every 5 s (`control/csi/tombstones.go:9,33-49`), and only where the node plugin runs.
- #75 uses `qos-probe-<vm>-<unix time>` (design.md:242). The rule it states for every flag check
  (design.md:221-227) says only "do the operation ..., then undo it".
- #70, following that rule, uses the fixed id `csi-expand-probe`
  (`70/docs/issues/70/design.md:222`, uncommitted). So a rerun within the sweep's window fails
  `AlreadyExists`. So does a rerun after a run whose cleanup failed. The check reports this as an
  instance refusing.

Fix:
- Add to the rule: "with a probe id unique to the run (a deleted id is held by its tombstone,
  `service.go:372-377`), deleted by the same trap rules as `qos-check`'s".
- The orchestrator carries this to #70: `csi-expand-probe-<unix time>`.

## Nits

1. design.md:54 ("any other error is returned as it is") and design.md:199 ("returns a store
   error from `readHeader` unchanged"). Read literally, this skips `errStatus`, which turns a
   store error into `Unavailable` (`service.go:133-151`). gRPC would then send `Unknown`. The CSI
   client moves on to the next instance only on `Unavailable`, `DeadlineExceeded` or `Canceled`
   (`control/csi/meta.go:62-82`). Say "returned through `errStatus` (`Unavailable` for a store
   error), as every other `CreateVolume` error is", and have the test assert `Unavailable`.
2. design.md:245-246: "Anything else stops the check: `metadata instance on <vm> does not take
   limited volumes`". This message also covers a lost ssh to fl-a, an etcd outage and a placement
   refusal. Append metactl's own error to the message.

Reviewed commit: ca9095096cd3a5f882615680267f20b6f7bc8df3
