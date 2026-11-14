# Issue #83: A supported lease clock contract — design

[Spec](spec.md); [decisions](decisions.md), if any; [policy proposal](adr-proposal.md).
Approved 2026-11-13; not implemented.

The facts it starts from:
- `Engine::us` converts Instant since startup into u64 microseconds; turn/flush/run and the
  fast path share that origin (`flaminio-node/src/engine.rs:604,713,748,2127,2227`).
- `EngineCtx::pump_with` reuses one Instant across a batch before delivering outputs outside
  the engine lock (`flaminio-io/src/ublk.rs:582`). Both fast-read implementations sample again
  on completion (`:1590,1630`), and a 10 ms ticker is only on queue 0 (`:1178`).
- Core fast reads check lease/session/membership at admission and completion
  (`flaminio-repl/src/target.rs:650–739`). `depose` clears sessions/work (`:1124`);
  no production clock-fault transition exists. Timing defaults are L=5 s, rho=100,000 ppm
  (`types.rs:49–58`). Simulator Clock has a fixed rate and offset (`sim/mod.rs:290`).
- Existing net commands close outgoing sessions, while accepted connections have reply
  channels (`flaminio-node/src/net.rs:33–62`). Both directions need quarantine cancellation.

## Profile admission (S1, S6, S8)

Initial production profile is **physical-boottime-v1**, an explicit proposal. Require
`flaminio-io node --clock-contract PATH` before opening storage, registering or listening.
The root-owned, non-group/world-writable regular file is opened without symlink following:
`version=1`, profile name, node identity, `host_kind=physical`, qualified clocksource name,
`lease_us=5000000`, `drift_ppm=100000`, `rate_bound_attested=true`, `observation_error_ns=1000`, `no_live_rollback=true`.
Use a bounded JSON parser, reject unknown/missing fields and mismatched node/timing values.
The file is an operator assertion of qualified physical hardware, not a measured certificate.
Settled at approval, 2026-11-13: physical-boottime-v1 is accepted with the plan (see decisions.md, Settled at approval).

The Linux backend requires functioning CLOCK_BOOTTIME/CLOCK_MONOTONIC and readable host
clocksource/provenance inputs. Check available hypervisor indicators (CPU hypervisor flag,
DMI product/vendor and `/sys/hypervisor/type`); observed virtualization or conflicting/unknown
host provenance refuses. Containers require read-only host provenance and a contract for the
underlying physical node; container identity alone is insufficient. Unsupported probe platforms
refuse until an explicit detector/profile is implemented; no `--force`/environment bypass.
A trusted root/hypervisor can conceal virtualization or rollback; that is outside the threat
model and prohibited operationally. Local probes never prove the pairwise future rate bound.

All production nodes use this fixed initial L/rho. Refuse local overrides; qualification and
provisioning must establish the same profile cluster-wide. A future mixed-profile protocol
requires another design, not a permissive startup probe. Existing lab VMs are deliberately
unqualified; update their docs to explain refusal rather than supplying a false physical file.

## Elapsed clock and observer (S2, S3)

Add a pure `ClockGuard` in `flaminio-repl/src/clock.rs` and injected `ElapsedClock` interface in
`flaminio-node/src/clock.rs`. Linux implementation lives in `flaminio-io/src/clock.rs`, using
its existing libc dependency. Core Time remains u64 microseconds. Checked timespec conversion
and origin subtraction refuse negative, backwards or overflowing values; no wall-clock input.

An observer samples MONOTONIC M1, BOOTTIME B, MONOTONIC M2. Its current suspend-offset interval
is [B−M2, B−M1], computed in signed wide arithmetic. Startup stores a shared anchor interval
widened by **1 ms** for suspend-detection sampling uncertainty. Separately require both clocks
to report resolution<=1 us and qualified absolute observation error<=1 us, including clock
quantization at the read instant. Operator attestation covers this error as well as rate;
clock_getres alone does not certify accuracy. Reject larger/unknown error or resolution.
Disjoint later intervals, backwards B/M, syscall errors and conversion/generation overflow
fault. A wide sample interval may be inconclusive; it never proves absence of suspend.
Short/inconclusive suspend remains safe only under S1 because B includes its actual duration.
Long scheduler pauses merely widen intervals and advance B; they do not invalidate by timeout.
Common stopped/rolled-back clocks can evade all these checks and remain excluded.

Each existing execution thread has a small observer and the shared immutable anchor. The
process domain uses atomic generation/admission state; no new steady-state mutex or metadata
RPC is added. Per-thread sampling avoids treating reordered concurrent observations as clock
rollback. On any observer fault, atomic quarantine applies to every thread, not only its queue.

