# Issue #83 implementation units

Base: main `f952596f1b32c719efd2c65ab7347c052fc54f56`, after PR #106 / #78.
The published clear spec/design and appendix are approved (2026-11-20).
This cross-component change uses ordered units; one fresh implementer owns each unit.
Root owns documentation/Makefile/git and does not read runtime code. Acceptance follows all units.

| Unit | Scope | Design/rules | State |
|---|---|---|---|
| U1 Clock foundations | repl clock/types/target invalidation; node injected clock/domain types and exports | Elapsed clock/observer, quarantine primitives; S2–S5; checked error margin and total permit/fault order | complete; focused checks pass |
| U2 Signed process handoff | proto/generated API, Go captoken + metadata Attach issuer, Rust net verifier, exact maximum-view fixture | fresh-process appendix, S5/S7; unchanged Attach roles/FLCT, exact FLCH134, nonce32, actual epoch advance | complete; codec/issuer/interop checks pass |
| U3 Runtime enforcement | node engine/net and io clock/backend/node/ublk/control/main, including startup and all completion paths | Profile, observer, quarantine/completion, contracts; S1–S6; no cached authority time, all starts enforce current-process proof | complete; scoped runtime checks pass |
| U4 Authorized callers | CSI dataplane/Node.device, metactl/admin and startup scripts; documented new control syntax | appendix current callers, S5/S8; agent/admin handshake, legacy startup refusal, no unsafe fallback | complete; Go/script/lint checks pass |
| U5 Executable models | repl simulator event mapping/checker/clock and process-handoff schedules | Model/mutations, appendix; S7 plus S2–S5; independent ideal time/history, all required caught mutants | complete; model/lint/node checks pass |

Dependencies: U1 → U2 → U3 → U4 → U5 → independent test engineer → review/address loop → final checks/conformance.
Units may extend the immediately relevant caller/export wiring to keep compilation coherent; report exact paths.
No lab runs, host suspend, VM pause or shared-disk rollback are authorized.
#15 device-lifetime and #79 initialization APIs remain their issues' scope: preserve approved
composition, implement existing #83 entry points strictly, and report any actual prerequisite
that prevents safety before proceeding. Do not add a legacy or environment bypass.

Independent S1–S8 acceptance and approved private test seams are now active after U5.
