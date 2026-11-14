# Issue #70: CSI volume expansion and XFS support — specification

Why: [intent](intent.md). Settled choices: [decisions](decisions.md). How: [design](design.md).
**Accepted 2026-11-13 ([decisions](decisions.md)); not implemented.** Changed after approval;
Settled 2026-11-13 (user's instruction): accepted (see decisions.md): S2's last five sentences
(rounds 4 to 9); S4's sentence on #73's roles (round 6); S6's third sentence (round 2) and its
last two (rounds 2 to 5), which describe the node-wide wait (the design's Risks); S9 (round 3);
the Tests list (rounds 2 to 7); Settled questions (rounds 5, 6).

This is the offline raw block and ext4 step ([intent](intent.md#this-step)); online resize and
XFS stay open and have no rule here.

## Terms

- **Committed size:** the size in the volume header; never decreases.
- **Prefix/suffix:** a volume's chunks before a growth step, and the whole chunks the step
  appends; chunk size is fixed.
- **Growth step:** one conditional metadata transaction that appends suffix chunks and raises
  the committed size. Nothing of it exists before it commits.
- **Attached size:** the committed size a target read at its attach. It serves that size until
  it ends.

## The invariant

Growth preserves every acknowledged write, and the placement, ISR and version of every prefix
chunk; it changes no epoch. Every committed size is a complete volume: no reader, attach or
deletion sees part of a step. A target serves only its attached size; a device takes a larger
size only when an attach creates it. Nothing shrinks a volume.

## Behaviour

**S1. One admission policy.** CSI selects the new size from the signed capacity range with
#80's range helper, rounding up to whole chunks of the volume, and checks it with #80's
geometry admission for the volume's chunk size and RF. Negative or contradictory ranges are
`InvalidArgument`; no supported size within the limit is `OutOfRange`. A volume already at
least the required size succeeds unchanged, unless it exceeds a positive limit (`OutOfRange`).
The metadata service applies the same geometry check. No request shrinks a volume or changes
its chunk size, RF or minISR.

**S2. Growth in committed steps.** The metadata service appends whole chunks in growth steps.
Each step compares the header and #78's placement gate as read, writes the new chunk records
and the header with the raised size, and leaves every prefix record unchanged. New chunks are
placed by the volume's recorded failure domain and policy (#18) with its RF, on live,
non-witness, non-draining nodes, every replica in the ISR. A step that does not commit changes
nothing; a request resumes from the committed size. Growth is refused unless the metadata
service runs with `--volume-expansion`. Legacy volumes (no recorded first epoch) grow the same way.
Each step charges #72's capacity ledger and #73's namespace and class quota in its own
transaction, by their rules; a step without room or quota is refused with `ResourceExhausted`
and writes nothing. This plan lands with or after #72, so growth always charges the ledger.
Once #73's tenancy is active, a volume with no owner record does not grow (`FailedPrecondition`
`unassigned`), and a system-owned one charges no quota. A call that began before activation
never commits a step uncharged: it is refused with `Aborted`. On an active store, a step that
reads no owner record answers `unassigned`, even if the volume is assigned after the step's read;
a later call reads the new owner and charges it.

**S3. Concurrency.** Attach, ISR changes, moves, deletion, drain marks, node registration and
other growth requests interleave safely with steps: a step whose compares fail reads again and
places again. Concurrent requests converge on the largest size asked, with no chunk index
written twice. Reads see one committed size and exactly its chunks (#77). Deletion's tombstone
names every committed chunk's replicas. A failed request leaves its committed steps, and the
volume stays usable at that size.

**S4. Controller expansion.** `ControllerExpandVolume` grows the volume through S1 and S2 and
returns its committed size with `node_expansion_required=true` for block and ext4 alike. It
neither checks nor changes attachment. Only an administrator identity, which the CSI
controller holds, may call `ExpandVolume`; a node agent, a data plane or any other identity
cannot. Once #73's tenancy is active, the caller needs the role of the owner's kind:
`csi-controller` for a tenant-owned volume, `system-provisioner` for a system-owned one. A
missing volume is `NotFound`; XFS or another unsupported capability is `InvalidArgument`; a
disabled or older metadata service is `FailedPrecondition`.

**S5. Targets and replicas.** A running target applies metadata views to its attached chunks
only and serves none appended after its attach; its prefix ISR changes and moves continue. A
target attached after a growth serves the larger size; its suffix chunks pass the normal
fencing and reconciliation before I/O. A replica takes a larger size for a volume it holds only
from a Hello its fence accepts, and only then raises the size it knows; a Hello below the fence
changes no size, chunk or fence. Recovery accepts a volume whose chunks recorded different
sizes. Local and remote replicas follow the same rules.

**S6. Device size at attach.** The ublk device takes the attached size when it is created.
Nothing resizes a live device: a staged volume takes its new size only when unstaged and staged
again. A replica node that cannot create a suffix chunk at that attach refuses the volume's
session before its fence moves, like an unreachable replica: the target's member-failure rules
apply to every chunk it holds there, and no acknowledged write is lost. The attach still succeeds
at the larger size; I/O to a chunk none of whose replicas accepts the session waits, without a
deadline, until one of them accepts it, which needs slots there for every chunk of the volume.
For ext4, the node plugin's steps that read or write the device (NodeStage's `blkid -p` and
`mount`, NodeExpand's `resize2fs`, NodeUnstage's `umount`) wait with it, holding its lock.

**S7. Node expansion.** `NodeExpandVolume` acts only on a volume staged on this node and owned
under #79's records, its device pinned under #15's device-lifetime contract. It requires the
volume capability. A device smaller than the required bytes fails with `FailedPrecondition`,
asking for a restage. Raw block needs no filesystem work. A writable ext4 stage grows with
`resize2fs` on the pinned device; readonly, a mismatched capability or a filesystem other than
ext4 fails without running a tool. It reports the device size read after the work.

**S8. Retries.** A repeated or interrupted `ControllerExpandVolume` resumes from the committed
size. A repeated `NodeExpandVolume` runs `resize2fs` again, which grows nothing already grown.
Restart and failover attach at the committed size with old and added data intact.

**S9. Advertise only what is supported.** The plugin advertises `VolumeExpansion` `OFFLINE`
and controller and node `EXPAND_VOLUME`, also while the metadata flag is off: every refusal then
names the flag. The lab deploys the external resizer with its RBAC and `allowVolumeExpansion:
true`, and turns the flag on in a second rollout. The docs state: offline only, block and
ext4, no shrink, whole-chunk rounding, the restage procedure, the rollout order (every data
plane node first, then the flag) and that a data plane rollback after a growth is unsupported.

## Tests

- **S1–S4:** Go service tests on memory and real etcd: ranges and #80 vectors, zero writes on
  refusal, placement by recorded domain, step boundaries and interrupted steps, races injected
  before a step's commit, delete after growth, authorization and #73's roles, the ledger and
  quota charges, an unassigned volume, an activation or assignment during a call;
  CSI controller tables.
- **S5, S6, S8:** simulator: growth during prefix I/O, failover onto the larger size; it catches
  the injected `drop_grown_view`. Target unit test; replica and engine memory-store tests:
  larger and stale Hellos, a failed creation, mixed descriptor sizes after restart, malformed
  refreshes, slots exhausted at the attach (on one replica, and on every replica of a chunk,
  then released by a freed slot).
- **S7, S8:** CSI node tables with fake owners, devices and tools: block, ext4, smaller
  device, readonly, mismatch, device identity changes, tool failure, a tool that ignores
  cancellation, repeat; a NodeStage probe that ignores cancellation.
- **S9:** capability tests; the lab-flag script test; the lab check `make csi-expand` for block and ext4, then
  `make csi-failover` on the grown volume (authorized later; nothing ran while drafting).

## Settled questions

Settled with the recommendations on 2026-11-13 ([decisions](decisions.md)): expansion does not
wait for #72 (changed after approval, rounds 5 and 6; Settled 2026-11-13 (user's instruction):
this plan lands with or after #72, and growth always charges its ledger (see decisions.md)); 64
chunks per step; suffix chunks prefer the volume's last target; expansion is advertised while the
metadata flag is off; the rollout order has no automatic version check. Settled 2026-11-13 (user's
instruction): the node-wide wait found after approval is accepted as the design's Risks state it
(see [decisions](decisions.md)).
