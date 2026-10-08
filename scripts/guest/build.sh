#!/usr/bin/env bash
# Builds the data plane in release mode. Arguments are passed to cargo.
cd "$HOME/flaminio/dataplane" || exit 1
log "cargo build --release $*"
set +e
cargo build --release "$@" 2>&1 | grep -vE '^\s+(Compiling|Downloaded|Downloading)'
rc=${PIPESTATUS[0]}
set -e
exit "$rc"
