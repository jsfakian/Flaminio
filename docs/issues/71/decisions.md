# Issue #71: Observability and the kubectl flaminio operations CLI — decisions

[Intent](intent.md) · [Spec](spec.md) · [Design](design.md) · [Contracts](contracts.md).

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status:
needs_incremental_review).

The round-3 fixes (plan review round 3) came after the approval; see Changes after approval.
The round-4 fixes (plan review round 4) came after the approval; see Changes after approval.
The round-5 fixes (plan review round 5) came after the approval; see Changes after approval.
The round-6 fixes (plan review round 6) came after the approval; see Changes after approval.

## Settled at approval

- **Histogram bucket bounds:** the ten bounds in contracts. Spec "Open questions"; design
  "Risks" (Open questions); contracts "Metric families and bounds".
- **Alert thresholds and hold times:** those of the rules table in contracts. Spec "Open
  questions"; design "Risks" (Open questions); contracts "Alert rules".
- **Ports:** the observer on `:7412`, and the lab's metadata metrics listener on 7402. Spec "Open
  questions"; design "Risks" (Open questions); contracts "Endpoints".
- **Allowlists:** an exact, configured list of SPIFFE IDs per endpoint. Spec "Open questions";
  contracts "Endpoints".
- **The scrape identity:** a #69 reader, even though it can also call the open reads and
  `ListTombstones`. Spec "Open questions"; design "Risks" (Open questions); ADR proposal
  "Consequences and open choices"; contracts "Endpoints" (Scraping).
- **A reattach can hide a counter reset:** counters stay per attachment, starting at zero, and
  the limitation is accepted, as for a process restart. Spec "Open questions"; design "Risks"
  (Open questions); contracts "Telemetry socket" (Field rules).
- **The dependency on #69:** it stands as the intent states (reader identity, `Collect`,
  `Derive` rejecting invalid data per volume). Spec "Open questions"; intent "This step".
  Changed after approval (round 3): `Derive` as #69 states it, failing as a whole on invalid
  data.
- **The cost on the I/O path:** shared counters per attachment and a read-only chunk walk at most
  once a second. Per-queue copies or counts kept in the target are used only if the overhead
  campaign shows a cost. Design "Risks" (I/O path); decisions, round 2.
- **The Proposed ADR's decisions:** the telemetry socket, the server-only observer, pull
  scraping, the metadata listener rather than a reader-identity exporter, reader identities, and
  counting at commit, as proposed. ADR proposal "Decisions" (named "Decisions proposed" at
  approval) and "Options".
- **Every format in contracts:** as written. Contracts header.
- **Role-specific DNS SANs checked per Prometheus job:** no recommendation in the plan.
  Settled 2026-11-13 (user's instruction): see below. Design "Risks" (Prometheus checks less
  than the CLI). Changed after approval (round 3): the observer certificate has the role DNS
  SAN, which the observer job checks. Binding an observer to its node in Prometheus: Settled
  2026-11-13 (user's instruction): not in this step, and the residual is accepted for it (see
  below).
- **The metadata listener's load on large clusters (V reads a minute per instance):** no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): the listener as designed,
  changed only on a measured cost (see below). Design "Risks" (Open questions); ADR proposal
  "Options".
- **promtool's pinned release:** no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): chosen and pinned when `rules-test` is written (see below). Design "Risks" (Open
  questions) and "Alerts and runbooks". Round 3 moved `rules-test` to its own CI job.
- **The release overhead budget:** no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): set from the measured campaign (see below). ADR proposal "Consequences and open choices"; design "Lab".
- **Where Kubernetes accepts Events about cluster-scoped objects:** no recommendation in the
  plan. Settled 2026-11-13 (user's instruction): the summaries step checks it with envtest (see
  below). Intent "What stays
  open in #71"; ADR proposal "Consequences and open choices".

## Settled at the user's instruction of 2026-11-13

- **Binding an observer to its node in Prometheus (the role DNS SAN per job, and the Prometheus
  residual of rounds 2 and 4):** not built in this step. The observer job checks the shared name
  `observer.flaminio.invalid` and binds a node by scrape address, and the residual that design
  "Risks" (Prometheus checks less than the CLI) states is accepted for this step under this
  instruction. Recommended by Claude, no recommendation in the plan: a per-node name needs a
  node-id encoding and per-node jobs that the plan does not design, and the residual reaches only
  alerts, never fencing, durability, authorization or the CLI, which checks the URI SAN.
