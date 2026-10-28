#!/usr/bin/env bash
# Lab check for follow-ups §2: restart every replica of VOL, attach it on NODE, and time the
# first read and a read of one block in every chunk. Fails if the first read waits longer than
# LIMIT seconds. Takes VOL over: it is detached everywhere first and left attached on NODE.
# Also (docs/5): reads across a chunk boundary into a buffer aligned only to 4 bytes, and fails
# if NODE's node logged a failed local read or a request timeout, or, with zero copy, never
# copied a read through its staging area. NODE must hold a replica of VOL.
#   ZERO_COPY=1 VOL=vol1 NODE=fl-a LIMIT=5 scripts/checks/reconcile.sh
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
[[ " ${replicas[*]} " == *" $NODE "* ]] || die "$NODE holds no replica of $VOL: no read would be local"
# The chunks after the first with a replica on NODE, for the misaligned read's probe.
local_chunks=$(jq -r --arg n "$NODE" \
    '[.volume.chunks | to_entries[] | select(.key >= 1 and (.value.replicas | index($n))) | .key] | join(" ")' \
    <<<"$info")
log "$VOL: $chunks chunks on ${replicas[*]}; restarting every replica, then attaching on $NODE"
for vm in $(vms_of overlay); do
    [[ $vm == "$VM_W" ]] && continue
    ENV_VARS="VOLUME=$VOL" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" detach 2>/dev/null || true
done
for vm in "${replicas[@]}"; do run_on_vm "$vm" "$SCRIPTS/guest/node.sh" down; done
for vm in "${replicas[@]}"; do ENV_VARS=$(node_env "$vm") run_on_vm "$vm" "$SCRIPTS/guest/node.sh" up; done

# Timed on NODE itself, so the SSH round trips do not count.
vssh "$NODE" "VOLUME=$VOL NODE=$NODE META=$(vmvar "$NODE" WG):$META_PORT CHUNKS=$chunks CHUNK_BYTES=$chunk_bytes LOCAL_CHUNKS='$local_chunks' bash -s" <<'EOF' | tee /dev/stderr | awk -v limit="$LIMIT" '
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

# The node's log from here on: its journal cursor now.
cursor=$(sudo journalctl -u flaminio-node -n 0 --show-cursor -o cat | sed -n 's/^-- cursor: //p')
[[ -n $cursor ]] || { echo "no journal cursor for flaminio-node"; exit 1; }
t0=$(now)
tok=$(meta attach "$VOLUME" "$NODE" | jq -r .token)
answer=$(control "attach $VOLUME $tok")
[[ $answer == ok* ]] || { echo "attach failed: $answer"; exit 1; }
path=${answer#ok }
for _ in $(seq 100); do [[ -b $path ]] && break; sleep 0.1; done
echo "device $path ready: $(since "$t0") s"
read_block "$path" 0
echo "first read (block 0): $(since "$t0") s"
# A direct read into a buffer 4 bytes past a page boundary, which the ublk device accepts and
# the disk under the store does not (docs/5). It reads the two blocks around the start of a
# local chunk with data, so the plan has device reads, the second piece at a non-zero offset;
# it must return what a page-aligned read does.
# shellcheck disable=SC2086 # one argument per chunk
sudo python3 - "$path" "$CHUNK_BYTES" $LOCAL_CHUNKS <<'PY'
import mmap, os, sys
path, chunk, block = sys.argv[1], int(sys.argv[2]), 4096
fd = os.open(path, os.O_RDONLY | os.O_DIRECT)
mem = mmap.mmap(-1, 4 * block)
def read(at, shift, n):
    buf = memoryview(mem)[shift:shift + n]
    got = os.preadv(fd, [buf], at)
    if got != n:
        sys.exit(f"probe: read {got} of {n} bytes at {at}")
    return bytes(buf)
for c in map(int, sys.argv[3:]):
    if any(read(c * chunk, 0, block)):
        break
else:
    sys.exit("probe: no chunk after the first with a replica here has data in its first block")
at = c * chunk - block
if read(at, 4, 2 * block) != read(at, 0, 2 * block):
    sys.exit(f"probe: a misaligned read across chunks {c - 1} and {c} returned other data")
print(f"misaligned read across chunks {c - 1} and {c}: the same data")
PY
for ((c = 1; c < CHUNKS; c++)); do read_block "$path" $((c * CHUNK_BYTES / 4096)); done
echo "one block of every chunk: $(since "$t0") s"
# What the node logged since the attach, once journald has taken in everything logged so far.
sudo journalctl --sync
journal=$(sudo journalctl -u flaminio-node --after-cursor "$cursor" -o cat)
if failed=$(grep -E 'local read(:| failed)|on a local read|a request timed out' <<<"$journal"); then
    echo "failed reads logged since the attach:"
    echo "$failed"
    exit 1
fi
# With zero copy, the probe's read must have been copied through the staging area, if the
# store's disk refuses a buffer aligned to 4 bytes: its DMA alignment is above 3 (512 for
# virtio-blk; NVMe takes 4, and then nothing needs the copy).
# A partition's limits are its disk's; a store that is not a block device reads as 0.
store=$(systemctl show -p ExecStart --value flaminio-node | grep -o -- '--device[ =][^ ;]*' |
    cut -c10- || true)
dma=0
if [[ -n $store ]]; then
    dev=/sys/class/block/$(basename "$(readlink -f "$store")")
    [[ -e $dev/partition ]] && dev=$(readlink -f "$dev/..")
    dma=$(cat "$dev/queue/dma_alignment" 2>/dev/null || echo 0)
else
    echo "note: the node's --device was not found; not checking for a staged copy"
fi
if grep -q 'zero copy: local reads go straight into the requests' <<<"$journal" && ((dma > 3)) &&
    ! grep -q 'copied through the staging area' <<<"$journal"; then
    echo "zero copy, but no read was copied through the staging area ($store: DMA alignment $dma)"
    exit 1
fi
echo "no failed local read logged"
EOF
log "OK: the first read after restarting every replica of $VOL waited under $LIMIT s, and no local read failed"
