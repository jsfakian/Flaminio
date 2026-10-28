#!/usr/bin/env bash
# Run every dataplane cargo-fuzz target for FUZZ_SECONDS each (follow-ups §6).
# Installs the nightly toolchain and cargo-fuzz when missing.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root/dataplane/fuzz"

seconds="${FUZZ_SECONDS:-60}"
targets=(superblock metadata_pages journal repl_frame capability_token)

if ! command -v rustup >/dev/null; then
  echo "rustup is required to install the nightly toolchain for cargo-fuzz" >&2
  exit 1
fi
rustup toolchain install nightly --profile minimal >/dev/null
if ! cargo +nightly fuzz --help >/dev/null 2>&1; then
  echo "installing cargo-fuzz..."
  cargo +nightly install cargo-fuzz --locked
fi

echo "fuzzing ${#targets[@]} targets for ${seconds}s each"
for t in "${targets[@]}"; do
  echo "==> $t"
  cargo +nightly fuzz run "$t" -- -max_total_time="$seconds"
done
echo "fuzz: all targets finished cleanly"
