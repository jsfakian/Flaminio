# Issue #91 — node/store join and nonstorage lifecycle

Proposed, unapproved. Single authority for the name join, candidate class and old-store
retirement predicates used by [reader](reader-contract.md) and [admission](admission-contract.md).
No metadata field contains a Kubernetes UID; no rule below pretends that it does.

## Supported join and persisted evidence

The supported Kubernetes profile is exact byte equality `metadata Node.id == Node.metadata.name`.
The installer and ordinary node/CSI configuration use that existing name convention. No aliases,
IP/advertised-address matching, hostname normalization or caller labels establish identity.
Require an ID accepted by both Kubernetes Node-name and metadata `domain.ValidID` grammars,
≤128 ASCII bytes; unsupported names hold visibly. Enrollment persists `metadataID=name` and
its exact immutable Kubernetes NodeUID. Every collection checks that equality against actual
Node objects and owned enrollment UIDs. Same-name/new-UID needs independent enrollment;
only the currently observed UID can be a current survivor. Older enrollments remain retirement
state, never a second survivor. A metadata ID with no current Kubernetes enrollment is Unknown
unless explicitly configured as external. External IDs cannot equal any live/staged/retained
Kubernetes enrollment name; duplicates, aliases or collisions fail the entire pass.

Admission requires #90 ACTIVE `identity-v4`; a pre-#90 server cannot supply the permanent
history proof below and cannot enable this profile. This does not grant a writer role or
change #90. `everStorage` is monotonic in each enrollment, initially false, set true before
allow whenever a metadata Node record/history/reference for that exact ID is observed.
Persist `observedStores`, a sorted set of canonical `(device_uuid, maxObservedGeneration)`
observed as current stores or exact membership/move endpoints for that ID, ≤8 entries.
Generation0 means ref-only UUID whose historical generation is not returned; current-store
observations require positive generation. Update evidence only for the actually current
Kubernetes UID; a deleted UID’s saved evidence is never rebound to a new UID/current store.
Never overwrite an old UUID with a new one. Before reserving, slot copies `metadataID`,
`candidateClass` and immutable `retirementStores` UUID set from enrollment plus this pass.
Overflow, unknown UUID provenance or incomplete reads holds; do not truncate retirement data.
Each enrollment remains≤4KiB including status; reduce diagnostic entries to fit rather than
truncate authority/evidence. Generation is an observation/session lifetime, not earned identity.
A later generation of the same original UUID can represent returned earned data.

## Proven nonstorage candidate

Strict `ListVolumesRequest` adds `candidate_metadata_id=21:string`. Response adds
`candidate_no_store_history=23:bool`, `candidate_history_revision=24:int64`. The request ID
must equal enrollment metadataID. In ACTIVE, the service reads #90's never-deleted exact
`store-lifetime/<id>` key before enumeration and compares its revision/absence again using a
read-only conditional transaction with the existing mode/header checks. true/revision0 means
that key was absent throughout those compared reads; present history is false/positive revision.
Malformed history or unavailable/changed predicate errors. The guard requires the same proof
on both bracketing strict enumerations; old servers/default false never prove absence.

`ProvenNonStorage` requires all of: same actual Node UID and owned anchor; `everStorage=false`;
no matching Node in either complete ListNodes observation; both strict history checks return
true/revision0; complete validated volume views contain no exact replica/ISR/pending/released/
fenced or move endpoint with node==metadataID; no conflicting external mapping/enrollment.
The permanent lifetime record is required even when ordinary ListNodes omits a forgotten node.
Under #90 ACTIVE, every legitimate current or historical store reference, including unpublished
creation/deletion cleanup, has a previously created never-deleted lifetime record. Thus its
absence cannot conceal valid storage holdings in deliberately unpublished/deleted volumes.
A reference without that history is a contradiction and holds; no label or empty volume set
alone proves nonstorage. A formerly registered node is never downgraded to ProvenNonStorage.

