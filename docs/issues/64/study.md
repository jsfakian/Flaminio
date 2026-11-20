# Issue #64 — erasure-coding study and not-yet outcome (Accepted with the plan, 2026-11-13)

Date2026-11-12. Review revisions dated 2026-11-20 are unapproved; the recorded not-yet outcome
and reopening criteria remain settled. Scope: paper study, no implementation or new measurement. References in the
[intent](intent.md) and [design](design.md) identify current code and benchmark evidence.

## Use and capacity (intent1–2; S1, S2)

The likely candidate is large low-write-rate data on capacity devices in clusters with enough
independent domains. No observed workload establishes that need here. RWO block applications
still issue4KiB updates; backup object stores already hold cold immutable data. The reviewed #72
increment supplies only a proposed per-node capacity ledger and create/move admission. Device
classes require separate future policy, and a cold-only scope would also need a cold
classification policy; neither is supplied by #72 or this study. None can be assumed available.

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

**Paper assumptions, not a selected profile.** Let U be one aligned data/fragment unit and
X=kU a logical stripe of an illustrative linear systematic k+m code. RF2/RF3 store identical logical
bytes; all members are healthy for writes and one known coherent survivor set is already
identified for reads/rebuild, and the owning fencing/lease/minimum policy permits the operation.
The rows do not promise degraded availability. The coordinator is on no member: every data read/write is remote,
old data/parity is uncached, parallel requests fit in one phase, and it receives reads before
sending dependent writes. Exclude client-to-coordinator traffic, headers, digests, CPU, journal,
allocation rounding, retries, leases/fencing and version/commit/GC work. An operation below is
one contiguous request per member; a data round is one parallel request/reply phase. These are
not physical device I/O counts. Byte counts are both remote
payload transfer and logical store data I/O under these assumptions.

| Operation | RF2 data operations/bytes | RF3 data operations/bytes | Systematic k+m data operations/bytes | Minimum data rounds |
|---|---|---|---|---|
| Small aligned overwrite U | 0 reads; 2 writes, 2U | 0 reads; 3 writes, 3U | Delta parity: m+1 reads, (m+1)U; m+1 writes, (m+1)U | RF: 1 write; code: 1 read then 1 write = 2 |
| Full-stripe overwrite X | 0 reads; 2 writes, 2kU | 0 reads; 3 writes, 3kU | 0 reads; k+m writes, (k+m)U | 1 parallel write round for either |
| Healthy read U | 1 read, U | 1 read, U | 1 systematic read, U | 1 read round |
| Read U with its member lost | 1 surviving-copy read, U | 1 surviving-copy read, U | k coherent reads, kU, then decode | 1 parallel read round; failure discovery excluded |
| Healthy full-stripe read X | 1 read, kU | 1 read, kU | k systematic reads, kU | 1 parallel read round |
| Full-stripe read X with one data member lost | 1 surviving-copy read, kU | 1 surviving-copy read, kU | k coherent reads, kU, then decode | 1 parallel read round |
| Rebuild one lost member's share of X | 1 read, kU; 1 write, kU | 1 read, kU; 1 write, kU | k reads, kU; 1 write, U | 1 read then 1 write = 2 |

Delta parity reads the old U plus m old parity units. Reconstruct-write instead reads the
k−1 unchanged data units, (k−1)U, then writes the same m+1 units; it also has 2 dependent data
rounds. These are alternative algebraic costs, not safe in-place update protocols. The full
write amplification is (k+m)/k; small-update read and write amplification are each m+1 for
delta parity. For an entire logical payload L, rebuilding one lost RF member reads L and
writes L; rebuilding one lost coded fragment reads L and writes L/k. The latter restores a
smaller stored share and needs k sources, not one. Streaming can overlap different stripes'
phases; these counts describe each stripe's dependency chain, not elapsed latency.

Current healthy RF writes wait for durable replies from every ISR member in that write round
(`Target::check_write`, protocol §4), with no separate healthy commit round. The coding column
attempts every listed data write, but chooses no q/ACK rule. **Additional coding version lookup,
durable-decision, publication and recovery rounds are unknown**, may overlap data work and
cannot be inferred from its 1/2 data-round lower bounds. Thus the table supplies transfer and
read-before-write round estimates, not end-to-end durable ACK estimates or a production protocol.

