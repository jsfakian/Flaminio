# Issue #57 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The TPM library:** as the ADR recommends, pure-Go TPM2 support, the pinned `go-tpm` (v0.9.8
  when checked), after its API, security and licence checks; no cgo or system TSS.
  [spec.md](spec.md), Open questions; [design.md](design.md), The facts it starts from;
  [adr-proposal.md](adr-proposal.md), Proposed decision and Options.
- **Protected memory and transport:** as proposed, key handling starts only after a
  protected-memory preflight (no dumpability, `mlockall`, a sufficient locked-memory limit,
  hibernation off and no unencrypted swap in a qualified profile), with no fallback to pageable
  memory; secrets cross the TPM only in salted, parameter-encrypted sessions, over an
  interruptible local transport with a 5 s unseal budget. [spec.md](spec.md), Open questions;
  [design.md](design.md), Material provider and startup, and TPM object and profile.
- **Qualified PCR defaults:** as proposed, no universal default: each enrollment names an
  explicit qualified profile (SHA256 bank, selected PCRs and values, measured components); the
  recommended production candidate is a signed, measured UEFI/UKI boot profile whose selection
  comes from measured-boot evidence; the lab uses an explicit lab-only profile labelled as no
  hardware protection. [spec.md](spec.md), Open questions; [design.md](design.md), TPM object
  and profile.
- **Escrow custody:** as proposed, a separately encrypted off-node recovery file (`FLTPMR01`,
  AES-256-GCM) and a random 32-byte unlock key that the trusted operator creates and keeps off
  the node, apart from the escrow; recovery is read back and verified with a canary before a
  sealed source is activated. [spec.md](spec.md), Open questions; [design.md](design.md), Bundle
  and escrow contracts; [adr-proposal.md](adr-proposal.md), Proposed decision.
- **#50's envelope, API and source semantics:** as the plan assumes, #50's metadata-owned KEK
  interface and static KeySource, with the envelope's source identity unchanged (#50's plan was
  approved the same day). [spec.md](spec.md), header and Open questions; [design.md](design.md),
  header and Parallelizable?.
- **The intent's "key found nowhere":** as the plan states, narrowed to testable secret write
  paths, with explicit limits for historical media. [spec.md](spec.md), Open questions (S8, S9).


## Changes after approval — review round1, 2026-11-19

The historical approvals/settled answers above are preserved. The following revisions are
**unapproved**, with no new accepted risk, implementation or hardware qualification inferred.

- **R1-C1:** use #50's fixed static identity and material-derived HMAC version/bounds. No source
  flags, aliases, rotation or changed pin; retain prior artifacts without assuming encrypted
  snapshot/backup support that #51–#53 currently refuse.
- **R1-C2:** remove the invented8-call bound; retry follows gRPC code, ErrorInfo diagnostic.
- **R1-C3:** store complete canary Binding, exact64-byte unequal-halves DEK envelope and SHA256;
  cross-holder/recovery reconstructs authenticated AAD without persisting plaintext DEKs.
- **R1-C4:** choose the concrete rsa-owner-pcr-v1 hierarchy/template/attributes/policy/session
  recipe in [contracts](tpm-contracts.md); real encrypted policy sessions have no password bypass.
- **R1-C5:** define the optional TPM device/credential/syscall/cgroup route under DynamicUser.
  Parent, every exec'd owner and all secret CLI paths preflight protected memory before input;
  missing device/bundle or failed locking disables encrypted source without blocking ordinary RPCs.
- **R1-S1:** supersede the assumed interruptible physical transport/no-leftover-worker guarantee.
  Five seconds bounds startup result acceptance. A single protected owner with flock plus a
  boot/PID/starttime record is quarantined until actual exit/cleanup; no late key or accumulating
  restart attempts. Kernel cleanup may require host maintenance, a named risk, not accepted here.
- **R1-S2:** immutable fresh candidate publication never overwrites active/prior bundles. Errors
  may leave an unselected candidate; activation is a separate explicit validated configuration.
- **R1-N1:** preserve historical local ADR acceptance while consistently marking these revisions
  Proposed/unapproved; existing numbered Accepted ADR files are unchanged.

Lab flow-only swtpm tests remain independent of data-plane admission. Current #83 blocks VM
attach; separately authorized pre-#83 legacy attach remains exploratory, with no profile bypass.
All tests described by this correction are planned implementation checks, not runs performed.


## Changes after approval — review round2, 2026-11-19

These revisions remain **unapproved**; historical approvals and review records are preserved.

- **R2-C1:** rsa-owner-pcr-v1 requires empty existing owner-hierarchy auth. Remove hierarchy-auth
  fields/FDs from local input and owner IPC; unsupported nonempty hierarchy/profile/input refuses
  before publication and never changes authorization. Unattended startup needs no missing secret.
- **R2-C2:** isolated swtpm integration uses a test-binary-only constructor for Unix transport,
  fixture state/owner UID and host facts, with actual protected-memory primitives and explicit
  private-device/transient-unit permissions. Shipping binaries have no bypass; host TPM/state
  is inaccessible and required coverage fails rather than silently skips missing prerequisites.
- **R2-N1:** source preflight covers KEK, escrow unlock and canary material, not pre-existing
  TLS/token startup. This clarifies the earlier broad wording while preserving ordinary RPCs
  when the optional TPM source cannot initialize.


## Changes after approval — review round3, 2026-11-19

These revisions remain **unapproved**; historical approvals and earlier decisions are preserved.

- **R3-C1:** the test-only Unix stream adapter serializes full command/reply pairs, completes
  writes and uses io.ReadFull for the10-byte big-endian TPM2 header plus exact bounded body. Invalid,
  fragmented/truncated/oversized/deadline cases are explicit fixtures; a failed stream is closed,
  never reused or interpreted through a raw single-read adapter.
- **R3-N1:** name the emulator invocation `swtpm socket --tpm2` explicitly before its private
  state/data/control arguments.
