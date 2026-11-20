# Issue #55 — plan review round 2, design-security

Scope: incremental `cd584f4ca92178b174cf1a70d522e87bed5dfcdf..ec52405ff1ca92fe8338989981cd416b9cbdccb0`
under `docs/issues/55/`, excluding `report.md`. Source checks use local `origin/main`
(`a1cc2421ad5aad62695c124537900b0ad0963e5e`); runtime files checked agree with the checkout.
The plan was approved on 2026-11-13. This review claims no implementation or runtime results.

## Earlier findings

- R1-S1: fixed. Design 125–130 forces FORMAT=0, clears inherited attachment state, pins
  recorded settings and rejects unsupported overrides. It excludes generic all-node/format
  deletion paths; planned fake tests cover hostile inherited FORMAT/ZERO_COPY/ATTACH values.
- R1-S2: fixed. Design 53 requires AS=io for capture metadata reads; auth.go permits that
  identity's GetVolume/ListNodes/ListVolumes. Clear #12 relocates the io pair and updates
  metactl accordingly, so this does not assume the old key path persists in pod mode.
- R1-S3: fixed. Design 48–53 filters nonsecret flag values before persistence, forbids raw
  ExecStart/environment/metadata responses and key/token paths; 111–112 confines attachment
  tokens to transient memory. Planned fixtures assert token/key-path removal.
- R1-S4: fixed. Design 41–46 binds product selection to flaminio-system controller/owner
  UIDs and host container cgroups; API/proc strings remain data, with no shell construction.
  The tenant-namespace decoy fixture covers the earlier namespace/prefix confusion.
- R1-S5: fixed within the explicitly stated exclusive-admin precondition. Design 108–123
  records a unique valid id, create_epoch and each acknowledged owned attach epoch, checks
  current identity before detach/delete, and preserves mismatches/unknown outcomes.
  The code exposes current epoch only and DeleteVolume{id}; the plan does not claim an
  atomic incarnation fence. Saved delete acknowledgement permits only bounded tombstone
  polling; absent id after an unacknowledged operation requires manual recovery.
- R1-C12: fixed for the security/authorization concern. Design 98–106 and 166–176 require
  CONFIRM=matrix before mutation, declare/check exclusive lab use, and specify lifecycle
  recovery with no blind replay of ambiguous mutations. No lock or concurrent-admin safety
  guarantee is invented.

## New findings

None in the design-security scope. The check-then-delete limitation is explicit and is not
silently accepted as a concurrent-administrator guarantee. No new metadata authorization,
format or replication safety rule is introduced.

A feasibility concern about holding the #12 DaemonSet store offline for inspection was sent
to the orchestrator for the independent correctness review; it is not duplicated here as a
security finding.

Findings: 0 Critical, 0 High, 0 Medium, 0 Low, 0 Nits.

Reviewed commit: ec52405ff1ca92fe8338989981cd416b9cbdccb0
