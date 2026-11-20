# Issue #57: TPM-sealed static key source — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Historical approval: 2026-11-13; review revisions are unapproved. Not implemented.
Step1 of the existing intent, using #50's specified metadata-owned static KEK interface.
KMSProvider resources, edge offline-quorum deployment and fleet/planned-policy migration are
later stages. The issue stays open; historical plan acceptance is recorded separately, with no
numbered ADR change or hardware qualification claimed.
Settled at approval, 2026-11-13: #50's KEK interface as assumed; the ADR is accepted with the plan (see decisions.md, Settled at approval).

## Terms

- **Holder:** metadata instance which unwraps #50 volume keys; every eligible instance seals
  its own copy of the same KEK in its local TPM.
- **Sealed bundle:** bounded versioned public/private TPM objects, parent identity/template,
  KEK source/version and explicit PCR-bank/selection/policy metadata; no plaintext KEK.
- **Qualified boot profile:** tested TPM2, firmware/boot measurement and PCR policy whose
  limits are stated. Merely having a TPM or matching PCR7 is not verified boot assurance.
- **Recovery copy:** authenticated encrypted off-node KEK escrow plus independently held
  unlock credential, verified recoverable before activating a sealed source.

## The invariant

A sealed bundle releases the intended KEK only under its TPM/parent and PCR policy. Failure
never falls back to a cleartext file or releases a wrong key. The same current KEK remains
recoverable; TPM enrollment/failure/recovery never deletes prior key artifacts. This increment
adds no rotation or encrypted snapshot/backup support.

## Behaviour

**S1. Explicit mode.** Configure a TPM-sealed source instead of #50's weaker static-file source.
Reject mixed or unknown modes. Unencrypted operations remain independent. Source status shows
sealed mode, source/version, TPM/profile and PCR selection without secret bytes.

**S2. Safe enrollment.** Operator provides a KEK through a protected memory-only channel and
creates a policy-bound, non-migratable sealed object. It never places plaintext KEK in a node
file, argv, environment, log, crash dump, unencrypted swap or hibernation image. Verify off-node
recovery first. Publish only a fresh immutable candidate; errors never overwrite the previous
bundle or change active selection. Activation is a later explicit configuration change.

**S3. Binding.** Another TPM, changed selected PCR, altered bundle or parent/policy mismatch
cannot unseal. No empty-password/user-auth alternative bypasses PCR authorization. A qualified
profile states measured components and limitations; unsupported/unmeasured profiles refuse by
default. The first recipe requires empty existing owner-hierarchy auth, accepts no supplied
auth secret and refuses unsupported nonempty hierarchies without changing authorization.
Software TPM tests demonstrate protocol only, not physical protection or anti-rollback.

**S4. Startup.** Attempt unseal once at holder startup with a bounded caller wait and no external
call. A timed-out kernel operation may remain quarantined under exclusive ownership until it
actually exits; never accept its late key or accumulate new owners across restarts. After success use #50's in-memory source interface. No attach, wrap/unwrap or I/O waits for a
TPM transaction. TPM failure disables only this holder's encrypted key service, with a typed
local-source-unavailable result allowing callers to try another eligible holder.

**S5. Fail closed.** Missing/wrong TPM, timeout, policy mismatch or malformed bundle releases no
key and changes no metadata/key record. Diagnose safely, indicating the recovery procedure and
selected PCR mismatch where locally knowable. Existing attached volumes continue with their
keys; a new encrypted attach obeys #50 failure behavior. No error contains secret material.

**S6. Multiple holders.** Each holder seals the same source/version KEK locally and verifies its
derived #50 static identity and the complete wrapped-key canary Binding. Failover may use another unsealed eligible holder.
Never copy a TPM private object as a way to enroll another holder. One-holder status explicitly
warns that its independently stored recovery copy is the only alternative after TPM loss.

**S7. Recovery.** Operator decrypts escrow only into protected memory and seals again after
qualified boot/PCR update, board/TPM replacement or node replacement. Verify unsealing and
opening a known wrapped-key canary before publishing a new immutable candidate and explicitly
selecting it. #50's current immutable source pin is unchanged; no new KEK/version is introduced.
Retain prior bundles/escrows; no crypto-erasure or old-key deletion is implicit.

**S8. Honest threat boundary.** A stolen disk alone cannot unseal a properly enrolled hardware
profile. This does not protect a running compromised holder, stolen whole unchanged node,
malicious host with a software TPM, TPM-state rollback or an unmeasured boot component. Historical
plaintext key files cannot be certified erased by deleting them; migration reports this limit.

**S9. Tests and evidence.** Local software-TPM tests cover enrollment/restart, PCR mismatch,
wrong TPM, no fallback, malformed/time-limited calls and escrow recovery. Authorized lab adds a
software TPM without recreating/wiping VMs, verifies attach/recovery and searches known secret
write paths. Such scans are evidence of these paths, not proof all physical media lacks remnants.

## Tests

- S1–S3: parser/bundle bounds and tampering; TPM policy/user-auth bypass attempts, parent mismatch;
  immutable candidate publication and recovery confirmation before explicit selection.
- S4–S6: startup local-only trace, quarantined TPM/record/restart cases, failover to another holder, no calls on
  attach/I/O after successful unseal, no mutations on failure, key/error/log redaction.
- S7–S9: escrow loss/wrong credential, recover changed PCR/new TPM, canary/version mismatch,
  prior artifacts remain, software-TPM reset/clone limits and authorized lab evidence.
- S2/S4: common protected preflight before TPM-source material in parent/CLI/exec owner; unit credential,
  TPM device, memlock, no-swap/hibernation/kdump failures preserve baseline TLS/token RPC startup.
- S9: isolated test-binary Unix transport/state injection, actual memory primitives and explicit
  test-unit permissions; required cases cannot access host TPM/state or silently skip.

## Open questions

Historical choices below remain recorded. Review corrections and exact source/canary/recipe,
owner/unit and immutable-candidate contracts are [Proposed and unapproved](tpm-contracts.md).
No universal PCR default is claimed without platform evidence. Historical-media limits remain;
#50's concrete source identity/API are followed, with no invented concurrency or retry contract.

Settled at approval, 2026-11-13: the library is pinned pure-Go go-tpm (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: protected memory and encrypted TPM sessions as the design proposes (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: no universal PCR default; explicit qualified profiles (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: off-node escrow with an operator-held unlock key (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: #50's envelope, API and source semantics as assumed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the claim is narrowed as stated above (see decisions.md, Settled at approval).
