# Issue #65: Measure transport cost before RDMA — design

[spec](spec.md) · [decisions](decisions.md)

Approved 2026-11-13; not implemented.
Review corrections of 2026-11-20 are proposed and unapproved (decisions.md).
Measurement increment; no transport ADR or runtime switch. Workload is synthetic unless
explicitly identified as an authorized durable RF2 campaign. Measurements have not been run.

## The facts it starts from

`flaminio-net/src/session.rs:43` Conn wraps generic AsyncRead/AsyncWrite; send encodes and
flushes one frame, recv allocates its bounded body (`:59`, `:81`). TLS accept/connect bind
peer identities to certificates before token/session admission. `frame.rs:14` bounds requests
to 1 MiB. `flaminio-node/src/net.rs:103` runs one executor thread and unbounded per-session
frame channels. No RDMA dependency or alternate endpoint discovery exists. Lab 1 GbE results
measure durable storage+overlay, not isolated TCP/TLS cost. kTLS is not implemented despite
architecture future text. Existing `make bench-writes` reformats benchmark disks and takes hours.

## Local harness and variants (S1–S3)

Add `flaminio-bench/examples/transport-cost.rs`, compiled only as a named example with dev
dependencies. It imports production Conn/Frame/TLS helpers. It has no attach/store access,
no production CA/key flags and no reusable unauthenticated server entry point. The example
creates its own ephemeral loopback socket pairs, ephemeral test CA/node certificates and
synthetic frame payloads. Insecure variants may use only those internally created pairs.
A benchmark-only reply loop validates request id, extent, payload pattern and checksum then
sends a synthetic ACK; labels never call it a durable replica ACK.

Four modes: tcp-tls, tcp-plain, memory-tls, memory-plain. Memory streams are bounded byte
pipes implementing the same AsyncRead/Write and production encode/decode; no direct Frame
channel shortcut. TLS runs through normal peer certificate/session checks with ephemeral
identity/token. Plain modes use Conn only and explicitly bypass session admission within the
example; that bypass is never exported by a product helper or feature. No plain listener is
added to flaminio-node. TLS peers must still pass wrong-peer/token refusal fixtures.

Each mode uses one sender/receiver pair with bounded outstanding IDs. 4K/QD1,4K/QD32,1M/QD8
use identical frame payloads, ACK parsing, stream split and flush policy. Match executor/thread
placement and capacities between pairs; report where kernel and memory scheduling differ.
The optional actual-Net handoff fixture is omitted: current `Net::spawn` returns no thread
join handle. Production handoffs are observed in the separately authorized RF2 profile; local
frame trials do not measure them or subtract them as a kernel/TLS component.

Validate CLI before allocating: `--mode <one|all> --size 4096|1048576 --depth 1..256
--warmup-seconds 0..60 --seconds 1..300 --reps 1..10 --output <new-directory> --seed <u64>`.
Defaults all,4096,1,3,15,5. Max 2 connections; max outstanding payload memory 256 MiB per
endpoint plus TLS/framing overhead documented in manifest. Memory pipe capacity ≤2MiB per
direction, bounded ACK queue/deadline; no unbounded channel from the benchmark scaffold.
Refuse output symlinks/nonempty directory, no overwrite. Timeout or mismatch cancels, closes
streams and joins tasks before exit; mark trial invalid, never include it in throughput mean.

## Samples and interpretation (S4, S6)

Use monotonic wall time and endpoint thread CPU clocks; keep the endpoint/helper TID-role map
and process start identities. Endpoint CPU ns/op = measured endpoint-thread CPU / validated
completions; its endpoint sum is explicitly partial, excluding kernel work on other threads.
Record helper-thread CPU separately where attributable, without counting any TID twice. No
cycle conversion from advertised CPU frequency. Linux perf task-clock/cycles and sample stacks
are optional capabilities, with availability/errors recorded. Also request a bounded
system-wide observation on the recorded online CPU set for each local measured interval and
matched idle interval; retain CPU set, time bounds, lost samples, softirq/ksoftirqd stacks and
background activity. Do not change affinity, isolation or privileges silently. System CPU
already includes endpoint/helper CPU: keep these views separate, never add them. The local
summary reports endpoint-only paired CPU differences and, when available, a separately labeled
host-CPU incremental estimate with its idle/background uncertainty. Neither is called a total
transport saving: unattributable off-thread work, missing CPUs/counters or noisy background
leaves kernel/transport-total CPU unknown. Missing system profiling does not invalidate an
otherwise valid endpoint-only result. Run low-rate
sampling profile paired with an unprofiled repeat; if overhead exceeds 5% or variance masks
it, flag attribution inconclusive. Record warmup separately, avoid counting TLS handshake in
steady-state; provide separate handshake wall/CPU if measured.

