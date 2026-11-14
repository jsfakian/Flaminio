# Issue #71: Observability and the kubectl flaminio operations CLI — design

[Spec](spec.md) S1–S9; every exact name and format is in [contracts](contracts.md);
[decisions](decisions.md); [ADR, accepted with the plan](adr-proposal.md). **Approved 2026-11-13; not implemented.**

The facts it starts from:
- **Demo signal.** `flaminio_meta_up` is TCP reachability
  (`deploy/demo/status-exporter.yaml:25–52`). No product metrics endpoint or plugin exists.
- **One commit per request.** Each tag task commits each request once, after one match sets
  `res`: `flaminio-io/src/ublk.rs:1283–1285` and, with zero copy, `ublk.rs:1459–1460`. Fast
  reads are arms of that match (`ublk.rs:1232–1237`, `1389–1393`). The MVP `serve` path (commit
  at `ublk.rs:346`) is not the node daemon.
- **Errors the frontend sees.** Only `-EIO`, `-EINVAL` and `-EOPNOTSUPP` (`ublk.rs:1247–1249`,
  `1273–1281`, `1406–1408`, `1449–1456`); every `IoError` but `Invalid`
  (`flaminio-node/src/engine.rs:241–250`) becomes `-EIO`.
- **Who drives the engine.** The main thread, holding the `Slot`, while nothing is attached
  (`ublk.rs:679`, `782–787`); the queue threads, through the `Shared.engine` mutex, while a
  volume is (`ublk.rs:506–507`, `576–578`). Other threads send an `Input` and a wake
  (`ublk.rs:724–730`), handled in `drive_until` (`node.rs:344–376`) or `pump_with`
  (`ublk.rs:586–590`); the control socket's `delete` does so (`control.rs:173–190`,
  `engine.rs:696–705`). `Attached` is set when the device is ready and cleared at its stop
  (`ublk.rs:682`, `1044`, `889`). The control socket is root-only (0600) and serves `attach`,
  `detach`, `delete` and `status` (`control.rs:51`, `83–111`).
- **Store.** `data_used` counts allocated bits, frees not yet checkpointed included; quarantined
  clusters are free bits blocked from reuse (`flaminio-store/src/store.rs:97–117`, `2159–2171`;
  `bitmap.rs:10`, `61–63`). So data = allocated + quarantined + reusable.
- **Target.** Resync `todo` and reconcile `pending` are private `ExtentSet`s
  (`flaminio-repl/src/target.rs:145–177`; `ExtentSet::blocks`, `extent.rs:65–68`). The target's
  `Stats` are test-only (`target.rs:107`).
- **Identity.** Any authenticated identity may call `GetVolume`, `ListNodes` and `ListVolumes`;
  the rest need admin, `agent` or `io` (`meta/service/auth.go:58–83`). A node identity's
  component is free-form (`pkg/pki/pki.go:234–235`); a certificate has server usage, client
  usage or both (`pki.go:111–117`). Replicas admit only `io` (`flaminio-net/src/session.rs:116`).
- **Metadata.** `Volume` carries replicas, MinISR, target and per-chunk ISR
  (`api/proto/flaminio/meta/v1/meta.proto:104–127`); `ListVolumes` returns ids only
  (`meta.proto:159–161`); IDs match `domain.ValidID` (`meta/domain/domain.go:87–90`).
- **Lab.** The node daemon is a transient systemd unit (`scripts/guest/node.sh:65`), metadata a
  service (`scripts/guest/meta.sh:28`), identities from `cmd_pki` (`scripts/lab.sh:564–597`).
  Provisioning prints ublk and io_uring state (`scripts/guest/provision.sh:55–56`).

## Frontend counters (S1, S9)

- **Module.** `flaminio-io/src/observe.rs` (Linux): `IoCounters` (fixed `AtomicU64` arrays for
  the families in contracts), a pure `classify(op, res)`, and `record`, which adds `Relaxed`.
