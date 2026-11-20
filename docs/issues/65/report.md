# Issue #65 — report

## Review rounds

### Plan review round 1 — 9d806703f4642ecdbcbd9bde6cbe4fdd2430b22c

Scope: full intent/spec/design/decisions/local appendices against source and current dependencies.
Reviewers: review65_r1_combined (design-code and design-security).

Combined report: Full R1 review of all four issue-65 intent/spec/design/decisions files, preserving the Nov 13 approved measurement slice and settlements. Read current Conn/session/TLS/token/frame contracts and refusal fixtures, actual Net spawn/channels/session lifecycle and daemon event sink, benchmark crate manifests and aligned-copy path, existing RF2 campaign reset/job/integrity flow, performance-plan measurement rules, accepted ADR0001/0003, and AGENTS/reviewer/supervisor instructions. Three findings: two Medium, one Low; no additional security findings or Nits. No production auth, fencing, wire-format or durability change is proposed, so no invented simulator requirement or future RDMA design review was imposed. No external dependency plan contract was needed for this measurement-only review; dependency_commits is empty. Future implementation tests remain proposed, not executed. No runtime, lab, benchmark, network setup or disk action was run. Reviewed HEAD remained 9d806703f4642ecdbcbd9bde6cbe4fdd2430b22c and git status was clean at final verification. Read-only except this report.

- **R1-C1 — Medium:** The RF2 profile wrapper needs job-level attachment and matching completion windows across daemon restarts. Evidence: docs/issues/65/design.md:77-84 proposes an outer campaign wrapper with PID-start checking but does not define attachment/window hooks. scripts/lab.sh:1857-1870 stops the node during every write_reset, and write_volume:1884-1886 starts a new daemon. The campaign invokes measured jobs at :2060 and :2074, with whole-disk writes, preconditioning and idle at :2063-2068 and integrity/restart work at :2094 between measured jobs. Attaching once around the campaign profiles a dead PID after the first reset; attaching for a broad campaign interval instead mixes unrelated operations into CPU/sample numerators. PID-reuse rejection alone does not repair either case, so S4/S5 CPU per successful durable I/O is not reproducible as designed.
- **R1-C2 — Medium:** Endpoint thread CPU alone does not account for the kernel-network CPU that the comparison is intended to measure. Evidence: spec.md S4 requires accounting for shared/idle CPU and attribution to kernel TCP. design.md:55-67 defines CPU ns/op only from endpoint thread CPU clocks and makes perf optional; its shared/cgroup/host background accounting is specified only for the separate RF2 campaign at :80. TCP may execute receive/softirq work outside the endpoint threads (for example ksoftirqd), and async I/O helpers may run elsewhere. Thread CPU clocks and perf attached only to those TIDs exclude that work rather than putting it in the unknown bucket. In contrast memory streams remove kernel processing. The resulting paired CPU difference can therefore omit precisely the displaced work while being reported as total endpoint/transport CPU.
- **R1-C3 — Low:** The optional actual-Net fixture cannot satisfy the stated join-before-exit lifecycle through the current Net API. Evidence: design.md:39-41 includes an optional fixture using actual NetCommand/NetEvent, and :48-51 requires cancel/close/join on failure. flaminio-node/src/net.rs:97-165 spawns a detached OS thread and returns only Net { cmds }; NetCommand::Close at :158-160 cancels an outgoing session, not the network thread or accepting loop. Dropping all Net handles permits eventual thread exit, but gives the harness no join or completion handle. Without a specific benchmark-only lifecycle, repeated fixtures can overlap teardown or leave listeners/tasks alive while later trials start, violating the stated isolation and contaminating counters.

Full real reviewer artifacts: audit `review-inputs/65-r1-combined.json`.

Resolution: R1-C1: proposed per-job reset-safe PID/readiness/measurement windows with matching retained successful fio counters and resume/error handling; R1-C2: endpoint CPU explicitly partial with separate non-additive host/off-thread observations and unknown coverage; R1-C3: optional detached actual-Net fixture removed. Added focused future fixtures. Local links, historical-decision prefix, scoped diff and whitespace checked; no runtime/lab execution. Technical revisions remain unapproved. Historical approvals remain separate; subsequent revisions unapproved. No runtime/dependency/lab changes.

Status: needs_incremental_review.

### Plan review round 2 — ea0bda3952d61b8cc646b1275e54016bc4922381

Scope: incremental `9d806703f4642ecdbcbd9bde6cbe4fdd2430b22c..ea0bda3`, excluding report, and affected paths.
Reviewers: review65_r2_combined (design-code and design-security).

Combined report: Incremental review of 9d806703f4642ecdbcbd9bde6cbe4fdd2430b22c..ea0bda3952d61b8cc646b1275e54016bc4922381 excluding report.md. Read actual prior combined JSON in both /tmp and canonical review-inputs plus issue report, current intent/spec/decisions to preserve the approved measurement-only slice, changed design/spec/decisions and affected campaign, guest fio and Net lifecycle paths. Confirmed each actual R1 finding fixed; reviewed revised counting, windows, off-thread/system coverage, artifact fields, fixture removal and associated future tests for correctness/security fallout. Zero comments including Nits. No external dependency plan contract was relevant; dependency_commits is empty. Historical approvals remain historical and substantive corrections remain proposed/unapproved; no approval-only finding. No RDMA transport expansion, runtime safety rule or simulator requirement invented. Static diff whitespace check passed. No runtime, lab, benchmark, profiler, disk or implementation tests were executed; future tests remain proposed. Final git HEAD remained the reviewed full hash and working tree was clean. Read-only except this JSON report.

The required fresh review scopes report **zero comments including Nits**.

Full real reviewer artifacts: audit `review-inputs/65-r2-combined.json`.

Resolution: The required fresh review scopes returned zero comments including Nits. No approval is inferred. Historical approvals remain separate; subsequent revisions unapproved. No runtime/dependency/lab changes.

Status: clear.
