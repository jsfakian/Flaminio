# Issue #57: TPM-sealed static key source — design

[spec](spec.md) · [decisions](decisions.md) · [ADR proposal](adr-proposal.md) · [contracts](tpm-contracts.md)

Historical approval: 2026-11-13. Review revisions are unapproved; nothing is implemented.
This first increment loads #50's same static KEK from a local TPM. It adds no KMSProvider,
rotation, encrypted snapshot/backup support or offline one-node deployment. No TPM, key,
firmware, service or VM was changed while writing this plan.

## The facts it starts from

`scripts/guest/meta.sh:21` passes systemd credentials; `:41` hides devices with PrivateDevices.
It uses DynamicUser and no capabilities. The existing tree has no TPM dependency or libvirt TPM.
#50's current contracts fix Kind=ID=static and derive KEKVersion from material, permit a TPM
material loader, and expose INSTANCE_UNAVAILABLE as gRPC Unavailable. Clients already advance
on that code; ErrorInfo is diagnostic. #50 has no in-flight call bound in this increment.
#53 currently rejects encrypted archives, so its future encrypted key-reference graph is not
implemented or assumed here. #74's privileged data-plane helper is not a metadata TPM broker.

Pinned go-tpm v0.9.8 supports the selected recipe. Its physical device transport uses blocking
I/O; Linux device close may wait for pending TPM work. Therefore a caller deadline cannot
promise kernel cancellation. Exact primary references and concrete interfaces are in
[tpm-contracts.md](tpm-contracts.md), including the owner lifetime record and quarantine.

## Static material and startup (S1, S4–S6)

Add `pkg/keysource/tpmstatic`; it loads material, then constructs #50's static KeySource.
Configure `serve --key-source tpm-sealed-static --sealed-bundle PATH --tpm-profile PATH`;
refuse simultaneous --kek-file, --insecure and unknown modes. Device is fixed /dev/tpmrm0.
There are no configurable source/version aliases. Derived identity must match bundle/escrow
and the existing cluster pin; pin transactions and metadata authorization stay #50's.
Storage status tpm-sealed-static is operational, not cryptographic SourceIdentity.

Run common protected-memory preflight before TPM-source KEK/unlock/canary reads in the parent and each
exec'd TPM owner. Start one private owner, with one5s caller deadline and no retry in this
metadata process. Accept a KEK only after full response, owner exit0, derived identity and
canary verification before the deadline. Startup failure advertises encrypted-source unavailable
while ordinary unencrypted RPCs start. New encrypted creation/release returns Unavailable,
so callers may try another holder. No metadata/key mutation occurs because unseal failed.

The owner alone opens TPM under the shared lifetime protocol. Timeout permanently cancels
result acceptance; signal the pinned owner, discard late material and quarantine until actual
exit. A fixed boot/PID/starttime state record excludes another owner even if flock releases
before blocked kernel cleanup. No TPM clear, lock replacement or growing retry pool. A stuck
owner can require host maintenance; its locked memory remains protected until it exits.

After successful startup, wrap/unwrap are #50's local-memory operations with1s contexts and
no TPM calls. Preserve its code-based retry table and current concurrency scope. Existing
attached targets retain their keys when this metadata holder cannot start its key source.

## TPM recipe and measured boot (S2, S3)

Recipe rsa-owner-pcr-v1 in [contracts](tpm-contracts.md) fixes owner-hierarchy RSASRKTemplate,
SHA256 keyed-hash child attributes, PCR→Unseal command policy and parent-salted AES128-CFB
sessions. Trial sessions compute the digest; only a real policy session authorizes Unseal.
No user/password/empty-auth bypass. Public decoding checks attributes, parent Name and policy.
This first profile requires empty existing owner-hierarchy auth and accepts no auth input;
nonempty hierarchy/profile/input refuses before publication without changing authorization.
Never reset TPM ownership or reuse/evict unrelated persistent handles. Flush transient objects with honest timeout ownership.

Enroll needs an explicit root-owned qualified profile: schema1 JSON≤4096 with profile_id,
host_kind (hardware|swtpm-lab), pcr_bank=sha256, ascending unique pcr_indices0..23,
measured_components (1..32 strings≤128 ASCII bytes), evidence_sha25632, qualification_id and
owner_auth_policy=empty-existing. No other hierarchy-auth policy is supported.
IDs use1..64 ASCII identifier bytes. Reject duplicate/unknown keys, empty/all-zero/unmeasured
production selections, unsupported banks and profile/bundle mismatch. Enrollment reads PCRs
and records values, then verifies unseal; runtime compares selected policy through the TPM.
No universal PCR default is invented. Signed/measured UEFI/UKI is a production candidate;
platform evidence supplies its actual selection. swtpm-lab proves flow only and is never
reported as hardware binding, verified boot or anti-rollback protection.

## Escrow, canary and candidates (S2, S6, S7)

[tpm-contracts.md](tpm-contracts.md) fixes sealed JSON, FLTPMR01 and protected CLI input schemas.
A complete canary Binding (test volume id, first epoch1, XTS profile), #50 static envelope and
SHA256 of a valid64-byte canary DEK make verification reproducible after cross-holder recovery.
No plaintext canary/production DEK is kept. Derive identity from recovered KEK before using it.