- **Seam.** Each loop iteration takes `start = ctx.start()` after reading the iod
  (`ublk.rs:1220–1227`, `1374–1382`) and calls `ctx.record(op, len, res, start)` just before its
  one commit (`ublk.rs:1283`, `1459`). Nothing else records, so each request counts once.
- **Owner.** `run_device` creates one `IoCounters` per attachment beside `Shared`
  (`ublk.rs:1000–1018`), shared by all queues: nothing to merge. `Shared.io` is
  `Option<Arc<IoCounters>>`. Without `--telemetry` it is `None`: `start()` returns `None`
  without reading the clock, and `record` returns. `Attached` becomes
  `Option<Attachment { volume, dev_id, io }>`, set and cleared where it is now
  (`control.rs:87–105`, `193–206` keep reading `volume`, `dev_id`). The MVP `serve` path
  (`ublk.rs:166–349`) is unchanged.

## Store and progress snapshot (S2)

- **Target.** `Target::recovery(&self) -> Recovery` (`progress` is already the name of a
  private method, `target.rs:2183`) counts chunks with a resync and sums their `todo.blocks()`,
  and counts chunks in `Phase::Reconciling` and sums their `pending.blocks()`. It reads only. A
  resync still waiting for `Mapped` counts its chunk and its `todo` so far.
- **Cost.** One read-only pass over the chunks: at most 2^20 per target
  (`meta/domain/domain.go:184`), at most 4 targets, with runs summed only for chunks that have
  work. Every engine `turn` already ticks each target (`engine.rs:735–741`), and each tick walks
  every chunk (`target.rs:862`, `2187–2189`). The socket asks for at most one snapshot a second
  and keeps at most one request pending. So observing adds at most one read-only walk a second
  under the engine lock, no more than each turn already does, and never fails I/O. The
  campaign's p99 limit (Lab) bounds its effect.
- **Engine.** `Engine::handle` (`engine.rs:636–710`) takes `Input::Observe { reply }`. It reads
  `service.store()` (superblock disk UUID and device size, cluster size, `stats()`) and, per
  target, volume, epoch, chunks and `recovery()` in bytes (`flaminio_net::BLOCK`). It does not
  call `settle_local`, writes nothing, and replies once. The engine's driver answers it in both
  states, as for `delete`; a replica-only node answers from the main loop.

## Telemetry socket (S3)

- **Spawn.** `flaminio-io/src/telemetry.rs`: `spawn`, after `control::spawn` (`ublk.rs:771`)
  when `--telemetry PATH` is given, sets up the directory and socket as contracts says. Its one
  thread serves one connection at a time and answers only `observe`; any other line sends no
  `Input`.
- **Engine part.** The thread reuses an `EngineObservation` younger than 1 s. Otherwise, if no
  request is pending, it sends `Input::Observe`, keeps the reply's receiver and waits 2 s on it.
  If one is pending (an earlier wait timed out), it sends nothing new and waits on that one. A
  timeout answers the error and keeps the receiver; a late answer fills the cache. So at most
  one `Observe` is ever queued, even while the engine stalls. It clones `io` from `attached`
  under that existing lock and reads the atomics after releasing it: no lock is added.

## Node observer (S3, S5, S7)

