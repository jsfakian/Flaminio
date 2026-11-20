# Proposal: Bound WAN transfer admission without weakening recovery points

- **Status:** Historical decision Accepted with the plan, 2026-11-13. Review-round-1
  changes in the final section are Proposed/unapproved. Number/move into docs/adr/ on implementation.
- **Date:** 2026-11-12

## Context

[#59](https://github.com/jsfakian/Flaminio/issues/59) requires caps/windows and automatic resume.
SDK payload accounting omits TLS/TCP/retries; bytes already queued remotely cannot be recalled
at a wall-clock boundary. Object immutability, retained keys and manifests remain safety state.

## Proposed decision

Use one pure per-run limiter/window/backoff policy and shared Rust/Go vectors. Route every
object request through one wrapper. Claim an IP-wire cap only with a qualified owned external
egress shaper and declared burst/measurement boundary. Offer payload-only pacing under an
explicitly weaker name, never as wire conformance. Propose no new admission after observed
window closure plus declared bounded tail; literal zero post-boundary traffic needs a qualified
external gate and cannot include already-inflight remote packets. These were the semantics presented for historical approval; the settlement below records
that approval. Current exact profile revisions are distinguished below.
Settled at approval, 2026-11-13: these semantics are accepted, and the intent's wording is
clarified to them (see decisions.md, Settled at approval).

Persist operation pins/receipts through pauses and process death; publish a recovery point only
after full destination commit. Pace the mover, never application I/O. Shared-site budget and
indefinite-outage retention policy are later work. Actual RPO can grow without bound.

## Options

Payload tokens alone avoid privileges but do not enforce the requested wire envelope. A global
host shaper can disturb metadata/replication and cannot isolate run ownership. Dedicated run
namespace/shaper contains traffic and privilege but adds platform qualification/setup complexity.
Keeping a request alive across a closed window permits ACK/keepalive traffic; cancel/reprobe
costs bounded in-flight object retransmission and verification requests.

## Consequences

Requires platform-specific gate evidence, SDK progress control and durable resume/pin protocol.
Strict profiles may be unavailable; fail visibly instead of advertising an unproved guarantee.
Snapshot retention can exhaust capacity during a long outage; this step reports and preserves
references rather than silently dropping the only unshipped point.
Settled at approval, 2026-11-13: the decision above is accepted with the plan (see decisions.md,
Settled at approval).

## Confirmation

See [design](design.md) vectors, independent byte/window capture, crash points, lost ACK and
restored-data checks. Reject publication of payload-only or unqualified-window results as a pass.

## Review-round-1 revisions — Proposed, unapproved

[Transport profile](transport-profile.md) chooses the Linux shaper/gate/helper and object-only
SDK connector; [policy contracts](policy-contracts.md) fixes activation, deterministic zone
rules, #53 root admission and graph-bounded resume accounting. The envelope is authoritative
at owned local egress; downstream buffering makes an arbitrary remote-arrival envelope
unprovable. Wire-v1 has an explicit supported rate floor; payload-v1 carries no wire guarantee.
No-policy commands preserve #52 failure behavior. A live enabled run retries transient failures,
but an incompatible commit budget or a #53 late root terminates without weakening retention.
These concrete revisions change the formerly accepted interpretation and await approval;
they neither accept a new active ADR nor alter an accepted dependency's safety contract.
