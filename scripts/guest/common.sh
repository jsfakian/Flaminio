# shellcheck shell=bash
# Prepended to every guest script (scripts/guest/*.sh) by run_on_vm and run_detached; runs inside
# the lab VMs.
set -euo pipefail

log() { printf '\033[1;34m[%s]\033[0m %s\n' "$(hostname)" "$*" >&2; }
die() { printf '\033[1;31m[%s] error:\033[0m %s\n' "$(hostname)" "$*" >&2; exit 1; }

# The dedicated Flaminio data disk: the whole disk whose serial contains "data"
# (libvirt sets "flaminio-data", LXD sets "lxd_data").
data_disk() {
    local disk
    disk=$(lsblk -dn -o PATH,SERIAL | awk '$2 ~ /data/ {print $1; exit}')
    [[ -n $disk ]] || die "no data disk found (lsblk: $(lsblk -dn -o PATH,SERIAL,SIZE | tr '\n' ';'))"
    echo "$disk"
}

# Sends one command to the node's control socket and prints the answer. The socket's path goes to
# python3 as an argument: sudo resets the environment. CONTROL_SOCKET overrides it only under
# FLAMINIO_SCRIPT_TEST=1 (scripts/tools/node-scripts-test.sh).
control() {
    local sock=/run/flaminio/node.sock
    [[ ${FLAMINIO_SCRIPT_TEST:-0} != 1 ]] || sock=${CONTROL_SOCKET:-$sock}
    sudo python3 -c 'import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(float(sys.argv[3]))
s.connect(sys.argv[2])
s.sendall((sys.argv[1] + "\n").encode())
print(s.makefile().readline().strip())' "$1" "$sock" "${2:-60}"
}

# Admission must precede implicit format/claim/startup effects. The daemon repeats it at
# startup; this read-only check does not certify hardware rate or permit virtual lab hosts.
clock_preflight() {
    local io_bin="$HOME/flaminio/dataplane/target/release/flaminio-io"
    [[ ${FLAMINIO_SCRIPT_TEST:-0} != 1 ]] || io_bin=${IO_BIN:-$io_bin}
    [[ -x $io_bin ]] || die "flaminio-io not built; run make deploy"
    sudo "$io_bin" clock-check --clock-contract "${CLOCK_CONTRACT:-/etc/flaminio/clock-contract.json}" --json ||
        die "clock profile refused; qualified physical host and operator clock contract required"
}

attach_challenge() {
    local answer nonce
    answer=$(control attach-challenge) || die "cannot obtain healthy daemon attach challenge"
    [[ $answer =~ ^ok\ ([0-9a-f]{64})$ ]] || die "healthy handoff-capable daemon required"
    nonce=${BASH_REMATCH[1]}
    [[ $nonce != "$(printf '%064d' 0)" ]] || die "invalid all-zero attach challenge"
    echo "$nonce"
}

# Authorized target start, as this node's agent. Keep the issued pair across all socket and
# busy retries; a new epoch retires an older lifetime through normal metadata reconciliation.
# Print only the validated device path. Never adopt status or detach on credential loss.
attach_target() {
    : "${VOLUME:?}" "${NODE:?}" "${META:?}"
    local id='^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$'
    [[ $VOLUME =~ $id && $NODE =~ $id ]] || die "invalid volume or node id"
    local nonce out pair token handoff answer path remaining start=$SECONDS
    local meta_bin=/usr/local/bin/flaminio-meta p=/etc/flaminio/pki
    [[ ${FLAMINIO_SCRIPT_TEST:-0} != 1 ]] || meta_bin=${META_BIN:-$meta_bin}
    nonce=$(attach_challenge) || return
    remaining=$((60 - SECONDS + start))
    ((remaining > 1)) || die "attach challenge exhausted the caller wait"
    out=$(sudo timeout "$remaining" "$meta_bin" --server "${META%%,*}" --cert "$p/agent.crt" --key "$p/agent.key" --ca "$p/ca.crt" \
        --target-nonce "$nonce" attach "$VOLUME" "$NODE" only-if-target) || die "authorized target handoff failed"
    pair=$(python3 -c 'import base64, hashlib, json, sys
try:
    r = json.load(sys.stdin)
    t, h = r["token"], r["targetHandoff"]
    if not isinstance(t, str) or not isinstance(h, str) or not 0 < len(t) <= 4096:
        raise ValueError()
    token, handoff = base64.b64decode(t, validate=True), base64.b64decode(h, validate=True)
    if not token or len(handoff) != 134 or handoff[:6] != b"FLCH\x01\x01" or handoff[6:38] != bytes.fromhex(sys.argv[1]) or handoff[38:70] != hashlib.sha256(token).digest():
        raise ValueError()
    print(t, base64.urlsafe_b64encode(handoff).rstrip(b"=").decode())
except (ValueError, KeyError, TypeError):
    sys.exit("metadata server did not return a bounded process-bound target handoff")' "$nonce" <<<"$out") ||
        die "target handoff response refused"
    read -r token handoff <<<"$pair"
    while true; do
        remaining=$((60 - SECONDS + start))
        ((remaining > 1)) || die "attach wait ended; credentials retained only for this invocation; quiesce before operator recovery if ambiguous"
        answer=$(control "attach $VOLUME $token $handoff $(((remaining - 1) * 1000))" "$remaining") || answer="error: socket attempt failed"
        if [[ $answer =~ ^ok\ (/dev/ublkb[0-9]+)$ ]]; then
            path=${BASH_REMATCH[1]}
            echo "$path"
            return
        fi
        case $answer in
            *"has an existing device lifetime; retry this handoff only after that device stops"* | \
            "error: already attaching" | "error: existing target lifetime is no longer live" | "error: no answer within "* | \
            "error: the node dropped the attach without an answer (see its log)" | "error: socket attempt failed") sleep 0.1 ;;
            *) die "attach $VOLUME refused: $answer" ;;
        esac
    done
}

export PATH="$HOME/.cargo/bin:$PATH"
