# Issue #58: Asynchronous cross-cluster replication — intent

[Issue #58](https://github.com/jsfakian/Flaminio/issues/58), README roadmap phase 4,
ARCHITECTURE §14.2, §17.2 and §1.1. This change is the smallest form of the issue's steps 1
and 2, with the promotion of step 3: two clusters paired by an explicit exchange of trust, a
standby volume, a snapshot shipped at an administrator's request, and promotion. Periodic
shipping, failback, consistency groups and `ReplicationRelationship` build on it: issue #58
stays open after this change. It sits on #51's first step (snapshot generations, the cut,
snapshot reads per replica), planned and not built: assumed done as planned.

## Why

ARCHITECTURE §14.2 says what is wanted. The code has none of it: no peer cluster, relationship
or standby in `meta.proto`, `meta/domain`, `flaminio-meta` or the data plane.

1. **Every check assumes one cluster.**
   - A process loads one CA bundle and verifies every peer against all of it (`Files.load`,
     `pkg/pki/pki.go:267`; `Files::load`, `flaminio-net/src/tls.rs:50`).
   - The peer's SPIFFE ID must then be in the process's one trust domain (`PeerIdentity`,
     `pki.go:256`; `peer_identity`, `tls.rs:102`).
   - The metadata service's caller rules know administrators and a node's components; a method
     without a rule is refused (`Authorize`, `meta/service/auth.go:63`).
   - A replica accepts a session only from a `node/<n>/io` peer whose `Hello` token verifies
     under the cluster's one token key and names that node, the volume and the epoch (`accept`,
     `flaminio-net/src/session.rs:103`). Only `Attach` signs tokens (`service.go:743`).

   A second cluster has another CA, trust domain and token key: its peers are refused
   everywhere, as they should be.
2. **Adding the peer's CA to the bundle would hand the cluster over.** Nothing ties a CA to a
   trust domain. Any CA of the bundle can sign `spiffe://<this cluster>/flaminio/admin/x`, which
   passes both checks and may create, delete and attach every volume. The peer's own identities
   would still be refused. So "an exchange of CA bundles" has to mean a bundle per trust
   domain, used where the peer is expected and nowhere else.
3. **A replica takes data from its volume's target alone:** a session of an epoch at or above
   its fence (`ReplicaCore::handle`, `flaminio-repl/src/replica.rs:170`), one writer per epoch.
   Data shipped from another cluster has to enter under that rule or beside it:
   - **Through the standby cluster's own target.** The peer reaches one receiving endpoint.
     Replicas keep their rule; ISR, resync and reconciliation apply unchanged. The target has no
     client: a target without a device is #17's (on #15), both planned and not built; a ublk
     device that nobody mounts works today.
   - **Straight into the replicas.** The writer needs a token, so it is the volume's target: a
     node of another cluster admitted by every replica and by `ChangeIsr`, with the WAN inside
     the lease and failover timing. Or it is a second writer beside the protocol, which nothing
     fences or reconciles.
4. **Nothing can refuse an attach.** `Attach` takes any volume, for any administrator
   (`service.go:696`); the CSI controller is one (`control/csi/controller.go:281`). A header
   has two flags, `Creating` and `Deleting` (`meta/domain/domain.go:37`). What exists: the
   header's write is conditional on its revision (`service.go:725`), and every attach raises
   the epoch, which deposes the writer before it (protocol §2, §3).
5. **A diff does not land at once.** Writes go to a chunk's top layer only (format §6.3), one
   chunk and one commit at a time, on each replica. A standby in the middle of a diff holds
   neither snapshot. No journal record removes a layer (`LayerMerge` is reserved). So an
   unfinished diff must be undone from the last snapshot (rewritten from #51's reads at a
   generation), or kept aside until it is whole and then applied without the peer, or dropped
   as a layer (a format change).
6. **What #51's first step gives, and what it leaves.**
   - It gives snapshot records, the cut by the current target, every in-sync replica holding
     every snapshot identically, per-replica reads at a generation, and "the layers from
     generation *a* up to *b* are what changed between two snapshots".
   - No snapshot can be deleted. Every ship then keeps a snapshot on each side for good, under
     a cap below 64 layers per chunk (`MAX_LAYERS`): a pair can ship a few dozen times. One
     ship a minute needs #51's step 3.
   - A snapshot of an unattached volume waits for its next attach; a snapshot is not readable
     as a volume.
7. **Only the data plane reads and writes chunk data.** Go has no code for the replication
   frames, and the halves meet only at the contracts in `api/` (ADR 0001). The frames carry no
   protocol version: a cluster's nodes are upgraded together. Two clusters are not.
