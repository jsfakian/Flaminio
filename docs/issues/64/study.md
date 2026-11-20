# Issue #64 — erasure-coding study and not-yet outcome (Accepted with the plan, 2026-11-13)

Date2026-11-12. Scope: paper study, no implementation or new measurement. References in the
[intent](intent.md) and [design](design.md) identify current code and benchmark evidence.

## Use and capacity (intent1–2; S1, S2)

The likely candidate is large low-write-rate data on capacity devices in clusters with enough
independent domains. No observed workload establishes that need here. RWO block applications
still issue4KiB updates; backup object stores already hold cold immutable data. Device-class,
capacity reserve and cold classification are draft #72 work. They cannot be assumed available.

| Scheme | Ideal raw/usable | Fragment losses tolerated from a coherent complete stripe | Domains | Extra domain needed for one replacement without reuse |
|---|---:|---:|---:|---:|
| RF2 |2|1|2|1|
|2+1|1.5|1|3|1|
|4+1|1.25|1|5|1|
| RF3|3|2|3|1|
|2+2|2|2|4|1|
|4+2|1.5|2|6|1|

2+2 saves1/3 raw vsRF3;4+2 saves1/2. These ignore journal/maps, version retention, padding,
thin-provision allocation and rebuild reserve. A completely populated k+m cluster has no
independent destination for a failed fragment until a domain returns/is added. Colocated disks
are not independent host/power domains. q=k coherent ACKed fragments have no further loss budget;
q≥k+D is necessary for D additional fragment losses, not a complete commit/availability proof.

## Consistency and write hole (intent3–4; S3, S4)

A fragment is in sync only when it belongs to the recoverable committed stripe version set,
not when its epoch/CRC is highest. A reader needs either the requested coherent systematic
fragment or k validated same-version fragments. Takeover must exclude old writers under the
existing epoch/lease rules and establish a recoverable version for every uncertain stripe.
No fragment alone is an authoritative source. ADR0006's full-copy singleton policy does not apply.

Worked failures of direct in-place parity updates:

1. **4+2 interrupted after3new/3old.** Neither version necessarily has4coherent fragments.
   Per-fragment valid CRCs do not make a stripe decodable as a valid application history.
2. **2+1, one new/one old survives target-member loss.** The third fragment was on the failed
   target node. Neither surviving pair is a complete version; “take highest seq” cannot recover.
3. **Partial-stripe update with old parity then another data loss.** Reconstruction combines
   mismatched parity/data and can corrupt a neighboring logical block the client never wrote.
   Allowing an unanswered write to vanish does not permit losing that acknowledged neighbor.

Candidate direction: retain old/new immutable fragment versions with stripe identity/hash and
recoverable commit evidence until safe reclamation. A new epoch may roll forward/back only a
complete consistent version respecting every acknowledged operation. Durable commit/reclamation
needs a specified quorum/decision protocol without per-write metadata-service round trips.
None is proved here. A fragment-majority vote, scalar highest sequence or all-CRC-valid test
is not that protocol. Concurrency/partial writes/snapshots require modeling, not analogy.

## Cost and confinement (intent5–6; S5, S6)

Let one fragment unit U=4KiB for illustration, logical stripe k×U. A full-stripe update writes
(k+m)U, amplification1+m/k before metadata. A4KiB systematic update using delta parity may read
old data plus m old parity fragments and write1+m fragments: read/write and network round-trip
costs depend on caching and durable version protocol. Reconstruct-write alternatively reads
remaining data; it is not free. Healthy systematic read may need one fragment; missing data
requires k coherent fragment reads+decode. Rebuild reads kU to recreate U, before concurrency.
All are byte-operation estimates, not measured timings or CPU.16–256KiB physical allocation
clusters, COW/version retention and alignment may greatly change them.

| Scope | What it simplifies | What remains |
|---|---|---|
|Full-stripe immutable writes|no in-place partial parity update|atomic publication, commit version, degraded source verification, interface restrictions|
|Cold conversion|ordinary replica writes until conversion|epoch-fenced conversion, concurrent write rollback, root/snapshot references, space for both forms|
|Freely writable coded volume|transparent block API|partial update/write hole, retained versions, recovery, degraded policy, high small-write cost|

A CSI StorageClass cannot assume applications write whole stripes. Reject/translate unaligned
requests under an explicit API or keep the4KiB logical contract; advertise no stripe atomicity.
The current RF2 result106MiB/s on1GbE and2.44ms small-write p50 do not predict fast coded costs.

## Dependencies, model and lab (intent7–9; S7–S9)

- #45 copier/rebuild and #72 placement/reserve need multi-source decode and distinct-domain
  fragment accounting, not one source node or replica-count capacity.
- #46 scrub needs fragment-local integrity plus same-version reconstructibility checks;
  majority-identical digests are inappropriate for different fragment bytes.
- #50 encryption/authentication needs a defined encode/encrypt order and authenticated original
  volume/address/version plus fragment identity. Coding alone does not authenticate a hostile
  fragment; CRC-only detection is not malicious substitution/replay defense.
- #51 snapshot/clone roots pin coherent stripe versions and keys. “Identical bytes on every
  replica” becomes “fragments of one immutable version decode identically,” with exact GC rules.

Model each durable fragment/version, old retention/commit proof, ordered epoch/session, partial
write and permanent loss of up to declared m domains, including the target's own fragment.
Check per-block linearizability and recoverability of acknowledged neighbors. Mutations must
catch mixed versions, early old-fragment reclaim, early ACK/publication, wrong source epoch,
uncorrelated-domain counting and premature ISR admission. Restarts and all nodes eventually
returning do not establish permanent-loss durability. Model before data-plane/library work.

Lab currently can explore2+1 VM loss but not m2 independent failure/rebuild. Qualification needs
k+m independent domains and a spare, known data/capacity devices, controllable permanent loss,
clock profile and recorded acknowledgements. Measure healthy/degraded latency, CPU, network,
space including retention/reserve and rebuild workload against matched RF2/RF3. No new run here.

## Proposed outcome (intent10–11; S10)

**Not yet** is the recommendation, pending human decision. Reopen when a real workload shows
space need beyond backup/cold storage, #72 accounting/classes exist, a candidate coding
commit/recovery model catches the write-hole mutations, and independent-domain hardware can
verify m-loss/rebuild behavior. Then write a Proposed ADR selecting scope/geometry/ack policy,
library/unsafe/security boundary, versioned format and the simulator-first order. Runtime,
rebuild/scrub and actual StorageClass m-loss evidence follow; #64 stays open. Align roadmap,
architecture §20.6/§11.4 and issue evidence with this pending recommendation when that docs
increment is reviewed; do not record it as accepted merely because this artifact exists.
Settled at approval, 2026-11-13: not yet, with these reopening criteria (see decisions.md, Settled
at approval).
