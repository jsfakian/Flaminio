# Issue #60: Key-source conformance first — design

[Spec](spec.md), [contracts](contracts.md). Preserves the existing [intent](intent.md).
Historical approval: 2026-11-13; R1 revisions are proposed and unapproved. Not implemented.
The production gate/constructor additions below support that conformance slice; no provider is built.

The facts it starts from:

- No cloud/key-source code exists on this branch. #50's current draft defines Go KeySource,
  metadata ownership, 1-second contexts and read-only protected key release in
  its `contracts.md`, with no in-flight bound. #60 adds the service gate specified below;
  #57 loads the same Kind static source after boot unseal.
- CSI endpoint fallback uses a 10-second attempt (`control/csi/meta.go:21,64–89`). Generic
  Unavailable currently cycles; ResourceExhausted stops in both CSI and Rust
  (`flaminio-metaclient/src/lib.rs:145–160`). Error details never control endpoint fallback.
- Attach commits its epoch (`meta/service/service.go:696–735`); it cannot be rolled back when
  a later ReleaseVolumeKey fails. #50's separate release is the revision-neutral tested step.

## Shared fixture contract (S1–S3, S9)

Add `pkg/keysource/conformance` test-support package with `Run(t,*Fixture)`. A Fixture supplies
factory `Open(SourceIdentity) KeySource`, known test-only KEK/DEK material, retained-version
control, fault controls/counters, cleanup and evidence mode local/live. It does not register
providers or appear in production configuration. Tests import it from external `_test` packages
so production keysource does not depend on testing or service harnesses.

Consume #50's exact contract rather than invent a second envelope: SourceIdentity Kind/ID/
KEKVersion, Binding VolumeID/FirstEpoch/Profile, Wrap/Unwrap(context,...), Envelope and KeyError.
Profile xts-aes256-v1, DEK64 bytes, KEK32 for static, Sealed exactly92 bytes: nonce12 +
ciphertext64 + tag16, with #50 canonical FLKEY001 AAD. Identity/profile/protobuf bounds and
semantic/size fixtures are exact in contracts.md; no generic envelope ceiling. Reject unknown/
duplicate critical fields before provider invocation. Storage_mode=file-static/tpm-sealed-static is operational
status; both construct the same cryptographic static source identity when explicitly configured.
A missing/unsealed KEK fails INSTANCE_UNAVAILABLE before construction, not an alias/fallback.

`Run` covers randomized fresh round trips and fixed wrong binding components, wrong KEK/source/
version, empty/overlong keys, each envelope field/bit, unknown versions, truncation/oversize and
output ownership. Wrong input must yield no plaintext buffer, but a broken source may violate
that rule: register deferred cleanup immediately for every nonnil key output before checking
errors, cancellation or assertions, including late/error buffers. The harness regression cases
in contracts.md prove failure-path cleanup without dumping keys. Structural/ciphertext tests
supplement AEAD binding tests, never claim that failing to find plaintext in random ciphertext is a cryptographic proof.

Rotation fixture returns distinct explicit V1/V2 sources, preserving V1 and old envelopes.
Unwrap through recorded V1 succeeds after V2 wraps; selecting V2 for V1 refuses. No transparent
fallback, rewrap, deleted version, cloud key mutation or new production version registry is
implemented. Future backend adapters must retain and route recorded envelope versions safely.

Add the single production descriptor/factory table in `pkg/keysource/config/registry.go`
(contracts.md). Actual CLI startup resolves both file and TPM material through config.Open
for Kind static; neither loader bypasses it. Tests compare these descriptors with their own
FixtureFactory map, then exercise the production factories. Only static is initially registered;
unknown/fault kinds refuse startup. Future registration requires fixture/evidence coverage first.
TPM boot tests remain #57's; this suite certifies no cloud milestone or production rotation.

## Fault source and error refinement (S4–S6)

`conformance/fault_source.go` wraps a real test-owned authenticated static source. Script each
Wrap/Unwrap invocation with success, unavailable, delayed-until-context, rate limited, denied,
disabled, pending deletion and corrupted response; operations use barrier channels/fake time,
not arbitrary sleeps. Observe entered/completed/cancelled/peak calls and bytes cleared. Rotate
selects a separately retained version. The test-only constructor cannot be resolved by a
production source kind. It models contract failures, not a provider's real lifecycle semantics.

Keep #50's exact codes: INSTANCE_UNAVAILABLE/Unavailable, SOURCE_UNAVAILABLE/ResourceExhausted,
DENIED/PermissionDenied, INVALID_ENVELOPE/FailedPrecondition. In ErrorInfo, domain remains
flaminio.keys.v1 and reason remains Kind. Add only diagnostic metadata key `key_reason` and
Go field `KeyError.Reason KeyFailureReason`; exact enum, allowed pairs, normalization and
16-byte bound are in contracts.md. No RetryAfter or detail-dependent retry is added. Remote
rate limit, timeout, disabled and pending deletion stop as SOURCE_UNAVAILABLE. Unknown raw
provider errors normalize safely; missing/malformed detail never overrides the gRPC code.

