# Issue #60 — production and conformance contracts

Proposed R1 revisions; unapproved and unimplemented. [Spec](spec.md), [design](design.md).
These additions belong to #60. #50 provides the base source/envelope/error contract and the
one-second provider context, **not** an admission cap. No cloud provider or identity is selected.

## One production construction path (S1)

Add `pkg/keysource/config/registry.go`: an immutable descriptor table, initially only
`Descriptor{Kind: "static", Open: keysource.NewStatic}`. `Descriptor.Open` has signature
`func([]byte) (keysource.KeySource, error)` for this static-only increment. `Descriptors()`
returns a copy of this table; `Open(kind string, kek []byte)` looks up that same table and refuses
unknown kinds. No runtime registration, test injection or separate accepted-kind list exists.
A future provider changes the constructor contract explicitly before registering its kind.

`meta/cmd/flaminio-meta/main.go` startup must resolve all configured sources through this Open:

| Existing/planned input | Material loader | Kind passed to Open | Operational storage mode |
|---|---|---|---|
| #50 `serve --kek-file PATH` | #50's bounded file loader | static | file-static |
| #57 `serve --key-source tpm-sealed-static --sealed-bundle PATH --tpm-profile PATH` | #57's boot unseal/canary path | static | tpm-sealed-static |

Parse/reject mixed, unknown or insecure source configuration first. A valid mode with unusable
material leaves this instance's encrypted service unavailable, as #50/#57 require. Missing
configuration does not invent a source. No CLI branch calls NewStatic directly; successful
loaders supply material to config.Open and preserve the derived identity. NewStatic copies
borrowed input into its source-owned key state before returning; loaders defer clearing their
caller-owned buffer before error checks. #57's process-wide protected-memory preflight also
covers that retained state. TPM loaders/preflight/ownership stay #57's; no TPM per call.

Only `_test.go` files own `map[string]FixtureFactory`. Registry coverage compares its keys to
config.Descriptors(), then invokes each production descriptor factory with fixture material.
A missing fixture fails CI; an extra fixture cannot make its kind configurable. The fault source
has no descriptor. CLI startup tests exercise both loader selections through the same resolver,
reject unknown/fault kinds and mixed modes, and verify equal KEKs yield equal static identities.
TPM startup uses #57's isolated test loader/owner, never the host TPM. No extra kind flag is added.

## Exact base bounds (S2, S3)

Use #50 `contracts.md` verbatim: each SourceIdentity component is 1..64 ASCII identifier bytes
`[A-Za-z0-9._-]`. Static Kind and ID are both `static`; KEKVersion is `k1-` plus the lowercase
hex of HMAC-SHA256(KEK, `FLKEKCHK001`)'s first eight bytes (19 bytes). Profile is
`xts-aes256-v1`; its encoding bound is 1..32 ASCII bytes. VolumeID uses domain.ValidID's 1..128
ASCII bytes, starting with an alphanumeric and continuing with `[A-Za-z0-9._-]`; FirstEpoch is the original positive claim revision. DEK is 64 bytes,
with distinct 32-byte XTS halves; static KEK is 32 bytes. Reject other key lengths.

Static Envelope.Version is 1 and Sealed is **exactly 92 bytes**: nonce12 + encrypted DEK64 +
tag16. Authenticate #50's canonical FLKEY001 AAD, including all binding/source/version fields.
Test Sealed lengths 0/91/92/93, DEK63/64/65, KEK31/32/33, identity 0/64/65 and forbidden ASCII,
profile 0/32/33 plus unknown profile, first-epoch/volume substitution and every sealed bit.
There is no generic 1024-byte envelope ceiling. Shared protobuf fixtures assert KeyEnvelope
<=328, EncryptionDescriptor<=376 and Volume.encryption<=380 bytes at #50's maximal field
bounds; a real static identity is narrower. Maximal encoding-size fixtures are separate from
semantic acceptance: arbitrary 64-byte static Kind/ID or 32-byte unknown profiles must refuse.

Retained V1/V2 fixtures directly open two sources with different derived identities and retain
V1. They do not mutate #50's immutable production pin or claim production rotation/version routing.

## Diagnostic error refinement (S4, S6)

Keep the exact base mapping; clients choose endpoints using the gRPC code **alone**:

| KeyError.Kind / ErrorInfo.reason | gRPC code | Endpoint decision |
|---|---|---|
| INSTANCE_UNAVAILABLE | Unavailable | next endpoint, within caller budget |
| SOURCE_UNAVAILABLE | ResourceExhausted | stop |
| DENIED | PermissionDenied | stop |
| INVALID_ENVELOPE | FailedPrecondition | stop |

Transport DeadlineExceeded/Cancelled may also try the next endpoint only while the parent
context is live, as today. A provider's inner timeout is SOURCE_UNAVAILABLE, not a transport
timeout. Missing, malformed, unknown or contradictory ErrorInfo never changes this code policy.
Unknown raw provider failures normalize to SOURCE_UNAVAILABLE/UNKNOWN, not a fallback attempt.

