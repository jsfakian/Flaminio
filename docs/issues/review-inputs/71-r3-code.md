# Plan review: issue #71, round 3 (incremental), design-code

Diff 6a9d1a3..215748e, checked against /tmp/flaminio-plans-2026-11-12/71.

## Round-2 findings
- R2-C1: fixed (own directory; contracts "Directory and socket"; see R3-3 for gaps).
- R2-C2: partly fixed. The bound is now stated and matches the code (domain.go:184 is the 2^20 cap; engine.rs:735-741 ticks each target; tick calls progress, target.rs:862, 2187-2189). But spec S3 still says "never blocks or fails I/O" (R3-1).
- R2-C3: fixed (`start()` None without counters; `nothing_without_counters`).
- R2-C4: fixed in #71, but rests on an assumption #69 contradicts (R3-2).
- R2-C5: fixed (a, b); (c) is now stated as review plus campaign, which is acceptable.
- R2-C6: `--direct=1` fixed; the MinISR fixed but its citation is wrong (R3-4).
- R2-C7: fixed on paper (lint runs `rules-test`; the Makefile and ci.yml need no change since ci.yml:30 runs `make lint`); see R3-5.
- `Target::recovery`: no existing `recovery` name in flaminio-repl (grep empty); no clash.

## Findings

### R3-1 Medium. S3 still promises "never blocks or fails I/O"
spec.md:45 ends "...and never blocks or fails I/O", while design "Cost" says the walk runs under the engine lock that every tag task takes (up to 4 x 2^20 chunks plus runs, once a second). The two contradict, and the report says the "never delays" claim was replaced. No test bounds the walk: `recovery_counts_resync_and_reconcile_work` checks counts only; the 2^20 worst case is only "measured by the campaign" at a stated rate, but the campaign has no stated pass criterion for the p99 effect.
Fix: change S3 to "adds at most one read-only walk a second under the engine lock; it never fails I/O". Add a unit test that `recovery` on a target at the 2^20-chunk bound with work in every chunk finishes within a stated budget (or counts chunk visits), and give the campaign a pass limit.

### R3-2 Medium. The "invalid volume" row assumes what #69 does not do
decisions.md and contracts.md "view outcomes" say an invalid volume exports nothing and the rest still export, "assuming #69 rejects invalid data per volume". #69's design (flaminio-69 design.md:39, 47-50) rejects contradictory data as invalid and prints no successful output until collection and derivation finish: it reads as a whole-derivation failure, and only NotFound volumes go to `omitted_volumes`. As written, the row cannot be built on #69's `Derive`, and `TestViewOutcomes` would test a behaviour that does not exist.
Fix: either make the row "any invalid volume: no per-volume series, view_ok 0" (matching #69), or add a dependency on #69 to return per-volume errors and record it in both plans. Pick one before approval, since the settled answer now states the per-volume form.

### R3-3 Low. Directory rules leave three gaps
(a) `mkdir` with mode 0750 is subject to the umask (a 077 umask gives 0700, and the next start's `lstat` check then fails); say it is followed by `chmod`/`fchmod`. (b) "D is the control socket's directory" is not defined: compare path strings, or device and inode after creation? Path strings miss a symlinked or `..` form. Use `lstat` dev and inode of both. (c) control.rs:46-52 uses `create_dir_all` and a plain `set_permissions` today; the contract does not say whether the control socket's creation is touched (it should not be), so say "control.rs is unchanged".
Fix: state each; extend `refuses_an_unsafe_directory` with a 077 umask create and an aliased control directory.

### R3-4 Low. resync.sh:33 is not the RF2 template
design lab step 3 says the RF2 `create VOL 16G 2 1 256M fl-a` is "as resync.sh:33 creates its volume". scripts/checks/resync.sh:33 is `create "$VOL" 16G 1 1 256M "$FROM"`, an RF1 volume (the CLI is `create <volume> <size> [replicas] [min-isr] [chunk] [prefer-node]`, main.go:7). The arguments in the step are right; the citation is misleading.
Fix: cite only the argument order, not resync.sh.

### R3-5 Low. `make lint` now depends on a large pinned `go run` and an open pin
Makefile:13 pins `BUF` as `go run ...@v1.73.0`; `PROMTOOL` would pull the whole prometheus module (and its test deps) into every `make lint`, locally and in CI, while decisions.md:45 still says the release is open. `make lint` runs offline or in a sandbox only if the module cache is warm.
Fix: record the pinned version in the design before implementation, confirm `go run github.com/prometheus/prometheus/cmd/promtool@<v>` builds with the repo's Go, or keep `rules-test` as its own target and CI job that installs a release binary.

## Nits
- contracts.md "Directory and socket" mixes the 0700/no-group variant into the middle of a long sentence; a small table of (group, D mode, socket mode) would be clearer.