- **The metadata listener's load on large clusters:** the listener stays as the ADR proposal
  selects it, V volume reads a minute per instance, bounded by #69's collection limits and off by
  default; it changes only if a measured load shows a cost. Recommended by Claude, no
  recommendation in the plan: it is the selected smaller change, its reads are already bounded,
  and a guessed load would not be evidence.
- **promtool's pinned release:** chosen when `make rules-test` is written, a `go run` pinned to
  one release as `BUF` is. The plan's recommendation (round 4, R4-5; design "Alerts and
  runbooks").
- **The release overhead budget:** the plan's own procedure: the lab's overhead campaign (design
  "Lab") measures the overhead before any budget is set, and until then its pass rule (every
  job's median p99 with telemetry at most 10% above its median p99 without; a breach builds the
  fallbacks in Risks before release) stands. The plan's recommendation (ADR proposal
  "Consequences and open choices").
- **Where Kubernetes accepts Events about cluster-scoped objects:** the plan's own procedure: the
  summaries step checks it with an envtest case before it picks the kinds' scope. The plan's
  recommendation (intent "What stays open in #71"; ADR proposal "Consequences and open choices").

## Choices settled by plan review round 1

- **This plan is step 1 of #71 (R1-C7, R1-C8, R1-S1, R1-S6, R1-S10).** It covers read-only
  metrics, the read commands of the CLI, a first doctor and the alert rules. The mutating verbs
  (drain, fail, claim, release, create, expand, delete, move) are cut: the issue does not ask for
  them, and their preconditions belong to their owners. `recover` is cut until #54's restore API
  exists; this step does not choose its grammar. The intent lists what stays open.
- **No summary CRDs, Conditions or Events in this step (R1-C1, R1-S5, R1-S11, R1-C15, R1-C16,
  R1-C17).** They wait for #43's CR reconciliation, which the issue names as their dependency.
  The claim that an Event about a cluster-scoped object must live in `default` was recalled, not
  verified. The later step checks it with an envtest case before it picks the kinds' scope.
- **Pull, not push, and no collector (R1-C9, R1-S9, R1-S4, R1-S3).** Prometheus and the CLI
  read each node's observer and each metadata instance. No producer sequence, instance ordering
  or cross-replica freshness is needed. A node's series carry the `node` label of its scrape
  target, so a node cannot write or hide another node's series.
- **A separate telemetry socket, a server-only observer identity (R1-S2, R1-C2).** The daemon
  answers `observe` on its own socket and nothing else there. The observer runs without root in
  the socket's group. Its certificate has only the server usage, so it cannot call the metadata
  service or a replica.
- **The engine is observed through its input channel (R1-C3).** `Input::Observe` is handled by
  whoever drives the engine: the main loop without a volume, a queue's pump with one. Frontend
  counters belong to one attachment and start at zero at attach.
- **One recording seam (R1-C10, R1-C14).** One call at each of the two commit sites. A pure
  classifier maps the committed result to a reason: `io_error`, `invalid` or `unsupported`.
  `IoError` does not change.
- **Recovery progress is the target's resync and reconcile (R1-C5).** A read-only
  `Target::recovery` accessor feeds it. "Stalled" is "remaining bytes unchanged for 10 minutes
  while work remains". There is no eligibility flag and no progress-age gauge.
- **No incarnation or cluster identity in names or labels (R1-C6, R1-S7).** A series is
  keyed by (node, volume id) or (node, disk UUID). A recreated volume continues its id's series.
  The CLI finds a volume with `GetVolume` and a disk by listing observers.
- **Traces and structured logs move to the next step (R1-C4, R1-S8).** This step changes no
  log line.
- **Doctor checks only what the node can read (R1-C11).** These are the kernel version, ublk,
  io_uring and the MTU across nodes. Clock, peer latency, disk health and risky configuration
  report `unsupported`.
- **Exact allowlists (R1-S12).** Each metrics or observation endpoint admits only the SPIFFE
  IDs it is configured with.
- **Doctor failures exit 4 (R1-C13).** Exit 2 stays for usage errors only.
- **Spec rules renumbered.** The old S1–S12 covered the whole issue. The new S1–S9 cover this
  step. Old S3 (traces, logs), S4 (summaries) and S8 (mutations, recovery) are not rules of this
  step.

