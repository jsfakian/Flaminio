# Issue #75: QoS, background scheduling and performance gates — intent

[Issue #75](https://github.com/jsfakian/Flaminio/issues/75), architecture §5.3, §16 and §19.

**This plan is the issue's first step:** per-volume IOPS and bandwidth limits with bursts, set
by a StorageClass and enforced by the volume's target before the replication state machine. The
issue stays open for the steps [below](#what-stays-open).

## Why

1. **There is no per-volume QoS policy.** `EngineConfig` has timing, `max_io` and heartbeat only
   (`flaminio-node/src/engine.rs:167-177`); CSI has no IOPS, bandwidth or burst parameters
   (`control/csi/controller.go:67-74`). #15 plans multi-volume dispatch, not limits.
2. **Bulk limits are not general scheduling.** The target has four bulk slots; commit gathering
   uses count, byte and time controls. They do not share foreground, rebuild, scrub and backup
   work fairly. #45's adaptive copier and #52's backup producer are future work; #46 plans its
   own scrub byte rate.
3. **Several transport queues are unbounded,** and bounding them is not a local change.
   `net.rs:98,139,212` use unbounded channels, `Net::send` drops a failed send (`net.rs:91-93`),
   the node's input is an unbounded `mpsc` (`flaminio-io/src/node.rs:288`). A delay after the
   target emits a request counts against its 3 s timeout (`flaminio-repl/src/target.rs:811-828`):
   the session fails and the ISR shrinks, the livelock `docs/replication-protocol.md:269-273`
   records.
4. **Measurements are hardware-specific.** The 2026-10-29 QD1 report records queue and polling
   costs, 4-vCPU settings and 1 GbE limits. `docs/performance-plan.md` requires matched spans,
   cache state, durability, CPU budgets and repetitions.
5. **No scheduled performance gate exists.** `lab.yml` is manual and exclusive; CI runs
   correctness checks only.

## What we want (this step)

1. A per-volume limit on client requests and bytes per second, with bursts, set by the
   StorageClass and fixed for the volume's life.
2. One charge per client request, whatever the queue, fast path, chunk split or replica count;
   a new target starts with no credit.
3. Throttling only delays requests before the target sees them: never an error, a session
   failure, an ISR change or a lease change.
4. Limited volumes are meant to exist only once every metadata instance and data plane runs this
   version, behind an explicit switch that operators turn on; the metadata service then refuses
   one to an older data plane rather than let it serve it unlimited, and rolling back is
   unsupported.
5. A lab check that a volume holds its limits.

## What stays open

Later steps of #75, each with its own plan; nothing here guards them.
- **Step 1, the rest:** PVC overrides, which need tenant ceilings no plan provides (#73's step
  does not plan them), changing a volume's policy, and two limited volumes on one node (#15).
- **Step 2:** weighted fair scheduling of rebuild, scrub and backup (#45, #46, #52), and the
  audit's bounded queues, memory reservations and priority for fencing and lease traffic. They
  must act before the target emits, or bound the delay after it well under `request_timeout`.
- **Steps 3-5:** benchmarks of competing volumes and of foreground load during each background
  producer (foreground p99 with each producer running, CPU µs per I/O, recovery progress);
  scheduled fio and pgbench gates on fixed hardware, which need a hardware agreement first;
  comparisons across RF, encryption (#50), raw devices and peer systems, with #84's conformance.

## Out of scope

- Changing #9's goals, #45's recovery semantics, #46's scrub, #52's backups or #50's encryption.
- Any change to the replication protocol, the simulator, the on-disk format or capability tokens.
- Running or publishing lab results while drafting.
