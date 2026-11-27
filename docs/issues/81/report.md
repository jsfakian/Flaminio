# Issue #81 — report

## Review rounds

### Plan review round 1 — f7be923d3255fe7cf55a41631969cedd54721fbe

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r81c1 (design-code), r81s1 (design-security).

- **Review81-R1-C1 — Medium:** Malformed-count handling can suppress a higher authoritative epoch, conflicting with #80 Evidence: design.md55–62; #80design.md79–86.
- **Review81-R1-N1 — Nit:** Spec/design link nonexistent decisions.md Evidence: spec.md3/design.md3.

Changes/resolution: Both reviewers independently reported the same two findings and no others. Reordered generation/request association then higher-epoch deposing using safe current geometry before payload validation on ISR and refresh paths; retain same/lower malformed-reply refusal without cache mutation and add explicit regressions. Added an explicitly unapproved choices record. No implementation/runtime/lab tests or approvals inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 0a8bfca37184f3dd4a45355c6020a58827017dae

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r81c2 (incremental design-code), r81s2 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed higher-epoch ordering and missing choices record resolved, checked affected core/engine paths and #80 contract, and reported zero comments including Nits. No implementation/runtime/lab test or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.

### Code review round 1 — 220f0caab2434dc97d39943ec71b14749de5c181

Scope: the complete implementation and acceptance-test diff from
`1d9b26b5b2ffbd5b3c956093aef45531e06683a5`. Independent code and security reviewers used
direct review because the built-in review-skill tools are unavailable in this environment.

- **Code81-R1-C1 — Low:** the engine metadata fixture treats duplicate chunk IDs as an RPC
  error, whereas the real service returns an authoritative `applied=false` reply in request
  order. Its new fixture assertion incorrectly preserves the divergence.
- **Sec81-R1-S1 — Medium (confidence 1.0):** independently confirmed the same divergence;
  a stale-epoch duplicate proposal must carry the higher authoritative epoch for fencing,
  rather than force timeout/refresh recovery.

No other Low-or-worse findings or Nits. Both findings share one fixture-parity fix:
distinctness now participates in the atomic acceptance predicate, and the regression checks
ordered duplicate entries, unchanged metadata and both current/stale epochs. Out-of-range
IDs still return `InvalidArgument`. The strengthened focused test failed before the fixture
fix and passed afterward; `make fmt lint test` also passes on the corrected fixture. Fresh
incremental reviews are recorded below.

### Code review round 2 — 7424968c09ed34f8c840e24deb600e63e50cea3a

Scope: `220f0caab2434dc97d39943ec71b14749de5c181..7424968c09ed34f8c840e24deb600e63e50cea3a`.
Fresh independent code and security reviewers each confirmed **Code81-R1-C1** and
**Sec81-R1-S1** fixed, checked the incremental change against the real service contract, and
reported no new findings or Nits. The workflow's Sonnet model is unavailable here; the fresh
available default-model reviewers used medium effort for this incremental round.

Final code/security loop state: **clear after two rounds**, with no accepted open findings.

## Implementation — 2026-11-27

Branch: `issue/81-bounded-isr-updates`, based on
`1d9b26b5b2ffbd5b3c956093aef45531e06683a5` (`origin/main`). The published spec and design
were approved on 2026-11-20, as recorded in [decisions.md](decisions.md).

The core now selects at most 64 distinct pending chunks per request with a cyclic cursor;
deferred membership and acknowledgement requirements are retained. Engine replies are guarded
by attach generation, live request, complete payload and monotonic version checks, with a
higher authenticated epoch deposing before payload conversion. Production models enforce the
same service bound. Wire/API/disk formats and replication policy are unchanged.

Targeted acceptance tests and required local checks pass. Independent code/security review
is clear after two rounds, and final spec conformance is PASS. Issue closure occurs on merge.

## Tests

| Check | Result |
|---|---|
| `make lint` | Pass: Rust/Go/protobuf/shell/actions and script checks. |
| `make test` | Pass: all Rust/Go tests and committed fuzz regressions. |
| `make race` | Pass: all Go tests under the race detector, shuffled, `COUNT=1`. |
| `make hygiene` | Pass: advisory/license/dependency/docs/static analysis and Go module checks. |
| `make contracts` | Pass: generated contracts/vectors, fuzz lockfile and both real mTLS interoperation tests. |
| `make sim` | Pass: 20,000/20,000 seeds (0–19,999), then all 115 release replication tests including ignored mutation campaigns. |
| Review fix: `make fmt lint test` | Pass after the fixture-parity correction; the focused regression also fails before the fix and passes afterward. |

