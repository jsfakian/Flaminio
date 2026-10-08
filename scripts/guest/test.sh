#!/usr/bin/env bash
# Runs the test suite in release mode. PROPTEST_CASES controls the crash-test campaign size.
cd "$HOME/flaminio/dataplane" || exit 1
log "cargo test --release (PROPTEST_CASES=${PROPTEST_CASES:-default})"
set +e
cargo test --release "$@" 2>&1 | grep -E '^(test result|error|warning|failures|---- )|panicked|FAILED'
rc=${PIPESTATUS[0]}
set -e
exit "$rc"
