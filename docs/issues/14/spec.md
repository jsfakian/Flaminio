# Issue #14: Tombstones on nodes outside Kubernetes — specification

Step 2 of the issue ([intent](intent.md)). Settled choices and accepted risks:
[decisions](decisions.md). How: [design](design.md).

## Terms

- **Tombstone:** a deleted volume's record of the nodes its chunk records named, and of those
  that have confirmed. Keyed by the volume's name alone.
- **Pending:** named on a tombstone, not confirmed.
- **Confirming for a node:** recording that the node holds no replica of the volume any more.
  The last confirmation removes the tombstone, and the id is free.
- **Confirmer:** whatever confirms for a node: its CSI node plugin's sweep, or an operator
  through `vol-confirm`.
- **Node without a sweep:** a data node that is not a k3s agent. In the lab, fl-c.

## The invariant

While a node is pending, only a confirmer for that node can remove the tombstone, and so let the
id be created again. **Exactly one confirmer per node:** the CSI sweep on fl-a and fl-b; an
operator, through `vol-confirm`, on fl-c. The harness never confirms.

Every delete of a name through a node's control socket deletes all its chunks there, whatever
their fence, unless the volume is attached on that node. So a delete that runs after the id is
freed wipes a volume created again with the name on that node, where it is not attached: with
RF 1, its data. Two confirmers for one node lead there: one confirms while the other has yet to
delete, or confirms a tombstone the other's deletion was not for. So does the harness's own
delete on every data node (S5), which can run after the sweeps have freed the id: one deleter
and confirmer per node, plus the harness while no other lab command runs (a risk accepted after
approval: decisions).

A node confirmed while it still holds replicas, with no delete to come, keeps them until a volume
created again with that name drops them at its first session there (issue #30). Until then they
hold space, and the old volume's tokens still open sessions on them; a volume created by an old
metadata service is still deposed by them (#30, S8 and S9).

## Behaviour

### The commands

**S1. Listing tombstones.** `flaminio-meta tombstones` lists the tombstones pending for one node,
or for every registered node, each once, with their nodes and pending nodes. It changes nothing,
and prints nothing on an error. A tombstone pending only for a node that is not registered is
not listed; it still keeps its id from being created again.

**S2. Confirming.** `flaminio-meta confirm` confirms one volume for one node.
- **Only that node's own agent:** any other caller (an administrator, no certificate, another
  node, another component) is refused before any call. Confirming for a node gone for good is a
  later step.
- **An explicit acknowledgement** that the node's replicas are deleted is required; without it,
  or with any other argument, it refuses and calls nothing.
- **It reads first:** it confirms only when the read shows the volume pending for the node.
  Otherwise there is nothing to confirm, which is not an error, and nothing is called.
- **What it prints comes from the read:** the caller, the volume, the node, the tombstone's nodes
  and pending nodes. The service's answer says nothing more.
- **Its usage text** names `lab.sh vol-confirm` as the only supported entry point, and says what
  a wrong confirmation does, as the invariant does: a delete of the name that runs after the id
  is freed wipes the new volume's replicas there; leftovers hold space until its first session
  there, old tokens still open sessions on them, and a volume created by an old metadata service
  is deposed by them. Not that the new volume serves the old one's data: not since #30, outside
  its S8-S9 cases (a store whose revision went back; a volume created by an old metadata
  service, at epoch 0).

**S3. Who may call the service.** Unchanged: an administrator for any node, a node's agent for
itself. S2's narrower rule is the CLI's.

**S4. The service.** Unchanged, but for one log line: each confirmation that newly confirms a node
logs, once, the caller's identity, its address and certificate serial, the volume, the node, the
nodes still pending, and the write's revision. A repeated or empty confirmation logs nothing.
One exception: when etcd committed the write but its answer was lost, the retry finds the node
confirmed, or, after the last confirmation, no tombstone, and nothing is logged (decisions). It
is for audit after the fact.

### The lab harness

**S5. Deleting a volume.** One deletion (`delete_volume`) serves wbench's deletions, the removal
of `$VOL` at a campaign's start, and `vol delete` (which today deletes the metadata only). It:
- **never confirms;**
- **deletes the volume's replicas on every data node,** each of them running: a node that is not
  is started first, only the campaign's node with its store formatted (the campaign formats it
  anyway). It does so only once the volume is detached everywhere, and gone from the metadata
  service. Those deletes ignore fences, and can run after the sweeps have freed the id; its
  check that the id was not created again sees only a finished creation. So the name created
  and attached by another caller (`make vol-create` then `make vol-attach`, or a second
  campaign), whose first session reaches a replica node between that check and the delete there
  (seconds), loses its replicas on the nodes it is not attached on: accepted (decisions);
