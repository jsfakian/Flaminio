# Issue #83 — report

## Review rounds

### Plan review round 1 — 9b112896122794e21e2c041c0991c533b0a7bf04

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c1 (design-code), r83s1 (design-security).

- **Review83-R1-C1 — High:** Fresh-process recovery lacks authorized enforceable new-epoch agent-to-daemon handoff Evidence: design.md98–103,142–144; node.rs309–313/auth.go66–70.
- **Review83-R1-C2 — Medium:** Admitted clock resolution1ms exceeds2us takeover quantization margin Evidence: design.md52,69–71.
- **Review83-R1-N1 — Nit:** Spec/design link missing decisions.md Evidence: spec.md3/design.md3.

Changes/resolution: Both reviewers independently confirmed all three. Added exact signed FLCH local handoff bound to current32-byte process nonce and exact replica-token digest, issued only by authorized new-epoch Attach; preserve node/io restrictions and enforce every target-start entry. Compose #79 init credentials with same-daemon-only recovery and refuse new-daemon old epochs. Qualified error/resolution<=1us, elapsed errorE3us and conservative waitceil((L+E)*(1+rho))+E; independent bound tests/caught mutants. Added unapproved choices. No implementation/runtime/lab tests or approvals inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 5a2fcd86c723b165c15ae0a1889929b81feaf204

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c2 (incremental design-code), r83s2 (incremental design-security).

- **Review83-R2-N1 — Nit:** Mutation test sentence split across Markdown table rows breaks mapping Evidence: design.md161–162.

Changes/resolution: Both fresh reviewers confirmed all three R1 findings resolved and reported only this Nit. Kept test/mutation description within one table row. FLCH134-byte binding and E3us/default5500007 bound independently checked; no implementation/runtime/lab test or approval inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — f9a0a519adf21d4ee2c9048f44139489446c94e7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c3 (incremental design-code), r83s3 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed the table-row Nit resolved and found no revision fallout, including Nits. Earlier R1 technical findings were confirmed resolved in R2. No implementation/runtime/lab tests or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.

### Code/security review round 1 — afb88d5c3c6ad5c9ab6bcd2aae5754ada6b3d2a6

Full non-generated change against merged main `f952596f1b32c719efd2c65ab7347c052fc54f56`,
split into the four [review partitions](units.md). Each partition received independent code
and security review by fresh high-effort agents. Built-in review skills were unavailable;
reviewers used the documented manual fallback. All eight completed before fixes began.

- **R1-B1 — High:** A fault at `Engine::submit`'s second admission sample returns before
  operation registration, losing the frontend's error completion and leaving its waiter blocked.
- **R1-B2 — Medium:** First detecting quarantine during `settle_local` clears local work without
  changing progress, reaching the healthy no-progress debug assertion.
- **R1-A1 — Medium:** Per-opened-input read-only provenance enforcement lacks negative coverage,
  including writable nested mounts and filesystem-query failure.
- **R1-A2 — Medium:** Checked takeover arithmetic lacks extreme result/intermediate overflow,
  representable-boundary and actual Target fail-closed tests required by the design.
- **R1-C1 — Medium:** The destructive agent claim path's preflight is absent from script fixtures.
- **R1-C2 — Medium:** Idle-session acceptance drops the sole network handle before checking
  closure, allowing normal shutdown to mask missing quarantine cancellation; timeout can count
  as closure.
- **R1-C3 — Medium:** Real control-handler tests omit malformed textual handoffs, legacy arity,
  bounds, canonical encoding and signed unknown versions before effects.
- **R1-D1 — Low:** Overlapping simulator pauses overwrite deadlines and mixed pause/suspend
  resumes clocks early.
- **R1-D2 — Low:** The integrated handoff model initially admits incarnation 1 / epoch 1,
  which the strict production verifier rejects.
- **R1-N1 — Nit:** The fuzz lock adds 108 package records: 103 registry records and five local
  crates, rather than 108 registry records.

The independent reviewers agreed on shared findings. No production profile, authorization or
signature bypass was reproduced. One fresh implementer addressed all nine findings: explicit
pre-registration error completion; quarantine drain during settling; opened-FD and extreme
timing tests; destructive-claim fixtures; live-network closure checks; actual-handler malformed
handoffs; composed simulator scheduling/suspend deadlines; and positive birth epochs with
strict incarnation ordering. Documentation wording is corrected here.

