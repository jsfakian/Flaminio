# Issue #65: Measure transport cost before RDMA — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Review corrections of 2026-11-20 are proposed and unapproved (decisions.md).
Measurement increment before the issue's transport ADR. No RDMA, kTLS, authentication
change or product transport switch. #65 remains open through its actual hardware/refusal tests.

## Terms

- **Production baseline:** existing framed mutual-TLS TCP path and handoff architecture.
- **Synthetic ACK:** benchmark response after frame validation; it does not claim durable storage.
- **Transport-only trial:** local, bounded synthetic request/reply without a replica store.
- **Counterfactual:** a matched benchmark variant omitting encryption or kernel networking;
  a measured estimate, not a proof of what RDMA/kTLS would achieve.
- **CPU per I/O:** measured CPU consumption divided by completed successful operations, for
  both endpoints separately and their sum, labeled with the observed CPU boundary. Endpoint
  thread CPU excludes off-thread work and is not total transport CPU. Wall latency is not
  summed across CPUs.

## The invariant

Measurement cannot weaken production transport authentication or durability. Every result
identifies what ran, counts only validated completions and distinguishes synthetic ACKs from
durable writes. Unavailable evidence is unknown, never zero or an inferred performance win.

## Behaviour

**S1. Reproducibility.** Retain source/build identity, host/kernel/CPU, workload, sizes, depth,
TLS/cipher configuration, topology, affinity, warmup/runtime, repetitions and profiling mode.
Use 4 KiB QD1, 4 KiB QD32 and 1 MiB QD8, with reproducible payloads and completion validation.

**S2. Local comparison.** On one machine compare the current TCP/TLS frame path, plain loopback
TCP, bounded memory byte streams with TLS and without TLS. Retain frame parsing/copies and
identical payload/ACK semantics. No disk or lab service participates. The no-kernel variants
are synthetic user-space streams, not an RDMA implementation.

**S3. Safety and bounds.** Insecure variants accept only internally created loopback peers or
memory streams. They cannot bind a configured address, access production credentials, join a
real replica session or become a daemon option. Bound connections, payloads, queues, runtime
and output. Invalid/truncated/mismatched replies fail the trial and invalidate its rate.

**S4. Attribution.** Record CPU per endpoint/thread, throughput and QD1 latency distributions;
attribute samples to TLS, kernel TCP, framing/copies/allocation, handoffs and unknown. Preserve
raw profiles and thread identities, including helper threads. Retain a separate bounded
system/CPU-scoped observation of kernel and background work for local trials when available.
Endpoint-only CPU and system observations are overlapping views, never additive. Missing or
unattributable off-thread work makes kernel/transport-total saving unknown, even when the
endpoint-only comparison is valid. Account for profiling overhead and shared/idle CPU;
unsupported counters or insufficient samples make that attribution inconclusive.

**S5. Production context.** Separately profile both target and peer during an authorized RF2
write campaign, with the same-session RF1/raw and link evidence required by the performance
plan. Each selected fresh/steady fio invocation has its own matching profile/completion window,
reacquiring target/peer process identities after resets; interrupted windows are invalid.
Include per-thread CPU and integrity results. Synthetic ACK timing is never substituted
for durable RF2 timing, nor extrapolated from the current slow network to fast NICs.

**S6. Honest estimates.** Report paired variant differences and uncertainty without adding
non-independent costs or calling observed savings rigorous upper bounds. Different buffering,
scheduling and offload interactions can change them. State which costs remain in every variant.
No claim of RDMA or kTLS benefit without their real implementation/hardware measurement.

**S7. Capability inventory.** Record whether each physical host has an RDMA device, whether
the guests ship/load software RDMA modules, and which guest paths can establish a test
connection. Unavailable/unauthorized inspection remains unknown. Loading modules or configuring
links requires explicit lab authorization; inventory never reconfigures production networks.

**S8. Verdict.** Commit benchmark artifact references and analysis; update performance plan and
lab docs. Revise the issue's unmeasured premise with actual evidence and remaining uncertainty.
List the decisions still needed for transport security, unsafe FFI/library choice, hardware and
fallback. No roadmap completion or ADR acceptance is implied by a measurement.

## Tests

- S1–S3: benchmark parser/limits and each variant on a small local run; corrupt/short ACK,
  outstanding cap, invalid args, no arbitrary address or credential path, normal TLS refusal.
- S4/S6: fixture analysis with missing counters, low samples, profiling overhead and variance;
  validate units/denominators and paired estimates, off-thread CPU and unavailable system
  observations, no double-counted endpoint/system CPU or endpoint wall time.
- S5: fake campaign reset/PID changes, per-job windows, partial/resumed jobs and failed attach;
  authorized RF2 paired profile campaign, clean data verification, retained raw outputs.
- S7–S8: read-only inventory fixtures and documentation/result schema checks.

## Open questions

Hardware/profile access and later security/library choices remain open. Existing intent's
phrase "bound" means the counterfactual estimates above, not a certified RDMA/kTLS saving.
Measurements may show the work is premature; inconclusive is a valid outcome.

Settled at approval, 2026-11-13: "bound" means the counterfactual estimates; premature or
inconclusive is a valid verdict (see decisions.md, Settled at approval).

Settled 2026-11-13 (user's instruction): hardware and profile access by the plan's procedure
(read-only inventory first, the RF2 profile and any RDMA setup only when authorized); the later
transport security and library choices in ADRs after the measurements, none in this step (see
decisions.md).
