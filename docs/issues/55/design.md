# Issue #55: Compact-budget evidence — design

[spec](spec.md) · [decisions](decisions.md)

Approved 2026-11-13; not implemented.
Measurement-only slice, no compact implementation or new runtime resource/default.
No lab measurement, disk preparation or footprint result was produced while drafting.

## The facts it starts from

`flaminio-io/src/node.rs:134` defaults depth 128 / io_buf 512 KiB: 64 MiB per queue.
`ublk.rs:1097–1127` allocates anonymous buffers, or shared memfd staging in zero-copy mode.
`flaminio-store/src/state.rs:51` retains decoded MapPageState;
`flaminio-format/src/meta.rs:483` owns Phys and checksum vectors, not an evicting cache.
At 64 KiB clusters, nominal 16-byte Phys plus 16 u32 checksums is 80 bytes/cluster,
1.25 MiB/GiB written excluding vector/page/allocator overhead. Actual size/layout and page rounding
must
be reported, not assumed portable. `node.rs:268–271` logs startup used clusters but no live map
count/RSS telemetry. 100 GiB lab disks cannot measure a 1 TiB / 1.28 GiB map footprint.
`control/failover/kube.go` is an HTTP client, not a pod-resource observer; manifests have no
complete budget. #12 pod deployment, #15 multi-device runtime and future #71 metrics change
inventory but are not assumed present in current source.

## Scope and inventory (S1, S2)

