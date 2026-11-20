# Proposal: Seal static KEK material locally without changing envelope identity

- **Status:** Proposed review revision, unapproved, 2026-11-19. Historical version accepted with
  the plan, 2026-11-13; preserved in decisions.md. Numbering/moving into docs/adr/ is separate.
- **Date:** 2026-11-12

## Context

[#57](https://github.com/jsfakian/Flaminio/issues/57) scopes TPM protection for #50's static KEK.
Losing a TPM/PCR policy without escrow loses encrypted data. A copied software TPM state is
not hardware binding. A TPM does not protect secrets after unseal into an ordinary pageable heap.

## Proposed decision

Metadata holders independently seal the same source/version KEK through pinned pure-Go
TPM2 support. Use the versioned rsa-owner-pcr-v1 policy/session recipe and explicit qualified measured-boot
profiles in [contracts](tpm-contracts.md). Unseal at boot into protected memory;
construct the existing static KeySource with unchanged cryptographic SourceIdentity. Status
separately names TPM storage mode. No fallback, TPM call on attach/I/O or implicit key deletion.

Require separately encrypted off-node recovery, independently held random unlock key and
a complete Binding-bearing canary. Publish fresh immutable candidates; select them explicitly
only after verification. Preserve prior artifacts and the same immutable #50 source pin; this
increment adds no key rotation or encrypted snapshot/backup support.
Do not select a universal PCR default without measured platform evidence. Existing plaintext
file migration cannot retroactively erase historical media. Software TPM is flow evidence only.

## Options

C TPM stack requires cgo/system dependency and unit changes; pure-Go support fits the current
CGO_ENABLED=0 build but needs dependency/API/session review. Per-data-node TPM DEK caches widen
revocation/rekey scope and are later work. Signed forward-PCR policy simplifies planned updates
but introduces a policy signing authority; explicit escrow recovery is the first increment.

## Consequences

TPM loss/PCR change blocks new encrypted key service on that holder until recovery. Remaining
holders may continue; one holder's escrow is critical. Protected memory/device access and boot
measurement profiles add operational constraints. Whole-node theft, running host compromise
and emulator rollback remain outside the protection claim. A caller timeout cancels result
acceptance, while a stuck physical TPM owner stays quarantined with restart exclusion until
actual cleanup;5s is not a kernel cancellation guarantee. These corrections are unapproved.
The earlier plan choices were accepted on 2026-11-13 as recorded in decisions.md; that
historical acceptance is preserved and does not extend to these review revisions.

## Review correction

R1-C1..C5, R1-S1/S2 and R1-N1 resolve the fixed material identity, retry behavior, complete
canary, exact TPM recipe, executable protected unit, honest timeout ownership and immutable
publication. The detailed mechanisms and negative fixtures are in [contracts](tpm-contracts.md).
No existing numbered Accepted ADR is changed. ADR0001/0004 remain consistent.

## Confirmation

See [design](design.md): wrong TPM/PCR/auth path, canary/escrow recovery, local-only startup,
no fallback, protected-memory refusal, no secret serialization and qualified lab evidence.
