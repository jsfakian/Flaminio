# Issue #75: Per-volume QoS limits (first step) — design

[Spec](spec.md). Settled choices: [decisions](decisions.md). Approved 2026-11-13; not
implemented. The step: [intent](intent.md).

The facts it starts from:
- **One volume per node, one engine.** The daemon serves one attached volume
  (`flaminio-io/src/ublk.rs:681-682`). Every ublk queue thread drives the same `Engine` behind one
  mutex (`ublk.rs:576-578`), for queued I/O (`ublk.rs:657-674`) and fast reads
  (`ublk.rs:1598-1600,1636-1638`).
- **The engine splits, the target emits.** `Engine::submit` checks alignment and range, then
  splits a client request at chunk boundaries into `Target::submit` calls
  (`flaminio-node/src/engine.rs:1685-1748`). `Engine::try_fast_read` admits every piece or none
  (`engine.rs:2127-2201`); a fast read `finish_fast_read` refuses goes again through `Input::Io`
  (`engine.rs:2223-2226`, `ublk.rs:1232-1239`).
- **The request timer starts at emission.** A session fails when a request it sent is older than
  `request_timeout`, 3 s (`flaminio-repl/src/target.rs:811-828`, `types.rs:60`). Work queued in
  the target (`target.rs:741-766`), in its classes (`target.rs:262-283`), runs no timer.
- **The volume record.** The data plane reads it with `GetVolume` at attach (`engine.rs:667-674`,
  `start_target` `engine.rs:1596-1683`) and on refresh (`engine.rs:1544-1579,1781-1789`).
  `Volume` uses tags 1-8 (`api/proto/flaminio/meta/v1/meta.proto:117-127`); Rust's messages are
  hand-written, checked by the interop test (`flaminio-metaclient/src/pb.rs:1-3`). `GetVolume` is
  open to every caller, `CreateVolume` to administrators (`meta/service/auth.go:64-65,79-80`).
- **Several metadata instances.** Clients call any instance (`flaminio-metaclient/src/lib.rs:6-7`,
  `control/csi/meta.go:64-79`). The header is JSON in etcd, decoded leniently
  (`meta/domain/domain.go:36-54`, `meta/service/service.go:280-284`); `Attach` writes back what
  it decoded (`service.go:724-725`), so an older instance drops a field it does not know.
  `Attach` moves the target and epoch before the data plane reads the volume
  (`control/csi/node.go:70-74`).
- **CSI.** `parseParams` refuses unknown StorageClass parameters
  (`control/csi/controller.go:140-163`); a create that finds the name compares four properties
  (`controller.go:239-247`).
- **The frontend.** 4 KiB logical blocks (`ublk.rs:187,954`); requests of at most `--io-buf`,
  1 MiB at most (`flaminio-io/src/node.rs:222-223`); depth 128, one queue (`node.rs:134-140`).
  FLUSH and DISCARD are answered 0 without the engine (`ublk.rs:1279,1454`). The device is
  privileged (`ublk.rs:906-908`). Queue 0 turns the engine every 10 ms (`ublk.rs:1178-1188`); the
  gather timer pumps when the engine asks (`engine.rs:912-914`, `ublk.rs:596-598,1144-1177`).

## The policy (S1, S5)

