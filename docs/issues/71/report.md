# Issue #71 — report

## Review rounds

### Plan review round 1 — 76cf82f7ae03bab66a9410264470a6f3991715de

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71c1 (design-code), r71s1 (design-security).

- **R1-C1 — High:** Events about the cluster-scoped summaries cannot be created or read in
  `flaminio-observability`. Evidence: design.md:67, 84–87; contracts.md:91, 121–125; the
  Kubernetes namespace rule was recalled, not verified.
- **R1-C2 — Medium:** the node observer's `agent` certificate may do more than telemetry.
  Evidence: `meta/service/auth.go:59–78`; contracts.md:73, 126–127.
- **R1-C3 — Medium:** the snapshot owner and handoff ignore how the engine is owned (main
  thread vs queues, per-queue counters, control thread state). Evidence: `ublk.rs:507`,
  `785–790`; `node.rs:203`; `control.rs:67–73`.
- **R1-C4 — Medium:** sampled spans have no path out of the daemon, and Rust logs have no
  approach. Evidence: contracts.md:53–65, 80–87; `eprintln!` at `ublk.rs:1579`, `1686`.
- **R1-C5 — Medium:** recovery progress has no producer. Evidence: `target.rs:109`, `144–171`;
  design.md:36–37.
- **R1-C6 — Medium:** volume incarnation and cluster identity are not readable through the
  collector's interface. Evidence: `meta.proto:117–127`; `domain.go:47`.
- **R1-C7 — Medium:** S8's backend preconditions have no contract and belong to other owners.
  Evidence: `meta.proto:163–165`, `240–245`; design.md:108–112.
- **R1-C8 — Medium:** `recover plan/apply --confirm` does not match #54's drafted contract.
  Evidence: contracts.md:152–153, 174–185; #54 contracts.md:25–41, 89–100.
- **R1-C9 — Medium:** multiple push collectors are not safe idempotent publishers. Evidence:
  design.md:71–73.
- **R1-C10 — Medium:** the frontend exactly-once tests need a ublk device and have no seam.
  Evidence: `ublk.rs:1207–1288`, `1360–1462`, `1747–1768`.
- **R1-C11 — Medium:** four doctor checks have no mechanism. Evidence: design.md:98–103;
  contracts.md:111–118.
- **R1-C12 — Low:** the series bound undercounts. Evidence: contracts.md:41–44.
- **R1-C13 — Low:** exit code 2 means usage and doctor failure. Evidence: contracts.md:169–170.
- **R1-C14 — Low:** `io_errors_total` reasons do not map to what the frontend sees. Evidence:
  `engine.rs:241–250`; `ublk.rs:1259–1280`.
- **R1-C15 — Low:** `disk get` and `volume get` have no lookup. Evidence: contracts.md:92–94;
  `domain.go:87`.
- **R1-C16 — Low:** retirement is undefined for Disk and StorageNode. Evidence:
  `meta.proto:15–61`; design.md:81–82.
- **R1-C17 — Low:** a summary `Disk` diverges from §17.2's `Disk`. Evidence:
  `docs/ARCHITECTURE.md:893–905`.
- **R1-C nit 1:** spec.md:3 links a missing `decisions.md`.
- **R1-C nit 2:** design.md:143 "control/node" reads as the Go tree.
- **R1-C nit 3:** quarantined clusters are free bits, not allocated ones. Evidence:
  `bitmap.rs:10`, `61–63`.
- **R1-C nit 4:** the MVP `serve` path is neither instrumented nor declared out of scope.
  Evidence: `ublk.rs:166–350`.
- **R1-S1 — High:** preconditions added as fields to existing RPCs fail open on an older
  server. Evidence: `meta.proto:163–165`; `domain.go:44–47`; design.md:108–112.
- **R1-S2 — High:** the node observer holds the root control socket and an `agent` certificate.
  Evidence: `control.rs:101–102`, `172–189`; `engine.rs:696–703`; `auth.go:58–78`.
- **R1-S3 — Medium:** what a node may assert in its Push is not scoped or budgeted (disks,
  volume flood, recovery gauges). Evidence: contracts.md:30–32, 42–45, 73, 92–94.
- **R1-S4 — Medium:** replay protection across producer restarts has no mechanism. Evidence:
  contracts.md:54, 69–71.
- **R1-S5 — Medium:** Events RBAC cannot work for cluster-scoped summaries without widening the
  reader. Evidence: contracts.md:121–125 (rule recalled, not verified).
- **R1-S6 — Medium:** destructive non-recovery verbs have no plan generator. Evidence:
  contracts.md:149–152.