For illustration only, U=4KiB, k=4, m=2 gives X=16KiB. RF2/RF3 small writes transfer/write
8/12KiB with no data reads; delta coding reads 12KiB and writes 12KiB (24KiB total). Full writes
transfer/write 32/48KiB for RF2/RF3 versus 24KiB coded. A missing 4KiB data unit reads 16KiB coded versus 4KiB
from a surviving replica. Rebuilding a member's stripe share reads/writes 16/16KiB replicated
versus 16/4KiB coded. No 4+2 geometry, method or acknowledgement policy is selected.

**Cold conversion estimate.** Assume a quiescent logical payload L divisible by X, with r=2 or
3 whole copies retained until an entire conversion unit is published. Reading one old copy
costs L bytes (L/X stripe reads); encoding writes E=(k+m)L/k bytes ((k+m)L/X fragment writes).
Each stripe needs a read then a write data round; publication/epoch/commit rounds remain unknown.
The data-only peak coexistence is rL+E, an increase of E over the old rL, before reclaiming
anything. For the illustrative 16KiB unit above: read 16KiB, write 24KiB, and retain 56KiB with
RF2 or 72KiB with RF3 during conversion. For this 4+2 example and equal payload L, E/(rL) is
3/4 or 1/2 respectively.
Converting back reads L systematic bytes and writes rL; it also retains both forms until safe
publication. The conversion unit could be a chunk; no volume-wide atomicity is assumed. Journal,
fragment metadata, padding, rollback versions, snapshot/root pins and rebuild reserve add space
or prolong coexistence. Concurrent writes require a separate, unproved transition protocol.
The store's 16–256KiB physical allocation clusters can greatly change all these logical costs.

| Scope | What it simplifies | What remains |
|---|---|---|
|Full-stripe immutable writes|no in-place partial parity update|atomic publication, commit version, degraded source verification, interface restrictions|
|Cold conversion|ordinary replica writes until conversion|epoch-fenced conversion, concurrent write rollback, root/snapshot references, space for both forms|
|Freely writable coded volume|transparent block API|partial update/write hole, retained versions, recovery, degraded policy, high small-write cost|

A CSI StorageClass cannot assume applications write whole stripes. Reject/translate unaligned
requests under an explicit API or keep the4KiB logical contract; advertise no stripe atomicity.
The current RF2 result106MiB/s on1GbE and2.44ms small-write p50 do not predict fast coded costs.

## Dependencies, model and lab (intent7–9; S7–S9)

- #45 copier/rebuild would need multi-source decode. #72's proposed ledger/create/move admission
  would need later fragment-size, coexistence and reserve accounting; it supplies no class/cold
  policy or coded placement. Distinct-domain fragment placement is also a future requirement.
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

## Recorded outcome (intent10–11; S10)

**Not yet** and its reopening criteria were settled at the 2026-11-13 approval. Reopen when a
real workload shows space need beyond backup/cold storage, capacity accounting (the reviewed
#72 ledger/admission increment) and separately specified device-class policy exist, a candidate
coding commit/recovery model catches the write-hole mutations, and independent-domain hardware
can verify m-loss/rebuild behavior. A later cold-only scope additionally needs a specified cold
classification policy. The historical approval attributed accounting and classes together to
#72; its current increment does not supply classes or cold classification.
Then write a Proposed ADR selecting scope/geometry/ack policy, library/unsafe/security boundary,
versioned format and the simulator-first order. None of those coding choices is accepted here.
Runtime, rebuild/scrub and actual StorageClass m-loss evidence follow; #64 stays open. Future
roadmap, architecture §20.6/§11.4 and issue-evidence alignment must reflect this approved outcome
and its reopening criteria, preserving the original issue steps.
Settled at approval, 2026-11-13: not yet, with these reopening criteria (see decisions.md, Settled
at approval).