- **Go.** `meta/domain/qos.go`: `QoS{IOPS, BytesPerSecond, BurstIOPS, BurstBytes uint64}`, JSON
  as the proto fields; `Limited()` (a rate is nonzero); `Validate()`, the [rule](#contracts),
  wrapping `ErrInvalid`. `Spec` gains `QoS`, checked by `Spec.Validate` (`domain.go:176`);
  `Header` gains `QoS *QoS` (`json:"qos,omitempty"`), nil unless limited.
- **Create.** `Service.CreateVolume` copies `r.GetQos()` into the spec (`service.go:357-361`), so
  create's existing transaction stores it. Nothing writes another value; every later header
  write keeps it. `volumePB` (`service.go:337-346`) returns it.
- **The switch, the one guard against older instances.** `Options.QoS` (`auth.go:19-28`), from
  `flaminio-meta serve --qos`, default off (`meta/cmd/flaminio-meta/main.go:84-101`). It gates
  `CreateVolume` only; `GetVolume`'s check and the returned `qos` never depend on it. Off, a
  limited create first calls `readHeader` (`service.go:275-291`): a visible header answers
  `AlreadyExists`, so a retry still matches; `NotFound` (absent, being created or being deleted)
  gets `FailedPrecondition`, which the CSI controller turns into the retryable
  `ResourceExhausted` (`controller.go:248-249`); any other error goes through `errStatus`
  (`service.go:133-152`) like every `CreateVolume` error, so a store error is `Unavailable` and
  the CSI client tries the next instance.
  Operators add it in a second rollout, once every metadata instance and data plane runs this
  version; nothing enforces that order. Once a limited volume exists, an older instance serves
  it unlimited and erases its policy for good on its next `Attach`, whatever the switch:
  rollback is unsupported; the only safe one deletes the limited volumes first. An older binary
  exits on the unknown flag (`flag.ExitOnError`, `main.go:84`), so a binary-only rollback that
  keeps `--qos` fails closed. The flag's usage and `main.go`'s doc (`main.go:19-23`) say so:
  `allow volumes with QoS limits; only once every metadata instance and data plane runs this
  version: an older instance erases their limits, so delete them before any rollback`. In the
  lab: [Lab flags](#lab-flags-of-the-metadata-service-s5-shared-with-70).
- **The data-plane check.** `Service.GetVolume` (`service.go:501-507`), after the read: a limited
  volume, an `io` caller (`callerOf`) and `enforces_qos` false answer `FailedPrecondition`.
  `GetVolume` stays open in `auth.go`; its comment (`auth.go:38-40`) names this check beside
  ChangeIsr's. Without authentication (`auth.go:42-44`) there is no caller, so no check.
- **CSI.** Four `parseParams` cases, `strconv.ParseUint(v, 10, 64)`, into `volumeParams.qos`, sent
  with the create (`controller.go:231-236`). On `OK` the returned `qos` must equal the request's
  (unset equals all zeros). If not, an older instance dropped it: the controller deletes the
  volume this call created (`c.Meta.DeleteVolume`, as `controller.go:265`), then answers
  `FailedPrecondition`; if the delete fails, its error. On `AlreadyExists` the policy joins the
  compared properties (`controller.go:245`) and nothing is deleted (the volume may hold data),
  so a retry after a lost answer (`control/csi/meta.go:72-79`) leaves that volume (Compatibility).
- **metactl.** `create` takes an optional ninth argument, four decimal numbers
  `IOPS,BYTES_PER_SECOND,BURST_IOPS,BURST_BYTES`, or refuses before the call
  (`meta/cmd/flaminio-meta/main.go:7,380-403`). Like the CSI controller it compares the returned
  `qos`; on a mismatch it deletes nothing and exits non-zero naming the volume. The interop test
  uses it.
- **Versions (S5).** Before the switch every limited create fails: refused; or dropped, then
  deleted by the CSI controller or reported by metactl; or refused by an older CSI. After it, an
  older data plane (a node added or rolled back) is refused `GetVolume`: its attach fails
  (`engine.rs:1480-1486`) after `Attach` moved the target. Drain moves replicas only
  (`control/drain/drain.go:1-7`); failover lets Kubernetes reschedule the pod
  (`control/failover/failover.go:1-3`). Nothing steers the pod away from that node: it waits, and
  the volume has no serving target until a capable node attaches it.

## The gate (S2)

`dataplane/crates/flaminio-node/src/qos.rs`: pure, no clock read, time in the engine's
microseconds (`engine.rs:604-606`). `Policy` mirrors `QosPolicy`; `Policy::check` is the
[rule](#contracts).
- **`Bucket`** per limited dimension: `rate`, `cap = b × 10⁶` with `b` the effective burst (the
  burst, or one second of the rate when the burst is 0),
  `bal: u128` from 0, `at`. Refill: `bal = min(cap, bal + rate × (now − at))`, saturating;
  `at = now`. A cost `c` is covered when `bal ≥ c × 10⁶`; a debit subtracts that.
- **`Gate<T>`**: both buckets and a FIFO of held `(bytes, T)`. A request costs 1 I/O and its
  bytes, each clamped to its bucket's effective burst `b`, so every cost is coverable.
  `covers(now, bytes) -> bool` refills and tests, never debits, false while anything is held.
  `take(now, bytes)` debits a covered cost. `offer(now, bytes, t) -> Option<T>` is `covers` then
  `take`, returning `t`, or holds it. `release(now) -> Option<T>` admits the head if covered.
  `due(now) -> Option<u64>` is when the head is covered: the latest over the dimensions of
  `now + ceil((c × 10⁶ − bal) / rate)`. `drain() -> Vec<T>` empties the FIFO.
- **The bound:** `0 ≤ bal ≤ cap`, so a window of Δ seconds charges at most `b + rate × Δ`, and
  admits that many requests or bytes while no cost exceeds `b`; saturation only loses credit. The
  clamp never acts on the frontend's requests (at most 1 MiB, a limited `b` at least 1 MiB); it
  keeps a larger `Input::Io` from blocking the FIFO.

## Admission in the engine (S2, S3, S4)

- **`TargetRt.qos: Option<Gate<(u64, Io)>>`**, `None` when unlimited. `start_target` builds it
  from `v.qos` after `Policy::check`, empty from that instant; a failure returns
  `unusable QoS policy: <rule>` (`AttachFailed`). The refresh ignores `qos`.
- **Queued path.** `Engine::submit`, after validation (`engine.rs:1694-1709`), before the split
  (`engine.rs:1710`): `offer(us, len, (op, io))`; if held, note the volume and return. The split
  becomes `submit_now`.
- **Fast path.** `Engine::try_fast_read`, after validation (`engine.rs:2136-2143`): if not
  `covers(us, len)`, return `None` before the target sees the read; the frontend then submits
  it, and it waits. Once every piece is admitted (`engine.rs:2200`), `take(us, len)`. Nothing
  runs in between: the engine is borrowed mutably.
- **Release.** `turn` and `flush` (`engine.rs:713-750`) begin with `release_held(now)`: per volume
  in `held: BTreeSet<String>`, one request at a time, drop the name if the target is gone, else
  `submit_now` what `release` returns (a deposition on the way fails the rest). The earliest `due`
  then becomes `qos_timer` if it differs from the last armed; `Engine::take_qos_timer()` takes it,
  `EngineCtx::pump_with` sends it on the gather timer's channel (`ublk.rs:596-598`). A lost
  timer waits for the 10 ms tick.
- **Detach and deposition.** `detach` (`engine.rs:1856-1878`) and the deposition branch of
  `drain_one` (`engine.rs:1841-1853`), before `targets.remove`, drain the gate and remove the
  name from `held`; drained requests complete `Err(NotAttached)` and `Err(Deposed)`, as the
  requests in flight do.
- **`enforces_qos: true`** in both `GetVolumeRequest`s (`engine.rs:668,1782`).
- **Default path.** Unlimited: one `Option` test per request, one empty-set test per pump.
  Limited: a few u128 operations under the mutex the frontend holds; an allocation only to hold.
- **Waits (S4).** A held request keeps its ublk tag: at most `depth × queues` wait (`--depth`,
  `--queues` 1-64, `flaminio-io/src/node.rs:44-54,203-204`). Each is admitted at most
  `max(1/iops, io_buf/bytes_per_second)` after its predecessor: in all, 128 s at the floors with
  the defaults (depth 128, one queue, 512 KiB), hours at the flags' limits. Requests past the tags
  wait in blk-mq, unbounded by QoS. Nothing held is emitted, so no `request_timeout` runs. The
  kernel re-arms a privileged ublk request's timeout (`ublk_timeout`); `make qos-check` checks it.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): S1, S5.
- `meta/domain/qos.go` (new), `meta/domain/domain.go`: S1.
- `meta/service/service.go`, `meta/service/auth.go` (comment), `control/csi/controller.go`: S1, S5.
- `meta/cmd/flaminio-meta/main.go`: S1, S5 (the switch, its usage and doc; `create`'s policy and
  its comparison).
- `dataplane/crates/flaminio-metaclient/src/pb.rs`, `tests/interop.rs`,
  `scripts/dev/meta-interop.sh` (serves with `--qos`): S1, S5.
- `dataplane/crates/flaminio-node/src/qos.rs` (new), `src/lib.rs`, `src/engine.rs`: S2-S4.
- `dataplane/crates/flaminio-io/src/ublk.rs`: S4 (the timer).
- `deploy/lab/qos-check.yaml` (new), `scripts/lab.sh`, `Makefile`: the lab check.
- `scripts/guest/meta.sh`, `scripts/lab.sh`, `Makefile` (`meta-flags`, `lint`),
  `scripts/tools/meta-flags-test.sh` (new): the lab flags (S5).
- Tests: `meta/domain/qos_test.go`, `meta/service/qos_test.go`, `control/csi/csi_test.go`,
  `meta/cmd/flaminio-meta/main_test.go`, `dataplane/crates/flaminio-node/tests/engine.rs`.
  Docs: `docs/lab.md`, `docs/followups.md`.

## Contracts

```proto
// Per-volume limits on client I/O (docs/issues/75). Reads and writes share them.
message QosPolicy {
  uint64 iops = 1;              // READ and WRITE requests per second; 0: unlimited
  uint64 bytes_per_second = 2;  // 0: unlimited
  uint64 burst_iops = 3;        // 0 on a limited dimension: one second of its rate
  uint64 burst_bytes = 4;
}
// Volume:              QosPolicy qos = 40;      unset: unlimited
// CreateVolumeRequest: QosPolicy qos = 40;
// GetVolumeRequest:    bool enforces_qos = 40;  the caller is a data plane that enforces qos
```
- **Tags.** 40 is above the sibling plans' `Volume` proposals (9-10 for #13 and #18, 11-14 for
  #70); if one takes 40 first, renumber before `make proto`.
- **The rule** (Go `QoS.Validate`, Rust `Policy::check`): valid iff `iops = 0 ⇒ burst_iops = 0`,
  `bytes_per_second = 0 ⇒ burst_bytes = 0`, and `bytes_per_second ≠ 0 ⇒
  bytes_per_second ≥ 1048576 ∧ (burst_bytes = 0 ∨ burst_bytes ≥ 1048576)`. Unset and all zeros
  both mean unlimited.
- **Errors.** CreateVolume `InvalidArgument` `qos: <rule>`, or, with `serve --qos` off and no
  volume of that id, `FailedPrecondition` `qos: limits are off on this metadata service`.
  GetVolume `FailedPrecondition` `volume <id> has a QoS policy and this data plane does not
  enforce it`. CSI `FailedPrecondition` `the metadata service did not store the QoS policy of
  <id>; the volume was deleted: upgrade every instance first`. metactl, exit 1: `volume <id> was
  created without its QoS policy (an older metadata instance): delete it and upgrade every
  instance`. Attach `unusable QoS policy: <rule>`, on the control socket's `error` line.
- **etcd.** The header's `qos` object, keys as the proto fields, absent when unlimited.
- **StorageClass.** `qosIops`, `qosBytesPerSecond`, `qosBurstIops`, `qosBurstBytes`: decimal
  integers, default 0; errors keep `parameter %s=%q: %v`. PVC annotations are not read.
- **Rust.** `Engine::take_qos_timer(&mut self) -> Option<Instant>`.

## Tests

| Rule | Test (file: case, level) |
|---|---|
| S1 | `meta/domain/qos_test.go: TestQoSValidate`, the rule's table, unit; `qos.rs: policy_check_matches_the_contract`, the same table, unit. `meta/service/qos_test.go: TestCreateVolumeStoresQoS` (switch on): stored, returned, `InvalidArgument`, absent when unlimited. `meta/cmd/flaminio-meta/main_test.go: TestCreateQoSArgument`: four decimal numbers reach the stored policy; three or five fields, `1e6`, `-1` and `18446744073709551616` are refused with no `CreateVolume` sent (the fixture counts calls, `main_test.go:79-84`). `csi_test.go: TestControllerQoS`: the names, `16Mi`/`-1`/`1e6` refused, the same retry OK, another policy `AlreadyExists` with the volume still there (no `DeleteVolume` sent); with the in-memory service. |
| S2 | `qos.rs`, unit, fake time: `starts_empty_and_refills_at_the_rate`, `caps_at_the_burst`, `burst_zero_is_one_second`, `both_dimensions_must_cover`, `covers_never_debits`, `a_cost_above_the_burst_is_admitted_at_a_full_bucket`, `a_large_head_is_not_overtaken`, `huge_values_do_not_overflow`, `admitted_never_exceed_rate_times_window_plus_effective_burst` (sums the counts and lengths of the requests admitted, 4 KiB-1 MiB at varied times, every window, policies with a 0 burst among them). `engine.rs`, integration: `a_request_across_chunks_is_charged_once` (1 IOPS, burst 2, once full: a request over two chunks, then a one-chunk request, both admitted with no QoS timer armed; a third is held; a double charge would hold the second), `fast_reads_share_the_bucket` (a covered read is taken once; the next is refused and waits), `a_fast_read_refused_at_finish_is_charged_again`. |
| S3 | `engine.rs`: `each_attach_starts_with_an_empty_bucket` (1 IOPS; t0 taken before each `Input::Attach`, re-attach included: the first write completes no sooner than t0 + 1 s), `held_requests_fail_on_detach`, `held_requests_fail_when_the_target_is_deposed` (then a pump with the volume gone does nothing), `an_unusable_qos_policy_fails_the_attach`. |
| S4 | `engine.rs: held_writes_outlast_the_request_timeout`: RF 2, `request_timeout` 300 ms, 2 IOPS, burst 1; t0 taken before `Input::Attach`; six writes submitted together after it complete Ok, in order, the k-th no sooner than t0 + k × 0.5 s; ISR unchanged. A free first request or QoS delaying an emitted request fails it. `qos.rs: due_is_exact`; `engine.rs: the_qos_timer_names_the_heads_instant`. Lab: `make qos-check`'s write job. |
| S5 | `meta/service/qos_test.go: TestCreateVolumeQoSOff`: the switch off refuses a limited create (`FailedPrecondition`), admits an unlimited one, answers `AlreadyExists` for the id of a limited volume created with the switch on, and answers a store error from `readHeader` with `Unavailable`. `TestGetVolumeQoSNeedsAnEnforcingDataPlane`: `io` without the flag refused; with it, `agent`, admin, or an unlimited volume answered. `csi_test.go: TestControllerQoSMixedInstances`: against an instance with the switch off and one that drops `qos` (an older instance), the create fails both ways and no volume is left; when that delete fails, its error is returned; a first try that drops `qos` with its answer lost leaves a retry `AlreadyExists` and the volume, with no `DeleteVolume`. `main_test.go: TestCreateQoSNotStored`: the fixture's service (switch on) gets an interceptor that clears the request's `qos` before the handler, as an older instance ignores tag 40; metactl exits non-zero naming the volume, and sends no `DeleteVolume`. `scripts/tools/meta-flags-test.sh`: the cases of [Lab flags](#lab-flags-of-the-metadata-service-s5-shared-with-70). `meta/service/qos_test.go: TestGetVolumeQoSSwitchOff`: a limited volume created with the switch on, then served by a service on the same store with it off: `qos` returned, an undeclared `io` caller still refused. `engine.rs: a_refused_volume_read_fails_the_attach`: the refusal's message, no target, I/O `NotAttached`. Interop: volume `q` created `100,1048576,0,0`; the Rust client is refused without the flag and decodes each field with it. |

Proof: `make test race lint contracts` (contracts runs the interop test). `sim`, `crash-test`
and `miri` are unaffected: `flaminio-repl`, the store and unsafe code do not change.

## Lab flags of the metadata service (S5, shared with #70)

One mechanism for every default-off `flaminio-meta serve` flag: `--qos` (this plan, variable
`QOS`) and `--volume-expansion` (#70, variable `VOLUME_EXPANSION`). Whichever of #70 and #75
lands first adds it; the other adds its flag and variable to it. `META_WAIT` (seconds, default
60, set in lab.sh beside them) bounds the wait after each VM's restart.
- **On each VM:** `/etc/flaminio/meta-flags` (root, 0644), one line `FLAGS=<flags>`, e.g.
  `FLAGS=--qos`. The unit `meta.sh up` writes reads it (`EnvironmentFile=-/etc/flaminio/meta-flags`)
  and ends `ExecStart` (`scripts/guest/meta.sh:28-33`) with `$FLAGS`; no file, no flags. `meta.sh
  up` never writes the file, so every caller of `meta up` keeps the flags: `make meta-up`,
  `cluster-up` (`Makefile:301-308`), `make chaos`'s restart of w (`scripts/lab.sh:1407`).
- **Setting them:** `make meta-flags [QOS=0|1] [VOLUME_EXPANSION=0|1] [VM=…]` runs `lab.sh meta
  flags`, in `cmd_meta` (`scripts/lab.sh:509-525`). lab.sh refuses any given value other than
  exactly `0` or `1` before anything reaches a remote shell (`ENV_VARS` is not quoted,
  `scripts/lib.sh:157`); an unset variable keeps that flag as it is. Then, VM by VM, `meta.sh
  flags` rewrites the file and restarts the service; it installs no binary and rewrites no unit.
  After each VM, lab.sh runs `(wait_for "metadata on $vm" "$META_WAIT" metactl "$VM_A" "$vm"
  nodes) || die "$vm did not answer; undo: make meta-flags QOS=0 VM=$vm, or make meta-up"`,
  before the next VM (`wait_for`, `scripts/lib.sh:211-219`, dies itself and polls every 5 s,
  hence the subshell). That is a liveness wait: whether an instance runs with a flag is what the
  rule below checks.
- **The rule for every lab check of such a flag:** do the operation the flag enables once through
  each cluster VM's own metadata instance (its endpoint, as `meta_endpoints` lists them,
  `scripts/lab.sh:671-675`; `metactl VM SERVER`, `scripts/lab.sh:529-535`), then undo it. An
  instance without the flag refuses, an older one ignores the new field, and either way the check
  fails naming that VM. For `--qos`: `make qos-check`'s probe (Lab). For `--volume-expansion`
  (#70): an expansion through each instance, refused where the flag is off. Each run uses probe
  ids of its own, `<prefix>-<vm>-<unix time>-<pid of lab.sh>`, checked with `valid_id`, since a
  deleted id stays taken until its tombstone is confirmed (`service.go:372-379`) and two runs may
  start in the same second; its probes are deleted by the trap rules of `qos-check`'s probe. No
  check reads the flags file or inspects a process.
- **Test:** `scripts/tools/meta-flags-test.sh`, run by `make lint`. `meta.sh`, with `sudo`,
  `install` and `systemctl` stubbed as in `scripts/tools/node-scripts-test.sh` and its unit and
  flags paths overridable (`META_UNIT`, `META_FLAGS_FILE`): the unit's `EnvironmentFile` and
  `$FLAGS`; `up` leaves the file alone; `flags` sets, clears and keeps each flag. lab.sh, with
  fake VMs as in `scripts/tools/lab-deletion-test.sh`: `QOS=2` and `QOS='1 x'` refused before any
  remote call; with `wait_for` stubbed to fail, the first VM's instance never answering stops
  `meta flags` naming it and the undo, with no call to the second VM; `qos-check`'s probe with a
  fake `metactl`: only w refusing fails naming w; only w answering without `qos` fails naming w,
  and a `delete` of w's probe is recorded; a's inline delete failing fails the check, and the trap
  deletes a's probe; the trap meeting `code = Unavailable` from a's instance deletes through b's;
  no instance answering prints the ids left; all passing deletes every probe. Whether systemd
  passes the flags is shown only on the lab, by the probe.

## Lab

`make qos-check` (`NODE=fl-a|fl-b`, default `fl-b`, anything else refused first; about 5 min;
needs `node-up`, `csi-deploy` and `make meta-flags QOS=1`).
- **Probe, first** (the rule of
  [Lab flags](#lab-flags-of-the-metadata-service-s5-shared-with-70)). For each VM of `cluster`
  (fl-a, fl-b, fl-w): `metactl "$VM_A" "$vm" create qos-probe-<vm>-<T>-<P> 1M 1 1 1M "$NODE" "" ""
  1,1048576,0,0`, T the run's unix time and P lab.sh's pid (`$$`), then `delete` it through the
  same instance. It passes on exit 0 with the printed `volume.qos` holding `"iops": "1"` and
  `"bytesPerSecond": "1048576"` (protojson writes 64-bit integers as strings; an older client or
  instance prints no `qos`). Anything else stops the check with `metadata instance on <vm> does
  not take limited volumes:` and metactl's own output.
- **The trap,** set before the first create: it deletes every id this run passed to `create`,
  whatever that create returned. For each id it tries the cluster instances in order: metactl
  exits 1 on any error and prints `rpc error: code = …` on stderr (`meta/cmd/flaminio-meta/main.go:77-79`),
  which the trap and the probe capture with `2>&1` through ssh, so output holding
  `code = NotFound` counts as deleted, and any other failure tries the next
  instance. An id no instance deletes is printed as left behind. It is cleared (`trap - EXIT`, as
  `scripts/lab.sh:843`) once every probe is deleted. RF 1 preferred on `NODE` puts a probe's
  replica where `NODE`'s sweep clears its tombstone.
- **Then** it detaches `$VOL` as `make bench-pod` does (`scripts/lab.sh:1083-1088`) and applies
  `deploy/lab/qos-check.yaml`: StorageClass `flaminio-qos-check` (`replicas: "2"`, `qosIops:
  "200"`, `qosBytesPerSecond: "1048576"`), namespace `flaminio-qos`, a 1 GiB block PVC, fio on
  `NODE`.
- **Pass**, from fio's JSON (`total_ios`, `io_bytes`, `runtime` as T s): `randread-4k-qd32`, 60 s,
  `190 × T ≤ ios ≤ 200 × T + 200`; `write-1m-qd64`, 90 s, `0.95 × T ≤ MiB ≤ T + 1` and a largest
  completion latency ≥ 30 s (past blk-mq's default timeout). Both: fio error 0; no `I/O error` or
  `timed out` line for the device in `journalctl -k` on `NODE`; the same `/dev/ublkbN` before and
  after; `metactl get` shows `{200, 1048576, 0, 0}`. It prints `NODE`'s `uname -r` with the verdict.
- **Cleanup.** Delete the namespace (its volume goes through CSI) and the StorageClass; nothing is
  formatted. Restore with `make vol-attach VM=<a|b>`.

## Docs

- **`docs/lab.md`:** `make qos-check` beside `make bench-pod`. The StorageClass parameters
  (`docs/lab.md:373-374`) gain the four, their rule, the switch and its two rollouts (`make
  meta-up`, then `make meta-flags QOS=1`; `meta up` keeps the flags), the rollback, in this
  order (`make meta-flags QOS=0`; delete the QoS StorageClasses and the limited volumes, which
  keep their limits meanwhile; then check out the older tree), the leftovers of Compatibility,
  the wait bound, and that no node that may attach a limited volume (any node its pod may run on)
  sets `kernel.hung_task_panic=1`.
- **Makefile help:** `qos-check`; `meta-flags`: `Set optional flaminio-meta serve flags without
  reinstalling (QOS=0|1, VOLUME_EXPANSION=0|1; unset: unchanged); turn one on only once every
  metadata instance and data plane runs this build, which nothing here checks: a rollback then
  erases limits`; `meta-up` adds `keeps the flags of meta-flags`.
- **`docs/followups.md`:** a row for #75, its first step and the rest open.
  **`docs/performance-plan.md`, `README.md`, benchmarks:** none (no measured result).

## Risks

- **Safety.** `flaminio-repl`, the protocol, fencing, leases and durability do not change; the
  one hazard, delaying an emitted request, is S4's test. A held write reached no replica.
- **Security.** Only StorageClass parameters (cluster-scoped) set limits; PVC annotations are
  ignored; `CreateVolume` is admin-only. This step does not restrict classes per namespace
  (Kubernetes' per-class quotas can), and no plan provides tenant QoS ceilings. An `io`
  certificate claiming `enforces_qos` is trusted, as the data plane holds the replicas anyway.
  Inputs are decimal u64; the engine's arithmetic saturates in u128.
- **Compatibility.** See the switch (rollback unsupported and erasing, as for `first_epoch`,
  `domain.go:44-47`) and Versions (an older data plane's attach fails after the target moved).
  Two leftovers while instances are mixed, both never attached, are stated rather than handled:
  the CSI controller's delete failing, and a retry after a lost answer whose first try reached an
  older instance. Each is the volume `pvc-<uid>` of a PVC of a QoS class left Pending with the
  controller's lost-policy or `exists with other properties` error, whose `metactl get` shows no
  `qos` and no target. No remedy is given: deleting it is the administrator's decision, and the
  PVC binds only after its tombstone clears and with the switch on. A retry that meets a current
  instance with the switch off answers `AlreadyExists` and matches (the switch).
  When #76 lands, the policy joins its create identity; it never changes.
- **Open.** Q1: request-count IOPS, combined reads and writes, empty buckets per target and an
  immutable policy are Proposed semantics. The timeout re-arm is checked only on the lab kernel;
  kernels from 6.6 (ADR 0003) are assumed alike. Long waits (S4) keep tasks in D state past the
  120 s hung-task warning; with `hung_task_panic=1` the node panics and its replicas leave their
  ISRs, hence the documented requirement for every node that may attach a limited volume. The
  default path's cost is argued, not measured. A fast read refused at finish is charged twice:
  rare, never past the limit.
  Settled at approval, 2026-11-13: Q1 as proposed; the kernel assumption with its per-kernel
  check; the `hung_task_panic` requirement for nodes serving limited volumes; the double charge
  (see decisions.md, Settled at approval). Changed after approval: that requirement widened to
  every node that may attach one (round 3, decisions.md). Settled 2026-11-13 (user's
  instruction): the widening is accepted; the default path's cost is argued here and measured
  by the issue's benchmark steps (see decisions.md).

## ADRs

- **Consistent:** 0001 and 0002 (the bucket in the Rust data plane, validation in Go, the policy
  in create's existing etcd transaction); 0003 (the timeout re-arm assumed from 6.6, checked on
  the lab kernel); 0005 (QoS never touches leases or the target's clock); 0004, 0006 unaffected.
- **Proposed:** [adr-proposal.md](adr-proposal.md): the limits' semantics, and QoS acting before
  the replication state machine. Approving the design accepts it.
  Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).

## Parallelizable?

Yes. Go (proto, domain, service, CSI, metactl) and Rust (`pb.rs`, `qos.rs`, engine, `ublk.rs`)
build against [Contracts](#contracts) and meet in `make contracts`.
