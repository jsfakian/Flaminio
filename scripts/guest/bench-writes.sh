#!/usr/bin/env bash
# Write benchmark steps in a lab VM, one per call (docs/performance-plan.md, W1–W2). Usage:
#   bench-writes.sh discard              discard the whole data disk (nothing may hold it)
#   bench-writes.sh written              sectors written to the data disk since boot
#   bench-writes.sh whole                write the whole data disk once (nothing may hold it)
#   bench-writes.sh precondition DEV SPAN   sequential 128 KiB writes over SPAN, twice
#   bench-writes.sh integrity-write DEV SPAN   SPAN written with fio's block checksums, 1M
#                                        sequential, then 2 GiB of 4K random writes over it;
#                                        each read back and checked
#   bench-writes.sh integrity-read DEV SPAN    the random writes read back and checked again
#   bench-writes.sh job DEV SPAN NAME RW BS QD RUNTIME OUT [MIX]
#                                        one measured round, writes durable when acknowledged
#                                        (O_DSYNC); prints a TSV row: name, write IOPS, write
#                                        MiB/s, write p50 and p99 (us), read IOPS
# Both sides use O_DSYNC: Flaminio acknowledges a write once it is durable on every ISR member,
# and the raw disk's writes are durable only with it (the virtio disk has a write cache).
drop_caches() { sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null; }
check=(--verify=crc32c --verify_fatal=1 --ioengine=io_uring --direct=1 --output-format=json)
random=(--rw=randwrite --bs=4k --iodepth=32 --io_size=2G --randseed=7411)
# fio_checked NAME OUT FIO_ARGS...: runs a checked fio job, prints what it verified.
fio_checked() {
    local name=$1 out=$2
    shift 2
    if sudo fio --name="$name" "$@" "${check[@]}" --output="$out" >/dev/null; then
        # A verify-only job counts the writes it skips: report reads alone then.
        jq -r --arg n "$name" --arg ro "${READ_ONLY:-0}" '.jobs[0] |
            (if $ro == "1" then "" else "\(.write.io_bytes/1073741824|floor) GiB written, " end) as $w |
            "  \($n): \($w)\(.read.io_bytes/1073741824|floor) GiB read back and checked, errors \(.error)"' "$out"
    else
        echo "  $name: VERIFICATION FAILED"
        jq -r '.jobs[0].error' "$out" 2>/dev/null
        exit 1
    fi
}

case "${1:-}" in
    discard)
        disk=$(data_disk)
        systemctl is-active --quiet flaminio-node && die "flaminio-node holds $disk"
        [[ -z $(lsblk -no MOUNTPOINTS "$disk" | tr -d '[:space:]') ]] || die "$disk is mounted"
        sudo blkdiscard -f "$disk"
        log "discarded all of $disk"
        ;;
    written)
        disk=$(data_disk)
        awk '{print $7}' "/sys/block/$(basename "$disk")/stat"
        ;;
    whole)
        disk=$(data_disk)
        systemctl is-active --quiet flaminio-node && die "flaminio-node holds $disk"
        sudo fio --name=whole --filename="$disk" --rw=write --bs=1m --iodepth=32 --ioengine=io_uring \
            --direct=1 --output-format=json --output=/tmp/bench-whole.json >/dev/null
        jq -r '.jobs[0].write | "  whole disk written: \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s"' /tmp/bench-whole.json
        ;;
    precondition)
        drop_caches
        sudo fio --name=precondition --filename="$2" --size="$3" --rw=write --bs=128k --iodepth=32 \
            --loops=2 --ioengine=io_uring --direct=1 --output-format=json --output=/tmp/bench-pre.json >/dev/null
        jq -r '.jobs[0].write | "  preconditioned: \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s"' /tmp/bench-pre.json
        ;;
    integrity-write)
        drop_caches
        fio_checked sequential /tmp/bench-int-seq.json --filename="$2" --size="$3" --rw=write --bs=1m --iodepth=32 --do_verify=1
        fio_checked random /tmp/bench-int-rand.json --filename="$2" --size="$3" "${random[@]}" --do_verify=1
        ;;
    integrity-read)
        drop_caches
        READ_ONLY=1 fio_checked "random, after a restart" /tmp/bench-int-read.json --filename="$2" --size="$3" "${random[@]}" --verify_only
        ;;
    job)
        dev=$2 span=$3 name=$4 rw=$5 bs=$6 qd=$7 runtime=$8 out=$9 mix=${10:-}
        drop_caches
        extra=()
        [[ -n $mix ]] && extra=(--rwmixread="$mix")
        sudo fio --name="$name" --filename="$dev" --size="$span" --rw="$rw" --bs="$bs" --iodepth="$qd" \
            --numjobs=1 --time_based --runtime="$runtime" --sync=dsync "${extra[@]}" \
            --ioengine=io_uring --direct=1 --group_reporting --output-format=json --output="$out" >/dev/null
        jq -r --arg n "$name" '.jobs[0] | .write as $w |
            [$n, ($w.iops|floor), (($w.bw_bytes/1048576)*10|floor/10),
             (($w.clat_ns.percentile["50.000000"] // 0)/1000|floor),
             (($w.clat_ns.percentile["99.000000"] // 0)/1000|floor), (.read.iops|floor)] | @tsv' "$out"
        ;;
    *) die "usage: bench-writes.sh discard|written|whole|precondition|integrity-write|integrity-read|job ..." ;;
esac
