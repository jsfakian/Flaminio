# Issue #55 — plan review round 3, design-code

Scope: incremental `ec52405ff1ca92fe8338989981cd416b9cbdccb0..47b183ebbabf2d1629644cff33cb2c0eaa033b4f`,
only `docs/issues/55/` excluding `report.md`. Source/dependency checks use local
`origin/main` (`a1cc2421ad5a`). No implementation, runtime tests or lab execution.

## Earlier findings

- R2-C1: fixed. The pod form now records and conditionally removes the existing per-node
  dataplane selector, waits for API pods and host containers/processes/store holders to be
  absent, and holds that state through inspection. Recovery-open explicitly restores and
  reapplies the hold; fresh-open sampling follows exact selector restoration. Interrupted
  cleanup can restore a proven owned hold independently of ambiguous volume deletion.
  Planned fakes cover a premature successor, a surviving host process, failed inspection,
  interruption, lost patch replies and changed identities/labels.
- R2-N1: fixed. The settled budget entry references the current per-node target paragraph.

## New findings

None. The new lifecycle matches the clear #12 plan: its DaemonSet selects the recorded
`node-restriction.kubernetes.io/flaminio-dataplane=pod` label (design, lines 71–73); removing
that match terminates its pod, and a fully detached pod exits within seconds (design,
lines 111–115 and 868–869; spec S4). Keeping the selector absent prevents immediate
replacement during inspection. The revision requires bounded host shutdown evidence rather
than trusting API disappearance, never force-deletes, and preserves the existing deployment
form. Label mutation/restoration is scoped to the recorded key and identities; changed or
ambiguous state is reconciled or preserved, not blindly overwritten. The added tests exercise
these conditions and can fail without the hold/restoration logic.

Findings: 0 Critical, 0 High, 0 Medium, 0 Low, 0 Nits.

Reviewed commit: 47b183ebbabf2d1629644cff33cb2c0eaa033b4f