- **R1-S7 — Medium:** "wrong cluster fails closed" has no cluster identity. Evidence:
  `superblock.rs:28`; design.md:107.
- **R1-S8 — Medium:** the span export path and its trust boundary are unspecified. Evidence:
  design.md:40–45; contracts.md:54–65.
- **R1-S9 — Medium:** freshness differs between collectors that each receive part of the
  producers. Evidence: design.md:72, 77–78.
- **R1-S10 — Medium:** reader rights over recovery status contradict #54 and #69, and the
  grammar does not match #54. Evidence: contracts.md:153; design.md:156.
- **R1-S11 — Low:** deletion of only retired summaries is not expressible in RBAC. Evidence:
  contracts.md:94, 123.
- **R1-S12 — Low:** the scrape credential's admission rule is vague. Evidence:
  contracts.md:75.
- **R1-S nit 1:** spec.md:3 links a missing `decisions.md` (same as R1-C nit 1).
- **R1-S nit 2:** a `node/<n>/meta` Push needs a client certificate, while the lab issues it
  for server use only. Evidence: `scripts/lab.sh:583`.

Changes/resolution: The plan is narrowed to step 1 of #71: read-only metrics, read CLI commands,
a first doctor and alert rules (intent "This step" and "What stays open"; `decisions.md`
created). Spec rules are renumbered S1–S9. Per finding:
- R1-C1, R1-S5, R1-S11, R1-C16, R1-C17: summaries, Conditions and Events are cut to a later step
  that depends on #43. The recalled Events claim becomes an open item for that step, to be
  checked with envtest before it picks the kinds' scope. The ADR proposal notes the §17.2 `Disk`
  reconciliation.
- R1-C2, R1-S2: a separate telemetry socket answers only `observe` (mode 0660, configured
  group). The observer runs without root, with a server-only `node/<n>/observer` certificate,
  and is no TLS client. Tests: the socket refuses every other command, a server-only certificate
  is refused as a client, and an `observer` component is denied all but the open reads.
- R1-C3: `Input::Observe` is answered by whoever drives the engine (main loop or queue pump), as
  `delete` is. The socket thread caches the engine snapshot for 1 s. One `IoCounters` per
  attachment is shared by all queues (nothing to merge), published through `Attached`, and
  starts at zero at attach. No lock is added.
- R1-C4, R1-S8: traces and structured logs are cut to step 2; no log line changes.
- R1-C5: `Target::progress` (read-only) feeds resync and reconcile chunks and remaining bytes
  through the engine snapshot; unit and integration tests and a lab drain-to-zero step.
  "Stalled" is defined over remaining bytes; eligibility and progress age are dropped.
- R1-C6, R1-S7: no incarnation or cluster identity in names or labels; series keyed by (node,
  volume id) or (node, disk UUID); plans are cut.
- R1-C7, R1-S1, R1-S6: all mutating verbs and conditional preconditions are cut; no RPC or
  `auth.go` change remains.
- R1-C8, R1-S10: `recover` is cut until #54's API exists; reader rights are unchanged here.
- R1-C9, R1-S9, R1-S4: push and the collector are cut. Prometheus and the CLI pull each observer
  and metadata instance; no producer sequence or cross-replica freshness exists.
- R1-C10: one `record` call at each of the two commit sites, a pure classifier with unit tests,
  and lab check `scripts/checks/observe.sh` comparing deltas with fio in both copy modes and for
  remote reads. The lab check proves exactly-once; the unit tests prove the mapping.
- R1-C11: doctor implements kernel, ublk, io_uring and cross-node MTU from read-only files.
  Clock, latency, disk health and config report `unsupported` and are listed as open in the
  intent.
- R1-C12: the bounds are recomputed per family with a count column: 73 per observer,
  4V + 2N + 2 per metadata instance.
- R1-C13: doctor failures exit 4; 2 is usage only; the precedence is fixed.
- R1-C14: results come from the committed `res` only (`io_error`, `invalid`, `unsupported`);
  `IoError` is unchanged.
- R1-C15: `volume get` calls `GetVolume`; `disk get` filters every observer's disk by UUID and
  prints every match.
- R1-S3: a node's series take `node` from the scrape target. Disks are keyed by (node, UUID).
  The line allows one attachment and at most 4 targets, and `sample_limit` bounds a target. Tests
  were added for a shared disk UUID, an oversized snapshot and another node's zero progress.
- R1-S12: exact SPIFFE allowlists on both servers; an unlisted reader gets 403 (tested). That
  the scrape identity, a #69 reader, can also read metadata is recorded as an open question.
