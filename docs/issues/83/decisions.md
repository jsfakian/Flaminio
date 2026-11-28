# Issue #83: draft review choices

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`4f18a1e256d6aec5ac17f1eb497bd7fb12bed18ededeb1b3f038a64a1d4f9997`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Implementation progress and evidence are recorded in [report.md](report.md).

## Settled at approval

- **Clock profile, fixed timing and attestation** ([spec](spec.md), Open questions, first item;
  [ADR proposal](adr-proposal.md), Decision; [design](design.md), Profile admission and Elapsed
  clock and observer): the Proposed profile, physical-boottime-v1. Qualified physical Linux hosts
  with `CLOCK_BOOTTIME` as elapsed authority; fixed cluster-wide L = 5 s and rho = 100,000 ppm;
  per-read error and resolution of at most 1 us, E = 3 us, and the takeover wait
  `ceil((L+E)*(1+rho))+E` (5,500,007 us at defaults); explicit operator attestation in the clock
  contract file, with no bypass. Virtual or unknown-provenance hosts, current lab VMs included,
  are refused until a separately qualified profile with an enforceable hypervisor clock and
  rollback contract exists. Quarantine stays sticky until operator restart. The
  [ADR proposal](adr-proposal.md) is accepted with the plan.
- **What local probes cannot prove** ([spec](spec.md), Open questions, second item;
  [design](design.md), Docs, risks and ADRs): as the plan states, the relative-rate bound and the
  absence of coordinated memory, disk or time rollback are not established by software. Hardware
  qualification and administrative exclusion of rollback stay deployment obligations under the
  operator's attestation.
- **Fresh-process handoff** ([fresh-process contract](fresh-process-contract.md), the whole
  Proposed contract; [design](design.md), Quarantine and completion ordering): accepted as
  written. An agent or administrator Attach under `only_if_target`, bound by the signed `FLCH`
  envelope to a 32-byte daemon challenge; node and io roles cannot mint it. A daemon restart
  during private initialization stays blocked for quiesced manual recovery.

## Choices

- **Review83-R1-C1:** existing agent/admin role issues new-epoch Attach, with signed FLCH binding
  its exact FLCT token digest to a32-byte unpredictable daemon challenge. Every target-start
  entry enforces it; no new permission for node/io. #15 u64 INSTANCE is not a nonce. #79 FLCI
  purpose remains separate; private initialization cannot restart under a new process at its
  old reserved epoch. Keep uncertain work/manual recovery rather than reopen format authority.
- **Review83-R1-C2:** qualify absolute read error/resolution<=1 us, elapsed-difference E=3 us,
  conservative wait ceil((L+E)*(1+rho))+E. Wider suspend-detection tolerance is not clock accuracy.
  Add independent worst-error phase tests and a caught ignore_observation_error mutation.
- **Review83-R1-N1:** supply this unapproved choices record for the existing links.

- **Review83-R2-N1:** retain the complete clock-error test description on its Markdown table row.

## Implementation integration — 2026-11-28

Start from main after merged #78 / PR #106. Keep #76 immutable creation, #77 coherent
Attach views, #78 placement gates, #80 admitted geometry/normal gRPC budgets and #86 uncertainty
bounds. New nonce validation precedes storage effects and handoff issuance follows an actual
new-epoch commit. Never enlarge metadata/wire limits to accommodate the envelope.

The approved physical-only profile, fixed timing, old-token refusal, sticky quarantine and
operator attestation are binding. #15/#79 unimplemented additions remain separate; implement
current target-entry and authorized-caller paths strictly and report any true prerequisite
before introducing a bypass or silently weakening an accepted contract.

### U2 legacy issuance boundary

Existing legacy Attach RPC behavior remains: an actual new-epoch CAS may return a FLCH binding
its valid legacy FLCT bytes. The strict daemon verifier refuses FirstEpoch 0 before allocation
or Hello. The appendix forbids admitting that target lifetime; it does not add a metadata
migration or separate pre-CAS legacy issuer policy. Tests must cover both RPC compatibility
and strict start refusal. No legacy header is rewritten to fabricate a FirstEpoch.

### U2 epoch exhaustion

The existing Attach increment can wrap at uint64 maximum. The new-epoch handoff contract
requires refusal at that shared mutation boundary: apply one pre-CAS FailedPrecondition guard
for all epoch-advancing Attach calls, with or without a nonce. A wrapping epoch is not valid
legacy behavior; retaining it on the empty-nonce path would preserve the same safety defect.
Read-only legacy responses and ordinary valid Attach semantics remain unchanged. No repair or
epoch reset is introduced. Acceptance must assert no mutation/handoff on exhausted increments;
the executable issuance model must refuse a mint without strict epoch advance.

