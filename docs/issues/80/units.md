# Issue #80 implementation and integration

Base: `946b4ea9deb5c069e51e33d6598739f19bceabbc` (main after #77 / PR #104).
The approved design permits the two independent halves below to run in parallel. They share
fixed geometry constants, the B formula/per-RF caps, error contracts and exact compatibility
RPC shape. No side raises limits independently. Root owns global docs and Makefile changes.

| Unit | Scope | Rules / contracts | State |
|---|---|---|---|
| Go admission and CSI | `pkg/geometry`, domain/service/auth, proto/generated API, CSI and admin clients/parsers | Checked geometry/count/admission, read-only authoritative compatibility, signed capacity selection, explicit receive cap; S1–S4/S8 | implemented; joined formatting/lint pass; acceptance next |
| Rust boundaries | repl geometry, store preflight, replica lifecycle, engine initial/later metadata views, checked Hello and metaclient | Validate before narrowing/mutation/cleanup; epoch-first deposition; zero-byte refusal; S4–S8 | implemented; joined formatting/lint pass; acceptance next |
| Shared acceptance and integration | Go/Rust geometry JSON vectors, exact protobuf future views, real bounded interop, frame fuzz seeds, memory-store and CSI/service tests | Full rule mapping and no hidden limit increase | after both implementations |
| Review and conformance | Full code/security review, address loop, project checks, ADR and final rule table | Fresh reviewers, exact reviewed SHAs, no Low+ left open before publication | pending |

After clearance, submit one PR, complete CI, mark it ready and wait for the user's merge
before any next issue. No shared-lab actions are required or authorized.