Both runtime regressions failed before their fixes; copied and registered frontend requests now
also prove termination. Nine omitted-guard controls were caught and restored, including the
properly scheduled same/mixed overlap cases. The first overlap fixture was initialized
incorrectly and its failure is excluded from evidence. Changed-package tests, scoped Clippy,
scripts and three consecutive native bridge runs pass. Full `make lint test race hygiene
contracts sim` and `make sanitize QUICK=1` pass on the restored source: 20,000 seeds and all
190 release tests pass. Status: fixes complete, awaiting a fresh incremental code/security round.

### Code/security review round 2 — 427a3a708ea3a6cb876b3a15849209a741596d3f

Fresh independent code and security reviewers inspected only
`afb88d5c3c6ad5c9ab6bcd2aae5754ada6b3d2a6..427a3a708ea3a6cb876b3a15849209a741596d3f`
and necessary callers/callees. Sonnet was unavailable; the code reviewer used the inherited
default model at medium effort. Security retained the default model at medium effort.

Both independently confirmed R1-B1/B2, A1/A2, C1/C2/C3, D1/D2 and Nit N1 fixed, and found
no new Low-or-higher findings or Nits. Pending I/O ownership, quarantine drain, private test
seams and strict issuance remain consistent with the frozen contracts. No edits or tests by
reviewers. Status: **clear after two implementation review rounds**; final conformance follows.

## Implementation — 2026-11-28

PR #106 / #78 is merged at `f952596f1b32c719efd2c65ab7347c052fc54f56`.
Proceed with #83 as the ready Phase-1 clock prerequisite for #79. No existing #83 branch/work
was found. The approved work is divided into [ordered units](units.md); preserve all landed
#76–#78/#80/#81/#86 fencing, snapshots, geometry and transport bounds.

### U1 Clock foundations

Implemented pure ClockGuard interval/conversion primitives, checked takeover arithmetic with
E=3 us (default 5,500,007 us), Target clock invalidation, injected node clock interfaces and
atomic domain/observer/sample/single-use permit foundations. Node engine's existing timing
fixture was updated for the new margin. Healthy generation 0 transitions permanently to
quarantined generation 1; reason/generation share the atomic word, and checked per-domain
identity prevents cross-domain sample reuse.

Focused Clippy, workspace compilation, formatting/whitespace and repl/node tests pass:
62 node unit tests, 20 integration tests, 143 normal replication tests (30 existing ignored).
The existing localhost integration fixtures needed escalation; no lab or clock hardware was
accessed. Acceptance, runtime wiring and executable clock/handoff models remain later units.

### U2 Signed process handoff

Additive Attach nonce/handoff fields, Go signing/verification and an opaque Rust VerifiedHandoff
are implemented. Nonce validation precedes reads/effects; signing follows an actual successful
new-epoch CAS and committed-view/FLCT validation. Legacy RPC compatibility remains distinct
from strict FirstEpoch-0 refusal. FLCT bytes, per-method roles, transport and disk formats are
unchanged. The shared Attach guard now refuses exhausted epoch increments before CAS.

Checks pass: proto generation, formatting, full lint, scoped vet, workspace all-target check,
14 net unit + 8 session tests, all three mTLS interop tests and Go captoken/service tests.
Exhaustion regression covers modern/legacy × nonce/no-nonce on memory and etcd with unchanged
header bytes/revisions; controlled guard omission causes epoch-zero mutations and is caught.
Real maximum Attach replies including FLCH are 4,110,389 / 4,122,410 / 4,130,344 / 4,135,490 /
4,138,460 bytes for RF1–5, inside unchanged B budgets and the normal 4 MiB limit.
Runtime enforcement, authorized callers, executable models and independent crypto/fuzz
acceptance remain later units. No lab runs or hardware clock qualification.

### U3 Runtime enforcement

Production profile/backend, clock sampling and sticky quarantine are integrated through engine,
network, control and copied/zero-copy/staged/pipelined frontend paths. Dispatch and final
success permits are distinct; handles, replies, commands and metadata endpoint retries are
generation guarded. Invalidation preserves physical fences, uncertainty and in-flight ownership.
Strict target starts require current-process proof and increasing started epochs. Exact live
proof replay can return its existing device; a new epoch stays busy until normal higher-epoch
observation/deposition retires the old lifetime. Status cannot authorize unconditional detach.

