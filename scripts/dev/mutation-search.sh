#!/usr/bin/env bash
# Finds the first seed at which a mutation check of the simulator catches its bug
# (docs/issues/11, the search): seeds [0, END) split into JOBS ranges (default: the number of
# cores), one exact test per range, in parallel, in release. A range's log is
# dataplane/target/mutation-search/CHECK/<first seed>.log.
#
# The answer is the lowest catch whose lower ranges were all searched in full without one. A range
# that ends otherwise (a seed that fails with no bug on, which the check reports as a panic) stops
# the search there: that seed is a bug to fix first.
#
# Usage: scripts/dev/mutation-search.sh CHECK END [JOBS]
#   e.g. scripts/dev/mutation-search.sh catches_fast_reads_across_a_fence 4000000
set -euo pipefail

usage() {
  echo "usage: $0 CHECK END [JOBS]" >&2
  exit 2
}
(($# == 2 || $# == 3)) || usage
check="$1"
end="$2"
jobs="${3:-$(nproc)}"
[[ "$check" =~ ^[a-z0-9_]+$ ]] || { echo "not a check's name: '$check'" >&2; exit 2; }
[[ "$end" =~ ^[1-9][0-9]*$ ]] || { echo "END must be a positive number: '$end'" >&2; exit 2; }
[[ "$jobs" =~ ^[1-9][0-9]*$ ]] || { echo "JOBS must be a positive number: '$jobs'" >&2; exit 2; }

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root/dataplane"
cargo="${CARGO:-cargo}"

echo "building the simulator's tests (release)..."
bin="$("$cargo" test --release --no-run -p flaminio-repl --lib --message-format=json \
  | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -n 1)"
[[ -n "$bin" && -x "$bin" ]] || { echo "no test binary in cargo's output" >&2; exit 1; }

logs="$root/dataplane/target/mutation-search/$check"
rm -rf "$logs"
mkdir -p "$logs"

size=$(((end + jobs - 1) / jobs))
starts=()
pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' INT TERM
for ((a = 0; a < end; a += size)); do
  n=$((a + size > end ? end - a : size))
  FLAMINIO_MUTATION_NO_HINT=1 FLAMINIO_MUTATION_FIRST="$a" FLAMINIO_MUTATION_SEEDS="$n" \
    "$bin" "sim::tests::$check" --include-ignored --exact --nocapture --test-threads=1 \
    >"$logs/$a.log" 2>&1 &
  pids+=("$!")
  starts+=("$a")
done
echo "searching [0, $end) for $check in ${#starts[@]} ranges of $size seeds; logs in $logs"
# Each range's end is read from its log, not from its status.
for p in "${pids[@]}"; do
  wait "$p" || true
done

for a in "${starts[@]}"; do
  b=$((a + size > end ? end : a + size))
  log="$logs/$a.log"
  # A wrong name runs nothing, and passes.
  if ! grep -qx 'running 1 test' "$log"; then
    echo "[$a, $b) ran no test: is '$check' a check's name? See $log" >&2
    exit 1
  fi
  # The harness prints the test's name ahead of the check's line, on the same line.
  if line="$(grep -m 1 -o "$check: caught at seed [0-9].*" "$log")"; then
    seed="$(sed -n 's/.*: caught at seed \([0-9]*\).*/\1/p' <<<"$line")"
    echo "$line"
    echo "first catch of $check: $seed"
    exit 0
  fi
  if grep -qF "$check: searched [$a, $b): no catch" "$log"; then
    continue
  fi
  echo "searched [0, $a) in full; [$a, $b) failed: $log" >&2
  exit 1
done
echo "no catch in [0, $end)"
