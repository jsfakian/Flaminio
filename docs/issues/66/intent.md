# Issue #66: NVMe-oF export and vhost-user-blk for VMs — intent

[Issue #66](https://github.com/jsfakian/Flaminio/issues/66), README roadmap phase 6;
ARCHITECTURE §1, §5.1, §20.3; ADR 0003. This change is the issue's step 1 only: the frontend
boundary, with ublk as its one frontend and a test frontend inside the process. No new
protocol. Steps 2–4 come later: issue #66 stays open after this change.

Built on #15 (several volumes per node) as planned on its branch: the engine thread, the
registry, the router. #17 (a target without a device) and #48 (ublk user recovery) change the
same code; neither is needed first.

## Why

ARCHITECTURE §5.1 lists four frontends. The code has one, and no place for a second.

1. **The node is the ublk module.**
   - On Linux, `node::main` hands the whole node to `ublk::serve_node`, attached or not
     (`flaminio-io/src/node.rs:411`). Elsewhere an attach is refused: "needs Linux (ublk)"
     (`node.rs:415`).
   - The node's loop, the attach and its limits, `Control`, `Slot` and `Attached` are in
     `ublk.rs` (682–897). The control socket imports them (`control.rs:26`).
   - An attached volume is `(volume, ublk id)` (`ublk.rs:682`). `status` builds `/dev/ublkb<id>`
     from it (`control.rs:105`), and `detach` stops the ublk device itself (`control.rs:202`).
   - #15 keeps this: its `Node`, registry and router are new in `ublk.rs`, and a registry entry
     is "the volume, the device's id" (its spec, Terms).
2. **While a device runs, the frontend owns the engine.** `Shared` holds the engine behind a
   lock, its input receiver and the store's file descriptor (`ublk.rs:506-538`). Queue threads
   call the engine directly: `handle`, `turn`, `flush`, `take_output` (582–600), the fast read
   (`try_fast_read`, `plan_fast_read`, `finish_fast_read`), the direct read (`plan_local_read`,
   `local_read_done`, `finish_plan`), and #10's settings (`holding`, `cluster_size`,
   `set_arrivals`, 978–989). They read the store's device themselves, at the offsets of a
   `ReadPlan`.
3. **What any frontend does with the engine is written against a `UblkQueue`:**
   - **Operations:** an id per request, `Input::Io`, and completions by id (`submit`, 657;
     `pump_with`, 602–633).
   - **A round's writes go together,** one group commit: `due`, `staging` and #10's arrivals
     (562–570, 1412–1446).
   - **A direct read:** plan, read from the store's device, verify, once more on a mismatch
     (#5), report; pieces from other replicas copied in (1238–1252, `local_read`, 1293–1341).
   - **A fast read:** admit, plan, read, verify, end; else the slow path (1629–1700).
   - **The end:** `Deposed` stops the device (619–630); a detach ends the target first.
   - **Errors:** `Invalid` is EINVAL, every other is EIO (1247–1249).
   - **Geometry:** 4 KiB blocks, one I/O at most 1 MiB (one frame), no volatile cache; flush
     and discard answered at once, as the protocol has no discard (946–965, 1277–1279).
4. **What is ublk's alone:**
   - **Threads:** libublk's `run_target` starts a thread per queue, and blocks until the device
     stops (1050).
   - **The ring:** each queue thread's ring carries the ublk commands, the reads on the store's
     device, the staging copies, the wake-ups and the timers (`serve_with`, 1071–1202).
   - **Buffers:** a buffer per tag (1114); or, with zero copy, the request's pages as the ring's
     fixed buffer `tag` (906–910, 1367), read into from the disk (`ReadFixed`, 1509), with a
     staging slot per tag (1102).
   - **Bounds:** queues × depth requests, a task per tag (1104–1127). The kernel bounds what is
     in flight, and so the memory.
   - **The device:** its id, `/dev/ublkbN`, `kill_dev`.
5. **The engine already serves a client with no device.** `Input::Io` in, `Output::Done` out;
   `Io::Read` returns the data (`flaminio-node/src/engine.rs:181-328`). Its tests attach, write
   and read that way (`tests/engine.rs`, `start_node`, 196; `Node::io`, 261). Targets are a map.
6. **The frontend's own logic has no test without a kernel.** `ublk.rs` has one test, of the
   arrivals counter (1748–1768). The engine tests "play the frontend" with a copy of their own:
   `serve_local_read`, `Frontend`, `drive` (`tests/engine.rs:477-543`), and a fast read by hand
   (690–713). The copy reads once; `local_read` reads again on a mismatch. That retry, the
   verification and the round's pump run only on a lab VM with a ublk device.
7. **The control plane names ublk in one place.** The CSI node plugin formats and mounts the
   path the data plane answers, whatever it is (`control/csi/dataplane.go:17`,
   `node.go:58-79`). The agent's claim policy refuses virtual devices by name, `ublk` among
   them (`control/agent/disks.go:82`).
8. **Which later frontend is needed is not settled.**
   - **The fallback** is the only one with a need on record, and it is unverified. ADR 0003
     decides a 6.6 kernel "with `ublk_drv` available", and lists as "still to verify" whether
     every distribution has it (also §20.3). No survey exists. The lab's Ubuntu 26.04 has the
     module only in `linux-modules-extra` (`scripts/guest/provision.sh:19-23`).
   - **External hosts:** the kernel's NVMe target (`nvmet`) exports any block device by path,
     and `/dev/ublkbN` is one. No data-plane code; not tried in the lab.
   - **KubeVirt:** a VM takes a raw block PVC as a virtio disk, which the CSI driver serves
     today (`node.go:188`). vhost-user-blk shortens that path; what it gains is not measured.

   So a second frontend in the data plane is certain only if the fallback is needed, or if a
   target of our own is chosen over the kernel's. Until then this step is worth its tests
   (item 6) and the module's split (item 1), which #17 and #48 also press on.

## What we want

1. **A frontend boundary:** what serving a volume to a client needs of the engine (item 3), in
   one place that names nothing of ublk. A registry entry is a volume and its frontend, not a
   volume and a ublk id.
2. **ublk as its first frontend, unchanged from outside:** the same devices, the same answers
   on the control socket, the same rules. Its threads, ring and buffers stay its own (item 4):
   local reads are still read on the queue thread, zero copy included.
3. **One volume's performance unchanged,** measured A/B against the build before, within #15's
   bounds (its spec, S12).
4. **A frontend inside the process, with no kernel device,** for tests only: it serves a volume
   through the same boundary, and by the same code as ublk for item 3. The engine tests play
   the frontend with it, not with a copy. No command of the control socket starts it.
5. **The rules every frontend keeps, stated and tested on that frontend:**
   - a write is answered only after the engine's `Done`; a flush may be answered at once only
     because of that;
   - a read is served only through the engine's admission, by the fast path or the queue;
   - a deposed or detached volume's frontend fails what is pending and serves nothing more;
   - what a client sends is checked before the engine sees it, and the requests in flight and
     their memory are bounded by the frontend.
6. **Done when:** the lab's checks on ublk pass as before (`make csi-test`, `make node-fault`,
   `make bench-reads` with zero copy), and the tests of item 5 run in `make test` with no ublk
   device.