The process nonce uses Linux getrandom(flags=0) after profile admission and before storage,
registration or listening; failures/short/all-zero output refuse with no fallback. Safe Linux
clock/resolution/readonly-filesystem/entropy syscall wrappers live in uring; IO keeps its unsafe
prohibition and exposes the pure bounded decoder through its library for fuzzing. Direct serde
1.0.229/serde_json 1.0.154 declarations use already-locked versions. Legacy MVP serve/replica and
startup --attach/--token refuse; offline inspection/store tools remain administrative operations.

Checks pass: fmt/lint, workspace all-target compilation, whitespace, IO18, net15+8, node62+20,
repl143 (30 ignored), uring26, all three mTLS interop tests and CLI refusal smoke checks.
Logs: `/tmp/u3-final-lint.log`, `/tmp/u3-final-tests.log`, `/tmp/u3-meta-interop.log` and
`/tmp/u3-cli-smoke.json`. This is unit validation, not independent S1–S8 acceptance or hardware
qualification. Authorized callers, models, adversarial acceptance and final sanitizers follow.

### U4 Authorized callers

CSI now requires a healthy challenge and own-agent only-if-target nonce Attach, validates the
normal handoff and retains the pair across bounded socket/busy/subsequent kubelet retries.
Scalar-status adoption and old-syntax fallback are removed. Unstage clears retained credentials.
Metactl exposes global --target-nonce HEX before attach (64 lowercase characters); its token /
targetHandoff protojson fields stay standard base64. Socket TOKEN retains that encoding;
HANDOFF alone uses canonical unpadded base64url179.

Guest clock_preflight/attach_challenge/attach_target helpers cover production node startup,
claims, ensure-node, host volume/partition/benchmark attach, reconcile and resync. The host
administrator performs metadata-only placement; the guest obtains its own agent handoff.
Default contract path is /etc/flaminio/clock-contract.json (CLOCK_CONTRACT chooses another file,
never an attestation/bypass). Preflight precedes implicit format effects; legacy MVP up paths
refuse before prepare/format/systemd-run. Cleanup/status remain available. Offline control/agent
does not launch/register and is unchanged; CSI already uses the agent identity/external daemon.

Fmt/lint, scoped CSI/agent/CLI/metactl tests, shell syntax and whitespace pass. Logs:
`/tmp/u4-fmt.log`, `/tmp/u4-final-lint.log`, `/tmp/u4-final-go-tests.log`.
Older mounted devices or metadata outage may prevent retirement within the bounded wait;
callers return an error rather than adopt or automatically detach. Guest credentials last for
one invocation; CSI retains them in memory. Independent adversarial/script acceptance follows U5.

### U5 Executable clock and handoff models

Runtime and simulator share pure packed ClockAdmissionState helpers; node retains its SeqCst
quarantine and permit CAS order. Piecewise BOOTTIME/active clock mappings distinguish scheduling
pause, physical suspend, bad samples and excluded stop/rollback without rewinding ideal history.
The append-only independent journal checks permits/sessions after fault and single-use permitted
late delivery. Actual Target/ReplicaCore invalidation retains fences, landed/acknowledged data,
pending uncertainty and reservations through fresh authorized epoch/fence/reconciliation.

All four new controls are caught with clean counterparts: skip_clock_invalidation (three bad/
backwards/overflow cases; admission violation, not a stale-data claim), ignore_observation_error
(429 opposing-error/resolution/phase pairs over rho 0/1/100000), accept_old_process_handoff
(no local-fence seed), and mint_handoff_without_epoch_advance (caught at issuance even if a
later Start would refuse). Fifteen parsed schedules assert every step's epoch/owner/mutation
expectations; own-agent issuance requires only-if-target. Symbolic authentication does not
substitute for the native crypto/CAS acceptance that follows.

Eight pause/suspend histories pass linearizability or sticky quarantine; two excluded traces
show undetectable observations/profile refusal while ideal time advances. The partial-write
oracle preserves acknowledged value 11 and permits unanswered value 73 to remain old or landed,
with final per-block linearizability. Deterministic zero-client mode and the old test-only
no-takeover-wait injection were corrected for process-origin-zero clocks, retaining the original
split-brain witness rather than weakening it.

