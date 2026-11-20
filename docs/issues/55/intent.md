# Issue #55: Compact mode — intent

[Issue #55](https://github.com/jsfakian/Flaminio/issues/55), README roadmap phase 4,
ARCHITECTURE §1.1, §1.2 and §5.1. This change is where the issue's step 2 starts: the budget
written so that it can be checked, and what Flaminio uses when idle, measured on the lab's
2-node + witness cluster. It builds no mode. Steps 1, 3 and 4 and the mode itself come after:
issue #55 stays open after this change.

## Why

1. **There is no compact mode.** ARCHITECTURE §1.1 and ADR 0004 say the data plane has one
   "from the start". `flaminio-io node` has no such flag (`flaminio-io/src/node.rs`, `USAGE`).
   §5.1 names three things. Against the code:
   - **"A single reactor thread."** There are no reactor threads. One engine thread owns the
     store and every state machine; with a device it is the device's queue thread
     (`node.rs:11`). `--queues` defaults to 1 (`node.rs:140`). The other threads are fixed:
     `flaminio-net` (`flaminio-node/src/net.rs:103`), `flaminio-meta`
     (`flaminio-metaclient/src/lib.rs:99`), `flaminio-control` and one per command
     (`control.rs:53`, `:60`), and the async runtime's own.
   - **"A small buffer pool."** There is no pool. Each queue allocates `--depth` × `--io-buf`
     at attach: 128 × 512 KiB = 64 MiB by default (`ublk.rs:1116`; `node.rs:134`). Its pages
     become resident as requests use them, and stay. With `--zero-copy`, a shared memfd mapping of
     the same
     size replaces those anonymous buffers (`ublk.rs:1097–1127`). Every write is copied into a new
     buffer (`ublk.rs:1263`).
     The store's device has two rings of 256 entries, a constant
     (`flaminio-uring/src/uring.rs:19`). The channels between the threads are unbounded
     (`net.rs:98`, `:139`, `:212`; `node.rs:288`).
   - **"Lazy map caching."** Opening the store reads every map page of every chunk replica on
     the disk (`flaminio-store/src/store.rs:555`, `load_chunk`) and keeps them decoded
     (`state.rs`, `MapPageState`). Nothing evicts one.
2. **The data plane's memory grows with what the disk holds, and no mode would change that.**
   - A map entry is 16 bytes of location and 16 checksums of 4 bytes: 80 bytes in memory per
     written 64 KiB cluster (`flaminio-format/src/meta.rs:450`, `:483`; format spec §5.5).
   - That is about 1.25 MiB per GiB written, 1.28 GiB per TiB. By this arithmetic the maps alone
     pass 512 MiB at about 400 GiB written. Each snapshot layer adds its own pages.
   - The bitmaps and chunk slots, which follow the disk's size, are a few MiB per TiB
     (`bitmap.rs:21`, `state.rs:174`).
   - None of it is measured. The lab's disks are 100 GiB (`scripts/lab.conf`): at most about
     125 MiB of maps. The lab can show the slope, not the crossing.
3. **Idle is not silent, and nobody measured it.**
   - **With a volume:** an engine turn every 10 ms on queue 0 (`ublk.rs:1178`); a heartbeat to
     each replica node every second and a metadata read every 2 s
     (`flaminio-repl/src/types.rs:54`).
   - **Without one:** the main loop wakes at least every 50 ms (`ublk.rs:786`).
   - **Always:** a liveness heartbeat every 3 s (`node.rs:331`).
   - **Beside it:** the CSI node plugin sweeps tombstones and copies labels every 5 s
     (`control/csi/tombstones.go:9`, `topology.go:48`); the failover controller looks every 5 s.
     Issue #12's pod adds a readiness probe that runs `flaminio-agent status` every 5 s.
   - **No busy-polling by default** (`--poll-us 0`). Polling and one queue per CPU are per-node
     settings that buy QD1 latency for a core
     ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md); performance plan, "Decided").
4. **Nothing is recorded or limited.**
   - No note in `docs/benchmarks/` gives a process's memory, threads or idle CPU. The
     performance plan measures throughput and latency only.
   - No manifest sets a request or a limit (`deploy/csi/driver.yaml`); issue #12 leaves "CPU
     pinning and resource requests" out. The unit runs without limits
     (`scripts/guest/node.sh:65`).