The complete required command was `make lint test race hygiene contracts sim`; the review
fix subsequently reran formatting, lint and all tests. Only the engine test fixture and its
assertions changed in that fix; replication/runtime, Go, contracts and simulator sources
were unchanged from their passing checks.

New target and simulator regressions cover 1/64/65/128/129-chunk boundaries, deferred
membership, retries, fairness and all four relevant mutations, including proposals dropped
before delivery. Eleven engine metadata unit regressions cover expired requests, replaced
attach generations, crossed versions, malformed replies and higher-epoch deposing. Large
engine integration cases verify 129-chunk shrink/rejoin with a deferred tail write and mixed
moves with data verification. Real Rust-client/Go-service interoperation verifies all final
chunk versions after bounded recovery, hidden commits and epoch takeover.

Pre-review correction: `max_meta_batch` records proposals at emission, before network loss,
with a regression proving dropped oversized proposals remain visible. The receiver's atomic
oversized refusal is unchanged.

## Conformance

Checked `f5573d1b394d551a7ecedb614d06e7709202ed2f` against the approved spec/design and the
actual validation logs. Checker output follows verbatim.

PASS

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 Bounded requests | yes | yes | `target.rs:2438` selects ≤64 unique chunks; `engine.rs:1902` guards forwarding; `shrink_after_losing_peer_fits_metadata_rpc_limit`, `TestChangeIsrBatchBoundary` and real interop verify boundaries. |
| S2 Preserve deferred work | yes | yes | Selection preserves pending chunk state; `deferred_shrink_keeps_write_waiting` and `deferred_joiner_counts_for_write_and_lease` verify deferred state and acknowledgement requirements. |
| S3 Authoritative per-chunk progress | yes | yes | `engine.rs:1579` correlates replies with saved request order; boundary, mixed-move and interop tests verify independently committed chunks and versions. |
| S4 Recover ambiguous outcomes | yes | yes | Live-request, generation and monotonic merge guards protect cache/core state; hidden-commit, expired-reply, malformed-count and stale-version regressions pass. |
| S5 Epoch fencing | yes | yes | Higher epochs depose before payload conversion; `takeover_between_batches_fences_old_owner`, replacement-generation regressions and real interop verify obsolete-owner refusal. |
| S6 Membership safety | yes | yes | Deferred shrink/join/move tests preserve effective quorum, leases, synchronization-before-removal and minimum ISR; simulator catches all three membership mutations. |
| S7 Eventual progress | yes | yes | Cursor advances on emission; `round_robin_batches_do_not_starve_tail`, 129-chunk engine recovery and real-service shrink/rejoin interop verify progress. |
| S8 Production-model agreement | yes | yes | Simulator and engine fixture enforce production refusal behavior; oversized-emission and membership mutations are caught; 20,000 seeds and all 115 release tests pass. |
| S9 Existing service and compatibility rules | yes | yes | Production service/auth/contracts remain unchanged; Go boundary/auth tests and real mTLS interop verify atomic refusal, target-only mutation and epoch/version enforcement. |

Contract drift: none; API/generated code, RPCs, control-socket lines, frames, disk records, tokens and CLI contracts are unchanged; `make contracts` passes.
ADR conformance: consistent; accepted ADRs 0001, 0002, 0005 and 0006 are preserved, including existing stricter readiness; proposed ADRs remain unaccepted.
Scope drift: none; all planned implementation files are touched; additional test dev-dependency/lockfile and workflow documentation changes are explained in `decisions.md`.
Docs: done; protocol §2 and follow-ups reflect bounded transactions and validated recovery; spec/design changes only update implementation status.
Open review findings: none; Code81-R1-C1 and Sec81-R1-S1 are fixed with an ordered authoritative duplicate-refusal regression; both fresh round-two reviews are clear.

## Lab

None required by the approved design. No shared-lab commands are authorized for this run.

## ADRs

Preserves accepted ADRs 0001, 0002, 0005 and 0006. No new ADR or wire/disk format change;
proposed ADRs 0007–0011 remain unaccepted.
