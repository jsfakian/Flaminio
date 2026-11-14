# Issue #71: observability and CLI contracts, step 1

Normative appendix to the [design](design.md). Every format below is accepted with the plan,
2026-11-13 ([ADR](adr-proposal.md)). Each is stated once, here, and the other files defer to it.

## Telemetry socket (S1–S3)

- **Daemon flags.** `flaminio-io node --telemetry PATH [--telemetry-group GID]`. GID is decimal.
  PATH must be absolute; a relative one exits 1 at parse with `--telemetry needs an absolute
  path`. Without `--telemetry` there is no socket, nothing is recorded and no clock is read. The
  lab uses `/run/flaminio-telemetry/telemetry.sock`. `telemetry.rs` is Linux-only
  (`#[cfg(target_os = "linux")]`), as `control` is (`flaminio-io/src/main.rs:11–12`).
- **Directory and socket (direct mode).** PATH's parent D is the telemetry directory and holds
  nothing else. The daemon sets it up after `control::spawn` (`ublk.rs:771`), which creates the
  control socket's directory, and before it serves. U is the expected owner: the daemon's
  effective uid, root in production. A failed step exits 1 with `telemetry directory D: REASON`.
  1. The daemon walks the absolute PATH from `/` to D's parent, with `lstat` on each component.
     Each must be a directory, not a symlink, owned by uid 0 or U, and not writable by group or
     others unless it is sticky. So nobody else can rename or replace an entry on the path to D.
     REASONs: `ancestor A is a symlink`, `ancestor A is open to others`. A path through a
     symlink such as `/var/run` is refused; the lab uses `/run`.
  2. If D is absent, `mkdir` creates it (no parents; it never follows a symlink). `chmod` and
     `chown` then set the table's mode and group, since the umask may mask `mkdir`'s mode.
  3. `lstat` of D shows a directory, not a symlink, owned by U, with the table's mode and group.
     REASON: `want a directory owned by uid U, mode M, group G`.
  4. `stat` of the control socket's directory C (the parent of `--control`,
     `flaminio-io/src/node.rs:177`) succeeds, and its `(st_dev, st_ino)` differs from D's.
     REASONs: `cannot stat the control socket's directory C`, `is the control socket's
     directory`.
  5. The daemon unlinks a stale PATH and binds. It sets the socket's mode with `chmod` and its
     group with `lchown`, as in the table. After steps 1 and 3 only root can change an entry on
     the path, so `chmod`, which follows symlinks, reaches the socket just bound. `control.rs` is
     unchanged.

| `--telemetry-group` | D | Socket |
|---|---|---|
| GID | mode 0750, group GID | mode 0660, group GID |
| absent | mode 0700, group not checked | mode 0600 |

- **Under #74's `--privsep`.** There is no telemetry under `--privsep` in this step. A helper
  request that lets #74's root helper bind the telemetry socket is a later step of #74. Such a
  node's observer finds no socket and reports what it sees: `flaminio_observer_snapshot_fresh 0`,
  which raises `FlaminioTelemetryLost`. The CLI lists the node as `stale`. The plan that adds the
  second of `--telemetry` and `--privsep` to `flaminio-io node` adds two things, each with a
  test. The daemon refuses the pair at parse and exits 1 with `--telemetry needs direct mode;
  telemetry under --privsep is a later step of #74`. The lab's `node.sh` dies with
  `TELEMETRY=1 needs PRIVSEP=0` (a `node-scripts-test.sh` case). #74 states the same rule.
- **Request.** One line, at most 64 bytes with its `\n`: `observe`. Each reply is one line, and
  then the connection closes. The 1 s read and write timeouts apply.

| Condition | Reply |
|---|---|
| `observe`, snapshot built | `ok ` + the JSON below, one line, at most 64 KiB |
| any other line, empty, over 64 bytes | `error: usage: observe` (no engine input) |
| engine silent for 2 s | `error: the engine did not answer within 2 s` |
| engine channel closed | `error: the node is shutting down` |
| node or volume id fails the ID grammar | `error: invalid id` |
| more than 4 targets | `error: more than 4 targets` |

- **The JSON.** Keys come in this order, without whitespace, with integers as JSON numbers.
  `ID` is `[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}`; `UUID` is lowercase `8-4-4-4-12` hex; `U` is a
  u64.

