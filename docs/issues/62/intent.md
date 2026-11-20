# Issue #62: Zone-cost-aware placement — intent

[Issue #62](https://github.com/jsfakian/Flaminio/issues/62), from the README roadmap (phase 5)
and ARCHITECTURE §1.1, §7.2, §9, §11.4 and §20.5. This change is the issue's step 3, taken
first: a volume's target counts what it exchanges with each node, and reports it by zone. It
moves no replica and redirects no read. Steps 1, 2 and 4 follow, judged by this step's numbers:
issue #62 stays open. Assumes #18 as planned.

## Why

In a cloud, bytes between zones are billed. Nothing in Flaminio knows how many it sends, so no
placement or read choice can be judged yet.

1. **Every byte of a volume on the network has its target at one end.** The pod reaches the
   volume through `/dev/ublkbN` on its own node, where the target runs (`ControllerPublishVolume`,
   `control/csi/controller.go:281`; `control/csi/node.go:70`): no network. The local replica is
   served in process; every other node over one session (`send_from_target`,
   `flaminio-node/src/engine.rs:1915`, `:2066`). Resync and move copies are read from an ISR
   member and sent to the joiner by the target (protocol §7; `flaminio-repl/src/target.rs:1819`,
   `:1591`). No replica talks to another. So the target's zone is the consumer's, and a
   volume's cross-zone bytes are those between its target and nodes of other zones.
2. **The flows, and the least each can cross.** With the target on node T:

   | Flow | On the network today | Crosses zones when | Least possible |
   |---|---|---|---|
   | Write | one copy to each ISR member and joiner but T's own (`start_write`, `target.rs:1378`) | that node is outside T's zone | `zone`: replicas − 1, with a replica in T's zone. `node`, `rack`: 0, with all of them there |
   | Read | none from T's usable replica; else one copy from the source | the source is outside T's zone | 0 with an ISR member in T's zone, else 1 |
   | Resync, move, rebuild (#45) | source to T, unless T is the source; T to joiner, unless T is the joiner | each hop on its own: up to twice per byte | a direct copy's: 0 with a source in the joiner's zone, else 1 |
   | Reconciliation, heartbeats (1 s per node), probes | small | the node is outside T's zone | not data |

   Under `zone` every write crosses zones replicas − 1 times at least. That is the
   failure-domain rule, which #18 keeps through moves and which no cost term may override. What
   can change is the rest: where the other replicas of a `node` or `rack` volume go, which
   member a target without a local replica reads from, and the second crossing of a copy.
3. **`Place` has no cost term, and no score to put one in.** It orders the candidates by: the
   preferred node, the fewest chunk replicas held, the lowest id; and skips a domain already
   taken at the volume's level (`placeChunk`, `meta/domain/domain.go:395`; the load,
   `meta/service/service.go:384`). A zone is read only as the domain at level `zone`
   (`domain.go:322`). Example: `node`, two replicas, the consumer on a1 (zone z1), a2 (z1)
   holding more replicas than b1 (z2). The second replica goes to b1, and every write crosses
   zones, though a2 meets the rule at no cost. §9's other scores (latency, failure history) and
   constraints (device class, disk state, watermark) do not exist.
4. **The consumer's node counts once, at creation.** The CSI controller passes the pod's node
   (`preferredNode`, `controller.go:56`, `:233`), and every chunk gets its first replica there.
   It is dropped without notice when that node is not a candidate (`domain.go:367`), and absent
   without topology (`flaminio-meta create` without it). Afterwards a volume is reachable from
   every node (`controller.go:253`): a rescheduled pod may land in any zone, and no replica
   follows it. `dataLocality` and §9's locality migration do not exist (`parseParams` refuses
   the parameter, `controller.go:157`; only the drain and `flaminio-meta move` ask for moves).
   That target then reads every block remotely and sends every write to all replicas.
5. **Reads ignore zones.** `source` (`target.rs:1644`) returns this node if its replica is
   usable, else the usable member with the lowest id; ids number the nodes in the order the
   chunk records first name them (`start_target`, `engine.rs:1623`). The same choice serves
   resync copies, digests and repairs (`target.rs:1801`, `:1819`, `:1852`). #47's first step
   adds a preference by latency there: a same-zone preference is the same seam.
