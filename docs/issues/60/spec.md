# Issue #60: Key-source conformance first — specification

Existing [intent](intent.md), scoped to its conformance/fault-injection slice. [Design](design.md).
Historical approval: 2026-11-13; the R1 revisions are proposed and unapproved. Not implemented.
Assumes #50's metadata-owned KeySource contract; exact #60 additions: [contracts](contracts.md).

## Terms

- **Binding:** volume ID, immutable first epoch and encryption profile from #50.
- **Source identity:** kind, source ID and KEK version; boot storage mode is separate (#57).
- **Envelope:** bounded authenticated wrapped key bound to both identities, never plaintext.
- **Fixture:** source with isolated test-owned version/fault controls and call counters.
- **Key release:** #50's read-only ReleaseVolumeKey step after an Attach may have committed.

## The invariant

Every configured source returns the correct key for the exact binding/source/version or fails
closed. Errors, timeouts and retries cannot replace a published key, mutate state during key
release, leak secrets or create unbounded provider work. Unpublished creation candidates are
not durable key identities.

## Behaviour

**S1. One suite.** Every implemented/configurable source passes the same conformance suite,
starting with #50's static source. Add Vault/KMIP/cloud fixtures when implemented; a configurable
source without a fixture fails registry coverage. Startup resolves through the same production
descriptors the suite enumerates; file and TPM loading both select Kind static. The test-only
fault source cannot be configured in production. This slice builds no cloud SDK/provider.

**S2. Binding and key ownership.** Wrap/unwrap 64-byte keys with #50's exact binding/envelope
limits. Wrong volume, first epoch, profile, source ID/kind/version or KEK, and tampered/truncated/
oversized/unknown envelopes refuse without plaintext on error. Buffer ownership/clearing follows
#50, including immediate deferred cleanup of every nonnil key output before error checks or
assertions, even for errors and late cancellation results. Storage mode never aliases
cryptographic identity or enables a decryption fallback.

**S3. Retained versions.** A controlled rotation creates a new explicit source/version while
retaining the old source for old envelopes. Old keys remain recoverable through their recorded
version; new wraps use the selected new version. Wrong selected versions refuse. Do not rewrite
stored envelopes, delete KEKs or implement rotation/crypto-erasure in this test slice.

**S4. Failure classes.** Preserve INSTANCE_UNAVAILABLE=Unavailable,
SOURCE_UNAVAILABLE=ResourceExhausted, DENIED=PermissionDenied and
INVALID_ENVELOPE=FailedPrecondition. Endpoint retry depends on code alone, including missing or
malformed detail. ErrorInfo.reason stays Kind; only bounded key_reason metadata refines
diagnostics (contracts). Disabled/pending keys remain a refusal, never a replacement key.
Unknown errors fail closed; no provider bodies/secrets enter diagnostics. No automatic retry or
RetryAfter is added.

**S5. Fault fixture.** A deterministic authenticated source scripts unreachable, slow, rate limit,
denied, disabled, pending-deletion, rotation and corrupt-envelope behavior. It passes normal
success/binding tests and counts calls, peak concurrency and cancellation. It proves the contract,
not cloud identity/API correctness.

**S6. Bounded execution.** Honor #50's 1-second call context. Add a new #60 production gate
shared by all service Wrap/Unwrap calls: eight active calls, authorization before acquisition,
nonblocking refusal when full and no queue or detached provider work. A permit belongs to the
actual call until it returns, including after cancellation; clear late/error key outputs. The
contracts define cancellation and timeout results. ResourceExhausted stops endpoint cycling;
local Unavailable may try another holder. No offline cache or cluster-wide limit is introduced.

**S7. Metadata boundary.** Authorize target/epoch before unwrap and validate ownership again
before protected delivery. Failed key release changes no header/descriptor/quota/fence revision.
An already committed Attach epoch is not rolled back: the old intent's unchanged-record rule
applies to the release step. Failed initial wrap keeps #76 creation hidden/retryable; encrypted
volume publication requires its durable bound envelope. The operation, admitted create identity/
plan and binding remain stable; unpublished candidate DEKs may differ between helpers/retries.
Clear losing candidates. Atomic publication selects one envelope, retained exactly thereafter.

**S8. Attached I/O.** Read/write/resync/ISR makes no source call after attach/key release while
faults run. Existing I/O retains normal safety/key lifecycle; fresh release fails as typed.
Recovery after publication reuses the winning retained envelope/key instead of regenerating it.

**S9. Local/live evidence.** Required tests run in make test/race without external network,
emulators or cloud credentials. Owned loopback metadata mTLS, embedded-etcd client/peer traffic
and replica TLS fixtures are permitted. Later live tests need explicitly configured isolated
fixture/workload identity;
missing configuration reports skipped, never passed. Ordinary CI performs no cloud mutation.
#60 stays open until AWS/GCP/Azure and workload identity pass in their actual clouds.

## Tests

- **S1–S3:** registry coverage, fixed/random binding cases, every envelope field/bit, wrong
  KEKs, retained old/new versions and exact length/size bounds.
- **S4–S6:** fake-clock/barrier faults, exact code/detail compatibility, two-endpoint counters,
  actual service mixed Wrap/Unwrap 8/9 and 32-call admission, cancellation/deadline/permit ownership
  and no secret/goroutine leaks. Harness regressions prove cleanup even when assertions fail.
- **S7/S8:** both-backend mTLS release tests, target change during unwrap, committed Attach
  followed by refused release, competing create helpers/lost publication reply and recovery.
  Ordinary Rust engine tests count ReleaseVolumeKey during verified I/O/resync/ISR; Go tests
  separately prove each authorized release invokes the counted source once.
- **S9:** live absent/present/invalid configuration and coverage manifest; no cloud/lab result
  is claimed by this draft.

## Open questions

#50 ownership/envelope/protected-delivery choices remain unapproved prerequisites. No SDK,
cloud deployment or identity provider is selected here. Coordinate the safe subordinate
diagnostic extension with #50's contract before shipping; clients continue to use code alone.
The new #60 admission/constructor contracts are proposals, not inherited #50 behavior.

Settled at approval, 2026-11-13: the suite consumes #50's proposed contract exactly (#50's own
approval settles its choices); the bounded reason extension as the design proposes, coordinated
with #50's clients before shipping (see decisions.md, Settled at approval).

Settled 2026-11-13 (user's instruction): the SDK, cloud deployment and identity provider are
chosen by an ADR before the first provider stage, none in this slice (see decisions.md).
