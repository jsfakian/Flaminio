# Issue #71: Observability and the kubectl flaminio operations CLI — specification

Why: [intent](intent.md). Settled choices: [decisions](decisions.md).
How: [design](design.md) and [contracts](contracts.md). **Approved 2026-11-13; not implemented.**

## Terms

- **Observation:** a value read from a node's daemon or from the metadata service, with the time
  it was read. It never grants storage authority. Unknown, stale and unsupported are distinct
  from zero.
- **Request:** one ublk request committed by the node daemon's queues, counted once.
- **Attachment:** one volume served as a ublk device, from the device's start to its stop.
- **Reader:** a #69 reader identity, `spiffe://<td>/flaminio/reader/<name>`.
- **Observer:** the per-node process that reads the daemon's telemetry socket and serves the
  node's metrics and evidence.

## The invariant

Observation is read-only and can be removed. With telemetry off, or with an observer or scraper
down, slow or wrong, replication, acknowledgement, I/O results and every authorization stay as
they are. Reading grants no mutation, and no reader credential can mutate.

## Behaviour

**S1. Frontend I/O metrics.** The node daemon counts every request its ublk queues commit, in
both copy modes. This covers fast, queued, remote and failed reads, writes, flushes, discards
and unsupported operations. Each request is counted exactly once, at its commit, by operation
and result. Successful reads and writes add their length to the byte counts. Reads and writes
add their time from the request's arrival to its commit to a latency histogram. Counters belong
to one attachment and start at zero at its start. With telemetry off, no clock is read and
nothing is recorded. The MVP `flaminio-io serve` path is not instrumented.

**S2. Capacity and progress.** The node reports its store's device size, data capacity, and
allocated, quarantined and reusable data. Their sum is the data capacity. It also reports pool
pairs and used pairs. For each target it runs, it reports the chunks resyncing or reconciling
and the bytes those still have to copy. These come from the target's own state, never from
elapsed time or a guess. Disk health is not reported in this step.

**S3. Telemetry boundary.** Observations leave the daemon only through a telemetry socket that
answers `observe` and refuses every other line. The socket has a directory of its own, which only
its owner can write; neither is the control socket's. Telemetry needs direct mode in this step.
Under #74's `--privsep` there is none until a later step of #74: the observer finds no socket and
reports the node as not fresh, which raises lost telemetry. The observer runs without root, cannot
read `/etc/flaminio/pki`, and calls no service. Its certificate has only the server usage, and no
metadata or replica client, nor Prometheus's metadata job, accepts it. `observe` makes the engine
build a read-only snapshot at most once a second, with at most one request pending. That adds at
most one read-only walk of each target's chunks a second under the engine lock, no more than each
engine turn already does, within the campaign's p99 limit. It never fails I/O.

**S4. Metadata metrics.** A metadata instance can export, for each volume, its replicas, MinISR,
chunks and under-replicated chunks. A chunk is under-replicated when its committed ISR is shorter
than the configured replicas. A move's extra replica changes nothing. It can also export each
node's alive and draining flags. The values come from its own reads and #69's derivation. A read
that fails, exceeds #69's limits, or holds a volume #69 rejects as invalid exports no per-volume
or per-node series and sets the view to not ok. A volume deleted during the read exports
nothing and leaves the view as it is. It never exports zero or healthy instead.

**S5. Authorization and exposure.** Every metrics and evidence endpoint is TLS 1.3 with client
certificates. It admits only the exact SPIFFE IDs it is configured with and answers 403 to any
other. No endpoint accepts a write. The CLI checks each observer's exact server identity. Readers
need no administrator, node or Kubernetes credential. The CLI never looks for one.

**S6. CLI.** `kubectl flaminio status | node list | node get | disk list | disk get |
volume list | volume get | doctor` read the metadata service and the observers with explicit
reader files. Each prints a table or versioned JSON, sorted, with bounded collection. Exit codes
are distinct: 0 complete, 1 failed, 2 usage, 3 incomplete, 4 doctor found a failure. An
observer that is unreachable, refuses the reader, fails the identity check, is stale or answers
invalid data is listed as omitted, never shown as zero or healthy, so a node without telemetry
makes a report exit 3. A failed metadata read prints nothing on stdout.