All errors carry safe source/volume identifiers only. Normalize raw transport/provider failures
to categorical safe Cause before returning; never keep a response body/key/token-bearing URL in
an exported error. Capture `%v`, `%+v`, `%#v`, JSON/log/error detail and cancellation paths with
secret sentinels; no token, key/envelope body or credential may appear. Do not print test keys
on assertion failure; compare hashes/boolean success with bounded public context instead.

Add a **new #60 production admission gate**, owned once by Service.New in
`meta/service/keysource.go`, shared by every Wrap/Unwrap path. Authorization and eligibility
precede nonblocking acquisition; eight active calls, no queue. Full returns
SOURCE_UNAVAILABLE/UNKNOWN (ResourceExhausted) without entering the provider. Use #50's
one-second child context; retain the permit through actual completion/cleanup, even after
cancellation, and reject late success. No detached goroutine or implicit retry is allowed.
contracts.md specifies parent cancellation, inner timeout, cleanup and the cooperative-context
limit. Mixed CreateVolume/finalization and ReleaseVolumeKey mTLS tests call this actual service
boundary with 8/9 and 32 calls; a fixture-only limit cannot satisfy the assertions.

CSI and Rust tests use two local endpoints and code-only decisions. INSTANCE_UNAVAILABLE
reaches the healthy second holder; SOURCE_UNAVAILABLE (including rate limit/timeout/full),
DENIED and INVALID_ENVELOPE stop with zero second-endpoint calls. Repeat with absent,
malformed and contradictory ErrorInfo. Transport outage retains ordinary bounded fallback
while the parent is live. Go service tests assert exact emitted wire details; Rust need not
parse the diagnostic extension. No cluster-wide singleflight or shared instance cap is claimed.

## Metadata and I/O integration (S7, S8)

Use both backend helpers and real loopback mTLS with authorized IO/current-target identities.
Register nodes, create #50 encrypted header/descriptor through #76, attach and capture committed
epoch/revisions. On release, assert authorization happens before Unwrap. Race target/firstEpoch
replacement while a source blocks; final #50 read-only validation refuses protected delivery,
clears transient keys and changes no resource revision. Failed release leaves already committed
Attach epoch intact. Never retry an Attach inside the conformance helper to hide this distinction.

