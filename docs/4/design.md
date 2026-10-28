# Issue #4: run every fuzz target for 10 minutes clean — design

## The fuzz crate becomes a library with thin targets

`dataplane/fuzz/src/lib.rs` gets one public function per target, holding the body that is in
`fuzz_targets/<target>.rs` today, unchanged:

```rust
pub fn superblock(data: &[u8]) { ... }
pub fn metadata_pages(data: &[u8]) { ... }
pub fn journal(data: &[u8]) { ... }
pub fn repl_frame(data: &[u8]) { ... }
pub fn capability_token(data: &[u8]) { ... }
```

Each `fuzz_targets/<target>.rs` shrinks to
`fuzz_target!(|data: &[u8]| flaminio_fuzz::<target>(data));`.

**Rule:** `lib.rs` never refers to `libfuzzer_sys`. The crate still depends on it, for the
binaries, so `cargo test` compiles it (its build script needs a C++17 compiler: `build-essential`
on the runners, Xcode's tools on macOS), but nothing in the test binary references libFuzzer, so
the linker leaves it out and the test harness keeps its own `main`.

## The regression test

A unit test module in `src/lib.rs`, run with `cargo test --locked --lib` in `dataplane/fuzz`
(`--lib` only skips doc-tests; the binaries are not built by `cargo test` either way, as they
have `test = false` and there is no `tests/` directory):

- `TARGETS: &[(&str, fn(&[u8]))]` lists the five harnesses by name.
- **The set check (S6):** the names in `TARGETS`, the file stems of `fuzz_targets/*.rs` and the
  directory names under `regressions/` must be equal. Paths come from `CARGO_MANIFEST_DIR`.
- **The replay (S4):** for each target, the files of `regressions/<name>/` in sorted order. For
  each file:
  - the path is written with `std::io::stderr().write_all`, which libtest does not capture, so an
    abort still leaves it in the log;
  - the harness runs on a thread spawned with an 8 MiB stack, inside `catch_unwind`; the result
    comes back through a channel;
  - waiting longer than 10 seconds records a timeout and ends the replay: the remaining
    inputs are listed as not run. The stuck thread is left behind until the test process exits,
    so at most one is ever spinning;
  - a panic records the file.

  The test fails at the end, listing every failed file, so one run shows everything a change
  broke.

## Seeding the fuzzer

`scripts/dev/fuzz.sh`:
- takes its targets from `fuzz_targets/*.rs` instead of a hard-coded list, runs them all, and
  exits with an error at the end listing those that failed;
- creates `corpus/<target>` itself: cargo-fuzz only creates it when it chooses the corpus, and
  libFuzzer refuses a corpus directory that does not exist;
- runs `cargo fuzz run <target> corpus/<target> regressions/<target> -- -max_total_time=<s>
  -timeout=10`: libFuzzer's default timeout is 1 200 seconds, which would let a 10-minute hang
  pass as a slow input.
  libFuzzer writes new inputs only to the first directory and never deletes seed files, so the
  committed regression inputs are left as they are.

## Fuzz workflow

The `fuzz` job, on failure:
- uploads `dataplane/fuzz/artifacts/` (now with `retention-days: 90`);
- copies them to `~/.cache/flaminio-ci/fuzz-artifacts/<run id>/` on the runner;
- sets a job output, `inputs`, to `true` when `artifacts/` holds any file, from a step that runs
  with `if: failure()`. A build or toolchain failure leaves it unset.

A second job, `report`, on `[self-hosted, flaminio]`, needs `fuzz` and runs with
`if: failure() && needs.fuzz.outputs.inputs == 'true'`: without `failure()`, GitHub would add
`success()` to the condition and the job would never run after a failed `fuzz`. It has
`issues: write` and nothing else, checks nothing out, and sets two variables for `gh`: `GH_TOKEN`
to the workflow's token, so `gh` on the runner does not fall back to the administrator login it
is configured with, and `GH_REPO` to `github.repository`, which `gh` cannot read from a checkout
it does not have. It:
- creates the `fuzz` label if it is missing (`gh label create fuzz --force`);
- comments on the open issue labelled `fuzz` if there is one, or opens "Fuzz found failing
  inputs", labelled `fuzz`, linking the run.

Keeping it a separate job keeps `issues: write` away from the steps that build and run code.

## Make, lint and CI

- `dataplane/fuzz/Cargo.toml` (the root of its own workspace) gets `[profile.dev] opt-level = 3`:
  the replay then runs optimized, as the fuzzer's code is, with the debug assertions and overflow
  checks a dev build keeps. It is still faster than the fuzzer, which adds instrumentation, so the
  weekly run decides inputs close to the 10-second limit. cargo-fuzz's own builds (release) are
  unaffected.

- `make fuzz-regressions`: `cd dataplane/fuzz && cargo test --locked --lib`.
- `make test` runs it after the data plane's and the Go tests.
- `make lint` adds `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` in
  `dataplane/fuzz`. Clippy builds the five binaries too; they build on stable.
- CI's Rust test job runs `make fuzz-regressions` after `cargo test --locked` in `dataplane`.
  `CARGO_TARGET_DIR` (from the setup action) applies to the fuzz crate as well: its builds share
  the runner's persistent target directory, which is harmless.
- `docs/ci.md` gets the S7 procedure, and the table row of the Rust test job.

## Verifying S1

A Fuzz run with 600 seconds per target, dispatched on `main`. If it reports failing inputs, they
go into `regressions/` and get fixed in this issue's pull request; the issue closes on the first
clean run on `main` after it is merged.

## Decisions

- **Harness functions in the fuzz crate, not in the crates under test.** The harnesses build
  their inputs (repairing a CRC, choosing a page type) in ways that serve fuzzing only; keeping
  them next to the targets keeps the product crates free of fuzz code. *Rejected:* replaying
  regressions from `flaminio-format`'s and `flaminio-net`'s own tests, which would duplicate each
  harness.
- **A unit test in the library, not an integration test.** An integration test would make
  `cargo test` build and link the five binaries every time, for nothing.
- **Memory and leak limits stay with the fuzzer** (S5). Enforcing them in the stable test would
  need an allocator wrapper and LeakSanitizer; the weekly run replays every regression input
  with them anyway.
- **Crashes are added by hand** (S7), not by the workflow: a failing input needs a fix in the
  same pull request, which an automatic commit cannot provide. The workflow makes sure nobody
  misses one.

## Risks

- A harness that fails on purpose (the `assert_eq!` in `repl_frame`) reports that as a failure in
  the regression test too. That is intended: the assertion is the property being fuzzed.
- A hung input leaves one thread spinning until the test process exits; the replay stops at the
  first timeout, so there is never more than one.