Add `scripts/guest/resource-sample.py` and `scripts/checks/compact-budget.sh`. Expected lab
members (both forms require the CSI driver; no operator exists yet, #43):

| Host | Unit form | #12 pod form | Identity source |
|---|---|---|---|
| fl-a, fl-b | flaminio-node; CSI node with registrar/liveness sidecars | dataplane container and exec probe; same CSI node set | systemd MainPID/ControlGroup or DaemonSet UID → pod UID → runtime container/cgroup |
| fl-w | CSI controller with provisioner/attacher/liveness sidecars; failover | same | Deployment UID → ReplicaSet UID → pod UID → container/cgroup |
| all three | flaminio-meta and flaminio-etcd | same | named units' MainPID/ControlGroup |

The agent is a one-shot CLI: in the pod form, its exec probe is charged inside the dataplane
container. Completed init/claim containers are listed as exited, not expected to be running.
#12 uses direct mode; if #74's unit helper exists it is included in that unit's cgroup.
Record enabled optional product services separately from this required baseline; an unknown
service makes total-product coverage inconclusive. k3s and observer remain contextual totals.

Select only controllers in `flaminio-system` and follow their owner-reference UIDs, never pod
name prefixes. Resolve container IDs to host cgroups, verify executable hashes, deduplicate
unit/container membership and include descendants. PID identity is (boot_id, pid, start_ticks);
a daemon/container replacement invalidates the interval. Thread/probe churn changes thread
counts, not daemon identity. Per-thread CPU pairs use their own start identities; aggregate
CPU comes from the stable service/container cgroup, so exited probes are still counted.

Keep only nonsecret ExecStart values for depth, io-buf, queues, poll-us, gather-us,
commit-target-mib, flush-coalesce and the boolean I/O flags. Filter by flag name before logging;
never persist raw ExecStart/cmdline, environment, key/token paths, credentials or raw metactl
responses. Persist only validated volume id/epoch/target/placement and workload/accounting data.
Every proc/API string is data: validate IDs/paths, quote arguments, and construct no shell from
returned names. Capture metadata reads use the node's `io` identity (`AS=io`), never admin.

Per-node data-plane target: at most 1.0 core and 512 MiB. For units, verdict CPU/memory come
from `flaminio-node`'s cgroup; for pods, from the `dataplane` container cgroup, including its
exec probe, excluding pause/other product containers. Use `cpu.stat usage_usec` delta / local
monotonic elapsed and maximum sampled `memory.current`, across all three repetitions. The
memory metric includes anon, file, shmem (zero-copy memfd) and kernel charges, with no cache
subtraction. Missing stable cgroup counters makes the verdict inconclusive. Report PSS/RSS,
virtual size and product totals separately; never add PSS to cgroup memory or sum shared RSS.
This measures a target for the named state, not feasibility under a 512 MiB allocation limit.

## Sampling and metrics (S3, S6)

Window: 60 s after 30 s settle, sample each second, repeat three times with local monotonic
start/end. Use `/proc/PID/schedstat` and task schedstat (runtime ns) for thread attribution
where available; the PID path covers only its leader, not whole-process CPU. Sum only paired
thread identities and label incomplete lifetimes/exit coverage as incomplete attribution;
`/proc/PID/stat` ticks, parsed around comm with actual CLK_TCK, are a cross-check.
Missing schedstat is unknown for that diagnostic, not silently tick-derived CPU. Collect
status/smaps_rollup, thread counts and cgroup v2 cpu.stat, memory.current/stat/events. Pair only
matching identities and monotonic counters; retain units and min/median/max. OOM, restart,
changed settings or unresolved background activity invalidates an ordinary idle verdict.

The host invokes the sampler through `vssh "$vm" 'sudo python3 -I -' < resource-sample.py`
with validated, quoted scalar options. It streams JSON stdout; no remote temporary script or
`run_on_vm` bash concatenation. Preflight checks Python/cgroup support; absence is incomplete.
Bound output to 64 MiB/run, 4096 processes and 65536 threads. Sample only product processes
in detail; collect contextual k3s cgroup totals at window ends, without per-thread/smaps dumps.
A cap breach invalidates affected coverage; it never silently drops costly processes.

Wakeups use an already authorized tracefs instance under `/sys/kernel/tracing` with
`sched_wakeup`/`sched_wakeup_new`, thread identities and per-CPU `stats` overrun/lost-event
checks. Capture does not change trace configuration. Without a qualified source record null
and reason; provisioning installs no perf. Context switches stay separate. Optional profile
output has its own 64 MiB cap; lost events/cap breach invalidates wakeups. Compare paired
unprofiled windows to expose observer cost; unresolved cost makes CPU/wakeups inconclusive.
No eBPF agent or kernel security change is installed implicitly.

Check attachment/full ISR/workload/progress before and after each window, outside sampling:
control-socket calls create threads and wake the engine. Use #71's bounded read-only observation
if available, not an assumed metric; otherwise record what cannot be established. Normal
heartbeats/probes/controllers remain enabled. No VM wall-clock subtraction for CPU intervals.

## States and map slope (S4, S5)

`make compact-budget MODE=capture STATE=<no-device|attached|post-load> OUT=<new-dir>` is
read-only and prepares no state. Unprovable state means inconclusive. `MODE=matrix` requires
`CONFIRM=matrix` before remote mutation; it creates/writes/deletes only its disposable volume.
Preflight: no active demo/campaign, fio, other attachments, or PVC consumer pods; no concurrent
admin (CONFIRM declares exclusive operator use); fl-c/any extra data node absent or stopped
and metadata-not-alive. Verify every chunk's
replicas and full ISR are exactly {fl-a, fl-b} after creation and around load/captures. Keep
existing unattached idle replicas: record their known initial allocation/layers; never delete
them. Check-then-act exclusivity is an operator precondition, not an enforced lab lock.

Generate `cbudget-<UTC stamp>-<random hex>` within `valid_id`, confirm it absent, then persist
operation intent before create. Save create's returned `volume.epoch` as create_epoch and
expected_epoch; this is the initial incarnation epoch, not a nonexistent GetVolume first_epoch
field. Advance expected_epoch only from acknowledged owned attaches. Keep raw tokens only in
memory for attachment, never stdout/artifacts. Recheck id/current epoch against expected_epoch
before any destructive cleanup action; mismatch, missing id or an ambiguous RPC outcome leaves
data untouched and reports manual recovery. DeleteVolume accepts id only: this check is not an
atomic fence against another administrator recreating it. The concurrency risk is named, not
accepted; absent exclusivity the harness refuses mutation. Never invoke delete_volume's format
argument or let cleanup start fl-c via generic all-data-node helpers; operate only on a/b.
Cleanup stops its owned fio, verifies ownership before detach, detaches via the existing socket
path, rechecks epoch, and deletes metadata once. Save the delete acknowledgement; existing CSI
sweeps delete a/b replicas. Poll their tombstones for at most 120 s, then report leftovers.
After a persisted delete acknowledgement, absent id is expected: poll only, never re-delete.
If the reply was lost before persistence, absent id is ambiguous: preserve/manual recovery.
Neither absent-id case calls a direct by-name replica delete.

Record the starting deployment/settings. Every unit restart explicitly sets `FORMAT=0`, clears
inherited ATTACH/TOKEN and pins all node_env I/O settings from the original validated options,
including explicit false/default values; reject unsupported overrides rather than guess.
Never claim/format disks. All stops/restarts are matrix-only and recorded before the action.
For #12 pods, hold the existing DaemonSet off one fully detached node for offline inspection:

1. Record node UID, DaemonSet UID/template revision, pod UID, and the original presence/value of
   `node-restriction.kubernetes.io/flaminio-dataplane` (required `pod` in this form). Persist
   the hold intent before a node-label mutation; leave all other node labels unchanged.
2. Remove only that selector key using an API patch conditional on the observed node UID,
   resourceVersion and label value. Re-read the same controller/template and require the key
   absent. Changing a node selector match makes the DaemonSet remove its pod, independent of
   OnDelete ([docs](https://kubernetes.io/docs/concepts/workloads/controllers/daemonset/)).
3. Wait at most 300 s for owned pods assigned/targeted to this node to disappear and runtime
   containers/processes
   to exit; verify no process holds the recorded disk. Recheck absent selector/same identities
   immediately before inspection. An API disappearance alone is insufficient; missing host
   evidence or a successor/old process still alive refuses inspection. Never force-delete.
4. Keep the hold throughout inspection. To replay once, restore the saved label conditionally,
   wait at most 300 s for the successor Ready/store-open acknowledgement, then persist a new
   hold intent, remove the selector again and repeat step 3 before reinspection. Finally
   restore the saved label and wait for the successor before fresh-open/no-device capture.

Label restoration is an API patch conditional on unchanged node/controller/template identity
and the held key still being absent. An already-restored exact value is idempotent; a changed
identity/value is preserved and reported for recovery. Normal, failed-inspection and cleanup
paths restore the exact original label/deployment; persist acknowledgements and bounded wait
results in the lifecycle manifest. A lost patch reply is resolved only by re-reading the same
identities/key: record whether the intended state is observed, never blindly remove/overwrite.
Restore a proven owned hold independently of ambiguous volume ownership; volume deletion stays
refused. No node.sh up, unit-mode switch, cordon-only hold or global DaemonSet change is used.

Matrix: no-device baseline; attach an owned 16 GiB volume (RF2), capture idle; populate a dense
2 GiB prefix, later extend to a total 16 GiB with distinct logical extents. After each write,
stop fio and verify placement/full ISR, capture attached idle, then warm using read-only fio
within that written prefix: `bs=io_buf` (default 512 KiB), `iodepth=depth` (128), `direct=1`,
`ioengine=io_uring`, one pinned job per configured queue, 30 s. Record the exact job; unproven queue
coverage makes the post-load state inconclusive.
Do not claim every staging page resident merely from issuing reads.
Capture post-load idle. Hold RF/layers/cluster/queues/depth/io_buf/verify/zero-copy/poll fixed
and record actual settings; do not silently force poll=0. W has no data-plane slope/verdict.

For each population, after those captures: detach everywhere, stop each data plane in turn and
run read-only `flaminio-disk inspect "$disk" --slots` on its recorded disk; pod stops use the
selector hold above. Never inspect a live store.
The inspector reports checkpointed data bits and pool pairs, not live mapped-page counts.
Accept allocation evidence only when journal says `to replay 0 batches` and no invalid slots.
Otherwise restart without formatting (Store::open replays/checkpoints), stop and inspect once
more; remaining replay/errors mean estimate/unknown. Then restart, before any attach/load,
settle and capture **fresh-open/no-device** on both data nodes. Missing/unstable fresh captures
make the slope inconclusive. Restore services even when inspection fails; no retry loop.

Use fresh-open flaminio-io PSS deltas as the primary per-node slope; post-load deltas separately
show buffer/allocator retention. Compute deltaPSS/deltaWritten and deltaPSS/deltaAllocated only
where that node's allocation is qualified. Subtract its own fixed initial allocation, account
for its replicas/layers and page rounding; reject changed unrelated allocations. Record pool
pairs separately. Derive expected dense map pages from touched chunks × layers × rounded pages
per chunk (partial chunks included), not from total pool usage; sparse/unknown maps stay an
estimate. A regular-file fixture checks this arithmetic without replacing daemon measurements.
Compare to nominal 1.25 MiB/GiB at 64 KiB clusters: 512/1.25 ≈ 409.6 GiB is only an estimate;
non-map costs cross earlier. Multiple layers/devices and unbounded queues remain reported gaps.

## Contracts and files touched

New script/sampler/analysis files under scripts/checks, scripts/guest, scripts/tools; lab.sh
and Make help dispatch proposed `compact-budget` only. No product API/proto/wire/store format
change. CLI `MODE=capture|matrix|cleanup STATE=... OUT=...`, `CONFIRM=matrix` for matrix/cleanup;
SAMPLE_SECONDS/SETTLE_SECONDS/REPS bounded to 10..600/0..600/1..10. Capture/matrix create a
new owned nonsymlink OUT; cleanup opens an existing owned nonsymlink OUT and validates its
schema/lifecycle before any action. Cleanup never starts a new run or writes test data.
Lifecycle records source/cluster/boot/deployment/disk IDs, owned volume/epochs, completed and
pending operations, original settings/selector-hold identities and leftovers. Persist each
intent before action and
its acknowledgement after; unknown results are refused, not replayed blindly. SSH loss exits
255 with the recovery command `make compact-budget MODE=cleanup CONFIRM=matrix OUT=<dir>`.
Exit 0 means a valid requested observation or completed cleanup; 1 incomplete/failed required
evidence or cleanup; 2 arguments/missing confirmation; 255 SSH loss. Optional wakeups may be
unknown without invalidating independently valid CPU/memory; required state/metrics may not.

Artifacts schema_version 1: manifest.json (source/diff hash, role/topology, boot identities,
options, memory/CPU constraints, state, dataset and observer), samples.jsonl (raw typed counters,
identity, units, monotonic timestamp), summary.json (CPU cores, memory semantics, threads,
wakeup availability, validity/reasons), plus owned matrix lifecycle manifest. No secret dumps.
Budget verdict target_met|target_exceeded|inconclusive applies only named state/population/metric.
No numeric field null becomes 0 in analysis. #71 telemetry can supplement, not replace identity.

## Tests

| Rules | Planned tests |
|---|---|
| S1/S2 | per-form expected members, missing sidecar, UID-qualified decoy tenant pod, duplicate cgroup, PID reuse/restart versus thread/probe churn |
| S1/S3 | opposite PSS/cgroup verdicts, shmem/file charges, schedstat ns and cgroup usec, tick cross-check, counter reset, missing permissions |
| S3 | loss/overrun and context-switch distinction, optional wakeups null, observer cost, output caps and k3s endpoints only, no socket calls inside window |
| S4 | 2/16 GiB per-node fresh-open deltas, retained-heap variation, sparse/partial-page/layer rounding, stale non-empty-journal evidence, changed baseline/settings, read-only warm-up |
| S5 | fake SSH/API: capture read-only/non-admin; no CONFIRM exits 2 with no mutation; fl-c replica/live node refuses; existing unattached replicas survive |
| S5 | inherited FORMAT=1/ZERO_COPY=1/ATTACH cannot format/change settings; pod restart uses #12 lifecycle; interrupted stages/SSH loss resume cleanup; crash after persisted delete acknowledgement polls only; recreated/missing/ambiguous epoch refuses all destructive calls |
| S5 | pod selector hold blocks successor throughout inspect/reinspect; absent pod with surviving process refuses; interruption/failed inspect restores exact label; lost patch reply and changed node/controller/label preserve recovery state |
| S6/S7 | schema/null/target scope, sanitized token/key-path fixtures, failed restore keeps lifecycle/leftovers, no invented measurement or total-product target |

Add `scripts/tools/compact-budget-test.sh` to `make lint`; it runs shell fakes and
`python3 -I -m unittest discover -s scripts/tools -p 'compact_budget_test.py'` for pure sampler/
analysis fixtures. No new third-party Python dependency or hardware throughput/RAM threshold
in CI. These are planned tests, not runtime results of this review.

## Lab, docs, risks and ADRs

The lab matrix is run only on explicit request, with its CONFIRM gate: it writes/deletes its own
disposable data and may detach/restart
owned service for offline evidence, never raw-formats. Persist cleanup plan before preparation;
restore the original unit/pod form and pinned settings, detach/delete only the owned test
volume and leave every pre-existing replica untouched. No original attachment is displaced: the
preflight refuses one. An interrupted/ambiguous operation is reported, not guessed away.
If cleanup cannot prove ownership, leave it and fail with exact recovery steps. Capture mode
needs no mutation authorization but shared-lab targets still run only on explicit request.

Add dated benchmark report with raw artifact paths, measured scope/counter limits and source
causes; add a budget section to performance-plan, update lab guide/followups and README roadmap
to identify this measurement slice. ARCHITECTURE and accepted ADR wording stay as intent
specifies; the results note distinguishes the planned mode from current evidence. No root-audit
wording correction is assumed. ADR 0001/0003/0004 remain;
consolidating operator with tenant-facing controllers risks CA boundary and is explicitly later.
arm64/large-page kernels/distributions and under-budget performance remain untested future work.

## Parallelizable?

Yes: read-only sampler/analysis and owned matrix orchestration share the exact result schema.
Mode/buffer/cache implementation must wait for measured evidence and its own approved plan.