**S7. Doctor.** For each node it reports `kernel` (at least 6.6, ADR 0003), `ublk`
(`/dev/ublk-control`), `io_uring` (`kernel.io_uring_disabled`) and `mtu` (equal on the data
interface across nodes). Each is pass, warn, fail or unknown, with evidence and its time.
`clock`, `latency`, `disk_health` and `config` report `unsupported` and name their owner. Doctor
writes nothing and changes no setting.

**S8. Alerts and runbooks.** Prometheus rules alert on lost redundancy, data or pool capacity
pressure, stalled resync or reconcile, and lost telemetry. Each rule links a runbook. Stalled
means work remains and its remaining bytes did not change for 10 minutes. Missing or stale data
raises lost telemetry, never a health or stall alert, and so does a target that is up but
exports no freshness series. No alert starts an action.

**S9. Bounds and cost.** An observer exports at most 73 series. A metadata instance exports at
most 4V + 2N + 2, with V volumes and N nodes bounded by #69's limits. With telemetry on, each
request costs two clock reads and at most four relaxed atomic adds; off, one branch. The I/O path
gains no lock, allocation, metadata call, wait or log line. The overhead is measured with
telemetry on and off, never claimed.

## Not in this step

Traces, structured logs, summary resources, Conditions and Events, `recover`, mutating verbs,
disk health, controller failures, rebuild progress and four doctor checks. The
[intent](intent.md) lists them; #71 stays open for them.

## Tests

- **S1:** a classifier test for every committed result; a recording test for buckets, sums and
  bytes; without counters, no clock read and no change; a lab check where counter deltas equal
  fio's totals in both copy modes, for local and remote reads.
- **S2:** `Target::recovery` while a resync and a reconcile run; the engine's `observe` against
  `Store::stats`; the lab check's resync drains to zero.
- **S3:** the telemetry socket refuses `attach`, `detach`, `delete`, `status` and anything else
  without touching the engine. Its mode and group are checked, and so are its directory's,
  including one created under umask 077. A relative path, a symlinked or open ancestor, a parent
  or directory with the wrong owner, a symlinked, non-directory or wrongly grouped directory, a
  missing control directory and the control directory under another spelling are refused. The
  one-second cache is tested, and so is a silent engine, which gets one pending request at most. A
  server-only certificate is refused as a TLS client, and Go and Rust metadata clients refuse an
  observer certificate. A shared golden line binds the Rust encoder to the Go decoder. The decoder
  is fuzzed.
- **S4:** full, short and moving ISR; failed, over-limit, invalid-volume and omitted-volume
  views export what contracts says.
- **S5:** TLS 1.2, an unlisted reader, a wrong trust domain, `POST` and unknown paths are
  refused. The observer refuses to start with a client, dual-use, other-node, other-component,
  IP-SAN or other-DNS-SAN certificate. The CLI refuses an observer certificate for another node
  and observer strings or times outside their grammar, and opens no file but the three its flags
  name. The scrape example's keys are checked, and mutated copies fail. An `observer` component
  is denied every metadata method but the three open reads.
- **S6:** golden table and JSON per command; the exit-code matrix; usage errors make no call;
  a metadata failure leaves stdout empty; an omitted observer gives exit 3; two nodes reporting
  one disk UUID are both listed.
- **S7:** checks over a fake root: kernel 6.5, 6.6, no MAJOR.MINOR and outside the grammar;
  ublk present and absent; io_uring 0, 1, 2 and missing; MTU equal, different and missing. The
  fake root is unchanged afterwards. In the lab, a broken io_uring and a broken MTU give exit 4.
- **S8:** promtool tests firing and not firing each rule, absent series included; another
  node's zero progress for a volume does not silence a stalled target's series; a metadata
  family scraped under the observer job fires nothing; a target that is up without a freshness
  series fires. CI's `rules` job runs them.
- **S9:** series counts at the bounds; `make race`; the lab's on/off overhead campaign, with
  `observe` polled once a second and its p99 limit. The I/O-path properties rest on review and
  the campaign.

## Open questions

The histogram bounds, alert thresholds and hold times, ports and allowlists are proposals. Can
the scrape identity be a #69 reader, which can also read metadata? A reattach can hide a counter
reset, as a process restart can. Is that acceptable? This step depends on #69's reader identity
and derivation.

Settled at approval, 2026-11-13: the contracts' bounds, thresholds, hold times, ports and
allowlists; the scrape identity is a #69 reader; the reattach limitation is accepted; the #69
dependency stands, on #69 as approved the same day (see decisions.md, Settled at approval).