- Nits: `decisions.md` created (R1-C nit 1, R1-S nit 1). The design names `control.rs` and
  `node.rs`. Capacity is stated as allocated + quarantined + reusable. The MVP `serve` path is
  declared out of scope. The `node/<n>/meta` Push is cut with push (R1-S nit 2).

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — da0177afcf1ee397976f3ba904714105d671695d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71c2 (incremental design-code), r71s2 (incremental design-security). Both found
every round-1 finding fixed or cut.

- **R2-C1 — Medium:** the default telemetry socket sits in `/run/flaminio`, which the lab makes
  0700 and fills with tokens, so the observer cannot reach it. Evidence:
  `scripts/guest/node.sh:61–62`; `control.rs:46`.
- **R2-C2 — Medium:** the snapshot is built under the engine mutex with an unstated cost, and
  "never delays I/O" has no test. Evidence: `ublk.rs:586–590`; `extent.rs:65–68`;
  `target.rs:385`.
- **R2-C3 — Low:** the off path still reads the clock. Evidence: design.md "Seam".
- **R2-C4 — Low:** S4 does not cover #69's omitted volumes or per-volume invalid data.
  Evidence: #69 design.md "Collection and derivation".
- **R2-C5 — Low:** three rules have no test that could fail: nothing recorded when off, the CLI
  never looking for credentials, and no lock or allocation on the I/O path. Evidence:
  design.md tests table.
- **R2-C6 — Low:** the lab steps omit `--direct=1` and the MinISR of the RF2 volume. Evidence:
  `scripts/checks/resync.sh:33`.
- **R2-C7 — Low:** `rules-test` is not wired into CI. Evidence: `.github/workflows/ci.yml`.
- **R2-S1 — Medium:** the non-root observer's credential has no issuance or file access, and
  Prometheus cannot check its URI SAN. The startup identity check is untested. Evidence:
  `scripts/guest/pki.sh:8–10`, `15–16`; `scripts/lab.sh:579`, `583–584`.
- **R2-S2 — Low:** the socket's directory is unspecified. A group-writable directory would let
  the group swap the socket for a symlink before root's chmod and chown. Evidence:
  `control.rs:49–52`; `deploy/csi/driver.yaml:132`.
- **R2-S3 — Low:** the alert rules select no job, so one observer could raise a cluster-level
  alert. `honor_labels: false` is relied on without being stated. Evidence: contracts.md:203,
  209.
- **R2-S4 — Low:** the CLI prints observer-supplied `evidence` and `interface` without checking
  them. Evidence: contracts.md:120–128, 140–143, 183–185.
- **R2-S5 — Low:** after an engine timeout, `Observe` inputs pile up in the unbounded engine
  channel. Evidence: `flaminio-io/src/node.rs:288`.
- **R2-S nit:** `volumes_recovering` counts any observer's progress, while `volume` takes only
  the target's. Evidence: contracts.md:168.