Fmt/lint, focused Clippy, node62+20, 100 clean seeds and all 183 release checks pass (173 original
plus ten new). Final strengthened recovery/error-phase cases and whitespace pass. Logs:
`/tmp/u5-sim.log`, `/tmp/u5-lint-escalated.log`, `/tmp/u5-node-tests-escalated.log`,
`/tmp/u5-partial-final.log` and `/tmp/u5-error-phase-final.log`. No hardware qualification or
native end-to-end admission claim follows from symbolic models.

## Tests

Independent acceptance passed on the frozen post-U5 source plus its approved private seams.
Acceptance did not reproduce a production semantic defect; the subsequent review found the
submission/settling error paths and coverage/model gaps recorded above. The complete required
project suite passed before review; fix validation and conformance remain later gates.

| Rules | Evidence |
|---|---|
| S1, S6 | Strict schema/profile/file/provenance fixtures, bounded native FIFO refusal, real CLI pre-effect refusal, status/quarantine checks |
| S2, S3 | Persistent observers, concurrent cancellation, sticky faults, overflow, exact L−1/L/L+1 completion checks |
| S4 | Actual copy/zero-copy/slow/staged completion paths, multiple CQEs, real store pins, held/pipelined writes, permitted Piece+Done retention |
| S5 | Real mTLS/CAS/crypto/control/Engine replay: 15 schedules, 73 actions; reconciled I/O, CLI, busy-proof retirement, roles, outage and restart refusals |
| S7 | 100 simulator seeds and all 186 controls passed; seven fuzz targets completed cleanly |
| S8 | CSI retained-proof retries/cache clearing, canonical encoding, script preflight ordering and legacy refusal |

Changed Rust/Go packages, ordinary and fuzz Clippy, locked graphs, regression replay, formatting,
shell/script checks, full ASan/TSan QUICK1 and final IO/node sanitizer deltas pass. Seven fuzz
targets ran for ten seconds each; thirteen new regression seeds cover the handoff/profile
boundaries. Miri's two aligned-buffer tests pass; this does not validate Linux syscalls.
Ordinary checks use the project-pinned Rust 1.99.0 from the dataplane working directory;
nightly is selected explicitly for fuzz/Miri/sanitizers. No user-global toolchain was changed.

The native bridge tests run actual mTLS Go Attach CAS/FLCT/FLCH and Rust control/Engine with
injected clocks and fake completion/publication sinks. They assert all 73 parsed actions,
header/epoch/owner/mutation outcomes, actual metactl, retained newer-proof busy/deposition retry,
reconciled read/write and steady I/O during real metadata outage. Incoming/outgoing idle/TLS/
pre-Hello/writer cancellation and queued/fallback/late metadata are also checked.

Approved private evidence/FileFacts, backend/queue/completion and live-lifetime interfaces are
shared with production; production constructors still use real probes and UringDevice. No
profile bypass, raw proof constructor or cached-time authorization was introduced. The fuzz
lock adds 108 package records (103 existing main registry records and five local crates), aligns
equivalent 1.0.3→1.0.2 and itoa 1.0.19→1.0.18
with existing main pins, and changes zero checksums. No production dependency upgrade.

Logs: `/tmp/83-accept-{native,frontend,held,packages,go-packages,sim,fuzz,miri,sanitize,asan-delta,tsan-delta,clippy-stable,fuzz-clippy,fuzz-regressions,profile,scripts}.log`.
Physical clock qualification and real ublk/kernel execution remain outside the approved fixture
scope. No lab, host suspend, VM pause or shared-disk rollback occurred.

Required project checks passed: `make lint test race`, followed by `make hygiene contracts sim`
after the existing fuzz-policy alignment. All three metadata TLS interoperability checks and
two native handoff checks pass; 20,000 simulation seeds and all 186 release controls pass.
Logs: `/tmp/flaminio-83-validation.log` and `/tmp/flaminio-83-validation-final.log`.
`make clock-interop` is
included in contracts/check, and its script harness is included in lint. Full quick sanitizers
include IO/frontend and Miri's limitation is recorded above. No hardware qualification is
claimed by local or model results.