### U2 existing metadata view

The existing Volume protobuf does not expose FirstEpoch. Do not invent another field: the
strict handoff verifier validates the signed FLCT claim and its exact capability digest,
then U3 matches volume/node/current epoch against the coherent metadata view. Landed #76
birth/epoch fencing supplies incarnation uniqueness. General FLCT v2 validation remains
0 < FirstEpoch < Epoch; strict FLCH additionally refuses v1's FirstEpoch 0.

### U3 baseline integration

The IO crate has no existing JSON parser despite the design's dependency estimate. Use direct
serde/serde_json declarations at already locked versions for bounded strict clock-contract and
inspection JSON; do not write a bespoke parser. Record actual manifest/lock changes and keep
unknown/duplicate/trailing fields and oversized inputs rejected.

The appendix's described ran_epochs guard is absent in the current baseline. Add the required
process-local highest-started-epoch guard after proof/domain/current-metadata/geometry checks,
before target session effects. Invalid geometry or credentials must not poison this history.
An exact existing live-lifetime retry may return it; stopped/older lifetimes need a fresh epoch.

The old standalone replicated serve mode has no authorized handoff and must refuse before
prepare/device effects. All live networked storage roles need enforced profile/domain or an
explicit refusal; assess offline inspection/digest/store tools as non-authorizing operations.

### U3 concrete profile inputs

The strict JSON contract keys are version/profile/node/host_kind/clocksource/lease_us/drift_ppm/
rate_bound_attested/observation_error_ns/no_live_rollback. The read-only clock-check validates
the file node's grammar; daemon startup additionally matches its configured node identity.
The elapsed source reported by status is CLOCK_BOOTTIME, distinct from the kernel clocksource.

Container provenance uses the fixed read-only /run/flaminio/host prefix with the host proc/sys
hierarchy. Validate the actual opened input files' filesystem read-only state, including nested
mounts, rather than inferring it from the parent mount. Direct-host inputs are proc CPU flags,
DMI vendor/product, hypervisor type and the current kernel clocksource. These observe required
prerequisites, not future rate accuracy, absence of concealed virtualization, or rollback safety.

### U3 syscall isolation and validation

The IO crate forbids unsafe code. Put the minimal synchronous clock_gettime/clock_getres and
read-only-filesystem syscall wrappers behind a safe Linux-gated flaminio-uring interface, with
precise SAFETY arguments and checked outputs, instead of widening IO's unsafe permission.
Profile parsing/backend policy stays in IO and its library exposes the pure bounded decoder
for fuzzing. The wrappers always compile on Linux. Add flaminio-io to the sanitizer crate set
so ASAN/TSAN exercise frontend/profile tests as well as uring's syscall boundary. Do not claim
aligned-only Miri covers these syscalls. Async completion/pin lifetime validation is mandatory.

### U3 retries and queued metadata

Control attach may replay the exact current-process capability/proof of an already-live
lifetime. A new-epoch proof never returns an older target as its result. U4 retains issued
credentials across socket/busy retries; the existing higher-epoch metadata refresh/depose and
kill/drain route can retire the old lifetime before retrying the retained proof. No automatic
status-based unconditional detach is authorized by credential loss; ambiguous cleanup requires
an exact lifecycle mechanism or quiesced operator action, not inference from a device path.

Metadata endpoint fallback runs independently of Engine. Add guarded production worker
admission with persistent observers before each queued attempt/retry, while preserving tools'
existing unguarded API. Late RPC results cannot reactivate a quarantined Engine. ReplicaCore
clock invalidation revokes volatile sessions without clearing durable fences, #86 uncertainty,
pending-write reservations/retirement evidence or actual in-flight I/O ownership.

### U4 authorized caller integration

Remove CSI scalar-status adoption and old control-syntax fallback that would bypass strict
process proof. One challenge/authorized Attach result is retained across socket/busy retries;
old-server or missing-handoff responses refuse before daemon effects.

Caller startup preflight uses operator-supplied /etc/flaminio/clock-contract.json by default;
CLOCK_CONTRACT may select another file path, never bypass validation or fabricate attestation.
Run it before node.sh/agent.sh implicit formatting/claim effects. Direct lab/reconcile/resync
attach paths must use the same handshake and bounded encoded operands. Metactl's nonce option
is global --target-nonce with 64 lowercase hex characters before attach; its protojson token and
targetHandoff fields remain standard base64. TOKEN keeps its existing standard-base64 control
encoding; only HANDOFF is converted to canonical unpadded base64url (179 characters). Final implementation checks determine exact usage text.