6. **The target's node is handed every node's zone, and drops it.** At attach the engine lists
   the nodes (`engine.rs:676`). The answer carries their labels
   (`flaminio-metaclient/src/pb.rs:11`), and `start_target` keeps the addresses only
   (`engine.rs:1645`; the later refresh too, `:1580`). Chunk records and the attach answer name
   nodes by id (`meta.proto:104`, `:207`). A node does not know its own zone: it registers an
   id and an address (`engine.rs:585`), and the CSI node plugin adds the labels later, from the
   Kubernetes Node (`control/csi/topology.go:35`). Placement puts all nodes without a zone
   label in one empty zone (`domain.go:320`); for cost, such a node's zone is unknown.
7. **Nothing counts a byte.** The target's `Stats` exist in tests only (`target.rs:107`), the
   engine counts local commits (`engine.rs:153`), the control socket's `status` answers the
   attached volume and its device (`flaminio-io/src/control.rs:104`), and no metric is
   exported (§18). Yet every request passes the target's `send` with its purpose (`ReqKind`,
   `ReadFor`; `target.rs:1182`, `:321`, `:296`) and then `send_from_target`, which knows the
   node: between them, each byte is seen with both.
8. **The target also polls the metadata service** (found in this trace, not measured): it reads
   its whole volume, every chunk record, every 2 s (`refresh`, `flaminio-repl/src/types.rs:63`;
   `GetVolume`, `service.go:501`), from the first endpoint that answers, in whatever zone. The
   answer grows with the volume, up to 2^20 chunks (`domain.go:184`).
9. **The lab can count, not bill.** fl-a is in zone lab-a, fl-b in lab-b, fl-c in none
   (`scripts/lab.sh:609`), on one network. volb's second replicas were on fl-a and on fl-c, as
   `Place` orders them by load
   ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md)). No result in
   `docs/benchmarks/` records bytes between nodes.
10. **§20.5 waits for the same figure:** what two replicas across zones cost per GiB written,
    against one replica on a provider disk.

## What we want

1. **The target counts each attached volume's bytes, per node and per flow:** sent and
   received, for client writes, client reads, copies (resync and moves, both hops) and the rest.
   It also counts the bytes the client itself read and wrote, so that the result is cross-zone
   bytes per GiB read and per GiB written.
2. **Each node's bytes are reported as same zone, another zone or zone unknown,** from the zone
   labels the metadata service holds for the target's node and for that node. A missing label is
   unknown, never guessed. A label set after the attach shows in a later report.
3. **An operator can read it from the node on request,** per volume, since the attach.
4. **Counting changes nothing else.** No read source, placement, ISR or timeout depends on it.
   The I/O path gains the additions only: no lock, no metadata round trip, no request.
5. **The floor is written down:** for each level and replica count, the fewest cross-zone bytes
   per byte written, read and copied (Why 2), where the report can be held against it.
6. **A lab check, and the baseline.** Known loads on a `zone` volume and on a `node` volume,
   with the target on a replica's node and on a node without one, and one drain. The counted
   bytes match what the load wrote and read, within the framing. The figures go in
   `docs/benchmarks/`, beside the floor: what steps 1 and 2 must then reduce.
7. **Docs:** `docs/lab.md`: how to read the counts, and what the lab's zones can show.

## Out of scope

- **Step 1: a cost term in placement,** at creation and in the drain's choice (#18), below the
  failure-domain rule. Where it ranks against load is a decision: above, the consumer's zone
  fills first; below, it only breaks ties.
- **Step 2: reads from the target's zone.** A preference among usable ISR members in `source`,
  beside #47's by latency; it needs this step's zones on the target.
- **Step 2: copy sources in the destination's zone.** Copies pass through the target, so only
  the source of a target without a local replica can be chosen today. A copier on the
  destination is #45's step 3.
- **Step 4: defaults on provider disks** (§20.5). An open question of the architecture.
- **A replica that follows the consumer:** `dataLocality`, locality migration, and telling the
  scheduler where a volume's replicas are (Why 4).
- **Metadata traffic** (Why 8), and etcd's own replication. Not counted here; the poll's size
  needs measuring, and its own issue if it is large.
- **A bill.** The counts are of what the node's engine sends and receives. TLS, TCP and a
  provider's prices are not in them.
- **Totals across attachments, and metrics export** (§18). The counts live with the target.
- **Zones for nodes without a label,** and labelling fl-c for good.
- **ARCHITECTURE §7.2, §9 and §11.4** keep describing what is wanted.
- **The MVP volume path** (`flaminio-io/src/volume.rs`, `remote.rs`).