## Decisions the spec will ask for

- **When.** Now, after #15, or only once a second frontend is scheduled (item 8).
  Recommended: after #15, kept small. The test frontend is a real second user of the boundary.
- **Shipped or test-only,** for the frontend inside the process. Recommended: test-only.
- **The survey ADR 0003 asks for:** `ublk_drv`, and the kernel's NVMe/TCP host, in each
  distribution we target. It decides step 4. Recommended: before any plan of steps 2–4.
- **The kernel's NVMe target or our own,** for external hosts. An ADR, with step 2.

## Out of scope

- **Step 2: NVMe/TCP for external hosts.** A listener that parses frames from hosts outside
  the trust domain: a fuzz target (none of the five in `dataplane/fuzz` covers it). Kernel
  initiators offer TLS by pre-shared key (from 6.7, above the baseline) and in-band
  authentication, both optional (to confirm in that step's plan): neither is mutual TLS with
  our identities (§15.2), so an ADR. Nothing records which host may use which volume: a
  capability token names a target node (`pkg/captoken`). Through `nvmet` instead: I/O passes
  the kernel twice, its keys are provisioned per node, and it gives no path to nodes without
  `ublk_drv`.
- **Step 3: vhost-user-blk.** A local socket and the guest's memory mapped into the data plane:
  `unsafe`, which the workspace forbids outside `flaminio-uring` (`dataplane/Cargo.toml:40`,
  §15.5), and new dependencies. CSI publishes a device or a mount, not a socket. The lab's VMs
  would run VMs: nested virtualization is not set up or checked (`scripts/host/prep.sh`).
- **Step 4: the NVMe/TCP loopback fallback.** Step 2's target on localhost, with the node's
  kernel as its host. Its device is an `nvme` disk by name: the claim policy would take a blank
  volume for a claimable disk (`disks.go:82`). It waits for the survey.
- **Failover of an export.** A pod's failover is the CSI publish, the agent's new epoch at
  stage and the failover controller's taint (`control/csi/controller.go:281`, `node.go:70`,
  `control/failover/failover.go`). An external host is not rescheduled: its exporting node is
  lost and the host stays. In their place: a record of the export in the metadata service,
  something that names the next node and takes the epoch there, and the host's change of path.
  Fencing is unchanged. A KubeVirt VM runs in a pod: with a PVC, the pod's failover applies.
- **#15, #17, #48:** the engine thread and registry, a target without a device, user recovery.
- **The engine's rules:** fast reads, fencing, epochs, leases, the replication protocol, the
  on-disk format. No new safety rule, so no simulator change.
- **What the engine lacks for any client:** discard, write-back mode, QoS (§7.3, §7.4).
- **The control socket's format, the CSI node plugin, the lab's scripts:** unchanged.
- **Privilege separation** (§5.1): the helper that opens the ublk control device.