- **Cross-plan item (#74, round 1, 96d792d):** under `--privsep` the unprivileged I/O process
  cannot bind the telemetry socket in root's directory.

Changes/resolution:
- R2-C1, R2-S2, #74 item: contracts "Directory and socket" states the rules once. The socket gets
  a directory of its own (the lab's is `/run/flaminio-telemetry`, root:GID 0750). The daemon
  creates it with `mkdir`, which never follows a symlink, and refuses an existing directory that
  is a symlink, has the wrong owner, mode or group, or is the control socket's directory. It
  binds, then sets the socket's mode to 0660 and its group with `lchown`. Under #74's
  `--privsep`, #74's helper binds it, as a dependency on #74. Tests:
  `refuses_an_unsafe_directory`, and `mode_and_group` now covers the directory. Lab step 1
  checks that the observer connects.
- R2-S1: the observer certificate is issued server-only with the node IP. `observer.sh`
  installs a copy in `/etc/flaminio/observer` (root:flaminio-observer 0750, key 0440), and
  `/etc/flaminio/pki` stays root-only. The scrape example verifies the CA and IP SAN with no
  `insecure_skip_verify` or `server_name`. The residual (any server certificate for the node IP
  passes Prometheus) is recorded under Risks, not accepted. Added `TestStartupRefusesIdentity`,
  and lab step 1 now checks that `flaminio-observer` cannot reach `node.sock` or
  `/etc/flaminio/pki`.
- R2-C2: the cost is stated: one read-only walk of at most 2^20 chunks per target
  (`domain.go:184`), at most once a second, the same walk each engine turn already does
  (`engine.rs:735–741`, `target.rs:862`, `2187–2189`). S3 now claims this bound instead of "never
  delays". The campaign polls `observe` once a second. Incremental counts are the fallback
  (open).
- R2-C3: `ctx.start()` returns `None` without reading the clock when there are no counters. S1
  and S9 say so.
- R2-C4: contracts has a view-outcomes table. An omitted volume exports nothing and leaves the
  view as it is. An invalid volume exports nothing and sets the view not ok, while the others
  still export. `TestViewOutcomes` covers each row. #69 is assumed to reject invalid data per
  volume.
- R2-C5: added `nothing_without_counters` and `TestOnlyFlagFilesOpened`. The I/O-path negatives
  are stated to rest on review and the campaign.
- R2-C6: the lab steps use `--direct=1`, `create VOL 16G 2 1 256M fl-a` (MinISR 1) and
  `create VOL2 16G 1 1 256M fl-b`.
- R2-C7: `make lint` runs `rules-test`, so CI's lint job does. `PROMTOOL` is a pinned `go run`,
  as `BUF` is (`Makefile:13`).
- R2-S3: every rule selector names its job. The example states `honor_labels: false`. A promtool
  case checks that a metadata family under the observer job fires nothing.
- R2-S4: the CLI checks `evidence` against per-check grammars (now in the doctor table) and
  `interface` against IF's grammar, and treats a mismatch as `invalid`. Added
  `TestObserverStringsChecked`.
- R2-S5: the socket thread keeps at most one pending `Observe`, waits on it rather than sending
  another, and fills the cache from a late answer. Added `one_observe_pending_at_most`.
- R2-S nit: `volumes_recovering` counts only each volume's target observer.
- Also corrected: the accessor is renamed `Target::recovery`, because `progress` already names a
  private method (`target.rs:2183`).

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 215748eaab772b53f1b3fe3f539b772ca4b59494

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71c3 (incremental design-code), r71s3 (incremental design-security). Both found
the round-2 findings fixed; R2-C2 was partly fixed, and R2-C4 rested on an assumption #69
contradicts. The plan was approved on 2026-11-13, so these fixes come after the approval.

- **R3-1 — Medium:** spec S3 still says "never blocks or fails I/O", while the walk runs under
  the engine lock. No test or campaign limit bounds it. Evidence: spec.md:45; design "Cost".
- **R3-2 — Medium:** the "invalid volume" row assumes per-volume rejection, while #69's
  `Derive` fails as a whole. Evidence: flaminio-69 design.md:39, 47–50.
- **R3-3 — Low:** the directory rules leave the umask, the control-directory comparison and the
  `control.rs` scope unstated. Evidence: `control.rs:46–52`.
- **R3-4 — Low:** lab step 3 cites `resync.sh:33`, an RF1 template. Evidence:
  `scripts/checks/resync.sh:33`; `meta/cmd/flaminio-meta/main.go:7`.
- **R3-5 — Low:** `make lint` would depend on a large `go run` with an open pin. Evidence:
  `Makefile:13`; decisions.md:45.
- **R3-C nit:** the directory paragraph mixes the no-group variant into a long sentence.
- **R3-S1 — High:** the observer's server key for the node IP passes every metadata client's
  CA-plus-host check, so a compromised observer could pose as that node's metadata instance.
  Evidence: `pkg/pki/pki.go:300–311`; `flaminio-metaclient/src/lib.rs:85–92`;
  `flaminio-node/src/engine.rs:1536`.
- **R3-S2 — Medium:** neither #71 nor #74 specifies the telemetry socket under `--privsep`.
  Evidence: 74 design.md:75, 134, 141–146, 228–230.
- **R3-S3 — Low:** D's parent is unchecked, `mkdir` is subject to the umask, and the control
  directory comparison is undefined. Evidence: `flaminio-io/src/node.rs:82`, `177`.
- **R3-S4 — Low:** the key's group and the observer's group membership are unstated, and the
  lab check does not prove them. Evidence: design.md:89–90; contracts "Observer files".
- **R3-S5 — Low:** the scrape example bounds only the observer job, and nothing tests
  `honor_labels: false` or the TLS keys. Evidence: contracts "Scraping".
- **R3-S nit 1:** there is no kernel evidence for an `osrelease` outside the grammar.
- **R3-S nit 2:** the CLI prints the observer's `checked_at` unchecked.
- **R3-S nit 3:** the ADR title and contracts header still say Proposed after acceptance.

Changes/resolution:
- R3-S1: the observer certificate has server usage only, the one DNS SAN
  `observer.flaminio.invalid` and no IP SAN, so no metadata or replica client accepts it (stated
  once in contracts "Observer certificate"). The CLI and the observer job check that name, and
  the CLI also checks the URI SAN. The startup check refuses an IP SAN or another DNS SAN. Tests:
  `TestObserverCertRefusedByMetadataClients` (Go) and `refuses_an_observer_certificate` (Rust
  `MetaClient`, `make meta-interop`). The residual is rewritten in design "Risks", the ADR and
  decisions.
- R3-S2: contracts "Under #74's `--privsep`" says telemetry needs direct mode in this step.
  Under privsep the observer reports the node as not fresh; with both flags the non-root process
  fails the directory step 1 and exits 1. The lab check runs direct mode. No helper request is
  specified here.
- R3-2: the view-outcomes table and S4 follow #69 as written: an invalid volume fails the
  view as a whole. `TestViewOutcomes` tests that. The consequence is a Risks item, and per-volume
  rejection would need a change to #69.
- R3-1: S3 and design "Cost" state the same bound: at most one read-only walk a second under
  the engine lock, and it never fails I/O. The campaign passes only if each job's median p99
  with telemetry is at most 10% above its median p99 without. A breach builds the fallbacks.
- R3-3, R3-S3: the directory rules are a numbered list. D's parent must be owned by the
  daemon's uid and closed. `mkdir` is followed by `chmod` and `chown`. The control directory is
  compared by `(st_dev, st_ino)`. `control.rs` is unchanged. A mode table replaces the run-on
  sentence (R3-C nit). The tests add umask 077, a regular file, a wrong group, an open parent and
  a symlinked control directory.
- R3-S4: the key is root:flaminio-observer 0440, and `flaminio-observer` is the only member of
  its primary group, with `flaminio-telemetry` supplementary. Lab step 1 checks the key's mode,
  the group's members, and that the observer cannot write D or read tokens.
- R3-4: lab step 3 cites `meta/cmd/flaminio-meta/main.go:7` for the argument order.
- R3-5: `rules-test` is its own target and CI job `rules`, so `make lint` stays offline. The pin
  is chosen when the target is written, and stays open.
- R3-S5: both jobs get label, sample and body limits (`flaminio-meta`: `sample_limit` 25000). The
  new `scripts/tools/prometheus-example-test.sh`, run by `make lint`, checks the keys,
  `honor_labels: false` and the absence of `insecure_skip_verify`.
- R3-S nit 1: added `release unparsable`.
- R3-S nit 2: the CLI parses `T` and re-formats it; `TestObserverStringsChecked` includes a `T`
  with control bytes.
- R3-S nit 3: the ADR title, its "Decisions" heading, its Confirmation and the contracts header
  now say accepted.
- Approval consistency: wherever spec, design and the ADR still presented a settled question as
  open, a "Settled at approval" or "Still open after approval" line was added. The ADR's status
  says it binds the implementation from the approval on. decisions.md says, under Approvals,
  that these fixes came after the approval, and lists them in "Changes after approval".

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 51cdf542d3f364de75f23e7b06dc580424b157e2

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71c4 (incremental design-code), r71s4 (incremental design-security). The observer
certificate fix holds against every Go and Rust TLS configuration in the tree. The plan was
approved on 2026-11-13, so these fixes come after the approval.

- **R4-1 — Medium:** `prometheus-example-test.sh` has no parsing method and no negative
  fixture, and the Makefile `lint` line is not listed. Evidence: `Makefile:92–94`;
  `scripts/tools/node-scripts-test.sh:1–9`.
- **R4-2 — Low:** the directory steps leave the order against `control::spawn`, the reach of
  the symlinked-control case and the test user's group unstated. Evidence: `control.rs:41–48`;
  `flaminio-io/src/node.rs:177`.
- **R4-3 — Low:** decisions.md still says Prometheus checks only the CA and the IP SAN.
  Evidence: decisions.md:114–116.
- **R4-4 — Low:** the Rust interop case's setup is unspecified, and `meta-interop` is not in
  `make test`. Evidence: `scripts/dev/meta-interop.sh:44–46`; `Makefile:128`, `413`.
- **R4-5 — Low:** "`make lint` stays offline" is not a reason, the promtool pin is open, and
  `docs/ci.md`'s table lacks the job. Evidence: `Makefile:13`; `docs/ci.md:13`.
- **R4-C nit 1:** the ADR keeps its "proposal" file name after acceptance.
- **R4-C nit 2:** the `.invalid` name protects only by convention.
- **R4-S1 — Medium:** `--privsep --telemetry` fails only by accident, nothing tests it, and the
  helper's part has no owner. With #74's lab default `PRIVSEP=1`, `FlaminioTelemetryLost` would
  fire on every privsep node. Evidence: 74 design.md:153–154, 287–289; 74 decisions.md:90–91,
  129–130.
- **R4-S2 — Low:** the Prometheus residual omits its reach (one key for every node's target, a
  free port) and the alerts it affects.
- **R4-S3 — Low:** the directory steps trust P's ancestors, leave step 4's order open, and omit
  the owner cases from the tests. Evidence: `control.rs:46–48`.
- **R4-S4 — Low:** the observer user and group checks can pass vacuously, and the startup checks
  of its user and files are undefined. Evidence: `scripts/guest/node.sh:61–63`.
- **R4-S5 — Low:** "`/etc/flaminio/pki` stays root-only 0700" contradicts #74, which makes it
  0750 for `flaminio-io`. Evidence: 74 design.md:157–158; `scripts/lab.sh:584`.
- **R4-S nit:** the stale decisions.md sentence (same as R4-3).

Changes/resolution:
- R4-S1: contracts "Under #74's `--privsep`" states once that the helper request for the
  telemetry socket is a later step of #74, in neither plan's present step:
  - `flaminio-io node` refuses `--telemetry` with `--privsep` at parse, with an exact message,
    and `telemetry_refused_under_privsep` tests it. Whichever of #71 and #74 lands second adds
    the check.
  - The lab's `node.sh` dies with `TELEMETRY=1 needs PRIVSEP=0`.
  - Privsep nodes run the observer with `--no-telemetry`. It exports
    `flaminio_observer_telemetry_available 0` and no snapshot series (the bound becomes 74), and
    `/v1/node` says `"telemetry":"unavailable"`.
  - `FlaminioTelemetryLost` does not fire for it, and a promtool case checks that. The CLI lists
    the node as `unavailable`.
- R4-S2: design "Risks" and the ADR state the reach and the free-port case. They name the
  affected alerts: `FlaminioCapacityPressure` (both levels) and `FlaminioRecoveryStalled` raised
  or hidden, and `FlaminioTelemetryLost` hidden. The residual stays open, with one DNS SAN and
  one job per node as the named way to close it.
- R4-S3, R4-2: the directory steps run after `control::spawn` and before serving.
  - Step 1 checks every ancestor of D (owned by root or U, closed unless sticky).
  - Step 4 exits 1 if the control directory, the parent of `--control`, cannot be `stat`ed.
  - `chmod` is safe because only root can change an entry on the path.
  - The tests inject U and use the test process's own gid. They add an open ancestor, a
    wrong-owner parent, a wrong-owner directory, a missing control directory, and the control
    directory spelled `T/x/../ctl` for step 4.
- R4-S4: lab step 1 plants a probe token as `node.sh` does, then checks that the observer cannot
  read it. It also checks primary-group members through `getent passwd`. The observer's startup
  checks only the certificate; the design's sentence now says so.
- R4-S5: contracts says only that `flaminio-observer` cannot read `/etc/flaminio/pki`; it is in
  neither root's group nor #74's `flaminio-io`. The `io`-key point goes to #74.
- R4-1: `scripts/tools/prometheus-example-test.sh` is replaced by the Go test
  `TestPrometheusExample` in `control/observe`. It parses the example with
  `sigs.k8s.io/yaml.UnmarshalStrict` into a struct that models every key, and five mutated
  copies must fail. It runs in `go test`, so no Makefile line is needed.
- R4-4: `meta-interop.sh` starts a second memory-store metadata server with an observer
  certificate and sets `FLAMINIO_META_INTEROP_OBSERVER`. `refuses_an_observer_certificate`
  expects `Answer::Failed`, never `Answer::ListNodes`. The design says `meta-interop` runs under
  `make contracts` and `make check`, not `make test`.
- R4-5: promtool's release is chosen when `rules-test` is written (still open after approval).
  The reason for the own job is now that it keeps the Prometheus module out of every
  `make lint`. `docs/ci.md`'s table gains the `rules` job.
- R4-3, R4-S nit: the decisions.md sentence is annotated with what each job checks.
- R4-C nit 1: the ADR status line says it keeps this file name until it is moved.
- R4-C nit 2: contracts says the protection holds while no metadata endpoint is configured by
  that name; the lab's endpoints are IPs.
- decisions.md says under Approvals that these fixes came after the approval, and lists them
  under "Changes after approval".

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 9dea4a9c0fe425adee46b8d620e59c0936daf34a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71c5 (incremental design-code), r71s5 (incremental design-security). Most findings
are about round 4's "unavailable" state. The plan was approved on 2026-11-13, so these fixes
come after the approval.

- **R5-1 — Medium:** the new "unavailable" state (the `telemetry` field, `--no-telemetry`, the
  `unavailable` reason, the `PRIVSEP` branches) has no decode, CLI or script tests.
- **R5-2 — Medium:** the `--telemetry`/`--privsep` refusal cannot be tested if #71 lands first.
  `--privsep` does not exist yet. Evidence: `dataplane/crates/flaminio-io/src`;
  `scripts/guest/node.sh:50–66`.
- **R5-3 — Low:** a privsep cluster can never exit 0, and the plan does not say whether that is
  intended.
- **R5-4 — Low:** the step-1 tests cannot reach the uid 0 arm, and a symlinked ancestor is not
  discussed.
- **R5-5 — Low:** meta-interop's trap kills only the first server, and the readiness wait uses
  the first port. Evidence: `scripts/dev/meta-interop.sh:7`.
- **R5-C nit:** the lab command run today, before #74's `PRIVSEP` exists, is unspecified.
- **R5-S1 — Medium:** "unavailable" comes from the observer's own `PRIVSEP` flag, not from the
  daemon's mode. The two drift in ordinary lab use (`scripts/lab.sh:1495–1513`; 74
  design.md:178, 190–191). The drift gives either a permanent alert or a silently unmonitored
  node.
- **R5-S2 — Low:** `FlaminioTelemetryLost` is silenced by a missing series, not by the explicit
  gauge. Evidence: contracts.md:291, 296–297.
- **R5-S3 — Low:** the metadata job accepts the co-located `io` key, which has the same IP SAN.
  #74's URI-SAN follow-up cannot close that for Prometheus. Evidence: `scripts/lab.sh:583–584`;
  74 design.md:184, 301–306.
- **R5-S4 — Low:** the lab check tests the observer account, not the running process.
- **R5-S5 — Low:** step 1 names neither `stat` nor `lstat` for a symlinked ancestor.
- **R5-S6 — Low:** the CLI's validation of the `telemetry` field is unspecified and untested.
- **R5-S nits:** "hide" against "raise or hide" for `FlaminioTelemetryLost`; the per-node-name
  closure needs a case-preserving encoding and job relabelling; decisions' `chmod` wording.

Changes/resolution:
- R5-S1, R5-S2, R5-1, R5-S6: the separate state is cut.
  - The observer now reports what it sees. Under #74's `--privsep` it finds no socket, so it
    exports `snapshot_fresh 0`, `FlaminioTelemetryLost` fires, and the CLI shows `stale`.
  - `--no-telemetry`, `flaminio_observer_telemetry_available`, the `/v1/node` `telemetry` field,
    the `unavailable` reason and `observer.sh`'s `PRIVSEP` branch are removed. The series bound
    is 73 again.
  - `FlaminioTelemetryLost` now also fires for a target that is up but exports no freshness
    series, with promtool cases that must fire.
  - The CLI treats a `/v1/node` reply whose `disk` or `targets` disagree with `fresh` as
    `invalid` (in `TestObserverStringsChecked`). `TestExitCodes` covers a `stale` node whose
    checks are still listed.
  - The `telemetry-lost` runbook names a privsep node as an expected cause.
- R5-2: contracts says once that the plan adding the second of `--telemetry` and `--privsep`
  adds the parse refusal and its test, and the `node.sh` rule with a `node-scripts-test.sh`
  case. #74 states the same rule. #71's tests list nothing it cannot run on its own.
- R5-3: contracts and S6 say a node without telemetry is `stale`, so the report exits 3. A
  cluster of such nodes exits 3 by design, and a script cannot tell such a node from one whose
  daemon stopped.
- R5-4, R5-S5: step 1 walks PATH as given with `lstat` and refuses a symlinked ancestor
  (`ancestor A is a symlink`). The lab uses `/run`. `refuses_an_unsafe_directory` adds a
  symlinked ancestor. Every passing case walks the real `/` and the sticky `/tmp`, so it
  exercises the uid 0 arm.
- R5-S3: the `meta` certificate gains the DNS SAN `meta.flaminio.invalid` (`scripts/lab.sh:583`,
  `--dns`), and the metadata job's `server_name` checks it. Neither the `io` nor the observer
  key passes. Design "Risks" says #74's URI-SAN follow-up covers metadata clients, not
  Prometheus. `TestPrometheusExample` checks both `server_name`s.
- R5-S4: lab step 1 asserts, from each observer unit's `MainPID` `/proc/PID/status`, the uids,
  gids, exactly two groups, and zero `CapEff` and `CapPrm`. It also checks `id -Gn
  flaminio-observer`.
- R5-5: meta-interop's second server is waited for over TCP on its own port (a TLS client would
  be refused), and the trap kills its pid as well.