8. **The peer is sometimes unreachable** (§1.1, ADR 0004): an edge link can be gone for days
   (#59).
9. **The lab is one cluster:** fl-a, fl-b and the witness fl-w, one trust domain
   (`flaminio-lab`), one token key (`docs/lab.md`). fl-c is its third data node, outside
   Kubernetes and not an etcd member. The guest scripts assume one node per VM
   (`scripts/guest/node.sh`: one unit, ublk id 1, `/etc/flaminio/pki`).

## What we want

1. **Two clusters pair by an explicit exchange,** made by an administrator on each side: each
   records the other's trust domain and CA bundle. A certificate counts as the peer's only when
   its chain ends in the peer's bundle and its identity is in the peer's trust domain. The
   peer's bundle verifies nothing else, and a peer that claims this cluster's trust domain is
   refused. Each side proves itself to the other. Removing the pairing ends the peer's access.
2. **A peer may do one thing:** write the data of the standby volumes whose relationship names
   it, and learn how far its own ship got. No metadata RPC, no replica session, no capability
   token, no other volume, and it reads no data back.
3. **A standby volume,** created by an administrator with its source (the peer and its
   volume): the source's size, its own replication factor and placement. No attach succeeds on
   it until it is promoted, whoever asks.
4. **Shipping on request.** An administrator of the source's cluster ships a snapshot of an
   attached volume: the first time all of it, afterwards what changed since the last snapshot
   shipped. Whole snapshots only, in the order they were cut.
5. **The invariant: a promoted standby holds exactly one shipped snapshot,** the last one that
   completed on it, and nothing of a later ship. That holds through a crash or a failover on
   either side, a lost link, and a promotion at any moment of a ship.
6. **A standby's replicas stay under their own cluster's rules:** in sync as any volume's,
   resynced after an absence. A completed ship is a snapshot on the standby too, and each side
   records which of its snapshots is which of the other's: the last common snapshot, which
   failback will start from.
7. **A ship resumes** after a lost link or a restart on either side, without sending again
   what the standby already holds durably. While the peer is unreachable, the source's I/O and
   snapshots go on: nothing on the source waits for the peer.
8. **Promotion** is one command of an administrator of the standby's cluster, and needs no
   peer. It ends the relationship and fences whatever was applying, by the epoch and not by a
   timeout. The volume then attaches as any other. A ship that arrives later is refused.
9. **The two clusters state their protocol version first,** and refuse one they do not know.
10. **The simulator ships and promotes during chaos** and checks rules 5 and 8. Each new rule
    has an injected bug that it catches (protocol §9).
11. **A lab check between two clusters:** a volume under write load is shipped several times,
    with the link cut during one ship; the standby is promoted during another; the promoted
    volume holds exactly the last completed snapshot, on every replica. Refused, each checked:
    an attach of the standby, the peer's certificate at the metadata service and at a replica,
    a ship into a volume that is not its standby. The time a ship of a known size takes is
    recorded, not judged.
12. **The DR lab cluster is one node on fl-c,** beside the first cluster's node there: its
    own trust domain, CAs and token key, one etcd member and one `flaminio-meta`, a disjoint
    data plane with its own fresh feature-capable disk, ports, identities and control socket,
    preserving #54's independently planned restore destination. `docs/lab.md` describes
    it. ARCHITECTURE §14.2 and §15, and the protocol, describe the rules as built.

## Out of scope

- **Periodic shipping and the measured RPO** (the rest of step 1). They need #51's step 3 on
  both sides, to drop every snapshot but the last common one, and a policy for a peer that
  stays away (#59).
- **Shipping through a shared bucket.** It is #52's repository format, which needs an ADR.
- **Failback** (the rest of step 3). This step records the common snapshot. Reversing a
  relationship, and discarding what the old source wrote after that snapshot, come later.
- **Consistency groups** (step 4): they need #51's step 5, one barrier across targets.
- **`ReplicationRelationship`, bandwidth caps and time windows** (step 5). There is no
  operator (#43); the relationship lives in the metadata service, with `flaminio-meta`
  commands (§2, principle 1). The caps are #59's, shared with backups.
- **The shared diff reader's implementation.** #52 reads the same thing: what changed between two snapshots
  of a volume, with its content at the later one, from in-sync replicas, restartable. One
  reader serves both; this review fixes its shared bytes, whichever lands first builds it.
- **A source that is not attached.** What it wrote after its last snapshot is cut, and
  shipped, at its next attach; a target without a device (#17) would do both at once.
- **Reading a standby before promotion,** and a trial failover that keeps the relationship:
  #51's step 4.
- **Encrypted volumes** (#50): whether ciphertext is shipped, and with which key reference.
- **Kubernetes objects of a promoted volume** (PV, PVC): a runbook, or Velero (§14.1, §14.3).
- **Rotating a peer's CA,** and short-lived certificates (#16). A new bundle is exchanged
  again by hand.
- **Several standbys of one source, chains, and volumes resized while paired.**
- **Continuous, journal-based replication** (§14.2, "Later").
- **Isolation in the lab.** Root on fl-c is root in both clusters: the check shows the rules,
  not that one cluster survives the other's compromise.