- **stops at once,** naming the node, with the manual procedure (S7) and the command to run
  again after it, when the volume is then pending for a node without a sweep: waiting would not
  help;
- **otherwise waits,** bounded, for the CSI sweeps to remove the tombstone, and on time-out names
  the pending nodes;
- **stops on anything unexpected,** and a lost connection leaves the campaign resumable;
- **finishes before any node is stopped or formatted:** a stopped node's sweep cannot delete.

**S6. The write campaign.**
- **Before its first reset,** and before removing anything, a volume other than wbench and
  `$VOL` with a replica on the campaign's node (or moving to or from it) stops it, named. Then
  `$VOL` is removed (S5), as today. This holds at the campaign's start: the resets format that
  node.
- **wbench is created only once the campaign's node is alive,** and every chunk must have a
  replica there.
- **A campaign that places wbench on fl-c** (RF 2, or fl-c as the campaign's node) stops at each
  of its deletions until an operator runs `vol-confirm`. **One whose `$VOL` has a replica on
  fl-c** stops once, at its start, whatever its RF and node: `$VOL` is removed there. An RF 1
  campaign on fl-b never stops for wbench.
- **The restore** no longer moves `$VOL`'s replicas back: `$VOL` is gone.

**S7. Confirming by hand: `lab.sh vol-confirm <volume> <node>`.** Run by an operator, with no
campaign or `vol` command running. It confirms only when all of these held, checked in this
order, and otherwise confirms nothing:
1. the node is one data node, not a metadata cluster member;
2. the node's tombstones list the volume (else: nothing to do, not an error);
3. nothing on the node can sweep: no CSI plugin, no agent sweep, no Kubernetes agent;
4. the volume is attached nowhere, and no stopped node's data plane is still running;
5. the node's data plane is the one its service runs, on the disk its claim policy calls ours;
6. its replicas are deleted;
7. after that, still attached nowhere, and a second delete finds none;
8. its own agent confirms, and the read just before the call shows the volume still pending:
   otherwise another caller confirmed, and it fails. A confirmation by another caller between
   that read and the call is reported as this one's: the service's answer cannot tell them
   apart. Accepted (decisions).

**S8. The stale tombstones, once,** by an operator, before the lab check, recorded in the PR:
`vol-confirm` for every tombstone pending for fl-c (wbench, volb, and the one-off `smoke-*` and
`resync-*` volumes), after the new `flaminio-meta` is deployed and fl-c's node started. Then
none is left.

**S9. Unchanged.**
- The service's rules and the API, comments included.
- The CSI sweep.
- `DeleteVolume`, and which nodes its tombstone names (docs/issues/8).
- Two harness runs at once can delete each other's replicas, as today.

**S10. Docs.**
- `docs/lab.md`: the prerequisites of a campaign and of `vol delete` (the new `flaminio-meta`,
  fl-c's node, the CSI node plugins on fl-a and fl-b); when a campaign stops at fl-c, and what
  to run; the two commands; `vol-confirm` as the only way to confirm by hand; no other lab
  command while a deletion runs.
- Follow-ups §14: step 2 done, with the lab check's result, and the later steps (decisions).

## Tests

- **S1, S2:** the commands against an in-memory service, with and without certificates: every
  refusal before any call, the read-first rule, the output, the agent-only rule.
- **S3, S4:** the caller rules for the new uses; the log line's fields, once per new
  confirmation, under interleaved confirmations.
- **S5–S7:** the scripts, locally and in CI (`make lint`), with the VMs faked: each rule's stop,
  the start and format of a stopped node, the order of `vol-confirm`'s checks, and that the
  harness never confirms.
- **The lab, a later step:**
  - a campaign of one job, two repetitions, RF 1 on fl-b: every reset deletes and creates wbench,
    and no tombstone is left;
  - a campaign with `$VOL` on fl-c stops; after `vol-confirm`, it goes on;
  - `vol delete` and `vol create` of volb, twice;
  - a volume written on fl-c, whose node is stopped before the deletion: the deletion starts and
    cleans it, then stops; `vol-confirm` refuses while fl-c's node is down, and confirms once it
    runs.
