#!/usr/bin/env bash
# The ublk frontend in a lab VM (MVP step 4). Usage: ublk.sh up|down|status|smoke|fio [RUNTIME]
#   up      start flaminio-io as the systemd service "flaminio-io" (FORMAT=1 reformats the disk)
#   down    stop it and remove the ublk device
#   smoke   ext4 on the device, write files, restart the daemon, verify every file
#   fio     read benchmark on the device (fills the first 32 GiB with writes first, every time)
bin="$HOME/flaminio/dataplane/target/release/flaminio-io"
id=0
dev=/dev/ublkb$id
size=${VOLUME_SIZE:-64G}
mnt=/mnt/flaminio

up() {
    [[ -x $bin ]] || die "flaminio-io not built; run make deploy"
    if systemctl is-active --quiet flaminio-io; then
        log "flaminio-io already running"
        return
    fi
    local format=()
    [[ ${FORMAT:-0} == 1 ]] && format=(--format --force)
    sudo systemctl reset-failed flaminio-io 2>/dev/null || true
    local replica=()
    [[ -n ${REPLICA:-} ]] && replica=(--replica "$REPLICA")
    sudo systemd-run --quiet --unit=flaminio-io --property=Restart=no \
        "$bin" serve --device "$(data_disk)" --size "$size" --id "$id" "${format[@]}" "${replica[@]}"
    for _ in $(seq 1 60); do
        [[ -b $dev ]] && break
        systemctl is-active --quiet flaminio-io || { sudo journalctl -u flaminio-io --no-pager | tail -20 >&2; die "flaminio-io exited"; }
        sleep 1
    done
    [[ -b $dev ]] || die "$dev did not appear"
    log "$(sudo journalctl -u flaminio-io --no-pager -o cat | grep -E 'volume ready|serving' | tail -2 | tr '\n' ' ')"
}

down() {
    mountpoint -q "$mnt" && sudo umount "$mnt"
    if [[ -e /dev/ublkc$id ]]; then
        sudo "$bin" del --id "$id" >/dev/null || true
    fi
    for _ in $(seq 1 30); do
        systemctl is-active --quiet flaminio-io || break
        sleep 1
    done
    systemctl is-active --quiet flaminio-io && sudo systemctl stop flaminio-io
    log "flaminio-io stopped"
}

status() {
    systemctl is-active flaminio-io || true
    [[ -b $dev ]] && lsblk -o NAME,SIZE,LOG-SEC,PHY-SEC,DISC-GRAN,ROTA "$dev"
    sudo journalctl -u flaminio-io --no-pager -o cat | tail -5
}

smoke() {
    up
    log "mkfs.ext4 on $dev"
    sudo mkfs.ext4 -q -F -E lazy_itable_init=0,lazy_journal_init=0 "$dev"
    sudo mkdir -p "$mnt"
    sudo mount "$dev" "$mnt"
    log "writing 64 files (1 GiB total) with checksums"
    for i in $(seq 1 64); do
        sudo dd if=/dev/urandom of="$mnt/f$i" bs=1M count=16 status=none
    done
    (cd "$mnt" && sudo sha256sum f* | sudo tee SHA256SUMS >/dev/null)
    sudo umount "$mnt"
    log "restarting flaminio-io (the store recovers on open)"
    down
    up
    sudo mount "$dev" "$mnt"
    if (cd "$mnt" && sudo sha256sum --quiet -c SHA256SUMS); then
        log "all 64 files intact after restart"
    else
        die "checksum mismatch after restart"
    fi
    sudo umount "$mnt"
    if sudo e2fsck -f -n "$dev" >/dev/null 2>&1; then
        log "e2fsck: clean"
    else
        die "e2fsck found errors"
    fi
}

bench() {
    local runtime=${1:-30} out
    up
    out="$HOME/results/ublk-$(date -u +%Y%m%dT%H%M%SZ)"
    mkdir -p "$out"
    # Always fill: anything may have discarded the volume since the last run (mkfs.ext4 does, by
    # default), and reads of discarded ranges return zeros without touching the disk.
    log "filling the first 32 GiB of $dev (setup, not measured)"
    sudo fio --name=fill --filename="$dev" --rw=write --bs=1m --iodepth=32 --size=32G \
        --ioengine=io_uring --direct=1 --output-format=json --output="$out/fill.json" >/dev/null
    jq -r '.jobs[0].write | "  filled at \(.bw_bytes/1048576|floor) MiB/s"' "$out/fill.json" >&2
    local jobs=(
        "randread-4k-qd1   randread 4k 1  -"
        "randread-4k-qd32  randread 4k 32 -"
        "read-1m-qd8       read     1m 8  -"
    )
    printf '%-18s %10s %10s %10s %10s\n' job IOPS MiB/s "p50(us)" "p99(us)"
    for spec in "${jobs[@]}"; do
        read -r name rw bs qd _ <<<"$spec"
        sudo fio --name="$name" --filename="$dev" --rw="$rw" --bs="$bs" --iodepth="$qd" --size=32G \
            --ioengine=io_uring --direct=1 --time_based --runtime="$runtime" --ramp_time=3 \
            --output-format=json --output="$out/$name.json" >/dev/null
        jq -r --arg n "$name" '.jobs[0].read as $d | [$n, ($d.iops|floor), (($d.bw_bytes/1048576)*10|floor/10),
            (($d.clat_ns.percentile["50.000000"] // 0)/1000|floor), (($d.clat_ns.percentile["99.000000"] // 0)/1000|floor)] | @tsv' \
            "$out/$name.json" | awk -F'\t' '{printf "%-18s %10s %10s %10s %10s\n", $1, $2, $3, $4, $5}'
    done
    sudo chown -R "$(id -u):$(id -g)" "$out"
    echo "$out"
}

case "${1:-status}" in
    up) up ;;
    down) down ;;
    status) status ;;
    smoke) smoke ;;
    fio) bench "${2:-30}" ;;
    *) die "usage: ublk.sh up|down|status|smoke|fio [RUNTIME]" ;;
esac
