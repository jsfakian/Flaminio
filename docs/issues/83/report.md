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

## Tests

Pending implementation and independent S1–S8 acceptance. Required final checks:
`make lint test race hygiene contracts sim`, `make sanitize QUICK=1` (now including IO/frontend)
and `make miri` for the expanded syscall/lifetime boundary. Miri's aligned-helper scope cannot
validate Linux clock/filesystem syscalls; native tests and sanitizers must cover those paths.
No hardware qualification is claimed by local or model tests.

## Conformance

Pending implementation, tests and clear code/security review.

## Lab

None authorized or required by this plan. Do not suspend hosts, pause VMs or roll back disks.
Current unqualified lab VMs must be documented as refused by the accepted physical profile.

## ADRs

The physical-boottime-v1 and fresh-process handoff policy is accepted with the plan. Prepare
numbered ADR 0020 for review; accept after clear reviews and conformance. Audit ADR 0007
remains Proposed and accepted ADR 0005 is not rewritten or used to restore the early-read rule.