Extend Go KeyError with `Reason KeyFailureReason` (string enum). Keep ErrorInfo.domain
`flaminio.keys.v1`, ErrorInfo.reason=Kind and existing validated source/volume metadata. Add only
`metadata["key_reason"]`, at most 16 ASCII bytes, one of RATE_LIMITED, TIMEOUT, UNREACHABLE,
DISABLED, PENDING_DELETION, UNKNOWN. SOURCE_UNAVAILABLE allows all six; the other three kinds
allow only UNKNOWN. Disabled/pending states therefore stop safely as SOURCE_UNAVAILABLE.
Absent/zero Reason encodes UNKNOWN. No RetryAfter field, retry delay or automatic retry exists.

`pkg/keysource.ParseReason(kind Kind, value string) KeyFailureReason` accepts only those
kind/value pairs; missing/unknown/overlong/invalid pairs normalize to UNKNOWN without echoing
input. The server formatter/encoder uses it. This extension is diagnostic only: Go/Rust endpoint
selection does not decode details. Diagnostic readers reject wrong domain/unknown Kind as an
unknown diagnostic, and use ParseReason for a recognized Kind. Bounded safe messages/Cause never
retain provider bodies, credential URLs, tokens or key/envelope bytes. Contract tests decode the
actual emitted ErrorInfo; client tests omit/corrupt/contradict details to prove code-only behavior.

## New service admission gate (S6, S7)

In `meta/service/keysource.go`, Service.New owns one `keyCallGate` of eight nonblocking permits.
The production server constructs one Service. All its Wrap and Unwrap calls, across encrypted
creation/finalization helpers and ReleaseVolumeKey, execute via `Service.withKeyCall(ctx, fn)`;
no source implementation or fixture owns the production gate. No provider call bypasses it.

1. Request paths authenticate/authorize and validate inputs, source/pin and operation eligibility
   **before** acquisition. Release checks target/epoch before acquisition. Recovery helpers first
   validate their admitted operation's conditional finalization authority; no external identity
   is invented for internal recovery. Revalidation after unwrap remains mandatory.
2. Check parent cancellation before acquisition, try once without waiting, then check it again
   before invoking fn. No queue or goroutine per contender. Already-cancelled parent returns its
   context status and invokes no source. A full gate returns SOURCE_UNAVAILABLE/UNKNOWN
   (ResourceExhausted) immediately, with no source call or endpoint cycling.
3. An acquired permit belongs to this synchronous invocation. Derive #50's one-second child
   context (or the earlier parent deadline), invoke fn once, and defer release exactly once.
   No detached provider work or automatic retry. fn handles/clears all key output before exit;
   its result is an envelope or protected delivery, never an unowned raw key.
4. On parent cancellation return its context status. If only the child deadline expires,
   return SOURCE_UNAVAILABLE/TIMEOUT. Never accept a late success. Register cleanup for every
   nonnil key result before checking error/context; clear even a result returned with an error.
   Hold the permit until fn/source actually returns and its output is disposed, including after
   cancellation. Do not release early and permit a ninth active provider call.
5. Conforming sources must honor context and return promptly; the normal bound relies on that
   contract. A noncooperative source is rejected by conformance. The gate still bounds its
   active calls; it cannot forcibly interrupt arbitrary Go code. Never simulate bounded return
   by abandoning a goroutine. Tests release a deliberately late fixture through owned barriers
   and join all work, checking late-output clearing and permit ownership until actual completion.

Service tests invoke real CreateVolume/finalization and ReleaseVolumeKey over mTLS for both
backends. Hold four Wrap and four Unwrap calls in one service; a ninth of either refuses without
provider entry. Repeat a 32-call mix; peak provider entry is <=8. Test denial/cancel before
acquisition even when full, inner timeout, cancellation after acquire, error/panic cleanup of the
permit, late/error key output, final validation failure and reuse of all eight permits afterward.
Tests assert the gate and provider counters independently. A fake-only cap cannot pass them.
No admission work is added to steady-state data I/O or shared across metadata instances.

## Secret cleanup in the harness (S2, S5)

Run and every fixture/helper immediately register deferred cleanup for **every nonnil** allocated
or returned key buffer, before error/context checks or any assertion. This includes DEK/KEK
inputs, Unwrap output returned with an error and late-cancellation output. Sources may violate
the no-plaintext-on-error contract under test; report that violation only after cleanup is armed.
Use secret-free boolean/digest comparisons, never an assertion that dumps buffers.

Harness regression tests retain aliases to a deliberately violating source's output, drive error
and late-cancellation cases and check those aliases are zero after case exit. The common case
runner accepts an internal assertion-failure hook: a controlled panic or runtime.Goexit in an
owned test goroutine exercises the same deferred cleanup path, then the outer test verifies
zeroing and completion without intentionally failing a testing.T or printing secrets. Fixtures
also clear their retained key copies during Cleanup. These tests pass only when failure-path
cleanup executes; success-path cleanup alone is insufficient.
