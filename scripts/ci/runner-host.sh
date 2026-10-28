#!/usr/bin/env bash
# Sets up a Linux host for Flaminio's self-hosted GitHub Actions runners (docs/ci.md). Run it on
# the host, as the user the runners run as, from a checkout of the repository. Idempotent.
#   scripts/ci/runner-host.sh tools              system packages, Rust toolchains, check tools
#   scripts/ci/runner-host.sh runner NAME LABEL  register runner $PREFIX-NAME with LABEL (flaminio
#                                                for CI, flaminio-lab for the lab) and start it as
#                                                the user service actions-runner-NAME
# The registration token comes from `gh` (logged in with admin rights on the repository) or
# RUNNER_TOKEN.
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO=${REPO:-jsfakian/Flaminio}
PREFIX=${PREFIX:-centaur}
RUNNER_VERSION=${RUNNER_VERSION:-2.338.2}
RUNNERS=${RUNNERS:-$HOME/actions-runners}
CARGO_BIN=$HOME/.cargo/bin

tools() {
    # A C toolchain for ring and the cc crate; shellcheck for make lint; rsync for the fuzz corpus.
    sudo apt-get install -y -qq build-essential pkg-config shellcheck jq rsync openssl >/dev/null
    if [[ ! -x $CARGO_BIN/rustup ]]; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
            sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
    fi
    local pinned
    pinned=$(sed -n 's/^channel = "\(.*\)"/\1/p' dataplane/rust-toolchain.toml)
    "$CARGO_BIN/rustup" toolchain install "$pinned" --profile minimal -c rustfmt -c clippy
    # The Makefile's dated nightly: Miri, the sanitizers (rebuilding std), cargo-fuzz.
    local nightly
    nightly=$(sed -n 's/^NIGHTLY *?= *//p' Makefile)
    "$CARGO_BIN/rustup" toolchain install "$nightly" --profile minimal -c miri -c rust-src
    "$CARGO_BIN/cargo" "+$nightly" miri setup >/dev/null
    "$CARGO_BIN/cargo" install --locked -q cargo-deny@0.20.2 cargo-machete@0.9.2 cargo-fuzz@0.13.2
    echo "tools ready: $("$CARGO_BIN/rustup" toolchain list | tr '\n' ' ')"
}

runner() {
    local name=$1 label=$2 dir=$RUNNERS/$1 tarball token unit
    tarball=$RUNNERS/actions-runner-linux-x64-$RUNNER_VERSION.tar.gz
    mkdir -p "$dir"
    [[ -f $tarball ]] || curl -sSfL -o "$tarball" \
        "https://github.com/actions/runner/releases/download/v$RUNNER_VERSION/actions-runner-linux-x64-$RUNNER_VERSION.tar.gz"
    [[ -x $dir/config.sh ]] || tar -xzf "$tarball" -C "$dir"
    if [[ ! -f $dir/.runner ]]; then
        token=${RUNNER_TOKEN:-$(gh api -X POST "repos/$REPO/actions/runners/registration-token" --jq .token)}
        (cd "$dir" && ./config.sh --unattended --url "https://github.com/$REPO" --token "$token" \
            --name "$PREFIX-$name" --labels "$label" --work _work)
    fi
    unit=$HOME/.config/systemd/user/actions-runner-$name.service
    mkdir -p "$(dirname "$unit")"
    cat >"$unit" <<EOF
[Unit]
Description=GitHub Actions self-hosted runner $PREFIX-$name ($REPO)
After=network-online.target docker.service

[Service]
WorkingDirectory=%h/actions-runners/$name
ExecStart=%h/actions-runners/$name/run.sh
Restart=always
RestartSec=10
KillMode=process
KillSignal=SIGINT
TimeoutStopSec=5min

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload
    systemctl --user enable --now "actions-runner-$name.service"
    echo "$PREFIX-$name ($label): $(systemctl --user is-active "actions-runner-$name.service")"
}

case "${1:-}" in
    tools) tools ;;
    runner) runner "${2:?runner NAME LABEL}" "${3:?runner NAME LABEL}" ;;
    *) sed -n '2,10p' "$0" >&2; exit 2 ;;
esac
