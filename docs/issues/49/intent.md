# Issue #49: Chaos test suite — intent

[Issue #49](https://github.com/jsfakian/Flaminio/issues/49), from the README roadmap (phase 2)
and [ARCHITECTURE](../../ARCHITECTURE.md) §19. This change is the issue's step 3, for the faults
`make chaos` injects today: a check of block read and write histories, and the harness rules
that make its result mean something. Steps 1, 2, 4, 5 and 6 build on it: issue #49 stays open
after this change.

## Why

§19 says what is wanted. What exists, traced in the scripts and tests:

| §19 item | Exists | Missing |
|---|---|---|
| Linearizability of block histories | The simulator's checker, on a model (`flaminio-repl/src/sim/check.rs`, protocol §9). In the lab: fio's verify (below), and `make partition-test`'s two rules on one block | A history of the real device, checked |
| Cluster faults | `make chaos`: four kinds, and a kill (below). Apart: `partition-test` (the target cut off), `csi-failover` (a VM powered off), `meta-smoke`, `drain-test` | Packet loss, zone loss, API-server outage, faults on the target under load, faults together |
| Disk faults | Nothing. A device error poisons the store until it is reopened (`flaminio-store/src/error.rs`, `store.rs`), and the replica closes the session (`replica/mod.rs`) | `dm-flakey`, `dm-dust`, `dm-delay`, hot-unplug; what must follow them is issues #45–#47's |
| `dm-log-writes` replay | A property test on a model device (`MemDevice`, `tests/crash_consistency.rs`; `make crash-test`, nightly) | Replay of what the kernel wrote, through `flaminio-uring` |
| Upgrades under I/O | Nothing | Waits for issue #48 |
| Scheduled runs | The Lab workflow, by hand only (`.github/workflows/lab.yml`); Nightly runs no lab check | A schedule, a lab-wide lock, recorded results |

`make chaos` (`cmd_chaos`, `chaos_fault`, `scripts/lab.sh`; `scripts/guest/chaos.sh`,
`write-load.sh`) runs `ROUNDS` rounds on vol1 (RF 2, `minInSyncReplicas` 1), the target
alternating between fl-a and fl-b. Each round: fio writes 4 GiB of random 64 KiB blocks on the
target and reads them back, while one fault after another hits the rest; once every ISR is full
the target's node is killed, the volume is attached to the other node, and fio reads the 4 GiB
there.

1. **The faults are four,** drawn with `$RANDOM % 4`, one at a time, 5 s apart: the replica's
   node restarted (down 10 s); the replica cut off the overlay for 15 s; fl-w's etcd member and
   metadata service down for 20 s; 100 ms on every packet of the replica's for 20 s. None hits
   the target, and none hits fl-c.
2. **"Verified" is fio's `--verify=crc32c`:** no I/O error during the load, and every 64 KiB
   block holds a block of such a job for that offset, intact. It does not say which write a block
   holds. Every round, and `make node-fault` and `make drain-test`, run the same job with
   `--randseed=7410`: the same offsets, the same data.
   - **Measured** (fio 3.28, a 64 MiB file on a workstation, 2026-11-12; to repeat with the VMs'
     fio): two runs of the job leave images that differ in 8 bytes of each block's header. A
     block put back from the first run passes `--verify_only` after the second. So does a block
     left by a run with another seed. A zeroed block fails.
   - **So a lost acknowledged write passes** from the second round on, and in the first on a
     volume an earlier run wrote. The README's "loses no byte" rests on a check that sees
     corruption, not loss. A per-round seed would not fix it.
3. **No read runs while a write is in flight.** fio reads after its writes; the new target is
   read after the failover. A stale read during a fault is never seen.
4. **The kill hits an idle volume.** The load has ended and every ISR is full before it
   (`cmd_chaos`, `wait_isr_full` then `kill-node`). Reconciliation of in-flight writes after a
   target crash, the rules most of the simulator's injected bugs break (protocol §6, §9), never
   runs in `make chaos`. Only `partition-test` has a writer in flight: one block, one at a time.
5. **A fault that did not happen is counted.** `chaos_fault` runs inside
   `log "  fault: $(chaos_fault …)"`: bash clears `set -e` there and drops its status, and the
   restarts' output goes to `/dev/null`. `chaos.sh kill-node` ends in `|| true`. The 21 faults of
   the step 8 result (`docs/phase1-plan.md`) were announced, not confirmed.
6. **A run cannot be repeated,** as `$RANDOM` is not seeded and no log says which fault ran when.
7. **An interrupted run heals two kinds of four.** The `EXIT` trap removes the partition and the
   delay on fl-a and fl-b. A replica's node stopped by a restart, or fl-w's etcd member and
   metadata service, stay down; the fio unit keeps writing.
8. **The suite's rule has no lab form yet.** The simulator's injected bugs are `#[cfg(test)]`
   switches of `flaminio-repl` (`replica.rs`, `target.rs`): no data-plane build has them. The lab is
   shared and has no lock (`docs/issues/14/decisions.md`), so a broken build deployed there can
   meet the next benchmark or demo.

**Why this step first.** Every fault kind of steps 1 and 2 needs a check that tells a lost write
or a stale read from a good one, and today's cannot (2–4). The simulator checks the protocol's
state machines; a history of the real device under the faults that already exist checks what it
cannot reach: the engine, the store, `io_uring`, the TLS transport, ublk and the kernel. It
costs the lab nothing new: the same run by hand, no disk reformatted, no device-mapper table.
The proof that the check fails needs no lab at all.

## What we want

1. A workload that records a history of the device: concurrent single-block reads and writes of
   values unique in the run, each recorded before it is issued and when it is answered, on its
   node's clock. It survives its client being killed or hung. A run's history is one such record
   per attachment, on that attachment's node; the harness orders them, and no clocks are compared.
2. A check of that history: every block linearizable, by the simulator's rules (protocol §9): a
   write without an answer may take effect or not, but not after a later attachment's first
   operation on that block completed. Content that is not one whole value written to that block
   in this run fails. It runs again on a kept history, anywhere.
3. `make chaos` runs the workload through every round's faults, through the kill, now under load,
   and through the failover, and fails on a violation, naming the block, the operations and the
   faults around them. The fio load stays, for what it does check.
4. Faults that count: each confirmed to have taken effect and to be healed, logged with its
   times, drawn from a seed the run prints and accepts. A run that stops for any reason leaves
   no fault in place and no client running.
5. The check shown to fail, without the lab: for each kind of violation, a history and a device
   that lies in that way fail it, and the same without the lie passes; in CI. What stands for
   "a build with an injected bug" in a lab check is a choice for the spec's open questions.
6. `docs/lab.md`, `docs/ci.md` and the README say what the run checks. The lab run, by hand, comes
   later, with its result recorded.

## Out of scope

- **Step 1, disk faults:** `dm-flakey`, `dm-dust`, `dm-delay`, hot-unplug under a data node's
  store. What must happen after them is issues #45, #46 and #47's.
- **Step 2, more cluster faults:** packet loss, zone loss, API-server outage, faults on the
  target, faults together. fl-a, fl-c and fl-w share a server (`scripts/lab.conf`): losing it
  loses the etcd quorum.
- **Two clients at once on two nodes,** the cut-off target against the new one. It needs one
  clock across VMs; `make partition-test` keeps its own checks until step 2.
- **Step 4, `dm-log-writes` replay** at every write boundary. Whether the guest kernel has the
  module is to be checked.
- **Step 5, upgrades under I/O:** waits for issue #48.
- **Step 6, a scheduled run** with recorded results. It needs a lab-wide lock first: the Lab
  workflow's `concurrency` group orders workflow runs only, not runs by hand.
- **Comparing the replicas** at a run's end (the simulator's convergence check).
  `scripts/guest/digest.sh` needs the nodes stopped, and checks the MVP's units, not
  `flaminio-node`.
- **`make node-fault` and `make drain-test`** keep the fio job, and its blind spot (Why, 2).
- **A build with an injected bug on the lab,** or the simulator's switches in the engine: the
  spec's open questions.
- **The campaigns the audit follow-up asks of #49** (the issue's comment of 2026-11-12), later
  steps of #49, each once its issue lands: #76 (an interrupted volume creation), #77 (a volume's
  metadata read across its replacement), #78 (placement after a drain is marked), #79 (CSI mount
  retries), #80 (the one volume geometry), #81 (bounded ISR updates), #82 (a live partition
  against a power-off on a mixed-storage node, with no recovery mutation: #82's design, Lab),
  #83 (on a qualified disposable host only: #83's design, Lab) and #84 (the block durability
  contract).