Keep raw per-trial counters and QD1 bounded histogram with declared units/buckets. CPU role
categories are disjoint sampled stack classifications: TLS, kernel TCP, framing/copy/alloc,
handoffs/scheduling, other, unknown; retain classification rules and raw perf data. A memcpy
sample is not proof of an exact byte count. Missing kernel symbols or permission does not
make kernel CPU zero. Wall latency buckets do not become additive CPU attribution.

Pair modes in randomized/interleaved order using recorded seed; report individual runs,
median and spread, not just best result. A matched difference estimates this host/workload's
counterfactual only. TLS/TCP/memory/copy changes interact; never sum differences into an RDMA
prediction. Compare p50/p99 latency only on sufficient samples; QD32/8 throughput is not
single-I/O latency saving. A1GbE durable campaign cannot validate +30µs fast-network targets.

## RF2 profile and inventory (S5, S7)

Extend the existing campaign with opt-in per-job profile hooks, not one attachment around
the campaign. `write_reset` stops the daemon and `write_volume` starts it again
(`lab.sh:1857-1886`); measured fresh/steady calls are at `:2060` and `:2074`. After setup,
immediately before each selected fio invocation, reacquire target and peer PID/start identity
and TID-role maps, attach owned bounded profilers and wait for readiness. Keep identities under
observation until detach; restart/PID reuse or failed readiness invalidates that window.

A window key is campaign/repetition/side/job/phase/round/attempt; a resume creates a new attempt
and never splices old profiles into new completion counts. Guest hooks bracket the fio
measurement after profiler readiness, retain per-host monotonic start/end and attach/detach
margins, and stop profiling before whole-disk writes, preconditioning, idle, integrity or the
next reset. The peer profile brackets the same invocation using acknowledged control markers;
retain each host's own bounds and marker uncertainty, without subtracting unsynchronized clocks.
Use only the matching retained fio JSON, summing successful completed operations/bytes across
all jobs/directions (group-reported aggregates counted once), recording direction-specific
denominators. Nonzero fio errors or zero valid completions invalidate CPU/op. No rate times
duration substitution. Missing/interrupted output, process changes or uncertain bounds
invalidate CPU/op for that window; do not contaminate another window's numerator/denominator.
Any included launch/stop margin CPU stays labeled as part of the observation, not pure I/O cost.

Save perf task-clock/stat and samples for both nodes, link throughput/RTT, raw/RF1 baselines,
cgroup/host background CPU and exact settings. Preserve reset, integrity and restoration
semantics; integrity failure invalidates its parent run. Traps detach only profiler PIDs owned
by this attempt and await them before the next job. Do not drop caches, alter sysctls, isolate
CPUs, kill services or change NIC settings silently. Failed collection can leave valid
throughput data but makes that window's CPU attribution unknown. Measure profile overhead
separately; the local endpoint/system accounting distinction also applies to RF2.

Inventory phase first is read-only: on each physical host and guest collect kernel, PCI NICs,
`/sys/class/infiniband`, `rdma link show`, `modinfo rdma_rxe siw` and interface topology if the
tools exist. No RDMA entry proves only no currently exposed device, not all hardware incapable.
Loading a module, creating RXE/SIW links and connectivity tests are a separate authorized
phase, with temporary names and exact cleanup. No RDMA-perf claim from software emulation.
Record uninspected paths unknown, no invented fl-a↔fl-b connectivity through WireGuard.

## Contracts

