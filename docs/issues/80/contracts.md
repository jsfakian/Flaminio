# Issue #80 — authoritative CSI compatibility contract (accepted with the plan, 2026-11-13)

This amendment reconciles #76 full create identity with CSI v1.13 capacity-range idempotence.
Accepted with the plan, 2026-11-13. Existing direct CreateVolume semantics/authorization remain unchanged.

## API and authorization

`rpc CheckCreateVolume(CheckCreateVolumeRequest) returns (CheckCreateVolumeResponse);`

- Request: `CreateVolumeRequest desired = 1; int64 required_bytes = 2; int64 limit_bytes = 3;`.
- Response: `Volume volume = 1; bool legacy_identity = 2;`.
- Caller: administrator only, explicitly in auth.go; node/agent/io/Reader/anonymous refused.
  Read-only: no creation/resume, epoch/ISR/placement/pin/progress mutation.
- Reject negative or contradictory signed range and invalid desired chunk/R/minISR/ID/parameter
  grammar as InvalidArgument before reading. No narrowing/overflow. desired.size_bytes is the
  already validated initial candidate; the returned existing size need not equal it.
- Use #77 coherent read retaining internal Header.create_spec. Compare C/R/minISR/preferred
  node/failure-domain/policy and every future non-capacity identity field, not a caller-supplied
  list of fields to ignore. Explicit capacity-range semantics are the sole size exception.
  Validate current V against supported geometry/budget and V≥required, V≤positive limit.
- Missing header after an AlreadyExists race => Aborted/retry current state; Deleting/tombstone
  => AlreadyExists/name unavailable; matching Creating operation => Aborted/in progress, no
  partial success or foreground resume here. Parameter mismatch => AlreadyExists.
- New-format malformed/missing create identity => DataLoss, never a legacy fallback. Only a
  genuinely legacy visible header with neither operation nor canonical identity uses #76's
  documented scalar compatibility exception; set legacy_identity=true and report limitation.
  The authoritative metadata service makes that classification; CSI cannot guess from scalars.
- Success returns the current coherent volume, including grown capacity if it still satisfies
  the range and unchanged non-capacity creation identity. It does not promise a lease against
  later deletion/change. CSI responds with that actual size, checked int64.
- Unimplemented on old service => CSI FailedPrecondition requiring the coordinated service
  upgrade; never fall back to scalar GetVolume acceptance. Other backend/context statuses keep
  their classification. Unsupported existing geometry requires the stated operator plan.

## Tests and size

Both backends: same scalars but each changed policy/preference/domain/minISR conflicts; compatible
size ranges including larger/grown current size succeed; out-of-range and incomplete/deleting/
replacement states refuse; corrupted new identity never becomes legacy; authorization and zero
KV writes. CSI fake verifies full desired fields, returned capacity, no unsafe older RPC fallback.
Exact worst-response fixtures include this wrapper/legacy flag in the existing B/header allowance;
no RPC limit is raised. make proto/contracts updates all generated consumers and method policy.

## Layered schema profiles

The B=2048 baseline applies only to its specified schema. #51 may enable snapshot catalog only
after its32768-base eligibility proof; #72 disk references and #13 release/fence lists likewise
need bounded complete shapes and revised fixtures/admission before activation. Existing volumes
must not be silently truncated or made unreadable during profile enablement. #86's bounded Fenced
representation is an explicit required transport dependency, not implied by the scalar N cap.
