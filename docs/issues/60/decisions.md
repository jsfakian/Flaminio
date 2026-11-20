# Issue #60 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`f1ad5fced275c051b7c7c5d0000516151dd70108de4a0ef9f719394b83494313`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **#50's ownership, envelope and protected-delivery choices** (`spec.md`, Open questions;
  `design.md`, Shared fixture contract and Risks and ADRs): as the plan states. The suite
  consumes #50's proposed metadata-owned KeySource contract exactly and invents no second
  envelope; #50's choices themselves are settled by #50's own approval, not by this one.
- **SDK, cloud deployment and identity provider** (`spec.md`, Open questions; `design.md`, Files
  and contracts and Risks and ADRs): no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): none is chosen in this slice; an ADR before the first provider stage chooses them
  (see Settled at the user's instruction of 2026-11-13, below).
- **The safe subordinate reason extension** (`spec.md`, Open questions and S4; `design.md`, Fault
  source and error refinement): as proposed. A bounded `reason` detail (`RATE_LIMITED`,
  `TIMEOUT`, `UNREACHABLE`, `DISABLED`, `PENDING_DELETION`, `UNKNOWN`) under the existing four
  KeyError kinds and the `flaminio.keys.v1` domain, coordinated with #50's Go and Rust clients
  before shipping.
- **The intent's unchanged-record rule on a failed release** (`spec.md`, S7; `design.md`, Risks
  and ADRs): as the plan states. The rule applies to the read-only key-release step; an Attach
  epoch already committed is not rolled back.

## Settled at the user's instruction of 2026-11-13

- **The SDK, cloud deployment and identity provider:** none is chosen in this slice; the first
  provider stage chooses the SDK or thin client per cloud, where the metadata service runs in a
  cloud and which identity calls the KMS, in an ADR written before that stage starts (intent, Out
  of scope), and #60 stays open until then. Recommended by Claude, no recommendation in the plan:
  this slice runs with no cloud, SDK or credentials (S9), so a choice now would add a dependency
  nothing uses, and a library or identity choice belongs in an ADR.


## R1 corrections after approval (proposed; unapproved)

Historical approvals/settlements above remain historical; they do not approve these revisions.
Nothing is implemented. Exact additions are in [contracts](contracts.md).

- **R1-C1 — production admission:** #50 supplies a one-second context and no concurrency cap.
  Retain the intent's bounded-many-attaches outcome by proposing #60's shared eight-call gate
  at the real service Wrap/Unwrap boundary, with auth first, no queue, exact cancellation/permit
  ownership and mixed production-service tests. This is new #60 scope, not inherited behavior.
- **R1-C2 — retry/detail:** retain the four exact #50 kind/code mappings and code-only endpoint
  decisions. ErrorInfo.reason remains Kind; KeyError.Reason/key_reason is a bounded diagnostic
  enum with named normalization. Remove unused RetryAfter; no automatic retry is added.
- **R1-C3 — registry:** actual file/TPM startup resolves Kind static through the single production
  descriptor table; only tests own the fixture map. Unknown/fault kinds cannot be configured.
- **R1-C4 — creation:** durable operation/identity/plan/binding stay fixed; unpublished candidate
  DEKs may differ. Clear losers; one finalization publishes the exact envelope retained on later
  retry/recovery, including a lost reply. No durable plaintext/key-staging protocol is added.
- **R1-C5 — layered I/O proof:** ordinary Rust FakeMeta counts release calls while actual engine
  I/O/resync/ISR runs; Go backend/mTLS tests separately connect release to counted Unwrap.
  No unconnected Go counter or new cross-language fixture server is claimed as engine evidence.
- **R1-C6 — exact bounds:** use #50's 64-byte DEK/32-byte KEK/92-byte Sealed and identifier/profile/
  protobuf bounds. Remove the invented 1024-byte generic envelope limit.
- **R1-C7 — local transport:** required tests need no external network/emulator/cloud credentials;
  owned loopback metadata mTLS, embedded-etcd client/peer HTTP and replica TLS are permitted.
- **R1-S1 — unconditional cleanup:** defer clearing every nonnil key allocation/output before
  errors/assertions, including violating error outputs and late cancellation results. Harness
  regressions verify the same cleanup on controlled assertion exit without printing secrets.