### U5 model preparation

The pure admission state-word helpers currently live only in the node crate. Extract those
helpers to repl::clock so simulation and runtime share encoding/decoding/closed construction
and generation acceptance, while node keeps its atomic CAS order and behavior. This avoids a
repl-to-node dependency or a second invented clock state machine. Expected fault/permit and
authorized-issuance facts remain independently derived from ideal event/history, never from
the shared admission predicate. U5 source starts only after U4's checked writer handoff.

### Independent acceptance testability boundary

Existing profile qualification hardwires global probes and frontend EngineCtx/UblkQueue types;
the approved injected-clock/fake-CQE acceptance cannot be established by lookalike test logic.
After U5, authorize a narrow behavior-preserving private evidence/policy split and private
backend/queue/completion interfaces reused by production and MemDevice/deferred tests. Real
production constructors still use fixed probes and UringDevice, without test flags/environment
admission routes. Exact-live retry validation must invoke actual Engine::attachment_permit;
no raw proof constructor or cached-time authorization is exposed. Native syscall-output helpers
permit checked malformed/short/error tests. Review these seams with the tests, and report any
actual behavior failure before making a semantic fix. No hardware qualification follows.

### Native acceptance invocation

`make clock-interop` runs the new authenticated Go issuer/Rust control-engine bridge via
scripts/dev/clock-handoff-interop.sh. Contracts and workstation check depend on it, so the
ignored native fixture test cannot silently disappear from required validation. The bridge
supplies only FLAMINIO_CLOCK_INTEROP_DIR to its test binary; no production admission bypass.
All shared fixture steps are checked with actual header/epoch/owner snapshots. The aggregate
oracle uses the exact parsed fixture count (15 cases / 73 actions), not a guessed threshold.

### Acceptance validation and dependency graph

Independent required acceptance passed without a reproduced production behavior defect.
Ordinary Rust checks run from dataplane under its pinned 1.99.0, not root's unrelated nightly
default; explicit nightly is used for fuzz/Miri/sanitizers. Do not change user-global defaults
or unrelated frozen code for nightly-only lint differences.

The separate fuzz graph now includes the IO parser library. Add 108 package records: 103 reused
main-lock registry records/checksums and five local crates; align its two prior fuzz-only
equivalent/itoa versions to main's existing pins, with no checksum substitution or production
upgrade. Both locked graphs pass.
Thirteen new regression seeds are committed. Native/fake-CQE tests prove software boundaries,
not physical clock qualification or actual ublk/kernel execution; Miri covers aligned helpers,
not Linux clock/filesystem/entropy syscalls.

### Full-check fuzz policy alignment

The IO parser fuzz target now reaches the existing libublk → bitmaps dependency. Fuzz's policy
says it follows the main workspace but had no advisory exception because that dependency was
previously absent. Mirror main's existing RUSTSEC-2026-0247 entry and identical reason in the
fuzz workspace; this applies the already documented same-dependency policy, not a new waiver
or package upgrade. The first full run passed lint/test/race and exposed this mismatch in
hygiene. Preserve the 4,097-byte oversized corpus boundary using non-whitespace bytes so the
normal diff whitespace gate remains useful.

### Code/security round 1 corrections

R1-B1/B2 require explicit error completion even before operation registration and transition
from healthy settling into the existing quarantine drain when a fresh sample first faults.
Completion ownership and actual CQE lifetime remain binding; a detected fault must leave
inspection available without a debug assertion panic.

R1-A1/A2 and R1-C1/C2/C3 require meaningful negative controls through production input-reading,
target construction, destructive claim, session cancellation and textual control handling.
Private test seams may supply opened-input filesystem facts; production still inspects each
actual descriptor. Idle-session tests keep the network alive while verifying quarantine-driven
closure, and timeouts fail. These corrections grant no additional admission route.
The textual handoff decoder stays private to the binary: actual-handler tables cover bounds,
alphabet, padding, unused bits, legacy arity and signed unknown versions, including an omitted
canonical-bit guard that the test catches. The existing binary verifier fuzz target is retained;
this fix introduces no public parser API solely for fuzz access.

R1-D1 composes scheduling pauses with a separate host-suspend interval, so overlapping process
pauses cannot resume execution or MONOTONIC early. R1-D2 initializes a valid positive volume
birth epoch and propagates it into independent strict issuance/start checks requiring
`0 < incarnation < epoch`, matching the existing production verifier. Both correct model
fidelity; the supported/excluded event policy remains as approved.
