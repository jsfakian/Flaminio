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

### Code/security review round 1 — f735f89493c2dbecb0b8713abacfb7f8b58f0b58

Full range: `946b4ea..f735f89`, split into Go and Rust parts because the complete
non-generated change exceeds one review pass. Fresh code/security reviewers check each
part and its cross-language contracts. Built-in review skills are unavailable; direct
high-effort review is used. Go code and security reviews are clear without nits.

**R1-R-C1 — Medium:** the engine membership validator accepts an R-member move whose
destination is already a replica. RF 2 / minimum ISR 1, replicas/ISR [a,b] and move a→b
passes validation; production Target then proposes source removal to [b], and the metadata
removal branch can accept R−1 replicas. The source state cannot result from a legal requested
move, so initial/refresh/ISR boundaries must reject it before authority or peer-map mutation.
Require destination absent at R and present at R+1, preserving legal planned/joined stages.
Independent code and security proofs use the exact validator and production replication
core and both emit the R−1 removal proposal. All four scoped reviews are complete: this is
the sole Medium finding, with no additional Low-or-higher findings or nits. The narrow correction makes destination membership equivalent to the temporary R+1 shape.
The new shared initial/refresh/ISR rejection matrix fails under the original predicate and
passes after the correction. It also covers higher-epoch deposition and accepts legal
planned/prejoin/joined move stages. All 62 node unit tests, 20 integration tests and all-target
Clippy pass. `make fmt lint test race hygiene contracts sim` passes after the fix, including
20,000 normal seeds, all 165 release tests and all three TLS interop tests. Fresh incremental
code/security review follows.

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
Acceptance and project checks pass; the review correction awaits incremental clearance.

## Tests

Joined `make fmt lint test` passes after all acceptance edits and the normal-reader error
adapter repair. Independent Go and Rust acceptance covers every mapped rule; no geometry,
framing, store or epoch safety defect was found in those boundary runs.

| Rules | Acceptance evidence |
|---|---|
| S1, S4 | Shared independent JSON vectors, checked counts/arithmetic, RF 1–5 placement and no KV mutation on rejected Create/Check |
| S2–S3 | CSI signed ranges/defaults/limit-only/chunk rounding, full desired identity forwarding, actual current/grown capacity, conflict/status/legacy cases and no scalar fallback |
| S3, S8 | Both-backend read-only compatibility/auth/lifecycle/replacement/identity-corruption matrices, unsupported persisted volumes unchanged |
| S4 | Checked admin/CSI suffixes and no RPC after overflow; outer header parse compatibility and nested identity corruption across the coherent-read race |
| S5 | Initial and later engine geometry/count/ID/membership/move validation, immutable refresh, saved ISR indices and higher-epoch deposition; authenticated hostile Hello over real TLS |
| S6 | 65,536 Hello roundtrip and 65,537 refusal, zero-byte invalid sends with connection reuse, 16 Hello fuzz seeds and preserved #86 Fenced regressions |
| S7–S8 | All five legal store clusters, actual table/max bounds, exact/excess slots, duplicates/existing keys, read-only preflight, startup refusal/delete and durable partial-create retry |

`make geometry-vectors crash-test CASES=2000` passes, including the new contract target and
the required short memory-store crash campaign. Scoped Rust suite and all-target Clippy,
including fuzz, pass. Go non-service packages and mapped both-backend service acceptance pass;
the final joined full suite confirms the repaired service cases too.

Controlled omissions fail their intended assertions and fixed counterparts pass: metadata
budget removal admits RF-next vector cases; suffix overflow removal lets 17592186044417M
wrap into a valid 1 MiB RPC; missing engine validation consumes core requests; missing slot
preflight accepts excess; missing Hello range validation emits invalid data. The experiments
use isolated copies/overlays and leave branch runtime bytes intact.

Acceptance caught an error-class integration issue: routing normal header reads through the
strict creation decoder changed #77's parse statuses. The adapter now preserves bounded
outer syntax/type errors through their existing Unavailable mapping while keeping nested
create_spec errors DataLoss. A deterministic corruption-after-initial-read Check test failed
before the precise guard and passes after it on both backends. Exact #77 commit context,
status, epoch, ISR and applied-history assertions remain intact.

### Joined maximum-view interoperability

`make meta-interop` passes all three tests, including real production-service/Rust-client
Get/Create/Check/Attach maximum future-move views at every admitted RF cap. The ordinary
fixture remains in the run. The client uses the normal explicit 4,194,304-byte receive limit.

| RF | N | Encoded Attach bytes | Admission B | Receive limit |
|---|---|---|---|---|
| 1 | 5137 | 4,110,252 | 4,193,840 | 4,194,304 |
| 2 | 3881 | 4,122,273 | 4,193,528 | 4,194,304 |
| 3 | 3119 | 4,130,207 | 4,193,984 | 4,194,304 |
| 4 | 2607 | 4,135,353 | 4,194,104 | 4,194,304 |
| 5 | 2239 | 4,138,323 | 4,193,456 | 4,194,304 |

These are representation boundary checks, not scale/performance certification.
`make race hygiene contracts sim` passes at `f735f89`, including shared geometry vectors,
all three TLS interop tests, 20,000 normal seeds and all 165 release tests. Local
`make sanitize QUICK=1` also passes at that parent commit (address/leak and thread sanitizers).
After the engine predicate correction, the complete required project checks pass again.
Incremental review and final conformance remain before publication; final CI repeats sanitizers.

## Conformance

Pending correction of R1-R-C1, incremental review clearance and the final independent rule check.

## Lab

None required by the approved design. No shared-lab actions are authorized for this run.

## ADRs

The geometry/admission policy was accepted with the approved plan. Numbered
[ADR 0018](../../adr/0018-admit-only-representable-volume-geometries.md) is prepared for review
with Proposed status until code/security review and conformance pass.
Existing language/etcd/fencing/ISR decisions remain unchanged.
