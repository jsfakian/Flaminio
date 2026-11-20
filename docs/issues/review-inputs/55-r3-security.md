# Issue #55 — plan review round 3, design-security

Scope: incremental `ec52405ff1ca92fe8338989981cd416b9cbdccb0..47b183ebbabf2d1629644cff33cb2c0eaa033b4f`
under `docs/issues/55/`, excluding report.md. Checks use the clear #12 design from local
origin/main (`a1cc2421ad5a`) where the new hold depends on its selector and lifecycle.
The plan remains approved as of 2026-11-13; this review claims no implementation or lab result.

## Earlier findings

- Round-2 design-security was clear; no security finding remains open from that round.
- R2-C1: fixed in the security-relevant lifecycle. The new per-node selector hold is persisted
  before mutation, conditional on recorded node identity/version/value, and retained through
  inspection. Pod disappearance alone is insufficient: host container/process/store closure
  must also be established. Recovery-open is bounded and followed by a new hold.
- R2-N1: fixed. The settled budget entry references the current per-node target paragraph.

## New findings

None. The added procedure in design.md:129–155 keeps mutation limited to the recorded selector
on an owned, fully detached node. It leaves other labels and the DaemonSet unchanged, preserves
identity/value changes for recovery, and reconciles lost patch replies by reading the same
identities/key rather than blind writes. Restoring a proven scheduling hold is independent
of ambiguous volume ownership, without authorizing volume deletion. Existing exclusive-admin
preconditions remain explicit; no concurrent-administrator safety guarantee is introduced.

The clear #12 design names this exact selector, defines graceful exit after full detach, and
requires claim --no-format on replacement. Thus the held-stopped/recovery-open procedure does
not bypass its deployment/format protections. Planned fakes cover premature successors,
surviving host processes, interrupted restoration, lost replies and changed identities/labels.
No new metadata authorization, wire rule or replication safety rule is introduced.

Findings: 0 Critical, 0 High, 0 Medium, 0 Low, 0 Nits.

Reviewed commit: 47b183ebbabf2d1629644cff33cb2c0eaa033b4f