- **Process.** `control/cmd/flaminio-observer`, package `control/observe`, run as user
  `flaminio-observer` without root or capabilities. Its user, groups, files and certificate are
  as contracts' "Observer certificate" says. Startup checks the certificate; the lab check
  checks the rest. It is nobody's TLS client, and no other service accepts its certificate as a
  server. It reports what it sees: on a node without telemetry (under #74's `--privsep`) it
  finds no socket, and the node is not fresh (contracts, "Under #74's `--privsep`").
- **Polling.** Every 5 s, `observe` with a 3 s deadline, decoded strictly, for its own
  `--node`; fresh means a success within 15 s. A failure adds to a counter and logs only when
  freshness changes. Every 60 s it runs the local doctor checks with read-only opens.
- **Server.** HTTPS with `pki.Files.ServerConfig` (TLS 1.3, client certificate required); each
  request passes `pki.PeerIdentity` and the exact allowlist (`pkg/pki/allow.go`).
  `/metrics` uses `prometheus/client_golang` (`go.mod:36`, now direct) with a private registry
  and constant metrics from the last snapshot. It adds no `node` label: the scrape target does.

## Metadata metrics (S4, S5)

`flaminio-meta serve` (`meta/cmd/flaminio-meta/main.go:83–152`) starts package `meta/metrics`
when `--metrics-listen` is set. It calls #69's `status.Collect` over an in-process client on
`*service.Service` (no interceptor, no network), then `Derive`. Each outcome (failure, limit,
omitted volume, invalid volume) exports what the contracts table says. It serves them under the
observer's HTTPS rules, on its own port, with the metadata server's certificate. Each instance
exports its own view; the rules take the max by volume.

## CLI (S5, S6, S7)

- **Binary.** `control/cmd/kubectl-flaminio`, package `control/cli`. Arguments are checked
  before any connection. One loader opens only the three files the flags name; no environment
  variable, default path or kubeconfig is read. Metadata: #69's `status.Collect` and `Derive`
  over gRPC with the reader files; `volume get` calls `GetVolume`.
- **Observers.** Per registered node, `GET https://<host of Node.address>:<--observer-port>/v1/node`,
  at most 4 at once, 5 s each, within `--timeout`, with `ServerName` `observer.flaminio.invalid`.
  Past the CA and that name, the leaf's one URI SAN must be `node/<id>/observer`, else the node
  is omitted as `untrusted`.
- **Joins.** Disks are keyed by (node, disk id); `disk get` prints every match. A volume's
  progress comes from its metadata target's observer. Doctor computes `mtu` across nodes and
  appends the unsupported checks. Every string from an observer is checked against its grammar
  in contracts before use. Nothing reaches stdout before the result is complete.

## Alerts and runbooks (S8)

`deploy/observe/rules.yaml` with promtool tests `rules_test.yaml`, and `prometheus.example.yaml`
(both in contracts). `make rules-test` is its own target and its own CI job, `rules` in
`.github/workflows/ci.yml`, one make target per job (`ci.yml:2`). That keeps the Prometheus
module out of every `make lint`. The target runs `promtool test rules` with `PROMTOOL`, a
`go run` pinned to one release as `BUF` is (`Makefile:13`). The release is chosen when the
target is written. `docs/runbooks.md` says, per alert and for doctor failures, what to inspect and
which owner tool acts; none suggests a destructive step. The `telemetry-lost` section names a
node under #74's `--privsep` as an expected cause in this step.

## Files touched

- `dataplane/crates/flaminio-io/src/{observe,telemetry}.rs` (new): counters, classifier, line
  encoder, socket (S1, S3). `ublk.rs`: seam, `Shared.io`, `Attachment`, spawn (S1, S3).
  `control.rs`, `node.rs`, `main.rs`: `Attachment` fields, `--telemetry*` flags, modules (S1,
  S3). The `--privsep` refusal belongs to the plan that adds the second flag (contracts). If
  #74 has landed first, #71 adds it in `node.rs` and `scripts/guest/node.sh`.
- `dataplane/crates/flaminio-node/src/engine.rs`: `Input::Observe`, `EngineObservation` (S2).
- `dataplane/crates/flaminio-repl/src/target.rs`: `Recovery`, `Target::recovery` (S2).
- `control/observe/`, `control/cmd/flaminio-observer/` (new): decoder, checks, server (S3, S5, S7).
- `meta/metrics/` (new), `meta/cmd/flaminio-meta/main.go`: metadata metrics (S4, S5).
- `control/cli/` (new), `control/cmd/kubectl-flaminio/main.go` (new) (S6, S7).
- `pkg/pki/allow.go` (new): allowlist parsing and the peer check, shared by both servers (S5).
- `go.mod`: `prometheus/client_golang` and `sigs.k8s.io/yaml` direct (S3, S4, S5).
- `deploy/observe/{rules,rules_test,prometheus.example}.yaml` (new); Makefile `rules-test` and
  `PROMTOOL`; `.github/workflows/ci.yml` job `rules` (S5, S8).
- `scripts/dev/meta-interop.sh`: a second `flaminio-meta serve --store memory` on another port,
  with an observer certificate (`--server --dns observer.flaminio.invalid`) as `--tls-cert`, and
  `FLAMINIO_META_INTEROP_OBSERVER` set to its address. Its readiness wait connects to that port
  over TCP (a TLS client would be refused), and the trap kills its pid as well as the first's
  (`meta-interop.sh:7`).
  `dataplane/crates/flaminio-metaclient/tests/interop.rs`: `refuses_an_observer_certificate`
  (S5).
- `scripts/guest/{observer.sh (new),node.sh,meta.sh}`, `scripts/lab.sh` (the `observer` identity,
  and `--dns meta.flaminio.invalid` on `meta`), `scripts/lab.conf`, `scripts/checks/observe.sh`
  (new): lab (S1–S8). Docs below.

## Contracts

[Contracts](contracts.md): the telemetry line and classifier, metric families and bounds, both
servers' endpoints, the CLI grammar, output and exit codes, doctor checks and alert rules. Rust
and Go share the line through a golden fixture. No proto, RPC, `auth.go` rule, frame, token or
on-disk change.

## Tests

| Rule | Tests (level; target) |
|---|---|
| S1 | `observe.rs`: `classify_maps_every_result` (each contracts row), `record_buckets_sums_and_bytes` (bucket edges, bytes only on successful reads and writes), `nothing_without_counters` (`start()` is `None` and `record` changes nothing) (unit; `test`). Lab `scripts/checks/observe.sh` step 3: counter deltas equal fio's totals. This step proves exactly-once on a real device; the unit tests prove the mapping. |
| S2 | `target.rs`: `recovery_counts_resync_and_reconcile_work`, beside `resyncs_progress_while_a_member_flaps` (`target.rs:3984`) (unit). `flaminio-node/tests/engine.rs`: `observe_reports_the_store_and_targets` (allocated + quarantined + reusable = data; equals `Store::stats`) (integration; `test`). Lab step 4: resync drains to 0. |
| S3 | `telemetry.rs`: `only_observe_is_answered` (attach, detach, delete, status, empty, 65 bytes: no `Input`), `the_engine_part_is_cached_for_a_second`, `a_silent_engine_gives_an_error_in_2_s`, `one_observe_pending_at_most` (silent engine, 10 requests: one `Input::Observe`), `mode_and_group` (both rows of the contracts table with the test process's own gid, the directory created under umask 077), `refuses_an_unsafe_directory` (step 1: a symlinked ancestor, an ancestor writable by others and not sticky, a parent owned by another uid; every passing case is created under `/tmp` explicitly, not `std::env::temp_dir()`, so it walks the real `/` and the sticky `/tmp`, both root-owned, and exercises the uid 0 arm (Linux-only, as `telemetry.rs` is); step 3: a symlink, a regular file, mode 0770, 0755, a wrong group, an owner other than the injected U; step 4: no control directory, and the control directory spelled `T/x/../ctl`), all unprivileged with U and the gid injected (unit). `observe.rs`: `encodes_the_golden_line` against `control/observe/testdata/observe-v1.line`. Go `control/observe`: `TestDecodeGolden`, `TestDecodeRefusesInvalid` (each `testdata/invalid/*.line`), `FuzzDecode` (`test`, `race`). `node.rs`: `telemetry_needs_an_absolute_path` (a relative `--telemetry` PATH exits 1 at parse with `--telemetry needs an absolute path`; the `--telemetry*` flag parsing in `node.rs` is not Linux-gated, only the use of `telemetry.rs` is `cfg(target_os = "linux")`, so the workstation build compiles and this test runs everywhere) (unit). If #74 has landed first: the `--telemetry` case of #74's `privsep_refuses_helper_flags` and `node-scripts-test.sh`'s `TELEMETRY=1 PRIVSEP=1` case (`test`, `lint`). |
| S4 | `meta/metrics`: `TestUnderReplicated` (full, short, moving ISR), `TestViewOutcomes` (each row of the contracts table: failed, over limit, one invalid volume among valid ones, omitted volume) (memory store; `test`). |
| S5 | `pkg/pki`: `TestServerOnlyCertRefusedAsClient`, `TestAllowlist` (wrong trust domain, malformed, empty). `control/observe`: `TestStartupRefusesIdentity` (client or dual usage, another node, another component, an IP SAN, another DNS SAN). `pkg/pki`: `TestObserverCertRefusedByMetadataClients` (a `ClientConfig` gRPC dial to `127.0.0.1` refuses a server presenting an observer certificate); `make meta-interop` (run by `make contracts` and `make check`, not `make test`): `refuses_an_observer_certificate` (a Rust `MetaClient` for `FLAMINIO_META_INTEROP_OBSERVER`: `call_blocking` of `ListNodes` returns `Answer::Failed`, never `Answer::ListNodes`). `control/observe` and `meta/metrics`: `TestEndpointRefuses` (TLS 1.2, unlisted reader 403, POST 405, unknown path 404, 17th request 503). `control/cli`: `TestObserverIdentityChecked` (another node's certificate is `untrusted`), `TestObserverStringsChecked` (control bytes in `evidence`, `interface` or a `T`, and `disk` or `targets` inconsistent with `fresh`: `invalid`), `TestOnlyFlagFilesOpened` (with `HOME`, `KUBECONFIG` and `FLAMINIO_*` set: no flags opens nothing and exits 2; flags open exactly their three files). `meta/service`: an `observer` component is denied every method but the three open reads (`test`, `race`). `control/observe`: `TestPrometheusExample` and its mutated copies (contracts, "Scraping"). |
| S6 | `control/cli`: golden table and JSON for each command; `TestExitCodes` (2 before any call, 1 with empty stdout, 3 on an omitted observer, 3 on a `stale` node whose checks are still listed, 4 on a doctor failure, the precedence order); deterministic order; two nodes reporting one disk UUID are both listed (`test`, `race`). |
| S7 | `control/observe`: `TestChecks` over a fake root (kernel 6.5, 6.6, no MAJOR.MINOR, outside the grammar; ublk present, absent; io_uring 0, 1, 2, missing); the fake root is unchanged afterwards. `control/cli`: `TestMTU` (equal, different, missing). Lab step 5. |
| S8 | `deploy/observe/rules_test.yaml`: the cases in contracts (`rules-test`, CI job `rules`). |
| S9 | `control/observe`: `TestSeriesBound` (1 attachment, 4 targets: 73; no snapshot: 2). `meta/metrics`: 4V + 2N + 2. The lab overhead campaign and its p99 limit, which also bounds S3's walk. "No lock, allocation, metadata call, wait or log line on the I/O path" is shown by review of the two seam lines and `record`, and by the campaign; no test asserts it. |

`make lint test race hygiene contracts rules-test`; `sim` as a regression, since no safety rule is
added. Nothing was run while drafting.

## Lab

- **`scripts/checks/observe.sh`** (minutes; needs this branch deployed, then `make pki-issue
  meta-up`, since the metadata service loads its certificate only when its unit starts
  (`scripts/guest/meta.sh:21–22`, restarted at `52`); #69's lab reader certificate on fl-a;
  nothing attached on the data nodes). A step whose
  condition fails fails the check:
  1. Restart every running node daemon in direct mode with `TELEMETRY=1 node.sh up` (with #74,
     `PRIVSEP=0 TELEMETRY=1`), and start an observer beside it. For each observer unit's
     `MainPID`, `/proc/PID/status` shows every `Uid:` and `Gid:` field as `flaminio-observer`'s,
     `Groups:` as exactly `flaminio-observer` and `flaminio-telemetry`, and `CapEff` and `CapPrm`
     as 0. `id -Gn flaminio-observer` lists exactly those two groups. As `flaminio-observer`,
     connecting to the telemetry socket succeeds. Connecting to `/run/flaminio/node.sock`, listing
     `/etc/flaminio/pki`, and creating a file in the telemetry directory fail. A probe token
     planted as `node.sh` plants tokens (`install -d -m 700 /run/flaminio`, then `tee`,
     `scripts/guest/node.sh:61–62`) cannot be read, and is removed. The observer key is
     root:flaminio-observer 0440. `getent group flaminio-observer` lists no other member, and no
     other `getent passwd` entry has that group as its primary group.
  2. As the reader, `status`, `node list`, `disk list`, `volume list` and `doctor` list every
     lab node, disk and volume and exit 0, or 3 omitting only nodes metadata reports dead. Both
     `/metrics` answer to curl with the reader's files, each reached by its name with
     `--resolve`: an observer as `observer.flaminio.invalid`, and every metadata instance as
     `meta.flaminio.invalid`, which proves the running instance serves the new certificate. The
     admin certificate gets 403.
  3. `metactl create VOL 16G 2 1 256M fl-a` (RF2, MinISR 1; arguments as in
     `meta/cmd/flaminio-meta/main.go:7`), attached on fl-a. fio with `--direct=1` runs 20,000
     random 4K reads and 256 MiB of 1M writes. Deltas equal fio's `total_ios` (4K) and
     `io_bytes`, with no error deltas. Again with `ZERO_COPY=1`, and with
     `create VOL2 16G 1 1 256M fl-b` attached on fl-a (remote reads).
  4. Stop the daemon of the RF2 volume's other replica node R and write 1 GiB with
     `--direct=1`. Within 2 minutes every metadata instance reports under-replicated chunks.
     Restart R: fl-a's `flaminio_resync_remaining_bytes` falls to 0, and under-replicated chunks
     to 0, within 10 minutes.
  5. On R, set `kernel.io_uring_disabled=2` and lower wg0's MTU by 40: `doctor` exits 4 naming
     both.
- **Changes and restore.** It creates two test volumes, and stops R's daemon for a while. It
  changes R's io_uring sysctl and MTU, and plants and removes a probe token (leaving
  `/run/flaminio` 0700, as `node.sh` leaves it after an `ATTACH`). It formats no disk. A trap
  restores the recorded sysctl and MTU, deletes the test volumes and their replicas (control
  socket `delete`, as `resync.sh` does), and restarts the daemons with their previous settings.
  The observers (with their user, group and `/etc/flaminio/observer`) and the metadata listener
  stay, read-only.
- **Overhead campaign** (hours). `make bench-reads` (zero-copy and verify; 4K QD32, 1M) and
  `make bench-writes` (4K RF2) alternate `TELEMETRY=0` and `1`, three times each, with
  `observe` polled once a second (the most the socket serves from the engine) during on runs. It
  records p50, p99, throughput, daemon CPU and spread. A job's runs count at a spread of at most
  10% (`docs/performance-plan.md` §2). The campaign passes when every job's median p99 with
  telemetry is at most 10% above its median p99 without. A breach fails it, and the fallbacks in
  Risks are built before release. `bench-writes` resets its node's disk as it always does; the
  lab is restored as that target documents.

## Docs

New `docs/observability.md` (families, labels, bounds, socket, observer, CLI) and
`docs/runbooks.md`. `docs/lab.md`, `README.md` (observers, ports, check, reader files),
`docs/ARCHITECTURE.md` §18 (links the reference; §17.2 unchanged), `docs/ci.md` (the `rules` job
in its "What runs when" table, and `rules-test`).
Once measured: `docs/performance-plan.md`, `docs/benchmarks/<date>-telemetry-overhead.md`.
`docs/followups.md` is unchanged: the open parts stay in #71.

## Risks

- **Safety.** Observation grants nothing and nothing decides on it. `Input::Observe` neither
  settles nor writes and keeps no durable state: no crash point. Fencing, ISR and
  acknowledgement are untouched.
- **I/O path.** With telemetry, two clock reads and at most four relaxed adds per request, on
  cache lines the queues share, plus at most one chunk walk a second under the engine lock. The
  campaign's p99 limit (Lab) bounds both; per-queue copies and counts kept in the target are
  the fallbacks.
  Settled at approval, 2026-11-13: shared counters and the walk, with the fallbacks only on a
  measured cost (see decisions.md, Settled at approval).
- **Who must not.** The `flaminio-telemetry` group reads ids, capacity and progress, and cannot
  attach, detach or delete (root-only control socket), write the telemetry directory, or read
  tokens or `/etc/flaminio/pki`. The observer cannot call the metadata service or a replica, and
  no client of either accepts its certificate (contracts, "Observer certificate"). Identities
  outside an allowlist cannot read. The CLI needs and seeks no administrator or node credential.
- **Prometheus checks less than the CLI.** It binds an observer's series to a node by scrape
  address only. Every observer certificate carries the same DNS SAN, so one observer's key,
  readable by that non-root process, answers for every node's observer target. Where a node's
  observer is not listening, anyone on that node can bind the unprivileged port 7412, with no
  redirection needed. For any node, a forger can raise or hide `FlaminioCapacityPressure` (both
  levels), `FlaminioRecoveryStalled` and that node's `FlaminioTelemetryLost`. The CLI is not
  affected, because it checks the URI SAN. Settled 2026-11-13 (user's instruction): no per-node
  binding in this step, and this residual is accepted for it (see decisions.md). A per-node name
  would need a DNS-safe, case-preserving encoding of node ids and per-node jobs relabelled to
  `flaminio-observer`.
- **The metadata job and the `io` certificate.** The lab's `io` certificate carries the node's IP
  SAN, as its `meta` certificate does (`scripts/lab.sh:583–584`). Under #74's `--privsep`, a
  non-root process reads its key. A metadata job that checked only the IP SAN would therefore
  accept it, and `FlaminioLostRedundancy` could be raised or hidden for every volume. #74's
  follow-up, "metadata clients check the `meta` URI SAN", cannot close this, because Prometheus
  checks no URI SAN. So the `meta` certificate gains the DNS SAN `meta.flaminio.invalid`, and the
  metadata job's `server_name` checks it (contracts, "Metadata flags" and "Scraping"). Neither
  the `io` nor the observer certificate has that name.
- **One invalid volume hides every volume.** #69's `Derive` fails as a whole on invalid data, so
  every metadata instance then exports no per-volume series: `FlaminioTelemetryLost` fires, and
  `FlaminioLostRedundancy` cannot. A per-volume rejection would need a change to #69.
- **Open questions.** The scrape identity, a #69 reader, can also call the open reads and
  `ListTombstones`. A reattach plus enough I/O within one scrape interval can hide a counter
  reset. Each metadata instance reads V volumes a minute: review for large clusters. Bucket
  bounds, thresholds, hold times, ports and promtool's pinned release are proposals.
  Settled at approval, 2026-11-13: the scrape identity is a #69 reader; the reattach limitation
  is accepted; the bounds, thresholds, hold times and ports are those in contracts (see
  decisions.md, Settled at approval). Settled 2026-11-13 (user's instruction): the metadata
  listener as designed, changed only on a measured cost; promtool's release chosen and pinned
  when `rules-test` is written (see decisions.md).
- **Compatibility and dependencies.** Telemetry and metadata metrics are off by default; a
  `schema` other than 1 is refused and shows as stale. This step needs #69's reader identity,
  `status.Collect` and `Derive` as #69's design states them. Under #74's `--privsep` there is
  no telemetry in this step. The helper request is a later step of #74 (contracts, "Under #74's
  `--privsep`"). It needs nothing from #43, #45, #54, #77 or #83.

## ADRs

Consistent with 0001 (counters and socket in Rust; observer, metrics and CLI in Go), 0002
(metadata metrics read through the service, not etcd), 0003 (doctor's kernel and ublk checks),
0004 (telemetry optional, off by default, its absence harmless) and 0005, 0006 (unchanged). The
[ADR](adr-proposal.md), accepted with the plan on 2026-11-13, covers the socket, the server-only
observer, pull scraping and the metadata listener.

## Parallelizable?

Yes. The golden line, the families and the endpoint formats in contracts let the Rust units
(counters, progress, socket) and the Go units (observer, metadata metrics, CLI, rules) proceed
independently once #69's reader identity and derivation exist.