## Choices settled by plan review round 2

- **The telemetry socket has a directory of its own (R2-C1, R2-S2, #74's round-1 item).** The
  lab uses `/run/flaminio-telemetry`, root:GID 0750; `/run/flaminio` holds tokens and can be
  0700. The daemon creates the directory without following a symlink. It refuses an existing
  one that is a symlink, has the wrong owner, mode or group, or is the control socket's
  directory. Under #74's `--privsep`, #74's helper binds the socket; that is a dependency on #74,
  not a mechanism here. Contracts states the rules once. (Changed in round 3: telemetry needs
  direct mode in this step.)
- **The observer's credential (R2-S1).** It is issued like every identity, server-only with the
  node IP (changed in round 3: no IP SAN). A root copy is installed in `/etc/flaminio/observer`
  (root:flaminio-observer 0750, key 0440). `/etc/flaminio/pki` stays root-only. Prometheus
  checks only the CA and the IP SAN. This is recorded as a residual. Settled 2026-11-13 (user's
  instruction): accepted for this step (see below). (Changed in
  rounds 3 and 4: the observer job checks the DNS name `observer.flaminio.invalid` and the
  metadata job the IP SAN; `/etc/flaminio/pki` is stated only as unreadable by the observer.)
- **Observing is bounded, not free (R2-C2).** At most one read-only chunk walk a second, and at
  most one pending request (R2-S5). Each engine turn already walks the chunks. The campaign
  polls at that rate. Incremental counts in the target are the fallback, not built now.
- **The accessor is `Target::recovery`.** `progress` already names a private method
  (`target.rs:2183`).
- **Metadata view outcomes (R2-C4).** An omitted volume exports nothing and leaves the view as
  it is. An invalid volume exports nothing and sets the view not ok, while the other volumes
  still export. This assumes #69 rejects invalid data per volume. (Changed in round 3: #69
  fails as a whole.)
- **Rules select their job; CI runs them through `make lint` (R2-S3, R2-C7).** (Changed in
  round 3: their own CI job.)
- **The CLI checks every observer string against a grammar (R2-S4).** Invalid strings make the
  node `invalid`.
- **The I/O-path negatives rest on review and the campaign (R2-C5c).** No allocation or lock
  test is added.

## Changes after approval

Plan review round 3, 2026-11-13:
- **The observer certificate has no IP SAN (R3-S1).** It has only the DNS SAN
  `observer.flaminio.invalid`, so no metadata or replica client accepts it. The CLI and the
  observer job check that name. Tests: Go `ClientConfig` and Rust `MetaClient` refuse it.
- **Telemetry needs direct mode in this step (R3-S2).** Under #74's `--privsep` it is
  unavailable until #74's helper provides the socket. The observer then reports the node as not
  fresh. The lab check runs direct mode.
- **The view fails as a whole on invalid data (R3-2).** This builds on #69's `Derive` as
  written. A per-volume rejection would need a change to #69; it is not a dependency.
- **S3 states the bound (R3-1).** At most one read-only walk a second under the engine lock,
  within the campaign's p99 limit: median p99 at most 10% above the off runs'.
- **Directory rules (R3-3, R3-S3).** D's parent is checked. `mkdir` is followed by `chmod`. The
  control directory is compared by device and inode. `control.rs` is unchanged.
- **Observer key and groups (R3-S4).** The key is root:flaminio-observer 0440, and
  `flaminio-observer` is the only member of its primary group. The lab check proves both.
- **`rules-test` is its own CI job (R3-5)**, which keeps `make lint` offline.
- **Scrape limits on both jobs, checked by a script test (R3-S5).**
- **Nits:** `release unparsable`; times parsed and re-formatted by the CLI; the ADR and
  contracts headers say accepted.

Plan review round 4, 2026-11-13:
- **Telemetry under `--privsep` is unavailable, not lost (R4-S1).** The helper request for the
  telemetry socket is a later step of #74, in neither plan's present step. The daemon refuses
  `--telemetry` with `--privsep`, with an exact message and a unit test, added by whichever of
  #71 and #74 lands second. The lab's `node.sh` dies with `TELEMETRY=1 needs PRIVSEP=0`. Privsep
  nodes run the observer with `--no-telemetry`, which exports
  `flaminio_observer_telemetry_available 0` (the bound becomes 74). `FlaminioTelemetryLost` does
  not fire for it, and the CLI lists the node as `unavailable`. This replaces round 3's "reports
  the node as not fresh". (Replaced in round 5: the separate state is cut.)
