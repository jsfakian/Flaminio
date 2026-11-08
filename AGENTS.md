# Flaminio

Replicated block storage for Kubernetes: a Rust data plane (`ublk` + `io_uring`, a replica store
on raw disks, synchronous replication) and a Go control plane (an etcd-backed metadata service,
the CSI driver, the node agent). Architecture: `docs/ARCHITECTURE.md`. Replication rules:
`docs/replication-protocol.md`. On-disk format: `docs/format/on-disk-format-v1.md`. Plan:
`docs/phase1-plan.md`, open work in `docs/followups.md` and the
[follow-up issues](https://github.com/jsfakian/Flaminio/issues?q=label%3Afollow-up); performance
work in `docs/performance-plan.md`. Each issue's plan: `docs/issues/<n>/`.

## Code map

**Data plane (`dataplane/crates/`)**

| Crate | Role |
|---|---|
| `flaminio-repl` | Replication core: target, replica and metadata state machines without I/O, and the deterministic simulator (`src/sim`) |
| `flaminio-store` | Replica store: chunk replicas on a raw device, journal, checkpoints, recovery |
| `flaminio-format` | On-disk format v1: superblock, metadata pages, journal encoding, checksums |
| `flaminio-uring` | `io_uring` + `O_DIRECT` block device, aligned buffers, staging |
| `flaminio-net` | Replication transport: framed protocol over mutual TLS 1.3, peer identities, capability tokens |
| `flaminio-node` | Node engine: the replica service and volume targets, driven by `flaminio-repl` |
| `flaminio-io` | The daemon: `/dev/ublkbN` (`ublk.rs`), the node (`node.rs`), the control socket (`control.rs`) |
| `flaminio-metaclient` | The data plane's gRPC client of the metadata service |
| `flaminio-bench`, `flaminio-disktool` | Store microbenchmark; format and inspect disks |

Fuzz targets: `dataplane/fuzz/` (regression inputs in `regressions/<target>/`).

**Control plane (Go)**

| Path | Role |
|---|---|
| `meta/domain` | Volumes, chunks, placement (`Place`), ISR and epoch transitions |
| `meta/service` | gRPC service over etcd (`service.go`); per-method authorization (`auth.go`) |
| `meta/kv` | etcd access |
| `meta/cmd/flaminio-meta` | Server and admin CLI (`metactl` in the lab) |
| `control/csi` | CSI driver: controller, node plugin, tombstone sweep |
| `control/agent` | Node agent: device discovery, claim policy |
| `control/drain`, `control/failover` | Drain and failover controllers |
| `pkg/pki`, `pkg/captoken` | Identities (SPIFFE URI SANs), capability tokens |
| `api/proto` → `api/gen` | Protobuf contracts and generated Go code (`make proto`) |

**Lab and scripts:** `scripts/lab.sh` (every lab command, through the Makefile),
`scripts/guest/` (run in the VMs), `scripts/host/`, `scripts/checks/` (lab checks),
`scripts/tools/` (script tests, run by `make lint`). Lab guide: `docs/lab.md`.

## Commands

Use the Makefile; `make help` lists everything.

```bash
make build test      # debug build; Rust and Go tests, fuzz regression inputs
make lint            # clippy -D warnings, rustfmt, go vet, gofmt, buf lint, shellcheck, actionlint, script tests
make race hygiene contracts sim   # the rest of what CI runs on a pull request (docs/ci.md),
make sanitize QUICK=1             # with the sanitizers' quick run (Linux, nightly toolchain)
make fmt             # format Rust and Go
make proto           # regenerate api/gen after an api/proto change
make meta-vectors    # regenerate the metadata transition vectors from the Rust model
make crash-test miri sanitize     # longer or nightly checks (sanitize in full)
```

Before a pull request: `make lint test race hygiene contracts`. Lab targets (`make deploy`,
`make bench-*`, `make chaos`, …) run on the shared lab VMs: some take hours and reformat disks.
Never run them unless asked.

## Principles

- Keep diffs small; don't add features unless asked.
- Read code in ranges: grep for the symbol, then read around it. Don't re-read a file you
  already have.
- Safety comes from epochs, fencing and linearizable metadata, not from timeouts
  (ARCHITECTURE §2, §8). A write is acknowledged only once it is durable on every in-sync
  replica.
- Replication, membership and recovery logic lives in `flaminio-repl`, as sans-I/O state
  machines the simulator checks. A new safety rule gets a simulator check and an injected bug
  that the simulator must catch.
- The steady-state I/O path makes no metadata-service round trips and takes no locks.
- Every connection is mutual TLS; every metadata RPC is authorized per method
  (`meta/service/auth.go`); replicas check capability tokens.
- etcd writes that depend on a read are conditional transactions, never read-modify-write.
- Ids reaching shell commands or socket lines are validated and quoted.
- Accepted ADRs in `docs/adr/` are binding. To change one, write a superseding ADR (`adr`
  skill).
- Keep `README.md`, `docs/lab.md`, `docs/followups.md` and the Makefile's help aligned with
  real commands and results.

## Issue pipeline

`/ship-issue <n>` (or its stages `/issue-to-spec`, `/spec-to-design`, `/ship-spec`): see
`docs/issues/README.md`. Skills and agents live in `.claude/`, the one source; `.cursor/`
holds thin pointers. Change the `.claude/` files, never only the mirror.