Proposed `make transport-cost` runs the local example through Cargo release; no lab access.
`make transport-profile` delegates through lab.sh to the opt-in authorized campaign wrapper;
its Make help states formatting/duration risks. `make rdma-inventory` is read-only lab/host
inspection; explicit setup/test flags require the separate capability phase above.
Output directory: manifest.json, trials.jsonl, summary.json, tid-map.json and optional perf
files, all schema_version1. Manifest includes source/dirty diff hash, toolchain, host profile,
mode settings, topology and synthetic=true. Each trial: mode,size,depth,repetition,seed,
completed,bytes,wall_ns,cpu_ns_by_role,latency_histogram,valid,errors,profile_overhead/unknown.
Each CPU observation also records scope (endpoint/helper/system), covered CPUs/TIDs, interval,
availability/loss and unknown off-thread coverage; views are never summed across overlap. RF2
window records add the window key, process start identities, profiler readiness, per-host
markers/margins, retained fio JSON path and per-direction completed counts/bytes.
Summary preserves invalid counts, dispersion, paired estimates and qualification warnings;
endpoint-only and host-CPU estimates stay distinct, transport-total saving may be unknown.
No certificate private keys, tokens, payloads or production credentials in output.
Exit0 valid requested trials,1 runtime/integrity/inconclusive required measurement,2 bad args.
Settled at approval, 2026-11-13: these commands and this schema as proposed (see decisions.md,
Settled at approval).

## Files touched

- `dataplane/crates/flaminio-bench/{Cargo.toml,examples/transport-cost.rs}`: S1–S4 harness.
- `scripts/tools/transport-cost-test.sh`, analysis fixture/tool: S1–S4/S6 schema/analysis checks.
- `scripts/checks/transport-profile.sh`, `scripts/guest/transport-profile.sh`: S5 opt-in wrapper.
- `scripts/lab.sh`, Makefile: explicit local/profile/inventory dispatch, S5/S7.
- `docs/benchmarks/`, performance plan, lab and followups: S6–S8 artifacts and scope.
No product session/auth/frame behavior changes; if a reusable test helper is needed keep it
in test/dev code, never add an insecure production configuration.

## Tests

| Rules | Planned tests |
|---|---|
| S1–S3 | tiny four-mode runs validate payload/ACK; max limits, depth cap, malformed reply, timeout, no bind/credential flags, output ownership, memory backpressure |
| S3 | production mutual-TLS identity/token refusal tests unchanged; scanner/build check no benchmark mode in daemon help |
| S4/S6 | fixture CPU/time units and successful-op denominator; nonzero off-thread work; unavailable system profiling; overlapping endpoint/system counters; low/lost samples, missing symbols, high overhead, negative/noisy paired differences |
| S5 | fake campaign changes PID between resets, repeats fresh/steady windows, injects precondition/integrity CPU, failed attach/readiness, partial/resumed/multijob fio and marker loss; no window cross-pairing; integrity failure invalidates run; cleanup awaits owned profilers and cannot kill unrelated processes |
| S7 | inventory fixture with absent tool/module/device and failed inspection; no mutation commands in read-only mode; explicit temporary link cleanup |
| S8 | artifact/schema validation and clear synthetic-vs-durable report labels |

Use `make lint test race hygiene contracts`; local microbenchmark smoke adds no raw-device or
performance threshold to CI. Never assert speed from wall-time unit tests. Existing simulator
and transport refusal tests remain unchanged. No future implementation checks run here.

## Lab

Only after authorization: opt-in RF2 campaign for stated jobs with existing reset/restore,
retaining both peer and target profiles. It destroys benchmark disk data according to existing
campaign preflight; no new automatic format. Inventory alone changes nothing. Software RDMA
setup cleanup removes only created links/modules when safely unused; it never changes live
lab routes. Real RDMA hardware measurements are later #65 work.

## Docs, risks and ADRs

Commit raw artifact paths and conclusions in a dated benchmark report; update performance-plan
open questions and lab hardware capability table. Amend issue premise only after actual data,
with an additive evidence section rather than rewriting requirement history. No performance
result exists yet. Symbol visibility, profiler overhead and loopback scheduling bias can make
results inconclusive. ADR0001/0003 consistent: benchmark-only Rust, no new runtime requirement;
ADR0005 untouched. No RDMA/security/library ADR is written before its required evidence.

## Parallelizable?

Yes: local harness/analysis and authorized profile/inventory scripts can use the explicit
result schema independently. Interpret findings together before proposing any transport.
