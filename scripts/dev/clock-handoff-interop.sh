#!/usr/bin/env bash
# Native clock handoff acceptance: production Go issuer and Rust daemon control/engine.
set -euo pipefail
cd "$(dirname "$0")/../.."
dir=$(mktemp -d /tmp/fl83-interop.XXXXXX)
pid=""
trap '[[ -z $pid ]] || kill "$pid" 2>/dev/null || true; rm -rf "$dir"' EXIT
go build -o "$dir/flaminio-meta" ./meta/cmd/flaminio-meta
go build -o "$dir/fixture" ./meta/testdata/clock-handoff-interop
"$dir/fixture" --dir "$dir" >"$dir/fixture.log" 2>&1 &
pid=$!
for _ in $(seq 100); do
    [[ -s $dir/addr ]] && break
    kill -0 "$pid" 2>/dev/null || { cat "$dir/fixture.log" >&2; exit 1; }
    sleep 0.1
done
[[ -s $dir/addr ]] || { cat "$dir/fixture.log" >&2; exit 1; }
cd dataplane
FLAMINIO_CLOCK_INTEROP_DIR="$dir" "${CARGO:-cargo}" test -p flaminio-io --bin flaminio-io native_handoff_fixture -- --ignored --nocapture --test-threads=1