Replace Instant authorization in Engine with fresh clock samples inside handle/tick/drain,
fast admission/completion, local-read completion and reply processing. Drop public production
APIs that accept caller-cached authority time; tests inject a scripted backend. Send-time
timestamps remain conservative enqueue times; a delayed socket send never renews from receipt.
Retain Instant/io_uring relative timers for wakeup hints only, resampling B when awakened.
Validate timing arithmetic with checked u128 intermediates. The per-clock elapsed-difference error bound is E=3 us: two absolute read errors<=1 us
each plus <1 us conversion quantization. Require takeover wait
`ceil((L+E)*(1+rho))+E`, using checked integer u128 arithmetic; default is5,500,007 us.
If old elapsed can under-report by E, its lease lasts at most L+E ideal old-clock units;
pairwise rate maps that to (L+E)*(1+rho), and new elapsed can over-report by E. The wait
therefore cannot finish early. This is independent of the wider suspend-detection interval.

## Quarantine and completion ordering (S4, S5)

Use shared `ClockDomain` with an atomic state word: even identifies an open generation; CAS to
odd quarantines it permanently. Fault reasons are a bounded enum. Success admission performs
one atomic CAS/RMW after fresh observation; fault and final permits therefore have a total
order. A permit already ordered before quarantine may deliver later and linearize before it;
there is no promise to atomically cancel a kernel completion already authorized.

Stamp FastLocal, LocalRead, queued engine/client completions and network commands with the
clock generation. Every admission/completion observer first checks the domain, samples clocks,
then checks generation at final permit creation. This includes copy/zero-copy fast paths,
slow local/remote data, write acknowledgements and replies queued outside the engine lock.
After asynchronous I/O, discard stale buffers as success candidates; release pins/plans only
once their actual device-I/O lifetime ends. Quarantined client operations return an explicit
I/O error after safe cleanup, never successful bytes. Writes may have landed despite error.

First fault atomically closes admission before dispatching its triggering event, cancels new
network traffic, and queues engine cleanup. Add production `Target::invalidate_clock` that
uses existing depose cleanup without erasing durable epochs. Invalidate pending attaches and
metadata callbacks, clear unpermitted success outputs, close outgoing and accepted sessions,
and stop registration/heartbeats. A new process-wide net cancellation command handles both
directions; writers check domain/generation immediately before admission of each frame.
No clock failure authorizes a fence, ISR shrink, replica deletion or target transfer.

The daemon stays quarantined with read-only control status; it cannot automatically reopen
on a good sample. Reject attach, delete and other storage mutations; detach/stop may clean up.
An operator restarts after diagnosing the cause. Normal store replay runs; every former target
requires the [agent-owned fresh-process handoff](fresh-process-contract.md), binding a signed
new-epoch Attach result to a32-byte process challenge; node/io never calls Attach itself.
Changed ownership or unavailable metadata blocks recovery. Startup `--attach --token` must
not bypass this fresh-epoch restart step. Full ISR fencing/reconciliation and required waits
remain. Incoming sessions never resume from in-memory pre-fault state.

## Model and mutation (S7)

Also model bounded per-read error/phase independently of ideal clocks. Check takeover against
ideal clock expiry, and catch `ignore_observation_error` under worst opposing errors.
Extend simulator real-time events with ProcessPause (clock advances, no node scheduling),
HostSuspend (B advances, active M stops), ClockFault (bad sample), VirtualClockStop and
SnapshotRollback. Clock mapping becomes piecewise; resume schedules from current real time
rather than applying the old affine inverse. Do not rewind checker history/real time when a
virtual node's memory/time is restored. Persisted replica epochs follow ordinary crash replay
for supported events; disk rollback is an excluded scenario, not silently repaired.

Use the same pure guard/state transition as runtime. Checker asserts no success permit after
quarantine in that generation, rejects late old handles, and requires new epoch before recovery.
Preserve per-block history linearizability for supported events. Add `skip_clock_invalidation`
mutation: after a deterministic backwards/error observation, it incorrectly permits an old
fast-read handle; the clock-admission invariant must fail. A forward/BOOTTIME suspend alone
may already expire leases, so it is not a valid claim that this mutation must produce stale data.
Excluded virtual-stop/rollback traces demonstrate indistinguishability/profile rejection; they
are never counted as supported linearizability success. Keep existing lease/wait mutations.

## Files touched

- `flaminio-repl/src/{lib.rs,clock.rs,types.rs,target.rs}`, `src/sim/{mod.rs,check.rs,tests.rs}`: S2–S5/S7.
- `flaminio-node/src/{clock.rs,engine.rs,net.rs}`, `tests/engine.rs`: S2–S5, generation/cancellation.
- `flaminio-io/src/{clock.rs,node.rs,ublk.rs,control.rs,main.rs}`: S1–S6, backend/CLI/all queues.
- `flaminio-io` clock/frontend tests and existing net session tests: runtime conformance.
  Crate paths above are under `dataplane/crates/`; no new crate/dependency is needed.

## Contracts

