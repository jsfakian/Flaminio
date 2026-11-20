# Issue #56: Two data nodes and a witness — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Historical approval: 2026-11-13; review revisions below are unapproved. Not implemented.
First part of step 2: topology contract, simulator campaigns and lab evidence. No protocol,
default, installer profile, one-node or three-data-node implementation. #56 remains open,
including ADR0006's required visibility/backup/resync priority and other topology checks.

## Terms

- **A/B:** the two data nodes; each initially holds every chunk. **W:** third etcd voter with
  no data replica. The lab also runs Kubernetes control-plane/controllers on W.
- **Metadata reachable:** a target can complete linearizable operations through an actual
  etcd majority; individual endpoints being reachable is insufficient.
- **Single-copy interval:** observational interval while any chunk has an ISR of one member.
- **Safety:** per-block acknowledged history and current epoch/lease rules remain valid.
- **Availability:** an operation eventually completes under stated reachability/ISR assumptions;
  detection/recovery time is measured, not a new safety timeout.

## The invariant

No partition creates two permitted writers or stale successful reads. With minimum ISR1,
acknowledged data may exist on one surviving disk; losing it before resync can lose those
writes, as ADR0006 accepts. No test or recovery step silently promotes an out-of-ISR copy.

## Behaviour

**S1. Fault contract.** Publish one table for A/B/W losses, each pairwise link cut, a data node
isolated from both others, and double faults, for minimum ISR1 and2. State target/ISR/quorum
assumptions and which operations serve, wait or cannot recover without accepted data loss.
Distinguish existing data I/O from provision/attach/pod reschedule and extra roles on lab W.

**S2. Data-node loss.** With one data node lost and survivor+W providing metadata majority,
minimum ISR1 permits writes only after committed membership removal and normal takeover
readiness where needed. Minimum ISR2 does not acknowledge writes needing the missing member.
Returning replicas resync before ISR admission. Permanent loss of the sole authoritative copy
is not automatically repaired from a stale disk.

**S3. Witness loss.** A/B can maintain metadata majority and serving I/O if connected. Lab
Kubernetes control/CSI/failover may be unavailable because W hosts them; do not describe that
as every operation unaffected. New failover is distinguished from already attached I/O.

**S4. Pairwise partitions.** With A↔B cut but both reaching W, the existing target may shrink
to itself under minimum ISR1. A manual attach to the other node raises the epoch and may stop
all I/O until the new target can reach the authoritative ISR. It cannot adopt stale local data.
Cutting only A↔W or B↔W while A/B communicate does not itself remove their metadata majority.

**S5. Isolation/double loss.** A data target without metadata majority may serve only while
current confirmations/readiness allow, never after its read lease expires. Successor readiness
requires fencing or the specified conservative takeover wait, not endpoint liveness. Double
fault outcomes follow actual committed ISR/quorum, including indefinite unavailability or
accepted single-copy loss; never promise survival of two arbitrary losses.

**S6. Simulator.** Add explicit two-data-node RF2 campaigns for minimum ISR1 and2 with realistic
clock bounds, failures/restarts/partitions and hand-forced attach. Retain existing three/four-node
campaigns. Include deterministic cases for every S1 table row and the serve-nowhere case.
Don't assume simulator metadata node models equal the three physical etcd members.

**S7. Mutation evidence.** For each existing protocol mutation, record a two-node caught witness,
a structural reason the action is unavailable, or an explicit unresolved coverage gap. Failure
to find a seed is not proof the mutation is impossible. Existing generic mutation detection
remains required. Every purported catch also passes without the injected bug.

**S8. Lab verdict.** One explicit check covers both data-node losses, witness loss and every
pairwise link cut with confirmed effect, then heals and checks restored ISR/data. The current
#83 profile refuses VM admission: enforcement-active VM runs return Blocked before mutations.
Legacy pre-#83 VM results may be explicitly exploratory only; suppress the unsafe failover
controller before any fault. #82 currently supports physical held lockout only, so full VM/pod
recovery stays Blocked; sampled power state never authorizes Kubernetes recovery. Workload
records acknowledged writes and reads so stale/lost data is detectable; fio verify alone is
insufficient. No wall-clock comparison across VMs as evidence of operation ordering.

**S9. Ownership and cleanup.** Fault rules target exact owned nodes/overlay addresses. Bound
runs and cleanup, preserve management SSH, never alter unrelated rules or disks. Interrupted
runs can independently heal their persisted fault inventory. Inconclusive/unhealed runs fail;
no pass from a fault that never actually took effect.

**S10. Single-copy evidence.** Report observed first/last single-copy coherent metadata views
(epoch and each chunk's isr_ver/ISR), sample time bounds, resync completion and uncertainty.
No public etcd revision is assumed. Missed transitions remain unknown;
call this observed ISR exposure, not proof of first single-copy write or ADR0006 alert coverage.

**S11. Scope and docs.** Update lab/protocol/topology contract and actual evidence. Runtime
counterexamples are bugs to file/fix in their owning issues, not reasons to weaken the check.
Use #81 for65+ changes, #82 for safe node fencing, #83 for host clock qualification and #49's
value encoding and bounded register checker through a separate coordinator-history adapter.
Keep the current64-chunk scoped lab limit explicit; do not reinterpret #49's v1 segment format.

## Tests

- S1–S7: scripted topology cases, seeded campaigns and documented mutation witnesses/limits.
- S8–S10: fake VM/network/control helpers, deliberate stale/lost-write device, no-effect fault,
  timeout/SIGINT/SSH loss and independently repeatable cleanup; single-clock history checking.
- S11: authorized lab evidence, clear limitations, unchanged protocol/defaults and generic suite.

## Open questions

None left unspecified in this increment. #83's physical-only admission and #82's physical
held-lockout recovery are concrete dependencies; current VMs support neither integrated path.
Legacy exploratory data-path checks cannot qualify them or enable Kubernetes recovery. Pending
two-node mutation witnesses remain coverage gaps, never waived as impossible.

The 2026-11-13 settled answers remain recorded historically in decisions.md. The round-1
revisions correct their VM power-proof alternative and distinguish legacy exploratory results
from #83 enforcement; these revisions have not been approved.
