# Observation is read-only, pulled over mTLS, and never storage authority

- **Status:** Accepted with the plan, 2026-11-13. It binds this plan's implementation from the
  approval on. It keeps this file name until it is numbered and moved into docs/adr/ with the
  implementation.
- **Date:** 2026-11-12, revised 2026-11-13 after plan review rounds 1 and 3
- **Issue:** [#71](https://github.com/jsfakian/Flaminio/issues/71)

## Context

Architecture §18 promises metrics, traces, Events and the operations plugin. Today the data
plane's counters stay in the process. Its only local interface is the root-only control socket,
which can attach, detach and delete (`dataplane/crates/flaminio-io/src/control.rs:83–111`). Any
authenticated identity may read volumes and nodes (`meta/service/auth.go:79–80`). Plan review
round 1 found that a push collector needs replay ordering and cross-replica freshness. It also
found that an observer holding the control socket and an `agent` certificate is not least
privilege ([decisions](decisions.md)). This record covers step 1 of the [intent](intent.md).

## Decisions

- **Telemetry socket.** The node daemon serves observations on its own socket. It answers only
  `observe`, sits in a directory of its own that only its owner can write, gets its access
  from a configured group, and is off by default. It needs direct mode in this step: under #74's
  `--privsep` there is no telemetry until a later step of #74. The control socket is
  unchanged.
- **Observer.** A per-node observer reads that socket without root. It serves `/metrics` and
  `/v1/node` over TLS 1.3 with client certificates and an exact allowlist. Its identity
  `node/<n>/observer` has only server usage and only the DNS SAN `observer.flaminio.invalid`.
  So it is no client of the metadata service or the replicas, and none of their clients accepts
  it as a server.
- **Pull.** Prometheus and the CLI pull from every observer and metadata instance. There is no
  push, collector or producer sequence. A node's series take the `node` label from the scrape
  target.
- **Metadata listener.** A metadata instance can serve cluster metrics (ISR, nodes) on a
  separate allowlisted listener. It is off by default and computed from its own reads with #69's
  derivation.
- **Readers.** A read-only user and the scraper use #69 reader identities. Nothing in this step
  reads or needs an administrator credential, and no endpoint accepts a write.
- **Counting.** Frontend requests are counted once, at their commit. A pure classifier,
  per-attachment counters and relaxed atomics do it, with no lock, allocation or wait on the I/O
  path.

## Options

- **The daemon serves HTTPS itself.** This avoids a process, but it puts a TLS server and an
  HTTP parser in the root I/O process. Rejected.
- **Producers push to a collector.** One scrape target, but it needs instance ordering, replay
  refusal and one publisher, or freshness differs per replica (round 1, R1-S4, R1-S9). Rejected.
- **A separate socket and a pulled per-node observer.** One more process per node, and each node
  is a scrape target. Selected: every series is bound to its scrape target, and failures only
  remove series. Prometheus binds an observer to its node only by scrape address. One observer's
  key answers for every node's observer target, and can raise or hide that node's capacity,
  stall and telemetry-lost alerts. This is a residual the design records.
- **A reader-identity cluster exporter instead of the metadata listener.** It would need a new
  deployment and a server certificate for a reader identity. Not chosen. The metadata listener is
  the smaller change, and its load is an open question.
  Settled 2026-11-13 (user's instruction): the metadata listener as designed, changed only on a
  measured cost (see decisions.md).

## Consequences and open choices

- **Exposure.** Labels carry volume ids, disk UUIDs and node ids. They are administrator-visible,
  not tenant-isolated telemetry.
- **The scrape identity.** A #69 reader can also call the metadata service's open reads (open).
  Settled at approval, 2026-11-13: the scrape identity is a #69 reader (see decisions.md,
  Settled at approval).
- **Proposals, not measured limits.** The bounds, buckets, thresholds and ports in
  [contracts](contracts.md) are proposals. The overhead is measured before any budget is set.
  Settled at approval, 2026-11-13: the values in contracts (see decisions.md, Settled at
  approval). Settled 2026-11-13 (user's instruction): the budget is set from the measured
  overhead campaign, whose pass rule stands until then (see decisions.md).
- **Later steps decide their own shapes.** Traces and logs, summary resources with Conditions
  and Events, `recover` (#54) and the remaining doctor checks each decide their own. The
  summaries step must reconcile with §17.2's `Disk`, which carries desired state. Before it
  chooses the kinds' scope, it checks with envtest where Kubernetes accepts Events about
  cluster-scoped objects. Settled 2026-11-13 (user's instruction): that envtest check decides
  where those Events are accepted (see decisions.md).
- **No accepted ADR is superseded.** Epochs, fencing and ISR acknowledgement stay binding.

## Confirmation

The [design](design.md) maps S1–S9 to unit, integration, promtool and lab tests. These cover a
socket that refuses every other command, a server-only certificate refused as a client, an
observer certificate refused by metadata clients, allowlist denials, and counter deltas equal to
fio's. It was accepted with the plan on 2026-11-13. No implementation, test result or
measurement is recorded.
