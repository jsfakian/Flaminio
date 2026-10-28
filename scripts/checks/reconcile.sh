#!/usr/bin/env bash
# Lab check for follow-ups §2: restart every replica of VOL, attach it on NODE, and time the
# first read and a read of one block in every chunk. Fails if the first read waits longer than
# LIMIT seconds. Takes VOL over: it is detached everywhere first and left attached on NODE.
#   VOL=vol1 NODE=fl-a LIMIT=5 scripts/checks/reconcile.sh
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# Helpers of lab.sh, which runs commands when sourced.
# shellcheck source=/dev/null
source <(sed -n '/^metactl()/,/^}/p;/^etcd_endpoints()/,/^}/p;/^meta_endpoints()/,/^}/p;/^node_env()/,/^}/p' "$SCRIPTS/lab.sh")
VOL=${VOL:-vol1}
NODE=${NODE:-$VM_A}
LIMIT=${LIMIT:-5}

info=$(metactl "$VM_A" "$VM_A" get "$VOL")
mapfile -t replicas < <(jq -r '[.volume.chunks[].replicas[]] | unique | .[]' <<<"$info")
chunks=$(jq '.volume.chunks | length' <<<"$info")
chunk_bytes=$(jq -r '.volume.chunkBytes' <<<"$info")
log "$VOL: $chunks chunks on ${replicas[*]}; restarting every replica, then attaching on $NODE"
for vm in $(vms_of overlay); do
    [[ $vm == "$VM_W" ]] && continue
    ENV_VARS="VOLUME=$VOL" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" detach 2>/dev/null || true
done
for vm in "${replicas[@]}"; do run_on_vm "$vm" "$SCRIPTS/guest/node.sh" down; done
for vm in "${replicas[@]}"; do ENV_VARS=$(node_env "$vm") run_on_vm "$vm" "$SCRIPTS/guest/node.sh" up; done

# Timed on NODE itself, so the SSH round trips do not count.
vssh "$NODE" "VOLUME=$VOL NODE=$NODE META=$(vmvar "$NODE" WG):$META_PORT CHUNKS=$chunks CHUNK_BYTES=$chunk_bytes bash -s" <<'EOF' | tee /dev/stderr | awk -v limit="$LIMIT" '
    /^first read/ { first = $(NF - 1) }
    END { if (first == "" || first > limit) { print "first read over the limit of " limit " s"; exit 1 } }'
set -euo pipefail
p=/etc/flaminio/pki
meta() { sudo /usr/local/bin/flaminio-meta --server "$META" --cert $p/admin.crt --key $p/admin.key --ca $p/ca.crt "$@"; }
control() {
    sudo python3 -c 'import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(600)
s.connect("/run/flaminio/node.sock")
s.sendall((sys.argv[1] + "\n").encode())
print(s.makefile().readline().strip())' "$1"
}
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.3f", b - a }'; }
read_block() { sudo timeout 300 dd if="$1" of=/dev/null bs=4096 count=1 skip="$2" iflag=direct status=none; }

t0=$(now)
tok=$(meta attach "$VOLUME" "$NODE" | jq -r .token)
answer=$(control "attach $VOLUME $tok")
[[ $answer == ok* ]] || { echo "attach failed: $answer"; exit 1; }
path=${answer#ok }
for _ in $(seq 100); do [[ -b $path ]] && break; sleep 0.1; done
echo "device $path ready: $(since "$t0") s"
read_block "$path" 0
echo "first read (block 0): $(since "$t0") s"
for ((c = 1; c < CHUNKS; c++)); do read_block "$path" $((c * CHUNK_BYTES / 4096)); done
echo "one block of every chunk: $(since "$t0") s"
EOF
log "OK: the first read after restarting every replica of $VOL waited under $LIMIT s"