This class may reserve the normal one-node slot for ordinary drain or anchor terminal/image
refresh. It needs no fictional store liveness. Its recovery requires same Node UID schedulable
and Ready, fresh complete repeated ProvenNonStorage predicates and fresh finalizer-bearing
Ready replacement after retiring the original anchor UID. Only then CAS releases the slot.
If storage registration/history/reference appears at any fresh collection, latch everStorage
before further decisions and switch to Storage or Unknown; re-evaluate storage minimum/full
ISR rules. Any ambiguous transition or metadata failure retains the slot. A new registration
after the final observation is the ordinary read-only observation race, never a permission to
alter metadata authority; the continuously present replacement covers its future drains.

## Storage and old-enrollment retirement

`Storage` requires a valid exact-name metadata node, supported current store UUID/positive
stable generation, canonical exact membership and complete data. everStorage remains true.
Candidate removal excludes **all** refs of that node name, regardless of which saved UUID
it uses. Standard storage recovery requires same Node UID schedulable/Ready, current store
live and every relevant earned ISR full; replacement coverage and old-Pod absence still apply.
Absent current record/dead store with everStorage/history is Unknown/StorageUnavailable,
never harmless nonstorage. A later store observation extends observedStores before reservation.

For a deleted/recreated old Kubernetes UID, first retire its exact slot-owned anchor UID.
If no slot was already occupied, the reconciler may reserve that old UID/anchor for retirement
only after complete fresh metadata collection and actual Node absence/different UID; it does
not require the impossible live-old-UID check of ordinary admission. The same one-slot CAS,
immutable saved evidence and finalizer ordering apply. This is owned old-anchor cleanup,
not a grant for the current same-name UID. Unknown inputs retain both interlock and state.
No metadata query is made for that Kubernetes UID. The immutable slot retirementStores and
exact metadataID are the actual old-store predicates. Release needs complete fresh observations
and either:

- **Returned exact store:** a new currently enrolled same-name Node UID is schedulable/Ready
  with a Ready finalizer-bearing anchor; current metadata store is a saved UUID, live with
  stable positive generation, relevant earned ISR full. Every other saved UUID satisfies the
  no-active-reference case below. This reestablishes observed availability of the same data;
  it releases the old slot but grants no admission to the new UID.
- **Old stores moved away:** none of the saved `(metadataID,UUID)` pairs appears in placed
  replica, earned ISR, pending-initial or active move endpoints in complete views.
  Released/fenced cleanup refs alone earn no survivor credit and do not retain node availability
  occupancy. Any remaining same-name live replacement UUID has its own current enrollment,
  Ready anchor and full relevant earned ISR before release. If there is no current same-name
  Kubernetes UID, require no active refs under that metadataID at all; a stale dead Node record
  or permanent lifetime record alone is not an active data holding and does not retain this
  deleted UID’s occupancy. A no-history claim cannot erase everStorage.

New references under another UUID do not falsely retain an old exact store; old earned refs
still naming the old UUID do retain it until moved/replaced or that exact store returns.
An unsaved replacement UUID must be validated as the new current enrollment’s actual store;
it is never attributed to the deleted UID. Unknown mappings, unbound UUIDs, history/schema
change or failed complete
read retains occupancy. Restart reconstructs exclusively from durable evidence, never by
rebinding old records to whichever store is current. No ISR/move/drain mutation is performed.

## Required fixtures

Exact-name success and differently named/alias IDs; duplicate/external collision; same name
new UID; Kubernetes-only worker terminal/image refresh, drain/uncordon/release then storage
drain; formerly-storage ListNodes absence; metadata outage; storage registration/reference
appearing during nonstorage recovery. Old UID/store A with new UID/store B while earned A
remains holds; moving active refs away from A releases; released-only A refs do not count as
survivors or retain availability; A returning under newer generation releases through full
recovery; same-A/new-UID with unchanged generation; overlap/restart restores exact saved UUIDs.
All service-reader and real-API cases are future tests, not executed evidence.
