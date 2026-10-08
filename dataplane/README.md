# Flaminio data plane

The Rust half of Flaminio ([ADR 0001](../docs/adr/0001-languages.md)). It is currently in
phase 0.

| Crate | What it is |
|---|---|
| [`flaminio-format`](crates/flaminio-format) | The on-disk format v1: superblock, layout, metadata pages, journal records and replay scanning. Sans-I/O, `#![forbid(unsafe_code)]`. Reference implementation of [`docs/format/on-disk-format-v1.md`](../docs/format/on-disk-format-v1.md) |
| [`flaminio-store`](crates/flaminio-store) | The replica store: recovery (checkpoint + journal replay), chunk create/delete, redirect-on-write and deferred writes, verified reads, snapshots, discard, fencing, checkpoints, `fsck`. Synchronous and single-threaded for now, running on any `BlockDevice` |
| [`flaminio-disktool`](crates/flaminio-disktool) | `flaminio-disk`, a development tool that formats and inspects disks or disk-image files |
| [`fuzz`](fuzz) | `cargo-fuzz` targets for every on-disk parser (outside the workspace) |

Progress toward the MVP is tracked in [`docs/mvp-plan.md`](../docs/mvp-plan.md).

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
