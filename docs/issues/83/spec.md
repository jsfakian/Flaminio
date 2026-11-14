# Issue #83: A supported lease clock contract — specification

Approved 2026-11-13; not implemented. [Intent](intent.md); [decisions](decisions.md), if any;
[design](design.md).
The initial deployment policy remains [Proposed](adr-proposal.md).
Settled at approval, 2026-11-13: the policy is accepted with the plan (see decisions.md, Settled at approval).

## Terms

- **Lease clock:** local elapsed time used for request send timestamps, expiry and takeover.
- **Profile:** explicit deployment assumptions and runtime checks under which lease timing is
  supported. A check observes prerequisites; it cannot prove a future clock-rate guarantee.
- **Clock generation:** process-local admission identity invalidated permanently on a clock
  fault. It supplements, and never replaces, durable volume epochs and replica sessions.
- **Quarantine:** daemon-wide refusal of new successful storage admissions, connections and
  completions after a detected clock fault, until an operator restarts the process.
- **Success permit:** final validated decision that an operation may report success. Delivery
  can occur later; an operation permitted before quarantine may linearize before that fault.

## The invariant

Under the declared clock profile, a target succeeds only under the existing lease, fencing,
ISR/membership and reconciliation rules. All timing decisions use one valid elapsed clock.
After quarantine begins, no new success permit or session is admitted in its old generation;
old buffered replies, tokens and completions cannot reactivate it. No wall-clock expiry grants
storage authority. Excluded environments are explicitly refused or prohibited, never certified
by simulation or a local timing probe.

## Behaviour

**S1. Supported profile.** Propose physical Linux hosts whose elapsed clock includes host
suspend and advances relative to every other target within factor 1+rho, with rho=100,000 ppm
and the existing default L=5 s. Use a common cluster timing configuration; refuse incompatible
local settings rather than claim different lease/drift values interoperate. Hardware/firmware
must maintain this property across suspend, with qualified per-read absolute error<=1 us
and reported resolution<=1 us. Takeover includes conservative elapsed-observation error as
defined in the design; the suspend-detector tolerance is a separate bound. VM/container-on-unknown-host time behavior,
unqualified live migration and rollback of running node memory/disks are unsupported. Explicit
operator deployment attestation is required; detection of virtualization or uncertain host
provenance refuses production storage admission. Root/hypervisor operators are trusted to
honor exclusions; their deliberate concealment is not detectable by userspace.
Settled at approval, 2026-11-13: this profile, physical-boottime-v1, is accepted (see decisions.md, Settled at approval).

**S2. Clock use.** Lease/core admission, tick, send timestamps and completion revalidation use
the same suspend-aware source and checked monotone microsecond conversion. Fresh samples are
required at each relevant transition, including copied/zero-copy fast reads and slow local/
remote reads. Cached batch time, wakeup timers and wall clocks cannot authorize success.
A delayed scheduler or ticker can delay progress but cannot extend a lease.

**S3. Detectable faults.** Failed/overflowed/backwards samples and a detected increase in the
suspend-aware-minus-active-time offset close the admission gate before processing that sample's
event. Sampling uncertainty is bounded and explicit. Ordinary process scheduling pauses while
the clock advances are supported without forced invalidation. Very short/undetected suspend
is safe only because the lease clock itself counts its actual duration. A stopped or jointly
rolled-back virtual clock can be indistinguishable from a valid clock; no detector guarantee
is made for it, and S1 excludes it.

**S4. Invalidation order.** On first detected fault, atomically close success admission and
advance the generation before handling queued network/metadata/I/O callbacks or issuing new
lease-bearing traffic. Invalidate all targets' confirmations, fast-read/local-read handles and
sessions; prevent queued old output from producing a new success permit. Close incoming and
outgoing replication sessions and refuse new ones. Already durable writes remain durable;
unanswered operations may have landed. Release buffers only after their I/O lifetime ends.
Existing success permits ordered before the fault may complete; no permit is created after it.

