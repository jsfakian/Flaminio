#!/usr/bin/env bash
# Interop test: the Rust metadata client (flaminio-metaclient) against the Go metadata service,
# over mutual TLS, with the service on its in-memory store. Usage: make meta-interop
set -euo pipefail
cd "$(dirname "$0")/../.."
dir=$(mktemp -d)
trap 'kill "${pid:-}" 2>/dev/null || true; rm -rf "$dir"' EXIT

go build -o "$dir/flaminio-meta" ./meta/cmd/flaminio-meta
go build -o "$dir/flaminio-ca" ./control/cmd/flaminio-ca
"$dir/flaminio-ca" init "$dir/ca" "interop CA"
"$dir/flaminio-ca" token-key "$dir/token.key"
issue() { # name spiffe-id flags...
    local name=$1 id=$2
    shift 2
    openssl ecparam -name prime256v1 -genkey -noout 2>/dev/null | openssl pkcs8 -topk8 -nocrypt -out "$dir/$name.key"
    openssl req -new -key "$dir/$name.key" -subj "/CN=$name" |
        "$dir/flaminio-ca" sign "$dir/ca" --id "$id" "$@" >"$dir/$name.crt"
}
issue server spiffe://lab/flaminio/node/m/meta --server --ip 127.0.0.1
issue admin spiffe://lab/flaminio/admin/test --client
issue fl-a spiffe://lab/flaminio/node/fl-a/io --client

port=$((20000 + RANDOM % 20000))
"$dir/flaminio-meta" serve --store memory --listen "127.0.0.1:$port" --trust-domain lab \
    --token-key "$dir/token.key" --tls-cert "$dir/server.crt" --tls-key "$dir/server.key" \
    --tls-ca "$dir/ca/ca.crt" 2>"$dir/server.log" &
pid=$!
admin() { "$dir/flaminio-meta" --server "127.0.0.1:$port" --cert "$dir/admin.crt" --key "$dir/admin.key" --ca "$dir/ca/ca.crt" "$@" >/dev/null; }
for _ in $(seq 50); do admin nodes 2>/dev/null && break; sleep 0.1; done
for n in fl-a fl-b; do
    admin register "$n" "$n:7410"
    admin heartbeat "$n"
done
admin create v 2M 2 1 1M
admin attach v fl-a

cd dataplane
FLAMINIO_META_INTEROP="127.0.0.1:$port" FLAMINIO_META_INTEROP_DIR="$dir" \
    "${CARGO:-cargo}" test -q -p flaminio-metaclient --test interop -- --ignored --nocapture
