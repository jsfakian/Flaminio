# Issue #80 — report

## Review rounds

### Plan review round 1 — 77236aca19edcc1b0a844ada11034bd09cd05fe0

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review80_code_r1 (design-code), review80_sec_r1 (design-security).

- **Review80-R1-C1 — High:** Scalar AlreadyExists fallback overrides full create identity conflicts Evidence: design.md CSI retries versus #76 canonical identity.
- **Review80-R1-C2 — High:** Legal geometry still accumulates unrepresentable Fenced uncertainty replies Evidence: replica.rs report/unpruned writes; frame.rs decoder bound and encoder assertion.
- **Review80-R1-C3 — Medium:** Administrative parseSize still wraps suffix multiplication Evidence: meta/cmd/flaminio-meta/main.go parseSize; 17592186044417M wraps to1MiB.
- **Review80-R1-C4 — Medium:** Validation covers initial attach but not later malformed metadata refresh/ISR responses Evidence: engine.rs MetaWait refresh/ISR conversion; Target::apply_view indexing.

Changes/resolution: Added exact read-only authoritative CheckCreateVolume range/identity RPC and auth/tests to honor both #76 and CSI; included checked admin parsing; specified validation of all later views while preserving higher-epoch deposition. Reproduced C2 safely, filed #86, and made its conservative bounded uncertainty/framing contract an explicit release dependency. Security independently confirmed all four with no additional findings.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 8a92832af76bde0a8da0017d1fbc7d0eecd92421

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r80c2 (incremental design-code), r80s2 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed all four R1 fixes resolved: administrator-only full-identity/CSI-range compatibility RPC including larger/grown volumes, required #86 bounded uncertainty and checked Fenced framing, checked administrative suffix arithmetic, and validation of later metadata views while preserving higher-epoch deposition. Existing positional ISR row association preserved. Zero comments including Nits; no implementation/runtime/lab testing or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.

## Implementation — 2026-11-27

Branch: `issue/80-supported-geometry`, based on
`946b4ea9deb5c069e51e33d6598739f19bceabbc` after the user's merge of #77 / PR #104.
The published clear snapshot was approved on 2026-11-20. Both prerequisites, #86 bounded
Fenced uncertainty and #77 coherent metadata reads, are now merged. The earlier geometry
helpers and Rust preflight groundwork were restored cleanly from their preserved copy.

Both implementation halves are complete and the joined `make fmt lint` passes, including
Rust/fuzz all-target clippy, Go vet, protobuf lint and script checks. The Go compatibility RPC
uses #77's coherent reads, while Rust extends #86's existing checked encoder. [Units](units.md)
tracks the remaining independent acceptance, code/security review and final conformance.
No completed acceptance or readiness is claimed yet.

## Tests

Joined formatting/lint and generated API checks pass. Pending mapped S1–S8 acceptance, including shared vectors,
worst metadata/Hello bounds, CSI compatibility, store preflight and later-view refusal.
Required final checks: `make lint test race hygiene contracts sim`; a short store crash
campaign is included for the store boundary changes. No lab checks are required.

## Conformance

Pending implementation, acceptance and review clearance.

## Lab

None required by the approved design. No shared-lab actions are authorized for this run.

## ADRs

The geometry/admission policy was accepted with the approved plan. Prepare the next unused
numbered ADR for review, then accept it only after code/security review and conformance pass.
Existing language/etcd/fencing/ISR decisions remain unchanged.
