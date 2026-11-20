# Issue #55 — plan review round 2, design-code

Scope: incremental `cd584f4ca92178b174cf1a70d522e87bed5dfcdf..ec52405ff1ca92fe8338989981cd416b9cbdccb0`,
only `docs/issues/55/` excluding `report.md`. Source/dependency checks use local
`origin/main` (`a1cc2421ad5a`). No implementation or lab execution.

## Earlier findings

- R1-C1: fixed — extra live data nodes excluded; each owned chunk's replicas/ISR verified a/b.
- R1-C2: fixed — cgroup memory.current is the verdict; charges and per-form scope are explicit.
- R1-C3: partly fixed — replay qualification/pool-pair semantics fixed; pod offline window remains R2-C1.
- R1-C4: fixed — primary fresh-open/no-device slope and retained post-load memory separated; pod execution depends on R2-C1.
- R1-C5: fixed — expected member/identity table; transient thread/probe churn distinguished.
- R1-C6: fixed — fixed idle baseline preserved instead of deleting unrelated replicas.
- R1-C7: fixed — architecture/ADR intent preserved; unsupported audit claim removed.
- R1-C8: fixed — shared staging replaces anonymous queue buffers.
- R1-C9: fixed — cgroup totals authoritative; schedstat leader/thread coverage qualified.
- R1-C10: fixed — read-only warm-up settings specified; unproved queue coverage inconclusive.
- R1-C11: fixed — Python SSH runner/tests, bounded k3s metrics and external state checks specified.
- R1-C12: fixed — confirmation/exclusivity and conservative lifecycle cleanup/recovery specified.
- N1: fixed — add performance-plan budget section.
- N2: fixed — missing spaces corrected.
- N3: fixed — startup-log lines corrected to node.rs:268–271.
- N4: fixed — tracefs event/loss source named.
- N5: fixed — operator absence and #43 ownership stated.

## New findings

### R2-C1 — Medium

**The #12 pod form has no held-stopped phase for offline inspection.**

Evidence: design.md:128–130 defines the pod restart as deleting its owned pod and waiting for
its successor. design.md:142–150 then requires stopping each data plane, inspecting its disk
without a live store, optionally reopening/stopping for replay, and finally restarting for
fresh-open capture. The clear #12 design's DaemonSet (`origin/main:docs/issues/12/design.md`,
"The DaemonSet") immediately replaces a deleted pod while its selector still matches the
node; `OnDelete` controls template updates, not replacement. The replacement's init claim and
`dataplane` container reopen that same disk. Waiting for it makes inspection live; attempting
inspection in the replacement gap races it. `node.sh down` is explicitly unavailable on this
pod node under #12's guard. Consequently the advertised pod matrix cannot perform its required
offline evidence/fresh-open ordering as written.

Fix: specify an explicit matrix-only, per-node held-stopped lifecycle before deleting the pod,
with the hold and original state saved before mutation. For example, record/remove the node's
existing `node-restriction.kubernetes.io/flaminio-dataplane=pod` selector label, wait for the
owned pod/container/store to be absent, inspect (or briefly restore/re-hold for recovery-open),
and restore exactly the original label before fresh-open capture. Preserve the deployment form
and keep cleanup able to restore that hold after interruption. Alternatively mark pod offline
allocation evidence unsupported rather than implying pod deletion provides it. Add a fake that
would fail if a successor starts before inspection ends, plus interruption/hold restoration.

### R2-N1 — Nit

`decisions.md:19` still points to the removed "Proposed budget table" wording; reference the current per-node target paragraph.

Reviewed commit: ec52405ff1ca92fe8338989981cd416b9cbdccb0

Counts: 1 Medium, 1 Nit; no Critical, High or Low findings.