**S5. Recovery.** Quarantine does not expire on a timer or successful clock read. Read-only
status remains available; storage attach and mutation refuse. An operator restart reruns profile
checks, recovers the store normally and obtains an authorized agent/admin new-epoch result
cryptographically bound to the new daemon process before opening a target. Node/io cannot
mint that permission; old standalone tokens and every legacy startup path fail closed. If this node is no longer target, only-if-target restart refuses; it cannot steal
ownership. Ordinary full-ISR fencing, lease wait where needed and uncertain-write reconciliation
still precede serving. Metadata unavailability leaves recovery blocked. Old tokens/sessions
are never sufficient to resume, and replica disk state is never rolled back or cleared.

**S6. Inspection and enforcement.** A read-only startup/clock-check result reports profile,
source, rho, supported/excluded events, observed prerequisites and whether operator attestation
is present. Unknown is a refusal, not a pass. Runtime status identifies healthy/quarantined,
generation and a bounded reason. Doctor integration in #71 consumes this contract without
claiming clock-drift measurement is proof. Refusal/quarantine logs contain no keys or tokens.

**S7. Model and tests.** Extend deterministic time modeling to distinguish real elapsed time,
per-node clock advance, process scheduling and suspend/virtual events. Supported scheduling
pause and physical suspend histories preserve linearizability or fail closed. Detectable fault
histories enforce S4 and require a new epoch before resumed success. Include a seeded
counterexample or admission-invariant failure when invalidation is deliberately skipped.
Excluded stop/rollback cases demonstrate detection limits and profile rejection; they are not
reported as successful supported fault tolerance. Keep existing lease/wait/fast-read mutations.

**S8. Documentation and compatibility.** Document hardware/operator prerequisites, initial
virtual/lab exclusion, suspend behavior, restart procedure and undetectable events. Preserve the
4 KiB/write durability contract and steady-state operation through metadata outages. Failures
of startup admission are an intentional deployment compatibility change; there is no unsafe
production bypass. Proposed audit ADR 0007 remains Proposed until separately accepted.

## Tests

- **S1, S6, S8:** startup/CLI fixture tests for explicit physical profile, absent/unknown/detected
  virtual provenance, mismatched timing, unsupported source, missing attestation and syscall
  errors. JSON output never labels rate or rollback safety as locally proven.
- **S2, S3:** injected clock unit/runtime tests for send-time expiry at L−1/L/L+1, ceil takeover
  wait, process pause, full/short suspend, uncertainty intervals, backwards/failure/overflow;
  all read paths and completion queues sample afresh without relying on a ticker.
- **S4:** engine/frontend/network tests inject faults before/after admission, store completion,
  final success permit and buffered output delivery. No new success or session crosses the
  fault generation; pre-fault permitted operations may deliver later. Buffer pins are released
  only after I/O ends; no durable data/epoch is erased.
- **S5:** memory-store/runtime restart tests require a greater metadata epoch, block on metadata
  outage/changed ownership and reject old callback/token/session reuse through recovery.
- **S7:** deterministic simulator scenarios for each event class, checker assertions and a
  missing-invalidation mutation; existing no-lease/no-wait/fast-read mutations stay caught.

## Open questions

- The physical-only profile, fixed initial timing and operator-attestation policy require review;
  qualifying virtual environments needs an enforceable hypervisor clock/rollback contract.
  Settled at approval, 2026-11-13: physical-boottime-v1 with fixed L/rho and operator attestation; virtual hosts stay refused (see decisions.md, Settled at approval).
- Local probes cannot establish the relative-rate bound or detect coordinated memory/disk/time
  rollback. Hardware qualification and administrative exclusion remain deployment obligations.
  Settled at approval, 2026-11-13: they stay deployment obligations under operator attestation (see decisions.md, Settled at approval).