For failed initial Wrap under #76, retain the operation, normalized create identity (including
encrypted), admitted plan, claim revision and Binding. No DEK/envelope is durable yet. Retries
and competing helpers may generate different candidate DEKs; clear each losing/error candidate.
One conditional finalization publishes the marked header and its matching bound envelope.
After publication, return that exact winning envelope/key; never regenerate it after a lost
reply. Test failed Wrap then retry, two helpers with distinct candidates, and lost publication
reply followed by retry/recovery. Assert no keyless visible header, stable operation/first epoch,
exact winning envelope and no second quota/capacity charge (#76/#72/#73 ordering unchanged).

Use a layered I/O proof; no Go/Rust test bridge is needed:

- `dataplane/crates/flaminio-node/tests/engine.rs`: extend existing Rust Model/FakeMeta with a
  ReleaseVolumeKey counter/script, using #50's protected-release fixture helper. After a real
  encrypted engine attach, save the count, script release failure and drive verified read/write,
  replica resync and ISR change through the existing memory store/replica TLS harness. Assert
  bytes and completion plus an unchanged release count. A new epoch/reattach must invoke release
  and fail; recovery after restoring it uses the retained winning envelope/key. Existing
  detach/depose/failure key cleanup stays #50's; no offline cache is enabled.
- `meta/service/keysource_test.go`: both-backend/mTLS tests prove authorized ReleaseVolumeKey
  enters Unwrap once and denied/stale/full-gate releases do not. Source counters observe this Go
  boundary only; the Rust counter observes engine metadata calls. Their composition proves
  attached I/O cannot reach a source, without claiming one harness sees the other's counters.

## Files and contracts

- `pkg/keysource/conformance/{suite,fault_source}.go`, external source conformance tests and
  registry test: fixture API, common binding/version/failure coverage (S1–S6/S9).
- `pkg/keysource/config/registry.go`, CLI startup and its tests: one descriptor/factory path (S1).
- `pkg/keysource` safe Reason encoding/ParseReason; `meta/service/keysource.go` shared gate and
  all Wrap/Unwrap call sites; `meta/service/keysource_test.go` (S4–S7).
- `dataplane/crates/flaminio-node/tests/engine.rs`: release counter and verified I/O (S8).
- `control/csi/meta_test.go`, `dataplane/crates/flaminio-metaclient/src/lib.rs` test module:
  local two-endpoint code-based fallback tests, including ignored malformed details (S4/S6).
- Future `scripts/dev/keysource-conformance.sh` and docs/keysource-conformance.md: optional
  fixture manifest/matrix, no default network/cloud action (S9).

Exact production/fixture/error contracts are in contracts.md. No new RPC, DEK/envelope/token/
data frame or disk format. ErrorInfo metadata gains only key_reason; envelope/source identity/
profile bytes and storage-mode separation stay #50's.
No SDK, custom HTTPS signer, KEK deletion API or cloud identity dependency is selected here.

## Tests and optional live evidence

| Rules | Concrete proof |
|---|---|
| S1–S3 | config/registry_test.go TestRegisteredSourcesConform and CLI main_test.go TestSourceResolver; conformance TestBindingFieldsAndEnvelopeBounds/TestRecordedVersionAfterRotation on production factories; static/boot-loaded static share vectors. |
| S4/S5 | conformance suite_test.go TestCleanupOnErrorLateResultAndAssertion; scripted faults, ParseReason/wire-detail bounds and sentinel redaction; fuzz envelope/diagnostic parsing; fake passes normal suite. |
| S6 | service/keysource_test.go TestSharedKeyCallGate with mixed 8/9/32 Wrap/Unwrap; denial, cancellation, timeout, late cleanup and permit reuse; Go/Rust two-endpoint code-only decisions; goleak/race. |
| S7 | Memory/etcd/mTLS TestReleaseFailureIsReadOnly; denied roles never invoke provider, target/incarnation CAS during unwrap, committed Attach retained; TestCreateCandidatesAndLostReply checks failed Wrap, competing helpers and exact winning envelope. |
| S8 | flaminio-node/tests/engine.rs encrypted_io_without_key_release: verified I/O/resync/ISR with unchanged FakeMeta release count; new attach refusal/recovery and key cleanup. Go boundary proof above connects release to Unwrap. |
| S9 | Missing live config t.Skip with named unrun provider; invalid explicit config fails; evidence manifest distinguishes local/fake/emulator/live and configured source registry. |

Future implementation validation: `make test` runs the ordinary Go suite and non-ignored Rust
engine/client tests; `make race` runs Go cases with goleak. `make contracts lint hygiene` checks
shared contract/build consistency. No dependency on the ignored live Rust interop fixture or a
test-enabled shipping server. No new replication rule or simulator mutation is introduced.
Tests use no external network/cloud/emulator/credentials. Owned loopback metadata mTLS,
embedded-etcd client/peer HTTP and replica TLS fixtures are permitted. No runtime check is
claimed by these documentation revisions.

A later explicit live fixture configuration names provider, isolated pre-owned key/version,
workload identity and allowed non-destructive tests. Destructive lifecycle/rotation requires
separate disposable-resource authorization and is never implied by a credential being present.
Live adapters retain the same binding suite; absent configuration is a visible skip, not passed
cloud conformance. No script creates or schedules deletion of a cloud KEK in this slice.

## Lab and docs

None. Document fixture contract/error taxonomy and the remaining AWS/GCP/Azure/workload-identity
stages in follow-ups/README roadmap; do not mark #60 complete. Provider notes link primary
[AWS context](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html),
[AWS pending deletion](https://docs.aws.amazon.com/kms/latest/developerguide/deleting-keys.html),
[GCP key versions](https://docs.cloud.google.com/kms/docs/resource-hierarchy), and
[Azure wrapping](https://learn.microsoft.com/en-us/rest/api/keyvault/keys/wrap-key/wrap-key).
These motivate authenticated bindings/retained versions; fake tests certify none of those APIs.

## Risks and ADRs

#50 provider ownership/protected-delivery/envelope decisions remain Proposed; no SDK/provider is
chosen. The local gate, registry and diagnostic refinements are explicit unapproved #60
contracts, not a new cloud architecture or a claim about #50's delivered behavior.
ADRs 0001/0002 and #57's storage-mode separation are consistent. Proposed audit0009 retained-key lifetime warnings
remain; successful conformance is not malicious-replica integrity or permission to erase keys.
The old intent's broad unchanged-attach wording is resolved by the explicit release/epoch boundary
above, not by rolling back durable ownership. Cloud accounts and workload identity remain later.
Settled at approval, 2026-11-13: this suite consumes #50's contract as proposed; #50's own
approval settles its choices (see decisions.md, Settled at approval). Settled 2026-11-13
(user's instruction): the SDK, cloud deployment and identity provider are chosen by an ADR before
the first provider stage, none in this slice (see decisions.md).

## Parallelizable?

Yes after #50's contract: generic suite/faults and service/client tests can proceed against the
same code/error contracts and gate/constructor boundaries. Future provider fixtures must
integrate before those kinds become configurable.