```text
{"schema":1,"node":ID,
 "disk":{"id":UUID,"device_bytes":U,"cluster_bytes":U,"data_clusters":U,
         "allocated_clusters":U,"quarantined_clusters":U,"pool_pairs":U,"pool_used_pairs":U},
 "targets":[{"volume":ID,"epoch":U,"chunks":U,"resync_chunks":U,"resync_remaining_bytes":U,
             "reconcile_chunks":U,"reconcile_remaining_bytes":U}],
 "io":null|{"volume":ID,
            "ops":{"read":C,"write":C,"flush":C,"discard":C,"other":C},
            "bytes":{"read":U,"write":U},
            "duration":{"read":H,"write":H}}}
C = {"ok":U,"io_error":U,"invalid":U,"unsupported":U}
H = {"buckets":[U × 11],"sum_ns":U}
```

- **Field rules.** `targets` holds at most 4, sorted by volume. `io` is null while nothing is
  attached. Its values belong to the current attachment and start at zero.
  `allocated_clusters + quarantined_clusters <= data_clusters`, and reusable is the rest.
  `buckets[i]` counts durations in (bound i−1, bound i]. Index 10 counts those above the last
  bound. The buckets are not cumulative.
- **Golden fixtures.** `control/observe/testdata/observe-v1.line` is what the Rust encoder
  produces, byte for byte, for the fixed input of its test. The Go decoder accepts it. The decoder
  refuses each `control/observe/testdata/invalid/*.line`: an unknown key, a missing key, schema 2,
  a bad id, 5 targets, 10 buckets, allocated + quarantined > data, trailing bytes, a fraction and
  a number above u64.
- **Classifier.** `op` is `iod.op_flags & 0xff`. READ is `read`, WRITE `write`, FLUSH `flush`,
  DISCARD `discard`, anything else `other`. The result comes from the committed `res`:

| `res` | result | Also recorded |
|---|---|---|
| ≥ 0 | `ok` | `bytes[op] += len` for read and write |
| `-EINVAL` | `invalid` | — |
| `-EOPNOTSUPP` | `unsupported` | — |
| any other negative | `io_error` | — |

  Read and write requests of every result add `now − start` to `duration[op]`, in ns. `start`
  is read only when the counters exist; without `--telemetry` nothing is read or added.

## Metric families and bounds (S1, S2, S4, S9)

The bucket bounds of `flaminio_io_duration_seconds` are
`0.00005, 0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.1, 1`, plus `+Inf`.

**Observer.** Its series carry no `node` label: the scrape target adds it.

| Family | Type | Labels | Series |
|---|---|---|---|
| `flaminio_io_operations_total` | counter | volume, op (5), result (4) | 20 |
| `flaminio_io_bytes_total` | counter | volume, op ∈ read, write | 2 |
| `flaminio_io_duration_seconds` | histogram | volume, op ∈ read, write | 26 |
| `flaminio_disk_device_bytes`, `flaminio_disk_data_bytes`, `flaminio_disk_data_allocated_bytes`, `flaminio_disk_data_quarantined_bytes`, `flaminio_disk_data_reusable_bytes` | gauge | disk | 5 |
| `flaminio_disk_pool_pairs`, `flaminio_disk_pool_used_pairs` | gauge | disk | 2 |
| `flaminio_resync_chunks`, `flaminio_resync_remaining_bytes`, `flaminio_reconcile_chunks`, `flaminio_reconcile_remaining_bytes` | gauge | volume | 4 × ≤ 4 = 16 |
| `flaminio_observer_snapshot_fresh` | gauge | — | 1 |
| `flaminio_observer_snapshot_failures_total` | counter | — | 1 |

The total is at most 73 series. The last two families are always exported; without a fresh
snapshot, only they are. The volume's io series exist only while it is attached.

**Metadata instance.** `node` here is the subject node from metadata.

| Family | Type | Labels | Series |
|---|---|---|---|
| `flaminio_volume_replicas`, `flaminio_volume_min_isr`, `flaminio_volume_chunks`, `flaminio_volume_under_replicated_chunks` | gauge | volume | 4V |
| `flaminio_node_alive`, `flaminio_node_draining` | gauge | node | 2N |
| `flaminio_meta_view_ok` | gauge | — | 1 |
| `flaminio_meta_view_failures_total` | counter | — | 1 |

The total is at most 4V + 2N + 2, with V and N bounded by #69's collection limits. A chunk is
under-replicated when `len(isr) < replicas`. The view's outcomes:

