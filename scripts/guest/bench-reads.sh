#!/usr/bin/env bash
# Read benchmark steps, one per call, so that the lab script can clear the host's caches
# between them (docs/performance-plan.md, section 2). Usage:
#   bench-reads.sh fill DEV SPAN         write all of SPAN once, with fio's checksum in every
#                                        1 MiB block (so `verify` can check it later)
#   bench-reads.sh warm DEV SPAN         read all of SPAN once, unmeasured
#   bench-reads.sh job DEV OFFSET SPAN NAME RW BS QD RUNTIME OUT
#                                        one measured job; prints a TSV row: name, IOPS, MiB/s,
#                                        p50 and p99 (us), fio's CPU and the ublk queue thread's
#                                        CPU (%, sampled mid-run; - for the raw disk)
#   bench-reads.sh verify DEV SPAN       fio reads all of SPAN and checks every block's checksum
#   bench-reads.sh disk                  the data disk's path
fio_common=(--ioengine=io_uring --direct=1 --invalidate=1 --group_reporting --output-format=json)
fill_opts=(--rw=write --bs=1m --iodepth=32 --verify=crc32c --randseed=7411)

drop_caches() { sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null; }

case "${1:-}" in
    disk) data_disk ;;
    fill)
        drop_caches
        sudo fio --name=fill --filename="$2" --size="$3" "${fill_opts[@]}" --do_verify=0 \
            "${fio_common[@]}" --output=/tmp/bench-fill.json >/dev/null
        jq -r '.jobs[0].write | "  filled \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s"' /tmp/bench-fill.json
        ;;
    warm)
        drop_caches
        sudo fio --name=warm --filename="$2" --size="$3" --rw=read --bs=1m --iodepth=8 \
            "${fio_common[@]}" --output=/tmp/bench-warm.json >/dev/null
        ;;
    job)
        dev=$2 off=$3 span=$4 name=$5 rw=$6 bs=$7 qd=$8 runtime=$9 out=${10}
        drop_caches
        sudo fio --name="$name" --filename="$dev" --offset="$off" --size="$span" --rw="$rw" --bs="$bs" \
            --iodepth="$qd" --numjobs=1 --time_based --runtime="$runtime" --ramp_time=3 \
            "${fio_common[@]}" --output="$out" >/dev/null &
        fio_pid=$!
        qcpu=-
        if [[ $dev == /dev/ublkb* ]]; then
            sleep $((runtime / 2 + 3))
            pid=$(pgrep -f 'flaminio-io node' | head -1)
            # The busiest thread of the data plane: the ublk queue thread.
            qcpu=$(top -H -b -n 2 -d 3 -p "$pid" | awk '/^ *PID/ {n++} n == 2 && $1 ~ /^[0-9]+$/ {print $9; exit}')
        fi
        wait "$fio_pid"
        jq -r --arg n "$name" --arg q "$qcpu" '.jobs[0] | .read as $r |
            [$n, ($r.iops|floor), (($r.bw_bytes/1048576)*10|floor/10),
             (($r.clat_ns.percentile["50.000000"] // 0)/1000|floor),
             (($r.clat_ns.percentile["99.000000"] // 0)/1000|floor),
             ((.usr_cpu + .sys_cpu)|floor), $q] | @tsv' "$out"
        ;;
    verify)
        drop_caches
        if sudo fio --name=fill --filename="$2" --size="$3" "${fill_opts[@]}" --verify_only --verify_fatal=1 \
            "${fio_common[@]}" --output=/tmp/bench-verify.json >/dev/null; then
            jq -r '.jobs[0] | "  verified \(.read.io_bytes/1073741824|floor) GiB, errors \(.error)"' /tmp/bench-verify.json
        else
            echo "  VERIFICATION FAILED"
            jq -r '.jobs[0].error' /tmp/bench-verify.json 2>/dev/null
            exit 1
        fi
        ;;
    *) die "usage: bench-reads.sh fill|warm|job|verify|disk ..." ;;
esac