Round-one fix validation also passed the complete required suite and quick ASan/TSan on the
restored tree. It includes 20,000 simulator seeds, all 190 release tests, actual copied/registered
frontend termination, per-opened-FD provenance refusal, extreme timing/Target refusal, actual
agent claim ordering, malformed control handling and live-network cancellation. Three native
bridge reruns passed two tests / 73 actions each before the full contracts rerun.

Logs: `/tmp/flaminio-83-r1-{validation,sanitize}.log`,
`/tmp/83-r1-{packages-restored,clippy-final}.log`,
`/tmp/83-r1-native-{restored,repeat2,repeat3}.log`,
`/tmp/83-r1-{submit,settle}-before.log`,
`/tmp/83-r1-control-{overlap,mixed-suspend,network-cancel-final}.log`.
The earlier incorrectly initialized overlap fixture is excluded; the correctly scheduled
omission controls above provide the before/after evidence. All temporary changes were restored.

## Conformance

Fresh read-only checker used the inherited default model at medium effort because Sonnet was
unavailable. Its result follows verbatim; ADR acceptance below follows this PASS gate.

PASS

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 Supported profile | yes | yes | `flaminio-io/src/clock.rs`: `fixed_profile_and_attestation_have_no_override`, `qualification_fixture_matrix_observes_only_prerequisites`, per-opened-input provenance tests. |
| S2 Fresh clock authority | yes | yes | `flaminio-io/src/ublk.rs`: `fast_completion_revalidates_exact_lease_boundary_without_a_ticker`; copied, registered, slow-local and pipelined completion tests. |
| S3 Detectable faults | yes | yes | `flaminio-node/src/clock.rs`: persistent-observer, concurrent-fault and overflow tests; `flaminio-repl/src/clock.rs`: checked conversion/offset guard tests. |
| S4 Invalidation ordering | yes | yes | `flaminio-io/src/ublk.rs`: CQE/pin ownership and pre-fault permit tests; `native_handoff_tests.rs`: live incoming/outgoing session cancellation; engine held-write tests. |
| S5 Restart recovery | yes | yes | `native_handoff_fixture`: 15 schedules/73 actions through actual mTLS/CAS/crypto/control/Engine; simulator partial-write recovery; metadata epoch-exhaustion regression. |
| S6 Inspection/enforcement | yes | yes | `clock_cli.rs`: `refused_startup_has_no_storage_or_socket_effects`; native fixture asserts quarantine status, generation, bounded reason and mutation refusal. |
| S7 Models/mutations | yes | yes | `sim/tests.rs`: supported/excluded clock histories and all four required clock/handoff mutants; final logs show 20,000 seeds and 190 release tests passing. |
| S8 Documentation/compatibility | yes | yes | README, architecture/protocol, lab runbook, doctor contract and final working `docs/followups.md` align; native metadata-outage I/O, CSI and script compatibility tests pass. |

Contract drift: none. Additive proto/generated fields, nonce32, FLCH134, canonical HANDOFF encoding, existing TOKEN encoding, CLI/control syntax, session cancellation and 5,500,007 µs takeover agree across participants. Replica FLCT, frames and disk formats remain unchanged.

ADR conformance: consistent. ADR 0020 exists and is indexed as Proposed, ready for the planned acceptance after this gate. ADR 0007 remains Proposed; accepted ADR 0005 is unchanged and its historical early-read shortcut is not introduced.

Scope drift: none. Supporting syscall wrappers, dependency declarations, fuzz corpus/lock alignment, required native bridge, private production-shared test seams and caller wiring are documented authorized extensions. No #15/#79 lifecycle APIs were invented.

Docs: done, including the final working followups update. Physical qualification and actual ublk/kernel execution remain explicitly outside fixture evidence; no lab qualification is claimed.

Open review findings: none. All nine deduplicated findings and Nit N1 are addressed; both fresh incremental reviewers independently confirmed clearance. Required project checks and quick ASan/TSan passed on frozen source `427a3a7`.

## Lab

None authorized or required by this plan. Do not suspend hosts, pause VMs or roll back disks.
Current unqualified lab VMs must be documented as refused by the accepted physical profile.

## ADRs

The physical-boottime-v1 and fresh-process handoff policy was accepted with the plan. ADR 0020
is now Accepted after clear code/security review and conformance PASS, as the approved pipeline
requires. Audit ADR 0007 remains Proposed and accepted ADR 0005 is not rewritten or used to
restore the early-read rule.
