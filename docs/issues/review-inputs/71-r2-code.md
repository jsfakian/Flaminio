# Plan review: issue #71, round 2, focus design-code

Diff 76cf82f..6a9d1a3 plus the changed documents read through, checked against /tmp/flaminio-plans-2026-11-12/71.

## Earlier findings
- R1-C1: fixed (summaries, Events cut; no trace of them left in spec, design, contracts).
- R1-C2: fixed (server-only `node/<n>/observer`, no TLS client; auth.go leaves it the three open reads, `auth.go:79`).
- R1-C3: fixed on paper (`Input::Observe` handled by `drive_until`/`pump_with` as `delete`; one `IoCounters` per attachment in `Attached`); but see R2-C2 for the cost of building it.
- R1-C4: fixed (traces, logs cut; no log line of the daemon changes).
- R1-C5: fixed (`Target::progress`; `todo`/`pending` verified at target.rs:167, 187).
- R1-C6: fixed (no incarnation or cluster identity left).
- R1-C7, R1-C8, R1-C9: fixed (mutating verbs, recover, push, collector cut everywhere; grep finds only "stays open" text in intent, ADR, spec "Not in this step").
- R1-C10: fixed (seam at ublk.rs:1283 and 1459 verified: one commit per iteration after one `res`).
- R1-C11: fixed (four checks implementable from /proc, /dev, /sys; four report `unsupported`).
- R1-C12: fixed (73 recomputed and correct: 20+2+26+7+16+2).
- R1-C13: fixed (exit 4). R1-C14: fixed (`classify` from `res`). R1-C15: fixed (`GetVolume`; disk by observers). R1-C16, R1-C17: cut. Nits 1-4: fixed.

## Findings

### R2-C1 — Medium
**The default telemetry socket is in a directory the observer cannot enter.**
contracts.md "Daemon flags" puts the socket at `/run/flaminio/telemetry.sock` (mode 0660, root:GID) for a process that runs without root. In the lab, when a daemon starts with `ATTACH`, `scripts/guest/node.sh:61` runs `install -d -m 700 /run/flaminio` and writes the volume token there as `tee` makes it (mode 644, `node.sh:62`). The directory, not the socket, then decides access: the observer gets EACCES and every node shows `stale`. Loosening the directory (0711) would expose the token files by name. `control::spawn` only does `create_dir_all` for the parent (`control.rs:46`). Design and contracts say nothing about the parent directory.
Fix: give the telemetry socket its own directory (for example `/run/flaminio-telemetry/`, root:GID, 0750) created by the daemon or `node.sh`, state its mode in contracts, and make `mode_and_group` also check the parent directory. Make lab step 1 fail if the observer cannot connect.

### R2-C2 — Medium
**The snapshot is built under the engine mutex, with a cost that is unbounded and untested; "never delays I/O" has no test.**
S3 says `observe` "never blocks, delays or fails I/O". In the design the telemetry thread sends `Input::Observe`; while a volume is attached it is handled inside `pump_with` with `Shared.engine` held (`ublk.rs:586-590`), the lock every tag task takes. `Target::progress` walks every chunk and sums `todo.blocks()`/`pending.blocks()`, and `ExtentSet::blocks` sums all runs (`extent.rs:65-68`; `target.rs:385` `chunks: Vec<Chunk>`). That is O(chunks + runs) once a second, with no stated bound (a large volume with small chunks, or a fragmented resync). The design's "no lock added" holds only for `Attached`. Tests name only the 1 s cache and a 2 s silence, not the stall; the campaign polls every 5 s and so does not hit a worst case.
Fix: state the bound (maximum chunks and runs) and either keep the counts incrementally in the target or skip chunks without a resync/reconcile cheaply; add a unit test that `progress` is linear in a stated size and a lab/bench step that records the p99 effect with the largest volume during a resync.

### R2-C3 — Low
**The off path still pays a clock read.**
design.md "Seam": each loop iteration takes `start = Instant::now()` unconditionally, then calls `record`; S1/S9 say that without `--telemetry` nothing is recorded and `record` is one branch. The off configuration then costs one clock read per request, which the on/off campaign measures as "on minus off" and understates.
Fix: take `start` only when `Shared.io` is `Some` (or pass `Option<Instant>`), and say so in S9.

### R2-C4 — Low
**S4 does not say what the metadata view does with #69's omitted volumes or with per-volume invalid data.**
#69's design (`/home/jsfakian/Documents/src/flaminio-69/docs/issues/69/design.md` "Collection and derivation") puts a concurrently deleted volume in `omitted_volumes` without failing the view, and rejects contradictory ISR/geometry as invalid data. #71 S4 covers only a failed or over-limit read. Undefined: whether an omitted volume exports no series while the view stays ok, and whether one invalid volume turns every volume's series off (hiding a real loss of redundancy elsewhere behind `FlaminioTelemetryLost`).
Fix: add both cases to S4 and contracts, and to `TestFailedReadExportsNoEntities` (omitted volume: no series, view ok; invalid volume: the chosen behaviour, with the reason).

### R2-C5 — Low
**Three rules have no test that could fail.**
(a) S1 "with telemetry off, nothing is recorded" (`Shared.io` None): no unit test; `record_buckets_sums_and_bytes` runs with counters present. (b) S5 "the CLI never looks for an administrator, node or Kubernetes credential": no test (for example default paths, environment variables, kubeconfig untouched). (c) S9 "no lock, allocation, metadata call, wait or log line" on the I/O path: only the campaign, which measures time, not these properties.
Fix: a test that `record` with `None` changes nothing and does not read the clock; a CLI test that only the explicit flags are opened (fake file system or strace-free assertion on the loader); for (c), state that review and the campaign are the evidence, or add a `record` benchmark/allocation assertion.

### R2-C6 — Low
**Lab steps are underspecified in ways that break the checks.**
Step 3 compares counter deltas with fio's `total_ios` for 4K random reads; it does not say `--direct=1`. Without it the block device's page cache and readahead make ublk requests differ from fio's I/Os. Step 4 uses "an RF2 volume" and writes while R is stopped; with MinISR 2 writes stop instead of shrinking the ISR (metactl `create VOL SIZE REPLICAS MINISR CHUNK PREFER`, as `resync.sh:33`). The MinISR is not given.
Fix: write `direct=1` and `create ... 2 1 ...` into the steps.

### R2-C7 — Low
**`rules-test` is not wired into CI.**
S8's promtool tests run through a new `make rules-test`, and the design's "before a pull request" line lists it, but "Files touched" has no `.github/workflows/ci.yml` change or tool install (CI jobs are one make target each, `ci.yml`; `make lint` does not include it), and the pinned promtool release is an open item. As written, rule tests never run on a pull request.
Fix: add a `rules-test` job (or fold it into `make lint`) with the pinned promtool install, and list the workflow file.

## Checked, no defect
`Input::Observe` driven as `delete` (`control.rs:173-190`, `engine.rs:696-705`, `drive_until` `node.rs:344-376`, `pump_with`); `Attached` changes confined to `ublk.rs:682/889/1044` and `control.rs`; both commit sites; `Store::stats` fields and quarantined arithmetic (`bitmap.rs:56-76`); `GetVolume` fields for `volume get`; ID grammar equals `domain.ValidID`; series count 73; #69 assumptions (`Collect`, `Derive`, 30 s, 4 in flight, reader identity, moving chunk not degraded) agree with its draft text.

Reviewed commit: da0177afcf1ee397976f3ba904714105d671695d
