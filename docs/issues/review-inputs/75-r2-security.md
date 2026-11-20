# Issue #75 plan review: round 2, design-security focus (incremental)

Scope: `git diff 6597c44..83ae6dd -- docs/issues/75/ ':!docs/issues/75/report.md'` (the coordinator's
correction: 83ae6dd replaces c2a4afc). Read in `/tmp/flaminio-plans-2026-11-12/75` against the code
of that worktree. The plan is now one step: per-volume limits from StorageClass parameters,
fixed at create, enforced in the volume's engine before `Target::submit`.

Checked and found to hold: the policy reaches only `CreateVolume` (admin only, `auth.go:64-65`),
and the older CSI refuses unknown parameters (`controller.go:157-162`). The request timer starts
when a request is emitted (`target.rs:811-828`), so a request held before `Target::submit` cannot
fail a session. `--io-buf` is at most 1 MiB (`node.rs:222-223`), so under the S1 floors the head
waits at most 1 s, even with both dimensions limited. The device is privileged (`ublk.rs:906-910`,
no `UBLK_F_UNPRIVILEGED_DEV`), so `ublk_timeout` re-arms instead of failing a request. FLUSH and
DISCARD never reach the engine, and with no volatile cache (`attrs: 0`) a held write is not
covered by any flush. The gather channel is unbounded and gives every timer its own task
(`ublk.rs:1144-1176`), so a QoS instant cannot delay a gather wait. Only `engine.rs:668,1782`
call `GetVolume` from Rust, and the interop script's client is an `io` identity
(`meta-interop.sh:22`). `--insecure` skips the check but cannot attach anything, because it
issues no tokens (`start_target`, `main.go:97`). `make qos-check` validates `NODE` against a
fixed list and formats nothing.

## Earlier findings

