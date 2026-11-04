#!/usr/bin/env bash
# #10: 1M QD8 O_DSYNC, Flaminio (RF 1, zero copy, fresh store) and raw across fl-b's disk,
# interleaved, so both see the same drive state. Run from the repo root.
set -euo pipefail
LAB=scripts/lab.sh
export SSH_IDENTITY=$HOME/.ssh/id_rsa_mac
META='p=/etc/flaminio/pki; sudo /usr/local/bin/flaminio-meta --server 10.77.0.1:7400 --cert $p/admin.crt --key $p/admin.key --ca $p/ca.crt'
FIO='--rw=write --bs=1m --iodepth=8 --time_based --runtime=60 --sync=dsync --ioengine=io_uring --direct=1 --output-format=json'
# node.sh as run_on_vm runs it: common.sh prepended, through a temporary file.
node_sh() {
    local vm=$1 env=$2 cmd=$3
    cat scripts/guest/common.sh scripts/guest/node.sh |
        $LAB exec "$vm" "env $env bash -c 'f=\$(mktemp); cat >\"\$f\"; bash \"\$f\" $cmd; rc=\$?; rm -f \"\$f\"; exit \$rc'"
}
report() { jq -r '"\(.jobs[0].write.bw_bytes/1048576|floor) MiB/s, p50 \(.jobs[0].write.clat_ns.percentile."50.000000"/1e6) ms, p99 \(.jobs[0].write.clat_ns.percentile."99.000000"/1e6) ms"'; }

flaminio() {
    local vol=$1 tok
    $LAB node down b >/dev/null 2>&1 || true
    ZERO_COPY=1 FORMAT=1 $LAB node up b >/dev/null
    sleep 8 # heartbeat in, as write_volume waits for
    $LAB exec a "$META create $vol 16G 1 1 256M fl-b" >/dev/null
    tok=$($LAB exec a "$META attach $vol fl-b" | jq -r .token)
    node_sh b "VOLUME=$vol TOKEN=$tok" attach >/dev/null
    $LAB exec b "sudo /usr/bin/fio --name=w --filename=/dev/ublkb1 --size=16G $FIO --output=/tmp/f.json >/dev/null; cat /tmp/f.json" | report
    node_sh b "VOLUME=$vol" detach >/dev/null || true
}

raw() {
    $LAB node down b >/dev/null 2>&1 || true
    $LAB exec b "sudo /usr/bin/fio --name=w --filename=/dev/vdb --offset=16G --size=84G $FIO --output=/tmp/r.json >/dev/null; cat /tmp/r.json" | report
}

echo "flaminio 1: $(flaminio wprof1)"
echo "raw 1:      $(raw)"
echo "flaminio 2: $(flaminio wprof2)"
echo "raw 2:      $(raw)"
