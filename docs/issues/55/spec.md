# Issue #55: Measure the compact resource budget — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Measurement-only first slice of step 2. No compact flag, limit, pooling, lazy maps,
controller consolidation, arm64 or distribution support. All remain later work; #55 stays open.

## Terms

- **Product set:** flaminio-io, metadata, CSI components and sidecars, and product etcd
  members, attributed once per host/cgroup. The node agent is a one-shot CLI, not a resident
  daemon. No operator exists in this slice (#43 owns it). Kubernetes/k3s and measurement
  tooling are separate contextual totals, not silently omitted or charged twice.
- **Idle states:** no attached device; attached/full ISR/no client I/O; attached and idle after
  a controlled load has touched queue/staging buffers. Store content and background work are
  recorded in each state; idle does not mean no heartbeat or timer.
- **CPU cores used:** delta CPU time / monotonic elapsed time; 1.0 is one full core. Idle CPU
  is reported, not described as consuming one core merely because 1 vCPU is assigned.
- **Memory:** per-process RSS/PSS/anonymous/private/shared and owned cgroup memory, distinguished;
  virtual address reservation is not resident RAM and summed RSS may double-count shared pages.

## The invariant

Measurement does not change the data path/defaults or claim a compact budget has been met by
sampling a lightly populated disk. Missing identities, counters or unstable background work
produce unknown/inconclusive evidence, never zeros or a pass.

## Behaviour

**S1. Budget definition.** State separately the proposed data-plane target of 1 core / 512 MiB,
total
product-set footprint and contextual Kubernetes/tool overhead. Define disk/cluster size,
written/mapped amount, layer/replica/device/queue count, idle state, sampling duration and
aggregation. The verdict uses the data-plane service/container cgroup
CPU and maximum sampled memory.current, including anonymous, file, shmem and kernel charges;
RSS/PSS remain separate diagnostics. The pod metric includes its exec probe; the unit metric
includes its children. No new allocation cap or accepted product guarantee is established by this
slice.
Settled at approval, 2026-11-13: 1 core and 512 MiB is the data-plane target measured against (see
decisions.md, Settled at approval).

**S2. Stable inventory.** Find every product process on fl-a/fl-b/fl-w through owned units and
pod/container cgroups, including transient components. Bind PID to boot/start identity, record
threads and versions, and prevent duplicate unit/container counting. Expected members per deployment
form are enumerated in the design; missing members invalidate
that scope's result. Thread/probe churn is recorded without treating it as a daemon restart.
Both #12 pod and existing unit deployment forms are supported explicitly. Matrix runs exclude
other live data nodes, fl-c included, and verify every test chunk is replicated on fl-a and fl-b.

**S3. Metrics.** Over a fixed monotonic interval measure CPU time, memory and thread count;
collect actual wakeup events where supported. Context switches are reported separately, never
renamed wakeups. Unsupported/permission-denied counters are unknown. Track process restarts,
background tombstone/resync/scrub activity and observer overhead; unstable intervals are marked.

**S4. Idle and population matrix.** Measure all three idle states and at least two controlled
written amounts with the same cluster size/replicas/layers. Compare memory slope to resident
map/page/bitmap accounting, not capacity alone. Fresh-open/no-device captures at both amounts
provide the primary per-node slope; post-load captures expose retained buffers/allocator memory.
Keep existing unattached replicas as a fixed, recorded initial allocation. State page rounding
and evidence quality. The current 100 GiB lab cannot prove the 512 MiB map crossing (~400 GiB).

**S5. Data safety.** Preparation writes only an explicitly owned disposable test volume after
exclusive lab/preflight checks; never overwrites raw disks or unrelated PVCs. Capture-only mode
is read-only. Restart/stop needed for inventory/inspection is explicit and separately authorized.
Matrix preparation needs explicit CONFIRM=matrix and checked lab exclusivity. Cleanup can
resume from its lifecycle manifest, verifies the owned incarnation/current epoch, preserves
unknown data, restores owned workload state and reports leftovers. Existing delete-by-id APIs
provide no atomic incarnation fence; exclusivity is an operational precondition, not a lock.

**S6. Reproducible results.** Retain raw counters, source/configuration/topology/tool versions,
CPU/memory constraints, exact workload, sample windows and validity. Record gaps/causes in code
and future work. Do not extrapolate unsupported arm64/kernel/distro/low-memory performance.
No claim of hardware-independent performance from the oversized lab machines.

**S7. Report and scope.** Write a dated benchmark report/performance-plan budget table showing
each measured/inconclusive/unbounded item and implementation gap. Existing architecture's
compact claims are target state, not current feature evidence. Later enforce/test the budget
with a real mode and under-load performance; this measurement cannot close the issue.

## Tests

- S1–S3: fixture process/cgroup inventory, PID reuse, counters/wrap/reset, missing permissions,
  shared-memory aggregation/verdict, thread/probe churn, per-form expected members, real wakeups
  vs context switches, observer overhead/unstable sample.
- S4–S5: fake lab state matrix, wrong ownership/unrelated volumes, missing amount, no raw format,
  interruption/cleanup/recreation, inherited unsafe settings, read-only non-admin capture;
  fresh-open slope, non-empty journal, placement on fl-c and slope units/rounding.
- S6–S7: result schema/analysis fixtures and links, then explicitly authorized actual lab run.

## Open questions

The final compact-mode scope/budget and total-product target require human decision after data.
Measurement permissions/observer tooling and map-allocation evidence must be qualified for the
actual deployment; no new footprint numbers or implementation choice is invented here.

Settled 2026-11-13 (user's instruction): measured first (S1–S7, the dated report and budget table),
then decided from that report in a later step; no number is set now, and product-set totals stay
observations (see decisions.md).
Settled at approval, 2026-11-13: permissions, observer tooling and map-allocation evidence as the
design proposes (see decisions.md, Settled at approval).