Trusted operator creates/read-backs authenticated escrow off-node with an independently held
random32-byte unlock key. Seal/recover receives protected inputs and proves escrow decryption
matches that material. Off-node custody is explicitly the administrator's responsibility; a
local path cannot prove it. No generic network key-provisioning service is introduced.

Publish only an immutable fresh candidate with exclusive temp/fsync/RENAME_NOREPLACE/directory
fsync. An error may leave an unselected candidate, never overwrite the previous bundle. Commands
never select/restart/change pins. After verification, the administrator explicitly configures
the new path; startup independently validates it and never falls back. Recover reuses the old
canary, creates a replacement TPM object and verifies before publishing. Prior bundles/escrows
remain; current immutable #50 pin means this is resealing the same key, not key rotation.

## Unit, protected memory and files (S1–S9)

The contract fixes the optional TPM unit profile: DynamicUser/no capabilities, TPM-only device
allowlist, sealed-bundle credential with unavailable sentinel, shared precreated lock/state files,
memlock/core/cgroup limits and required syscalls. File-static retains PrivateDevices. No broad
root device broker is added. TPM-source CLI, parent and owner processes repeat preflight,
including after exec, before KEK/unlock/canary material. Existing TLS/token-key setup is outside
this new preflight; its ordinary authenticated RPC startup remains available on source failure. No swap/hibernation/kernel crash capture in qualified hosts.
Memory limits can cause OOM; continued ordinary service after OOM is not promised. Protected
memory applies while alive; compiler/GC zeroization is best effort, not a guarantee.

Files touched on implementation:
- `pkg/keysource/tpmstatic/{provider,bundle,escrow,policy,owner}.go`: codecs/material/TPM owner.
- `meta/cmd/flaminio-meta/`: source setup, local admin/owner verbs, shared protected preflight.
- `go.mod/go.sum`: pinned pure-Go go-tpm, existing x/sys; no cgo/system TSS.
- `scripts/guest/meta.sh`, host udev/tmpfiles and script tests: exact optional unit profile.
- isolated swtpm test scripts/Make target, optional lab XML and recovery/source documentation.

No data-plane hot path, protobuf RPC, metadata transaction/auth rule or on-disk replica format
changes. The private inherited owner IPC and test/admin artifact versions are fully specified;
no socket listener or remotely supplied program/device is accepted. #74 remains unchanged.

## Tests

| Rules | Planned checks |
|---|---|
| S1/S6 | fixed derived identity; unchanged pin; file→TPM canary; gRPC code retry conformance; two holders |
| S2 | strict FD/schema/bounds; every process preflights before source-material input; immutable publication/crash/concurrent candidate fixtures |
| S3 | exact recipe vectors; wrong TPM/parent/PCR; altered attributes/policy/session order; trial/password/empty-auth bypass refused |
| S4/S5 | hung read/write/close; late response/deadline race; PID record/restart exclusion; no fallback/mutation; unencrypted RPC still serves |
| S7/S8 | escrow recovery/new TPM/PCR; complete canary Binding; wrong key/source; old artifacts preserved; emulator and historical-file limits |
| S9 | isolated emulator flow; rendered unit/runtime credential/device/memory negatives; separately authorized evidence with qualified profile |

Full negative fixtures are enumerated in [contracts](tpm-contracts.md). Use make lint/test/race/
hygiene/contracts plus make tpm-test through the [test-only constructor/unit route](tpm-contracts.md).
The test binary injects private Unix transport/state/host facts; actual memory primitives and
owner IPC run under explicit permissions/limits, with host TPM/state inaccessible. Production
binaries retain fixed paths and no test override.
Tests are planned; no runtime check was run for this documentation revision. CI installs required
TPM test tools explicitly; never silently skips required coverage. Reap only owned emulator state.

## Lab, docs, risks and ADRs

Only after separate authorization: add optional swtpm without VM UUID/disk recreation, retain
XML rollback, run local seal/restart/PCR-change/wrong-TPM/recovery tests and inspect known write
paths. #83's physical-only data-plane admission makes integrated VM attach Blocked before fault
or workload mutations. A separately authorized pre-#83 legacy attach is exploratory, never
production qualification; no fake profile or bypass. Local swtpm key-source tests do not require
data-plane admission. Full no-network volume attach still needs #56's later one-node deployment.
Restore original XML only after explicit source/escrow recovery; no disk wipe/TPM clear.

Document current profile/recovery/custody and quarantine procedures; preserve source versions and
prior artifacts. A stolen unchanged whole node, compromised live holder, unmeasured components
and copied emulator state are outside the claim. Historical plaintext files cannot be certified
erased. Current #50 ciphertext integrity/retention scope is unchanged. ADR0001/0004 are consistent;
audit0009 remains required for later encrypted retention. [ADR proposal](adr-proposal.md) preserves
historical acceptance and records these review revisions as Proposed/unapproved. Existing numbered
accepted ADR files are untouched. Quarantined kernel cleanup and administrator escrow custody are
explicit risks, not claims that tests prove them absent.

## Parallelizable?

Yes after #50's stated contracts: strict codecs/canary and TPM owner/unit work share the concrete
interfaces. Integration must prove all owner/session/device/memory negative fixtures before use;
this does not leave their mechanisms unspecified.