5. **The budget cannot be checked as written.** "About 1 core and 512 MiB RAM when idle"
   (§1.2) does not say:
   - **which processes.** A data node runs `flaminio-io`, the agent, and the CSI node plugin
     with two sidecars. In the 1-node and 2-node topologies it also runs etcd and
     `flaminio-meta`.
   - **what idle is:** no volume, or a volume attached with no I/O; before or after a load
     (the queue's buffers stay resident).
   - **what a core means when idle,** where the use should be near zero.
   - **at which disk size** (Why 2).
6. **The lab has the edge's shape, on x86_64 only.** fl-a, fl-b and fl-w are the 2 nodes +
   witness of §1.2 (`scripts/lab.conf`). They have 8, 4 and 2 vCPUs and 16, 8 and 4 GiB: well
   above the budget, so nothing shows when it is exceeded.

## What we want

1. The budget written so that it can be checked: the processes it covers, the idle states, how
   memory and CPU are counted, and the disk size and written amount it holds for.
2. One lab command that measures it, the same way each time, on fl-a, fl-b and fl-w.
   - **For each process Flaminio brings** (its own, the CSI sidecars, etcd): memory, threads,
     and CPU time and wake-ups over a fixed window.
   - **In each idle state:** no volume; a volume attached with a full ISR and no I/O; the same
     after a load.
   - **Wherever the data plane runs:** as issue #12's pod, or as the unit.
3. The data plane's memory against what its disk holds: measured at two written amounts or
   more, and compared with Why 2's arithmetic.
4. A results note in `docs/benchmarks/`: the numbers, and the budget's gaps. Each item that is
   over, or that nothing bounds, with its cause in the code. The mode is then sized from that
   list, not from §5.1's.
5. Nothing on the I/O path changes. No mode, flag, limit or default is added.

## Out of scope

- **The mode itself** (the rest of step 2): threads, buffers and pools sized by a mode; lazy
  map caching; requests and limits in the manifests. Each follows from outcome 4's list.
- **Performance under the budget** (the second half of "Done when"): the write and read
  campaigns with the data plane held to one core and 512 MiB. It needs the mode, or at least
  outcome 4, to say what to hold.
- **Step 1: the control plane in one Deployment.**
  - Today the CSI controller (with three sidecars) and the failover controller are two
    Deployments, both on the witness, with the same Secret (`deploy/csi/driver.yaml`).
  - Issue #43 puts the operator in a namespace of its own, so that pods of `flaminio-system`
    cannot reach the CA's key: "Compact mode (#55) must not undo this" (docs/issues/43, spec,
    question 3).
  - So "one Deployment" needs a decision first.
- **Step 3: arm64 builds and CI.**
  - Our crates have no architecture-specific code; CRC-32C picks ARM's `pmull` at run time
    (`flaminio-format/src/checksum.rs:6`). The CSI image is already built for arm64, for the
    demo's control plane (`scripts/lab.sh:1032`).
  - The data plane was never built or run on arm64. The CI runners and the lab VMs are x86_64;
    `make sanitize` names the target (`Makefile:135`).
  - Buffers are aligned to 4 KiB (`flaminio-uring/src/aligned.rs:10`); kernels with 16 KiB
    pages are unchecked.
- **Step 4: distributions.**
  - The lab is k3s on Ubuntu, where `ublk_drv` is not among the base kernel modules:
    provisioning installs `linux-modules-extra` (`scripts/guest/provision.sh:19`).
  - RKE2, Talos and MicroK8s were never tried (ADR 0003, "still to verify").
  - The NVMe/TCP fallback of §5.1 does not exist.
- **Kubernetes's own use** (k3s), and tuning etcd or the CSI sidecars: outcome 2 measures
  them, and they stay as they are.
- **Several volumes per node** (#15). The buffers and queue threads are per device: this
  change measures one volume.
- **The thread-per-core design of §5.1** (reactors, a registered buffer pool, privilege
  separation). The measurement is of the code as it is.
- **ARCHITECTURE's and ADR 0004's wording** ("from the start") stays. The results note says
  what exists.
