# Issue #56: Edge topologies: 1 node, 2 nodes and a witness, 3 nodes — intent

[Issue #56](https://github.com/jsfakian/Flaminio/issues/56), from the README roadmap (phase 4),
[ARCHITECTURE](../../ARCHITECTURE.md) §1.2 and ADRs
[0005](../../adr/0005-replica-confirmed-leases.md) and
[0006](../../adr/0006-edge-two-node-degraded-writes.md). This change is the first part of the
issue's step 2: what two nodes and a witness promise, written down, run in the simulator, and
checked on the lab, which has that shape. It installs nothing and changes no rule. Steps 1, 3
and 4 and the rest of step 2 come after: issue #56 stays open after this change.

## Why

1. **The lab is two nodes and a witness, and nothing says what that promises.**
   - fl-a and fl-b hold data; fl-w is "the third etcd member" (`scripts/lab.conf:48`). All three
     run an etcd member and `flaminio-meta` (`cmd_etcd`, `cmd_meta`, over `vms_of cluster`,
     `scripts/lib.sh:45`). Volumes are RF 2 with a minimum ISR of 1 (`cmd_vol`;
     `deploy/csi/driver.yaml:228`).
   - §1.2 gives the topology one table row, ADR 0006 one decision. The issue's "Done when"
     names four promises. No document says, fault by fault, what keeps serving, what waits and
     for what, and what can be lost.
2. **The lab checks cover part of the four** (`scripts/lab.sh`; results in
   `docs/phase1-plan.md`, steps 4e, 5, 7 and 8).

   | Promise | Run today | Never run |
   |---|---|---|
   | Losing either data node keeps volumes writable | `make node-fault`: fl-b's `flaminio-node` unit stopped for 20 s under fio on fl-a; no write fails, every ISR shrinks to fl-a. `make csi-failover`: the pod's VM powered off; the pod is Ready on the other node, its 256 MiB blob intact, at 126 s | A VM lost under a write load (data plane, etcd member, metadata instance and kubelet at once). fl-a lost as the replica. A write on the survivor after the failover: the pod only verifies (`deploy/lab/csi-test.yaml`) |
   | Its return resyncs | Both, and `make partition-test`, wait for full ISRs | What it costs: issue #44 |
   | Losing the witness does not touch I/O | `make chaos`, one fault kind of four: fl-w's etcd member and metadata service stopped for 20 s (`chaos_fault`) | The VM lost: the Kubernetes server and both controllers stay up. The fault is announced, not confirmed (docs/issues/49, intent, Why 5) |
   | No partition gives two writers | `make partition-test`: fl-a, the target, cut off the whole overlay, the volume attached to fl-b by hand from fl-w. 0 reads after the lease, 0 writes acknowledged after the cut, fl-b holds the last acknowledged write | fl-a and fl-b cut from each other with fl-w reachable from both. A data node cut from the witness only. `scripts/guest/partition.sh:13` can only drop all of `wg0` |

   fio's verify, which `node-fault` and `chaos` rest on, cannot see a lost write
   (docs/issues/49, intent, Why 2). The probes of `partition-test` can: one block each.
3. **The cut nobody ran is the one this topology is about:** each data node may think the
   other dead, and the witness answers both. By the trace:
   - **The target keeps writing, on one copy.** It removes the other node from every ISR
     through the quorum it still reaches (`member_failed`, `flaminio-repl/src/target.rs:1300`;
     `CheckIsrChange`, `meta/domain/domain.go:238`).
   - **Nothing fails over.** Both nodes still heartbeat and both kubelets answer; the failover
     controller marks a node only when it is not Ready and not alive
     (`control/failover/failover.go:83`).
   - **The other node cannot serve.** It is outside every ISR. A target there must fence every
     ISR member, and cannot remove the last one (protocol §6, step 2).
   - **An attach there stops the volume everywhere,** until the link heals: the new epoch
     deposes the serving target at its next poll (`on_meta_inner`, `target.rs`; 2 s), and the
     new one waits. A pod moved by hand, or a `kubectl drain`, does that. Nothing refuses it,
     and nothing says why the volume does not serve.

   So at most one writer, and in the last case none. No test and no document states either.
4. **The simulator never runs two nodes.** `Config::random` draws 3–4 nodes and RF 2–3
   (`flaminio-repl/src/sim/mod.rs:130`). The split-brain scenario fails over to a third node
   that holds no replica (`sim/tests.rs:113`). With two nodes every target sits on a replica,
   and the only other node is the only place to go.
   - **Tried on a copy outside the tree** (2026-11-12; 2 nodes, RF 2, minimum ISR 1, the rest
     as `Config::random`): seeds 0–19 999 pass, with 82 048 failovers, 43 632 crashes and
     143 304 partitions. Minimum ISR 2 passes the same seeds, as do 1 node at RF 1 and 3 nodes
     at RF 3.
   - **12 of the 14 injected bugs that need no move are caught** within 20 000 seeds. Reads
     from a member being removed, and fast reads without the fencing check, are not, in seeds
     0–319 999. Whether two nodes can show them at all is not settled. The move checks do not
     apply: there is no node to move to (`request_move`).
   - None of this is in the tree: no test would notice if it stopped holding.
5. **Writes on one copy are silent.** ADR 0006 accepts them on the condition that the risk is
   "visible and bounded": a `Degraded` condition with reason `SingleCopyWrites`, a metric and
   an alert; an extra snapshot and backup cycle; resync first. None exists. There is no status
   resource (#43), no metrics endpoint in `dataplane/`, `meta/` or `control/`, no snapshot or
   backup (#51, #52, #58). The one signal is the ISR in `flaminio-meta get` (`vol_degraded`).
6. **The lab's witness is more than §1.2's.** §1.2: "a tiny voting member that stores no volume
   data".
   - **fl-w is also Kubernetes's only server,** and runs the CSI controller and the failover
     controller (`scripts/guest/k3s.sh:30`; `deploy/csi/driver.yaml:32`, `:192`). Losing it
     stops provisioning, attach and failover; the metadata quorum holds on fl-a and fl-b.
   - **It shares a server with fl-a** (`lab.conf:50`): the lab shows quorum behaviour, not the
     loss of a server.
   - **To the metadata service it is a label on a node record,** `flaminio.io/witness`. Only
     `make meta-smoke` registers it, by hand (`lab.sh:606`): it runs no data plane. Creation
     and the drain skip labelled nodes (`meta/service/service.go:399`;
     `control/drain/drain.go:76`); `MoveReplica` does not (docs/issues/18, intent).
7. **A volume of more than 64 chunks may not survive the loss** (static trace; docs/issues/44,
   intent, Why 7). The target proposes every removal in one request (`send_meta`); the service
   refuses more than 64 changes (`MaxChangesPerRequest`). vol1 has exactly 64.
8. **The other topologies are sentences.** Traced for the later steps:
   - **The scripts build three members.** `vms_of cluster` is fl-a, fl-b and fl-w; the etcd
     and metadata endpoints come from it (`lab.sh:467`, `:671`). `k8s up` waits for 3 Ready
     nodes (`lab.sh:936`).
   - **The manifests need a witness.** Both controllers select `flaminio.io/witness`: with
     one node or three, nothing carries it and they never start. Issue #43 puts the operator
     there too.
   - **The driver's defaults are this topology's, everywhere:** RF 2, minimum ISR 1
     (`parseParams`, `control/csi/controller.go:137`). §8.2 and ADR 0006 say 3 and 2 except
     here. On one node the default cannot be placed (`Place`, `domain.go:360`). The parameter is
     `minIsr`; `minInSyncReplicas`, as the documents and the README write it, is refused.
   - **The rest assumes no count:** `Spec.Validate`, `Place`, `CheckIsrChange`. RF 1 runs on
     the lab (`make bench-writes`), with three metadata members.
   - **A remote witness is untried.** etcd runs with its default timing
     (`scripts/guest/etcd.sh:45`: 100 ms heartbeats, 1 s election), and nothing keeps its
     leader off the witness. The lab's delay fault never hits fl-w (`chaos_fault`).

**Why this step first.** The lab already has this shape: it needs no new VM and no installer.
The contract and its check are what steps 1 and 3 repeat for their shapes, and what a profile
(step 4) must install. The operator that would install a profile (#43) is not built.

## What we want

1. **The contract, in one place.** For two data nodes and a witness, as the lab builds them:
   what keeps serving, what waits and for what, and what can be lost, for each single fault (a
   data node lost, as the target's or as the replica's; the witness lost; each of the three
   links cut; a node cut off from both others) and for the double faults. It includes ADR
   0006's accepted risk, and the case of Why 3 where a volume serves nowhere.
2. **The simulator runs that shape,** in `make test` and `make sim`: a campaign of two nodes at
   RF 2, with a minimum ISR of 1 and of 2, and scripted scenarios of its partitions (the two
   nodes cut with the metadata reachable from both, with and without an attach on the other
   side; the target cut off and the other replica taking over). Every injected bug that two
   nodes can show is caught there; one that they cannot is named, with the reason.
3. **One lab command checks the contract's single faults,** on the lab as it is: each data VM
   lost in turn and back, the witness VM lost and back, each link cut and healed. It needs a
   cut between two VMs, which the lab cannot make today.
4. **Its verdict can see what it claims.** The workload's check tells a lost acknowledged write
   and a stale read from a good one, and writes on the survivor after each loss. Each fault is
   confirmed in effect and healed on observed normal exits. After management loss, a persisted
   inventory supports independently repeatable healing; unconfirmed cleanup fails the run.
5. **The time on one copy is reported,** from the ISRs the metadata service holds: when the
   first chunk was down to one member, and when every ISR was full again.
6. **`docs/lab.md` describes the check.** The result is recorded once the run is made, by hand.
7. **Nothing else changes:** no rule of the protocol, no default, nothing on the I/O path. A
   seed or a fault that breaks the contract is a bug found, to be reported and not worked
   around.

## Out of scope

- **Step 1, one node:** RF 1, one metadata member, no failover. It needs a lab of one VM with
  a single etcd member, the simulator's one-node shape, and a decision on restoring a lost
  member (§11.5). Its protection is backups and asynchronous replication: #52, #58, #59.
- **Step 3, three nodes:** RF 2 or 3, minimum ISR 2 by default, no witness. fl-c is a third
  data node, but outside Kubernetes and not an etcd member (`lab.conf:59`).
- **Step 4, a profile for each:** one setting that fixes the members, where the controllers
  run and the StorageClass. It waits for the operator (#43). The driver's defaults and the
  parameter's name (Why 8) go with it.
- **The rest of step 2:**
  - **ADR 0006's visibility:** the condition, the metric and the alert; the extra snapshot and
    backup cycle (#51, #52, #58); resync first. The issue cannot close without them.
  - **A witness across the WAN:** etcd's timing, where its leader sits, a slow or lossy witness
    as a fault.
  - **A witness that is only a metadata member:** where Kubernetes's server and the
    controllers run when the witness is a board or remote; arm64 (#55, step 3).
  - **Losing a server:** fl-w would have to leave rhythm.
- **Refusing an attach that cannot serve** (Why 3), and the disruption guard of §10.4. This
  change states and tests what happens.
- **Serving from a stale copy** when the node with the only in-sync copy is gone for good: an
  operator's decision to lose data. No command does it.
- **`MoveReplica` to a witness named by hand:** its own issue (docs/issues/18, intent).
- **`ChangeIsr` for more than 64 chunks** (Why 7): its own issue. The check runs at 64.
- **What a returning node's resync costs** (#44), **the resource budget** (#55) and **the
  gaps of `make chaos`** (#49): theirs. This check only waits for full ISRs, and uses #49's
  workload if it exists by then.
- **ARCHITECTURE §1.2 and the ADRs** keep describing what is wanted.