- R5-C nit: lab step 1 runs `TELEMETRY=1 node.sh up`, and `PRIVSEP=0 TELEMETRY=1` once #74
  exists.
- R5-S nits: design and ADR both say the forger can raise or hide `FlaminioTelemetryLost`. The
  per-node closure's needs (a case-preserving encoding, job relabelling) are stated in Risks.
  The decisions `chmod` wording now says "only root can change an entry on the path".
- decisions.md says under Approvals that these fixes came after the approval, and lists them
  under "Changes after approval".

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 7a3fcb3b4f8925112bf7b616185791c9e54755dd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71b6 (incremental, design-code and design-security). All round-5 findings were
found fixed. The plan was approved on 2026-11-13, so these fixes come after the approval.

- **R6-B1 — Low:** nothing tests that the lab's served `meta` certificate carries
  `meta.flaminio.invalid`. The prerequisites restart no metadata service, so the old certificate
  stays in service. Evidence: `scripts/guest/meta.sh:21–22`; `scripts/lab.sh:220`, `572–595`.
- **R6-C1 — Low:** if #71 lands after #74, #71 names no test for the `--telemetry`/`--privsep`
  refusal. Evidence: 74 design.md:262.
- **R6-S1 — Low:** step 1 walks "PATH as given, from `/`", but PATH is never required to be
  absolute.
