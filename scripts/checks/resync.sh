#!/usr/bin/env bash
# Lab check for follow-ups §3: a 16 GiB RF1 volume with 64 MiB written at the start of each of
# its 64 chunks (4 GiB) on FROM, every chunk moved to TO, where it has no replica: a resync into
# an empty replica. Fails if TO receives more than 4.5 GiB over the overlay, or if the data does
# not read back. FROM must have no volume attached; the check volume is deleted at the end.
#   FROM=fl-a TO=fl-b scripts/checks/resync.sh
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# Helpers of lab.sh, which runs commands when sourced.
# shellcheck source=/dev/null
source <(sed -n '/^metactl()/,/^}/p' "$SCRIPTS/lab.sh")
FROM=${FROM:-$VM_A}
TO=${TO:-$VM_B}
VOL=resync-$(date +%Y%m%d%H%M%S)
CHUNKS=64
LIMIT_MIB=4608

rx() { vssh "$TO" 'cat /sys/class/net/wg0/statistics/rx_bytes'; }
control() { # VM COMMAND
    vssh "$1" "sudo python3 -c 'import socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(600); s.connect(\"/run/flaminio/node.sock\")
s.sendall((sys.argv[1] + \"\\n\").encode()); print(s.makefile().readline().strip())' '$2'"
}
cleanup() {
    control "$FROM" "detach $VOL" >/dev/null 2>&1 || true
    metactl "$VM_A" "$VM_A" delete "$VOL" >/dev/null 2>&1 || true
    # A move leaves the source's replica behind (follow-ups §13): delete it on both nodes.
    for vm in "$FROM" "$TO"; do control "$vm" "delete $VOL" >/dev/null 2>&1 || true; done
}
trap cleanup EXIT

metactl "$VM_A" "$VM_A" create "$VOL" 16G 1 1 256M "$FROM" >/dev/null
tok=$(metactl "$VM_A" "$VM_A" attach "$VOL" "$FROM" | jq -r .token)
dev=$(control "$FROM" "attach $VOL $tok")
[[ $dev == ok* ]] || die "attach $VOL on $FROM: $dev"
dev=${dev#ok }
log "$VOL attached on $FROM at $dev; writing 64 MiB at the start of each chunk"
vssh "$FROM" "set -e; for _ in \$(seq 100); do [[ -b $dev ]] && break; sleep 0.1; done
    head -c 64M /dev/urandom > /tmp/$VOL; md5sum < /tmp/$VOL | cut -d' ' -f1 > /tmp/$VOL.md5
    for c in \$(seq 0 $((CHUNKS - 1))); do
        sudo dd if=/tmp/$VOL of=$dev bs=1M seek=\$((c * 256)) oflag=direct conv=notrunc status=none
    done"

rx0=$(rx)
start=$SECONDS
for c in $(seq 0 $((CHUNKS - 1))); do metactl "$VM_A" "$VM_A" move "$VOL" "$c" "$FROM" "$TO" >/dev/null; done
until [[ $(metactl "$VM_A" "$VM_A" get "$VOL" | jq --arg to "$TO" '[.volume.chunks[] | select(.replicas == [$to])] | length') -eq $CHUNKS ]]; do
    ((SECONDS - start < 1800)) || die "the moves did not finish within 1800 s"
    sleep 5
done
moved=$((($(rx) - rx0) / 1048576))
log "moves done in $((SECONDS - start)) s: $TO received $moved MiB over the overlay for 4096 MiB written"

log "reading back through $FROM, served by $TO now"
vssh "$FROM" "set -e; want=\$(cat /tmp/$VOL.md5); zero=\$(head -c 64M /dev/zero | md5sum | cut -d' ' -f1)
    for c in \$(seq 0 $((CHUNKS - 1))); do
        [[ \$(sudo dd if=$dev bs=1M skip=\$((c * 256)) count=64 iflag=direct status=none | md5sum | cut -d' ' -f1) == \$want ]] ||
            { echo \"chunk \$c: the written range differs\"; exit 1; }
    done
    for c in 0 $((CHUNKS / 2)) $((CHUNKS - 1)); do
        [[ \$(sudo dd if=$dev bs=1M skip=\$((c * 256 + 128)) count=64 iflag=direct status=none | md5sum | cut -d' ' -f1) == \$zero ]] ||
            { echo \"chunk \$c: an unwritten range is not zeros\"; exit 1; }
    done
    rm -f /tmp/$VOL /tmp/$VOL.md5"
((moved <= LIMIT_MIB)) || die "$TO received $moved MiB, more than $LIMIT_MIB MiB"
log "OK: the resync moved $moved MiB for 4 GiB written, and every range read back"
