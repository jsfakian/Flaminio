# Issue #71: Observability and the kubectl flaminio operations CLI — intent

[Issue #71](https://github.com/jsfakian/Flaminio/issues/71), architecture §§17.2/18.
**Approved 2026-11-13; not implemented.**

## Why

- **Reachability is the only exported demo signal.** `deploy/demo/status-exporter.yaml:25–52`
  opens TCP connections and emits `flaminio_meta_up`. It does not authenticate, and it does not
  measure replication, capacity, latency or recovery. The product has no metrics endpoint and no
  CLI plugin.
- **The data exists only in fragments.** The store's `Stats` hold data and pool use, quarantine
  and journal use (`flaminio-store/src/store.rs:97–117`). Only the tests can read the target's
  `Stats` (`flaminio-repl/src/target.rs:107`). Its resync and reconcile work sits in private
  state (`target.rs:145–177`). Fast reads complete in the frontend, not as an engine `Done`
  (`flaminio-io/src/ublk.rs:1232–1237`). The frontend has no latency or error counts.
- **The node agent can find and claim disks, not report them.** `control/agent/{disks,host}.go`
  probes `lsblk`, signatures and holders. It does not export capacity, kernel prerequisites or
  progress.
- **The CLI is absent.** `flaminio-meta` exposes individual metadata and admin RPCs, and
  `flaminio-agent` handles local disks. Neither is `kubectl-flaminio`.

## This step

The issue is larger than one reviewable change. Plan review round 1 found the draft's mutating
verbs and recovery plans larger than their mechanisms
([decisions](decisions.md)). This plan is **step 1**: what the code already produces, made
visible read-only.

1. Prometheus metrics: per-volume frontend latency, throughput and errors; per-disk capacity;
   ISR state from the metadata service; resync and reconcile progress. Labels and cardinality
   bounds are documented (issue step 1, except disk health and controller failures).
2. `kubectl flaminio status | node | disk | volume | doctor`, read-only, with a #69 reader
   identity and the existing service authorization (issue step 4, without `recover`).
3. Doctor checks the kernel version, ublk, io_uring and network MTU. Clock skew, peer latency,
   disk health and risky configuration report `unsupported` (issue step 4, part).
4. Alert rules for lost redundancy, capacity pressure, stalled resync or reconcile, and lost
   telemetry, with runbooks (issue steps 3 and 5, part).
5. The overhead of the per-I/O counters, measured on and off in the lab.

After this step a read-only user can inspect the lab's volumes, disks, nodes and resync progress
through metrics and the CLI. Doctor can find a broken kernel, ublk, io_uring or MTU prerequisite.

**Depends on #69's first step**, approved 2026-11-13 and not implemented. This plan assumes
it provides what #69's design states: the reader identity `spiffe://<td>/flaminio/reader/<name>`,
admitted to `ListNodes`, `ListVolumes`, `GetVolume` and `ListTombstones` only, and
`control/status` with `Collect` and a pure `Derive`. `Derive` counts the chunks whose ISR is
shorter than the volume's replicas, and fails as a whole on invalid data. Under #74's
`--privsep` there is no telemetry in this step: the helper request for its socket is a later
step of #74 (contracts, "Under #74's `--privsep`"). This step needs nothing from #43, #45, #54,
#77 or #83.

## What stays open in #71

- **Traces and structured logs** (issue step 2): sampled OpenTelemetry spans, structured Rust
  logs in place of `eprintln!`, and CSI request logging without request dumps
  (`control/csi/server.go:logCalls`).
- **Summaries, Conditions and Events** (issue step 3): Volume, Disk and StorageNode resources.
  They wait for #43's CR reconciliation. That step reconciles them with §17.2's `Disk`. Before it
  picks the kinds' scope, an envtest case checks where Kubernetes accepts Events about
  cluster-scoped objects.
- **Alerts on injected faults in a running Prometheus.** This step proves the rules with
  promtool, and in the lab it proves that faults change the series. A later step deploys
  Prometheus with #43's add-on.
- **`recover`**: the CLI integration of #54's restore operations, once #54's API exists.
- **Doctor:** clock skew (with #83's clock status), peer latency, disk health (SMART) and risky
  configuration.
- **Metrics:** disk health, controller failures (`control/failover`, `control/drain`), and
  rebuild, scrub or gray-failure progress as #45–#47 land. Trace sampling configuration and its
  overhead come with step 2.

The issue stays open until its "Done when" holds in full.

## Out of scope

- Mutating CLI verbs. The issue does not ask for them. Drain, move, delete and claim keep their
  owners' tools (`flaminio-meta`, `flaminio-agent`).
- Automatic repair or failover based on a metric, an alert or a doctor result.
- New replication rules, payload capture, unauthenticated endpoints, tenant-facing telemetry.
- Running lab checks or benchmarks while drafting.