- **R6 nit 1:** contracts.md:150–153 reads as `meta.flaminio.invalid` where
  `observer.flaminio.invalid` is meant.
- **R6 nit 2:** the passing directory tests depend on `std::env::temp_dir()` and on Linux.
  Evidence: `flaminio-node/tests/engine.rs:315`; `flaminio-io/src/main.rs:11–12`.

Changes/resolution:
- R6-B1: the lab check's prerequisite is `make pki-issue meta-up`. `meta-up` restarts the unit
  (`scripts/guest/meta.sh:52`), which loads its certificate only at start (`meta.sh:21–22`).
  Step 2 curls every metadata instance's `/metrics` as `meta.flaminio.invalid` with `--resolve`,
  which proves the running instance serves the new certificate.
- R6-C1: if #74 has landed first, the design's S3 row names the `--telemetry` case of #74's
  `privsep_refuses_helper_flags` and `node-scripts-test.sh`'s `TELEMETRY=1 PRIVSEP=1` case.
  Files touched adds `node.rs` and `scripts/guest/node.sh` under the same condition.
- R6-S1: `--telemetry` needs an absolute path; a relative one exits 1 at parse with
  `--telemetry needs an absolute path`. Step 1 walks the absolute PATH, and
  `refuses_an_unsafe_directory` and spec S3's tests add a relative path.
