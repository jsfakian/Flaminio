#!/usr/bin/env bash
# Runs every dataplane cargo-fuzz target for FUZZ_SECONDS each (follow-ups §6, docs/4/spec.md),
# with its committed regression inputs as a second corpus directory, which libFuzzer reads but
# never writes. Every target runs even after one fails; the script fails at the end, naming them.
# Installs the nightly toolchain (NIGHTLY, default nightly) and cargo-fuzz when missing.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root/dataplane/fuzz"

seconds="${FUZZ_SECONDS:-60}"
nightly="${NIGHTLY:-nightly}"

if ! command -v rustup >/dev/null; then
  echo "rustup is required to install the nightly toolchain for cargo-fuzz" >&2
  exit 1
fi
rustup toolchain install "$nightly" --profile minimal >/dev/null
if ! cargo "+$nightly" fuzz --help >/dev/null 2>&1; then
  echo "installing cargo-fuzz..."
  cargo "+$nightly" install cargo-fuzz --locked
fi
# The targets cargo-fuzz builds: the [[bin]] entries, which the regression test keeps in step with
# fuzz_targets/ and regressions/.
# (A read loop rather than mapfile, which macOS's bash 3.2 lacks.)
targets=()
while IFS= read -r t; do targets+=("$t"); done < <(cargo "+$nightly" fuzz list)
# A failing `cargo fuzz list` is invisible to set -e here: refuse to fuzz nothing and pass.
((${#targets[@]})) || { echo "fuzz: cargo fuzz list found no targets" >&2; exit 1; }

echo "fuzzing ${#targets[@]} targets for ${seconds}s each"
failed=()
for t in "${targets[@]}"; do
  echo "==> $t"
  # cargo-fuzz creates the working corpus only when it picks the corpus itself.
  mkdir -p "corpus/$t"
  # A 10-second timeout per input (libFuzzer's default is 1200 s); the regression test is stricter.
  # cargo-fuzz builds optimized with debug assertions and overflow checks unless given -O, so the
  # fuzzer checks what the replay's dev profile does.
  if ! cargo "+$nightly" fuzz run "$t" "corpus/$t" "regressions/$t" -- \
      -max_total_time="$seconds" -timeout=10; then
    failed+=("$t")
  fi
done
if ((${#failed[@]})); then
  echo "fuzz: failed: ${failed[*]} (a failing input, in dataplane/fuzz/artifacts/, or a build failure: see the log above)" >&2
  exit 1
fi
echo "fuzz: all targets finished cleanly"
