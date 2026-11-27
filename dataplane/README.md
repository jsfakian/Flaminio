# Flaminio data plane

The Rust half of Flaminio ([ADR 0001](../docs/adr/0001-languages.md)), serving the replicated
Phase-1 lab system.

| Crate | What it is |
|---|---|
| [`flaminio-format`](crates/flaminio-format) | The on-disk format v1: superblock, layout, metadata pages, journal records and replay scanning. Sans-I/O, `#![forbid(unsafe_code)]`. Reference implementation of [`docs/format/on-disk-format-v1.md`](../docs/format/on-disk-format-v1.md) |
| [`flaminio-store`](crates/flaminio-store) | The replica store: recovery (checkpoint + journal replay), chunk create/delete, redirect-on-write and deferred writes, verified reads, snapshots, discard, fencing, checkpoints, `fsck`. Synchronous and single-threaded for now, running on any `BlockDevice` |
| [`flaminio-uring`](crates/flaminio-uring) | `io_uring` + `O_DIRECT` block device and page-aligned buffers. The only crate allowed `unsafe`, in two opt-in modules |
| [`flaminio-bench`](crates/flaminio-bench) | fio-like microbenchmark that drives the store directly |
| [`flaminio-io`](crates/flaminio-io) | The data-plane daemon: hosts replica storage and a volume target with a `/dev/ublkbN` frontend through [`libublk`](https://crates.io/crates/libublk) (Linux only) |
| [`flaminio-disktool`](crates/flaminio-disktool) | `flaminio-disk`, a development tool that formats and inspects disks or disk-image files |
| [`fuzz`](fuzz) | `cargo-fuzz` targets for on-disk and replication parsers (outside the workspace) |

Progress and historical lab results are tracked in the
[`Phase-1 plan`](../docs/phase1-plan.md) and [`MVP plan`](../docs/mvp-plan.md).

## Bounded recovery reports

Replica uncertainty uses 65,536 slots per volume: held chunks plus retained precise write
records. At capacity, only the affected chunk becomes a conservative whole-chunk cover with
its maximum write version. Freed arena records are reused across chunks. Prune/Settle clears
a cover only above that maximum; restart coverage keeps its separate newer-epoch rule.

The budget can be reached by routine writes: idle chunks retain their last in-flight window,
and a stalled write can hold the prune bound while other chunks are written. A coarse chunk
may need whole-chunk reconciliation at takeover. This bounds representation and work without
claiming unchanged recovery latency or an aggregate limit across volumes.

Fenced reports remain inside existing wire limits, with checked encoding/decoding and complete
target validation before session or lease authority. Newly created chunks have explicit empty
rows. Upgrade producers and consumers together before new attach/provisioning; strict
consumers can refuse older producers that omit those rows. Mixed-version rolling support is
not certified. Unsupported held inventories are quarantined without data deletion or fence
changes, while other volumes remain available. Recovery requires the existing replica delete
for a metadata-deleted volume, or explicit operator cleanup and restart.

See [protocol §3/§6](../docs/replication-protocol.md) and
[issue #86](../docs/issues/86/spec.md).

## Geometry admission

Volumes use 4 KiB logical blocks and whole power-of-two chunks from 1 MiB through 1 GiB.
The 65,536-chunk wire ceiling is further restricted by metadata's 4 MiB future-view budget:
RF 1–5 admit at most 5137/3881/3119/2607/2239 chunks. Engine and session validation precedes
narrowing, peer/core changes and old-incarnation cleanup; higher authoritative epochs still
depose a target even when the reply's payload is invalid. Hello sends use checked encoding
and write no bytes on refusal.

Store preflight checks the actual layout and all unique missing chunk slots without writes.
Startup geometry outside the supported envelope is refused while preserving data/fences.
Explicit cleanup remains available; no automatic migration is added. These bounds do not
reserve distributed resources or certify scale. See [issue #80](../docs/issues/80/spec.md)
and [ADR 0018](../docs/adr/0018-admit-only-representable-volume-geometries.md).

## Crash consistency

`flaminio-store`'s `crash_consistency` test runs random workloads on a simulated device with a
volatile write cache. It cuts the power at a random point (sometimes a second time, during
recovery), recovers, and checks that every acknowledged write survived, that the operation in
flight is all-or-nothing, and that metadata and checksums are consistent. To run a long
campaign:

```bash
PROPTEST_CASES=50000 cargo test --release -p flaminio-store --test crash_consistency
```

## Build and test

Requires stable Rust ≥ 1.88 (let-chains).
On Linux, building `flaminio-io` also needs libclang (`apt install libclang-dev`): `libublk`
generates its kernel bindings with bindgen.

```bash
cargo test                          # unit, integration and property tests
cargo clippy --all-targets          # must be warning-free
cargo fmt --check
```

## Try the format

The tool works on regular files (sparse, so large sizes cost nothing), so any machine can use it:

```bash
cargo run -p flaminio-disktool -- format /tmp/disk.img --size 100G --label node-a/nvme0
cargo run -p flaminio-disktool -- inspect /tmp/disk.img
```

`format` refuses to overwrite an existing Flaminio disk unless you pass `--force`. On a real
device it formats whatever it is pointed at, so only point it at a scratch disk.

## Fuzzing

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz && cargo +nightly fuzz run journal      # or: superblock, metadata_pages
```