- R6 nit 1: the sentence now names `observer.flaminio.invalid`.
- R6 nit 2: the passing cases are created under `/tmp` explicitly, and `telemetry.rs` is
  `#[cfg(target_os = "linux")]`, as `control` is.
- decisions.md says under Approvals that these fixes came after the approval, and lists them
  under "Changes after approval".

No finding was judged inapplicable.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — e94d7af0d82ab7678d491242f4d06a6b5fb4d425

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71b7 (incremental design-code and design-security).

r71b7 confirmed all five round-6 findings fixed and reported one new finding:

- **R7-1 — Low:** The relative-path refusal is a parse rule in `node.rs` (not Linux-gated), but its only named test case sat inside `refuses_an_unsafe_directory` in the Linux-only `telemetry.rs`, so the parse rule had no test where it lives and none on a non-Linux `make test`. Evidence: node.rs:120, main.rs:13; design.md S3 test row.

Changes/resolution (one row of design.md's test table, made by the orchestrator; a change after the approval): the relative-PATH case moves out of `refuses_an_unsafe_directory` into a new `node.rs` unit test `telemetry_needs_an_absolute_path` (exit 1 at parse, the exact message), and the row states that the `--telemetry*` flag parsing is not Linux-gated while the use of `telemetry.rs` is `cfg(target_os = "linux")`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — f68610a4c6fc3545454538cbf7305071c4e04315

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r71b8 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
