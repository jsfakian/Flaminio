# Issue #4: run every fuzz target for 10 minutes clean — specification

## Behaviour

**S1. Ten minutes clean.** The Fuzz workflow (`.github/workflows/fuzz.yml`) runs every target
in `dataplane/fuzz` for 600 seconds and passes: no crash, timeout, out-of-memory or leak report.
A timeout is one input taking more than 10 seconds. Issue #4 closes on such a run on `main`. A failure it reports is fixed through S7 before then.

**S2. One harness per target.** What a target does with its input is a function of a library in
the fuzz crate, `flaminio_fuzz::<target>(&[u8])`. The `fuzz_target!` binary only calls it. The
same function is what the regression test replays, so the two cannot drift apart.

**S3. Regression inputs.** `dataplane/fuzz/regressions/<target>/` holds inputs, one file each,
that once made the target fail. They are committed. A file from the fuzzer keeps the name
libFuzzer gave it, `<kind>-<sha1>` (`crash-`, `timeout-`, `oom-`, `leak-`). A file made by hand is
named `seed-<what it covers>`. Every directory starts with one file, `empty`, of zero bytes.

**S4. Replayed on every pull request.** `make fuzz-regressions` feeds every file of every
`regressions/<target>/` to `flaminio_fuzz::<target>` on the stable toolchain. It fails, naming the
file, if the harness:
- panics;
- runs longer than 2 seconds on one input (the replay then stops, and lists the inputs it did
  not run);
- aborts the process (a stack overflow, for example): the file being replayed is written to
  standard error before it runs, so the log names it even then.

Each input runs on a thread with a 2 MiB stack, in an optimized build with debug assertions and
overflow checks. Both limits are stricter than the fuzzer's (10 seconds, an 8 MiB stack) on
purpose: this build runs faster and with smaller stack frames than the fuzzer's, which carries
sanitizer and coverage instrumentation, so equal limits would let an input that times out or
overflows in the fuzzer pass here. CI runs this in the Rust test job, and `make test` includes it. It needs no nightly toolchain and no libFuzzer
runtime.

**S5. Regression inputs seed the fuzzer.** `make fuzz` gives each run `regressions/<target>/` as
a second corpus directory, which libFuzzer reads but never writes, and a 10-second timeout per
input (`-timeout=10`). So the fuzzer itself replays every regression input at the start of every
run, under its memory and leak limits, which the stable test of S4 cannot enforce.

**S6. Every target is covered.** The regression test fails unless the `[[bin]]` entries of the
fuzz crate's `Cargo.toml` (what cargo-fuzz builds), the sources in `fuzz_targets/*.rs`, the
harnesses it knows and the directories under `regressions/` are the same set. `make fuzz` takes
its list of targets from `cargo fuzz list`, refuses an empty list, runs every target even after one
fails, and fails at the end naming every target that did, so one run reports them all.

**S7. From a failure to a fixed bug.**
- A failing Fuzz run keeps the failing inputs: as a workflow artifact for 90 days, and on the
  runner host in `~/.cache/flaminio-ci/fuzz-artifacts/<run id>/`.
- Any failed run opens an issue, labelled `fuzz`, linking the run, or comments on the one already
  open. The note says whether it kept failing inputs, or failed before fuzzing (a build or
  toolchain failure), so neither goes unnoticed. A job stopped at its time limit keeps and
  reports the inputs it found too. Runs never overlap.
- To fix one: copy the file into `regressions/<target>/`, see `make fuzz-regressions` fail (or,
  for an out-of-memory or leak input, `cargo +<nightly> fuzz run <target> <file>`, on Linux:
  LeakSanitizer does not run on macOS), fix the bug, see it pass, commit both. `docs/ci.md`
  describes this.

**S8. The fuzz crate is checked like the rest.** `make lint` runs clippy (warnings are errors)
and rustfmt on `dataplane/fuzz`, which is outside the data plane's workspace.

## Not changed

- The harness logic of the five targets, moved into the library, with one exception: the
  metadata pages harness now also reaches the page-pool bitmap decoder, which its selector never
  did (it mapped six values onto five decoders).
- The weekly schedule, and the corpus kept on the runner host.
