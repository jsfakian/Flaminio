#!/usr/bin/env bash
# Baseline fio numbers for the raw data disk. DESTROYS everything on the data disk.
# Usage: fio-raw.sh RUNTIME_SECONDS
runtime=${1:-30}
disk=$(data_disk)
out="$HOME/results/raw-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"

# name rw bs iodepth numjobs
jobs=(
    "randread-4k-qd1   randread  4k 1  1"
    "randwrite-4k-qd1  randwrite 4k 1  1"
    "randread-4k-qd32  randread  4k 32 1"
    "randwrite-4k-qd32 randwrite 4k 32 1"
    "read-1m-qd8       read      1m 8  1"
    "write-1m-qd8      write     1m 8  1"
)

# A fresh data disk is a preallocated file on the host: never-written blocks read as zeros
# without touching the physical disk, which would inflate read numbers. Fill it once.
marker="$HOME/.flaminio-preconditioned-$(basename "$disk")"
if [[ ! -f $marker ]]; then
    log "preconditioning: writing all of $disk once (one-time, takes a few minutes)"
    sudo fio --name=precondition --filename="$disk" --rw=write --bs=1m --iodepth=32 \
        --ioengine=io_uring --direct=1 --output-format=json --output="$out/precondition.json" >/dev/null
    jq -r '.jobs[0].write | "  wrote \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s"' "$out/precondition.json" >&2
    touch "$marker"
fi

log "raw baseline on $disk, ${runtime}s per job, results in $out"
printf '%-18s %10s %10s %10s %10s\n' job IOPS MiB/s "p50(us)" "p99(us)"
for spec in "${jobs[@]}"; do
    read -r name rw bs qd nj <<<"$spec"
    sudo fio --name="$name" --filename="$disk" --rw="$rw" --bs="$bs" --iodepth="$qd" --numjobs="$nj" \
        --ioengine=io_uring --direct=1 --time_based --runtime="$runtime" --ramp_time=3 \
        --group_reporting --output-format=json --output="$out/$name.json" >/dev/null
    jq -r --arg n "$name" '.jobs[0] | (if .read.io_bytes > 0 then .read else .write end) as $d |
        [$n, ($d.iops|floor), (($d.bw_bytes/1048576)*10|floor/10),
         (($d.clat_ns.percentile["50.000000"] // 0)/1000|floor),
         (($d.clat_ns.percentile["99.000000"] // 0)/1000|floor)] | @tsv' "$out/$name.json" |
        awk -F'\t' '{printf "%-18s %10s %10s %10s %10s\n", $1, $2, $3, $4, $5}'
done
echo "$out"