- R1-S1 (lanes break ordered sessions): fixed. The lanes are cut and the protocol is unchanged.
- R1-S2 (tenant overrides vs. undefined ceilings): fixed. Only StorageClass parameters set a policy, and nothing depends on #73.
- R1-S3 (shared pools let one volume fail another's sessions): fixed. The pools are cut, and QoS acts only before the target emits.
- R1-S4 (`LaneOpen` input): fixed (cut).
- R1-S5 (destination classification and peer policing): fixed (cut).
- R1-S6 (`-EAGAIN` to the tenant): fixed. A held request keeps its tag and never completes with an error, and the kernel re-arms the timeout for privileged ublk devices.
- R1-S7 (destructive gate steps, lock): fixed. Gates and the lock are cut, and `qos-check` formats nothing and validates `NODE`.
- R1-S8 (policy revision delivery): fixed. The policy is immutable and is read once, at attach.
- R1-S9 (release report provenance): fixed (cut).
- R1-S nit (network order): fixed (cut).
- R1-C1, C2, C3, C4, C10: fixed by the cut. QoS sits before emission, with no lanes, DRR, pools or caps.
- R1-C5, C16: fixed. The policy is fixed at create, so a create retry still matches.
- R1-C6: fixed. A tag is held, a timer is armed at the head's due instant, and the floors give a 1 s head wait.
- R1-C7: fixed. No ceilings or overrides.
- R1-C8: fixed. One charge point under the engine mutex, and the second charge is stated and tested.
- R1-C9: fixed. The default-path cost is stated; its measurement is deferred to a later step.
- R1-C11: fixed for an older CSI, an older data plane and an all-old metadata service. A mixed set of metadata instances is still open: see R2-S1.
- R1-C12: fixed. Tests are given per rule and `qos-check` is specified.
- R1-C13, C14, C17, C18: fixed. Cut or staged.
- R1-C15: fixed. FLUSH and DISCARD cost nothing.
- R1-C nits: fixed.

### R2-S1 — High

**A limited volume is served unlimited, and loses its policy for good, whenever a metadata
instance older than this change serves it alongside newer ones (rolling upgrade, CSI upgraded
first, or rollback).**

Evidence:
- The S5 analysis treats the metadata service as one version (design.md:56-58, spec.md:53-58).
  The intent promises "No older component serves a limited volume unlimited" (intent.md:38).
- In practice the metadata service has several instances on one etcd. The lab runs one per VM
  (`scripts/lab.sh:507-525`). The data plane sends each call to "the endpoints in turn ... any
  instance of the service will do" (`flaminio-metaclient/src/lib.rs:6-7`), and so does the CSI
  controller (`control/csi/meta.go:64-79`).
- An older instance's `GetVolume` returns no `qos` and runs no `enforces_qos` check. An older
  data plane therefore attaches the volume, and a newer data plane builds no gate, because
  `start_target` builds it from `v.qos` (design.md:79-81).
- An older instance's `Attach` decodes the header into its own `domain.Header` and writes it
  back (`meta/service/service.go:275-284,709-724`), which drops the unknown `qos` key. Every
  attach and failover is such a write. Once that happens the volume is unlimited for good,
  silently: the CSI compares the policy only at create (design.md:49-52), and "the refresh
  ignores qos" (design.md:81).
- The design records the erasure only for a downgrade (design.md:198-199). The CSI-first order,
  which S5 lists as failing closed, fails open as soon as one instance is old and another new: the
  create lands on a new instance and passes the CSI check, and an old instance erases the
  policy later.
- No test covers mixed instances (design.md:156).

Fix: make S5 hold across mixed instances. Two ways:
- (a) Gate on a cluster switch. New instances refuse a limited `CreateVolume` until a switch in
  etcd exists. `metactl` sets the switch after the last instance is upgraded, and the docs state
  that downgrading the metadata service below this version is then unsupported.
- (b) Store the policy where old instances cannot erase it. Keep it under a key the old code
  never rewrites, written in create's transaction, and have new instances always return a
  `qos` presence marker. The data plane then refuses to attach a volume whose answer lacks the
  marker.

Either way, state the required upgrade order in S5 and in Compatibility. Add tests:
- a header round-tripped through the pre-change `Header` struct (simulating an old instance's
  `Attach`) still yields the policy, or the case is shown to be refused;
- a `GetVolume` answer without the policy or marker fails a new data plane's attach.

### R2-S2 — Low

**Each PVC created against an older metadata service leaves an orphaned volume that nothing
deletes, and a tenant can repeat this.**

Evidence:
- On `OK` with the policy missing, the CSI controller returns `FailedPrecondition`
  (design.md:49-51). The volume it just created stays: "an administrator deletes it after
  upgrading" (design.md:197-198).
- `FailedPrecondition` is a final error for the CO. A retry gets `AlreadyExists` with other
  properties (`controller.go:239-247`), so the PVC stays Pending. Once the tenant deletes the
  PVC, no `DeleteVolume` is ever sent for that name.
- Each create/delete cycle in a limited class therefore adds one orphan, with its header and
  size/chunk chunk records in etcd (`domain.go:50-51`, `Creating` until every chunk record is
  written). This lasts as long as the mixed window does. Today a create never leaves an orphan:
  a lost answer is retried into `AlreadyExists`, which matches.
- `TestControllerQoSNotStored` (design.md:156) checks only that the create fails.

Fix: on `OK` with a missing or different policy, the controller deletes the volume it just
created. It already holds the admin identity that `DeleteVolume` needs, and the `OK` code shows
that this request created the volume. It then returns the error. Extend
`TestControllerQoSNotStored` to assert the volume is gone, and remove the "administrator
deletes it" sentence.

### R2-S3 — Low

**The stated wait bound only holds at the default queue depth and queue count, and the node-wide
effect of long waits is not stated.**

Evidence:
- The Risks section says "At the floors a full 128-tag FIFO waits about 128 s, past the 120 s
  hung-task warning; operators choose the limits" (design.md:203-205). The S4 bound is only for
  the head (spec.md:48-51).
- The tag count is `--depth` (u16) × `--queues` (1-64), both operator flags
  (`flaminio-io/src/node.rs:44-54,171,201-204`). At `iops = 1` the last tag waits
  depth × queues seconds: up to hours.
- Requests beyond the tags wait in blk-mq for a tag, which the 128 s figure does not count.
- Tasks blocked that long include the volume's own journal and writeback threads in D state. They
  trigger hung-task reports, and with `kernel.hung_task_panic=1` a tenant's I/O pattern then
  crashes the node. Every volume replicated on that node then loses an ISR member.
- Neither the risk nor a deployment requirement says so.

Fix: state the bound as depth × queues × max(1/iops, io_buf/bytes_per_second), plus blk-mq
waiters. Then choose one:
- refuse an attach whose bound exceeds a stated maximum, since `start_target` knows the depth,
  queue count and `io_buf`, and test that refusal; or
- document in `docs/lab.md` and the ADR proposal that nodes serving limited volumes must not
  set `hung_task_panic`.

Reviewed commit: 83ae6ddbbbf2dccb7601593e764a745609c95880