| #69's `Collect` and `Derive` | Per-volume and per-node series | `flaminio_meta_view_ok` |
|---|---|---|
| a read failed, a limit was exceeded, or any volume is invalid data (#69's `Derive` then fails as a whole) | none | 0, failures + 1 |
| a volume omitted (NotFound while collecting) | none for that volume | unchanged by it |
| otherwise | all | 1 |

## Endpoints (S3–S5)

- **Observer flags.** `flaminio-observer --node ID --trust-domain TD --tls-cert F --tls-key F
  --tls-ca F --allow SPIFFE-ID[,SPIFFE-ID...] [--telemetry PATH] [--listen ADDR]
  [--data-interface IF]`. The defaults are `/run/flaminio-telemetry/telemetry.sock` and `:7412`.
  IF matches `[a-zA-Z0-9._-]{1,15}`. It exits 1 at startup if its certificate is not exactly
  `spiffe://TD/flaminio/node/ID/observer` with server usage only (the certificate rules below),
  or if `--allow` is empty or invalid. Its user, groups and files are not checked at startup;
  the lab check checks them.
- **Observer certificate.** `flaminio-ca sign --id spiffe://TD/flaminio/node/N/observer --server
  --dns observer.flaminio.invalid`. It has server usage only, the one DNS SAN
  `observer.flaminio.invalid`, and no IP SAN. `.invalid` is reserved (RFC 2606), so no client
  dials it. Every metadata client (Go `pki.Files.ClientConfig`, Rust `MetaClient`) and every
  replica dials an IP or a real host name, and Prometheus's metadata job checks
  `meta.flaminio.invalid` (Scraping), so none accepts this certificate. This holds while no
  metadata endpoint is configured by the name `observer.flaminio.invalid`. Nothing enforces it,
  and the lab's endpoints are IPs. The observer's startup check also refuses an IP SAN or any
  other DNS SAN. The CLI and the observer job set `server_name` to `observer.flaminio.invalid`,
  and the CLI also checks the URI SAN. The lab issues it in `/etc/flaminio/pki` like every
  identity. `scripts/guest/observer.sh` then installs copies in `/etc/flaminio/observer`
  (root:flaminio-observer 0750): the key as root:flaminio-observer 0440, the certificate and
  `ca.crt` as root:root 0444. The user `flaminio-observer` has the primary group
  `flaminio-observer`, of which it is the only member, and the supplementary group
  `flaminio-telemetry`. It cannot read `/etc/flaminio/pki`: it is neither root nor in any group
  that directory grants (root's, or #74's `flaminio-io`).
- **Metadata flags.** `flaminio-meta serve --metrics-listen ADDR --metrics-allow
  SPIFFE-ID[,...]`, with the service's own `--tls-*` files. Serve exits 1 if one is given without
  the other. It refreshes every 60 s with a 30 s deadline. The lab uses port 7402. The lab's
  `meta` certificate gains the DNS SAN `meta.flaminio.invalid` beside its IP SAN
  (`scripts/lab.sh:583`, `--dns`). Metadata clients still dial the IP; Prometheus checks the
  name.
- **HTTP, for both servers.** TLS 1.3 only, with a client certificate required. A verified
  identity not in the allowlist gets 403. Paths other than those below get 404, and methods other
  than GET get 405. More than 16 requests at once get 503. `ReadHeaderTimeout` is 5 s, read and
  write timeouts are 10 s, and `MaxHeaderBytes` is 8 KiB. `/metrics` is the Prometheus text
  format 0.0.4. The observer also serves `/v1/node`; the metadata service serves only `/metrics`.
- **`GET /v1/node`.** One JSON object, at most 64 KiB. `DISK` and `TARGET` are as in the
  telemetry line. `T` is UTC RFC 3339 with nanoseconds.

```text
{"schema":1,"node":ID,"observed_at":T|null,"fresh":bool,
 "disk":DISK|null,"targets":[TARGET]|null,
 "network":{"interface":IF,"mtu":U}|null,
 "checks":[{"check":"kernel"|"ublk"|"io_uring","status":"pass"|"warn"|"fail"|"unknown",
            "evidence":S,"checked_at":T}]}
```

  `disk` and `targets` are non-null exactly when `fresh`. `checks` holds exactly the three, in
  that order.
  `network` is null without `--data-interface` or when its MTU cannot be read. The CLI treats as
  `invalid` a larger reply, an unknown or missing key, `schema` other than 1, a `node` other than
  the one it asked, `disk` or `targets` non-null without `fresh` (or null with it), an
  `interface` outside IF's grammar, an `evidence` outside its check's grammar below, or a `T`
  that does not parse as RFC 3339. The CLI prints a `T` re-formatted from
  its parsed value, never as received.
- **Scraping.** `deploy/observe/prometheus.example.yaml` has two jobs over `https`. Both set
  `honor_labels: false`, `label_limit: 10`, `label_value_length_limit: 128` and a `tls_config`
  with the reader's files and the CA, without `insecure_skip_verify`. Each sets its own
  `server_name`.

| Job | Targets | `server_name` | `sample_limit` | `body_size_limit` |
|---|---|---|---|---|
| `flaminio-observer` | one per node, with its `node` label | `observer.flaminio.invalid` | 200 | 1MB |
| `flaminio-meta` | one per metadata instance | `meta.flaminio.invalid` | 25000 (4V + 2N + 2 at #69's 4096 limits) | 16MB |

  `control/observe`'s `TestPrometheusExample` checks the file. It parses it with
  `sigs.k8s.io/yaml`'s `UnmarshalStrict` (`go.mod:70`, now direct) into a struct that models
  every key the example uses, so a misspelled or misplaced key fails. It then compares each job
  with this table and the keys above. Mutated copies must each fail: one job without
  `sample_limit`, `insecure_skip_verify: true`, a wrong `server_name`, `honor_labels: true`, and
  a misspelled key. Prometheus binds an observer's series to a node by scrape address only (the
  consequence is in design "Risks").

## Doctor checks (S7)

| Check | Read (only) | pass | warn | fail | unknown | Evidence grammar |
|---|---|---|---|---|---|---|
| `kernel` | `/proc/sys/kernel/osrelease` | MAJOR.MINOR ≥ 6.6 | — | < 6.6 | unreadable, outside the grammar, no MAJOR.MINOR | `release [0-9A-Za-z._+-]{1,64}`, `release unparsable` or `release unreadable` |
| `ublk` | stat `/dev/ublk-control` | character device | — | absent | other error | `/dev/ublk-control (present\|absent\|unreadable)` |
| `io_uring` | `/proc/sys/kernel/io_uring_disabled` | 0 | 1 | 2 | missing, other | `kernel\.io_uring_disabled=([0-9]{1,3}\|missing\|unreadable)` |
| `mtu` (CLI) | every node's `network.mtu` | all equal | — | differ: every reporting node fails | this node's `network` is null | built by the CLI: `IF mtu N`, plus `; NODE N, ...` when they differ |

`clock` (owner #83), `latency`, `disk_health` and `config` (later steps of #71) always report
`unsupported`, with the evidence `not checked; owner #83` or `not checked; later step of #71`.
The observer sends the first three evidences, and the CLI builds the others.

## CLI (S6)

```text
kubectl flaminio [FLAGS] status
kubectl flaminio [FLAGS] node list | node get NODE
kubectl flaminio [FLAGS] disk list | disk get DISK
kubectl flaminio [FLAGS] volume list | volume get VOLUME
kubectl flaminio [FLAGS] doctor [--node NODE]
FLAGS: --meta HOST:PORT[,HOST:PORT...] --cert F --key F --ca F --trust-domain TD
       [--observer-port 7412] [--output table|json] [--timeout 30s]
```

- **Arguments.** Flags may come before or after the command words. NODE and VOLUME use the ID
  grammar and DISK the UUID grammar. `--timeout` is between 1 s and 5 min. `--node` limits
  doctor's output, and `mtu` still compares every reporting node.
- **JSON.** `{"schema":1,"command":"volume list","started_at":T,"finished_at":T,
  "complete":bool,"omitted":[{"node":ID,"reason":R}],"items":[...]}`. R is one of
  `unreachable`, `denied` (TLS refused or 403), `untrusted` (wrong server identity), `invalid`,
  or `stale`. Doctor reports a `stale` node's checks, since `/v1/node` always carries them. A
  node without telemetry, as under #74's `--privsep`, is `stale`, so a report over it exits 3:
  it is incomplete. A cluster of such nodes exits 3 by design, and a script cannot tell such a
  node from one whose daemon stopped. Items:
  - `status`: one item `{"nodes","nodes_alive","nodes_observed","volumes",
    "volumes_under_replicated","volumes_recovering","disks","data_bytes",
    "data_reusable_bytes"}`, all U. `nodes_observed` counts fresh observers;
    `volumes_under_replicated` counts volumes with an under-replicated chunk;
    `volumes_recovering` counts volumes whose target's observer reports resync or reconcile
    chunks above 0; the byte sums cover the observed disks only.
  - `node`: `{"id","address","alive","draining","observer":"fresh"|R,"disk":UUID|null}`.
  - `disk`: `{"node","id","device_bytes","data_bytes","allocated_bytes",
    "quarantined_bytes","reusable_bytes","pool_pairs","pool_used_pairs"}`.
  - `volume`: `{"id","size_bytes","replicas","min_isr","epoch","target":ID|null,"chunks",
    "under_replicated_chunks","progress":{"resync_chunks","resync_remaining_bytes",
    "reconcile_chunks","reconcile_remaining_bytes"}|null,
    "progress_state":"reported"|"none"|"unknown"}`. `none`: no target, or the target's fresh
    observer has no entry. `unknown`: the target's observer is omitted.
  - `doctor`: `{"node","check","status","evidence","checked_at":T|null}`, eight per node in the
    order kernel, ublk, io_uring, mtu, clock, latency, disk_health, config.
- **Order.** Nodes and volumes sort by id, disks by (node, id), doctor by node and then check
  order.
- **Tables.** One header row with the item's fields in order; `status` prints one field per line.
  Sizes use IEC units with one decimal. Only validated ids, UUIDs, enums, numbers and
  addresses reach the output; an address that fails `net.SplitHostPort` or holds a byte outside
  `[0-9A-Za-z.:[]-]` prints as `?`.
- **Exit codes,** decided in this order:

| Code | When | stdout |
|---|---|---|
| 2 | usage: unknown command or flag, missing flag, bad id (before any connection) | empty |
| 1 | metadata read failed, refused or invalid; output failed; `get` found nothing with complete coverage | empty |
| 4 | `doctor` with at least one `fail` | report |
| 3 | an observer omitted, a doctor check `unknown`, or `get` found nothing with incomplete coverage | report |
| 0 | otherwise; `unsupported` checks do not change the code | report |

## Alert rules (S8)

`deploy/observe/rules.yaml`, group `flaminio`. Each rule has the annotation
`runbook_url: https://github.com/jsfakian/Flaminio/blob/main/docs/runbooks.md#<anchor>`.

Every selector names its job, so a series of one job never feeds another job's rule.

| Alert | Expression | For | Severity, labels |
|---|---|---|---|
| `FlaminioLostRedundancy` | `max by (volume) (flaminio_volume_under_replicated_chunks{job="flaminio-meta"}) > 0` | 2m | critical |
| `FlaminioCapacityPressure` | `(flaminio_disk_data_allocated_bytes{job="flaminio-observer"} + flaminio_disk_data_quarantined_bytes{job="flaminio-observer"}) / flaminio_disk_data_bytes{job="flaminio-observer"} > 0.85` | 10m | warning, resource=data |
| `FlaminioCapacityPressure` | `(flaminio_disk_data_allocated_bytes{job="flaminio-observer"} + flaminio_disk_data_quarantined_bytes{job="flaminio-observer"}) / flaminio_disk_data_bytes{job="flaminio-observer"} > 0.95` | 2m | critical, resource=data |
| `FlaminioCapacityPressure` | `flaminio_disk_pool_used_pairs{job="flaminio-observer"} / flaminio_disk_pool_pairs{job="flaminio-observer"} > 0.85` | 10m | warning, resource=pool |
| `FlaminioCapacityPressure` | `flaminio_disk_pool_used_pairs{job="flaminio-observer"} / flaminio_disk_pool_pairs{job="flaminio-observer"} > 0.95` | 2m | critical, resource=pool |
| `FlaminioRecoveryStalled` | `(flaminio_resync_chunks{job="flaminio-observer"} > 0 and changes(flaminio_resync_remaining_bytes{job="flaminio-observer"}[10m]) == 0) or (flaminio_reconcile_chunks{job="flaminio-observer"} > 0 and changes(flaminio_reconcile_remaining_bytes{job="flaminio-observer"}[10m]) == 0)` | 10m | warning |
| `FlaminioTelemetryLost` | `up{job=~"flaminio-observer\|flaminio-meta"} == 0 or flaminio_observer_snapshot_fresh{job="flaminio-observer"} == 0 or flaminio_meta_view_ok{job="flaminio-meta"} == 0 or (up{job="flaminio-observer"} == 1 unless on(instance) flaminio_observer_snapshot_fresh{job="flaminio-observer"}) or (up{job="flaminio-meta"} == 1 unless on(instance) flaminio_meta_view_ok{job="flaminio-meta"})` | 2m | warning |

The promtool tests cover, for each rule, a firing case, a case below threshold or hold, and absent
series, which raise only `FlaminioTelemetryLost`. Three more cases: a metadata family scraped
under the observer job fires nothing; another node's zero progress for a volume does not silence a
stalled target's series; and a target with `up` 1 but no freshness series (observer or metadata)
fires `FlaminioTelemetryLost`, as does an observer with `up` 1 and no series at all. The anchors
are `lost-redundancy`, `capacity-pressure`, `recovery-stalled`, `telemetry-lost` and `doctor`.