- New CLI: `clock-check --clock-contract PATH --json`, read-only/no metadata contact; exit 0
  for prerequisites satisfied, 1 for refusal. `node` requires the same file. JSON fields:
  `profile`, `source`, `lease_us`, `drift_ppm`, `attestation`, `observed_prerequisites`,
  `excluded_events`, `decision`, `reason`; no field claims measured drift certification.
- Root-only control socket gains `clock-status`: one bounded JSON line with profile, source,
  `state=healthy|quarantined`, generation and reason. Quarantined mutations return
  `error: clock quarantined; operator restart required`; existing status syntax stays intact.
- ClockSample carries checked elapsed microseconds and generation; SuccessPermit is single-use.
  Fault reasons: `profile`, `sample_failed`, `backwards`, `suspend_offset`, `overflow`.
- Additive Attach fields and signed FLCH local handoff ABI are in the appendix; existing FLCT
  replica token/wire/on-disk formats and per-method roles remain unchanged. Recovery uses an
  agent/admin-authorized Attach with only_if_target; #79 initializer credentials compose with it. No metadata permission
  is inferred from a wall-clock timeout or local generation.

## Tests

| Rules | Concrete tests and level |
|---|---|
| S1, S6, S8 | `flaminio-io` clock-check fixtures: root/file modes/symlink/size/schema, missing/virtual/unknown host inputs, timing/source mismatch, absent attestation, clock failures; CLI refuses before device/listen/register. |
| S2, S3 | Pure clock guard tests: offset intervals/tolerance, backwards B/M, 1 ns–1 ms suspend, wide/inconclusive samples, process pause, overflow; core send expiry L−1/L/L+1 and guarded takeover boundary, checked extreme timing; unequal clock resolution/phase/error at admitted limits. Catch an `ignore_observation_error` wait mutation with an independently timed takeover checker. |
| S2, S4 | Engine and ublk driver tests with scripted clock and fake completion sink: every copy/zero-copy/slow-local/remote path; no ticker; pause between each sample, read, final permit and delivery; pre-permitted delivery allowed, later permits forbidden. |
| S4 | Node network tests: accepted/outgoing connections close, queued frames and metadata replies from old generation cannot reopen; held/pipelined device work preserves pins until completion and never produces post-fault success. |
| S5 | Real authenticated appendix handoff fixtures plus memory-store engine restart test: partial durable write, quarantine, restart/new epoch/full reconciliation; outage, changed target, old startup token and late completion each refuse; no disk epoch/data rollback. |
| S7 | `sim/tests.rs`: deterministic scheduling pause, physical suspend, malformed clock, undetectable virtual stop and snapshot restore as distinct cases; skip_clock_invalidation caught by admission checker, existing no-lease/no-wait/fast-read mutations still caught. |

Run `make test sim race contracts lint` for implementation; sanitizer quick run for changed
async lifetime handling. Fake clocks and loopback RPC/transport suffice here. No test was run
while drafting, and a green fixed-rate run does not qualify suspend or hypervisor behavior.

## Lab

None authorized. Do not suspend hosts, pause VMs or roll back shared disks. A future #49
campaign must use a qualified disposable host/profile, history checking and explicit lab
exclusion/restore procedures; current VMs cannot certify this proposed physical profile.
Settled at approval, 2026-11-13: the profile is accepted; current VMs still cannot certify it (see decisions.md, Settled at approval).

## Docs, risks and ADRs

Update architecture/protocol clock assumptions, README deployment prerequisites, lab VM
exclusion and restart runbook, followups #83, and #71's doctor contract. #75 must measure
clock-syscall overhead; do not weaken checks based on an unmeasured performance assumption.

**Open risks:** operator attestation and actual hardware rate qualification remain essential;
all clocks can be rolled back together undetectably. The profile can reject existing virtual
and container deployments. Quarantine sacrifices availability and may return I/O errors;
restart needs metadata quorum and may recover unanswered writes. No unsafe bypass is accepted.
Settled at approval, 2026-11-13: these risks are accepted with the plan; hardware qualification and rollback exclusion stay deployment obligations (see decisions.md, Settled at approval).

[Issue-local proposed ADR](adr-proposal.md) records this policy and conservative timing guard.
Settled at approval, 2026-11-13: that ADR is accepted with the plan (see decisions.md, Settled at approval).
Accepted ADRs 0001–0003/0006 remain consistent. Accepted 0005's immediate-read wording has a
known contradiction; proposed audit 0007 would supersede it, but is not accepted here. Keep
current stricter takeover gates and never implement that historical early-read shortcut.

## Parallelizable?

Partly: Linux probes/CLI and pure guard/simulator can proceed against ClockSample/domain
contracts. Engine, net and ublk generation/completion changes require one coordinated Rust unit.
The appendix requires coordinated Go issuer/CSI and Rust verifier integration; no production
clock qualification or approval is implied.
Settled at approval, 2026-11-13: the plan is approved; production clock qualification is still not implied (see decisions.md, Settled at approval).