- **The Prometheus residual states its reach (R4-S2).** One observer key answers for every
  node's observer target. A free port needs no redirection. The capacity, stall and
  telemetry-lost alerts of any node can be raised or hidden. Settled 2026-11-13 (user's
  instruction): accepted for this step (see below).
- **Directory steps (R4-S3, R4-2).** Every ancestor is checked. The steps run after
  `control::spawn`, and a missing control directory exits 1. `chmod` is safe because only root
  can change an entry on the path. The tests inject the owner and use the test process's gid.
- **Lab checks cannot pass vacuously (R4-S4).** A probe token is planted, and primary-group
  members are checked through `getent passwd`. Startup checks only the certificate.
- **`/etc/flaminio/pki` (R4-S5).** It is stated only as unreadable by the observer, as #74 makes
  it 0750 for its group.
- **The scrape example is checked by a Go test (R4-1).** `TestPrometheusExample` parses strictly
  with `sigs.k8s.io/yaml` and runs mutated copies; no Makefile line is needed.
- **The Rust interop case is specified (R4-4).**
- **promtool's release is chosen when `rules-test` is written (R4-5).** The own job keeps the
  module out of `make lint`, and `docs/ci.md`'s table lists it.
- **Nits:** the ADR keeps its file name until moved. The `.invalid` name protects only while no
  metadata endpoint is configured by it.

Plan review round 5, 2026-11-13:
- **The "unavailable" state is cut (R5-S1, R5-S2, R5-1, R5-S6).** The observer took it from its
  own start flag, so it drifted from the daemon's real mode. The observer now reports what it
  sees. Under #74's `--privsep` it finds no socket: `snapshot_fresh 0`,
  `FlaminioTelemetryLost`, and `stale` in the CLI. `--no-telemetry`,
  `flaminio_observer_telemetry_available`, the `telemetry` field, the `unavailable` reason and
  `observer.sh`'s `PRIVSEP` branch are gone, and the bound is 73 again.
  `FlaminioTelemetryLost` also fires for a target that is up without a freshness series. The
  CLI refuses a `/v1/node` reply whose `disk` or `targets` disagree with `fresh`.
- **The `--telemetry` and `--privsep` refusal (R5-2).** The plan that adds the second of the two
  flags adds the refusal and its test, and the `node.sh` rule with its script test. #74 states
  the same rule. #71 lists no test it cannot run on its own.
- **A cluster of nodes without telemetry exits 3 (R5-3).** This is by design: the report is
  incomplete.
- **Step 1 walks PATH with `lstat` and refuses a symlinked ancestor (R5-4, R5-S5).** Passing
  tests walk the real `/` and `/tmp`, so they exercise the uid 0 arm.
- **The metadata job checks `meta.flaminio.invalid` (R5-S3).** The `meta` certificate gains that
  DNS SAN, so neither the `io` key (readable by a non-root process under #74) nor an observer key
  passes. #74's URI-SAN follow-up concerns metadata clients only.
- **The lab check asserts the running observer (R5-S4).** It checks the observer's
  `/proc/PID/status` (uids, gids, groups, capabilities) and `id -Gn`.
- **meta-interop's second server (R5-5).** It waits on its port over TCP, and the trap kills it.
- **Nits:** the forger can raise or hide `FlaminioTelemetryLost`. The per-node-name closure
  needs a case-preserving encoding and job relabelling. The `chmod` wording is fixed. The lab
  command today is `TELEMETRY=1 node.sh up`.

Plan review round 6, 2026-11-13:
- **The lab proves the served `meta` certificate (R6-B1).** The prerequisite is
  `make pki-issue meta-up`, since the metadata service loads its certificate only at start.
  Step 2 reaches every metadata instance as `meta.flaminio.invalid`.
- **The refusal test if #71 lands second (R6-C1).** It is the `--telemetry` case of #74's
  `privsep_refuses_helper_flags`, plus `node-scripts-test.sh`'s `TELEMETRY=1 PRIVSEP=1` case.
- **`--telemetry` needs an absolute path (R6-S1).** A relative one exits 1 at parse.
- **Nits:** the name is `observer.flaminio.invalid`. The passing directory tests create under
  `/tmp` explicitly, and `telemetry.rs` is Linux-only.
