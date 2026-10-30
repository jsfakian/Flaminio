#!/usr/bin/env bash
# Read benchmark steps, one per call, so that the lab script can clear the host's caches
# between them (docs/performance-plan.md, section 2). Usage:
#   bench-reads.sh fill DEV SPAN         write all of SPAN once, with fio's checksum in every
#                                        1 MiB block (so `verify` can check it later)
#   bench-reads.sh warm DEV SPAN         read all of SPAN once, unmeasured
#   bench-reads.sh range DEV SPAN DISK   read all of SPAN again, tracing the reads it makes from
#                                        DISK; prints "FIRST END TOTAL DISK_BYTES": the 1 GiB
#                                        regions of DISK it took 64 MiB or more from (FIRST to
#                                        END, END excluded; 0 0 if none), the bytes it took from
#                                        DISK, and DISK's size. The regions, one "INDEX BYTES" per
#                                        line, go to /tmp/bench-range-regions.txt (docs/issues/9)
#   bench-reads.sh job DEV OFFSET SPAN NAME RW BS QD RUNTIME OUT [JOBS]
#                                        one measured job; prints a TSV row: name, IOPS, MiB/s,
#                                        p50 and p99 (us), fio's CPU and the ublk queue thread's
#                                        CPU (%, sampled mid-run; - for the raw disk)
#   bench-reads.sh verify DEV SPAN       fio reads all of SPAN and checks every block's checksum
#   bench-reads.sh disk                  the data disk's path
fio_common=(--ioengine=io_uring --direct=1 --invalidate=1 --group_reporting --output-format=json)
fill_opts=(--rw=write --bs=1m --iodepth=32 --verify=crc32c --randseed=7411)

drop_caches() { sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null; }
# fio creates a missing file: a vanished device would silently become a file in /dev.
block_device() { [[ -b $1 ]] || die "$1 is not a block device"; }

case "${1:-}" in
    disk) data_disk ;;
    fill)
        block_device "$2"
        drop_caches
        sudo fio --name=fill --filename="$2" --size="$3" "${fill_opts[@]}" --do_verify=0 \
            "${fio_common[@]}" --output=/tmp/bench-fill.json >/dev/null
        jq -r '.jobs[0].write | "  filled \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s"' /tmp/bench-fill.json
        ;;
    warm)
        block_device "$2"
        drop_caches
        sudo fio --name=warm --filename="$2" --size="$3" --rw=read --bs=1m --iodepth=8 \
            "${fio_common[@]}" --output=/tmp/bench-warm.json >/dev/null
        ;;
    range)
        dev=$2 span=$3 disk=$4
        block_device "$dev"
        block_device "$disk"
        if ! command -v bpftrace >/dev/null; then
            { sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq bpftrace; } \
                >/dev/null 2>&1 || die "bpftrace is missing and could not be installed"
        fi
        fio=$(command -v fio) || die "fio is missing"
        want=$(numfmt --from=iec "$span") || die "the volume's size $span is not a size"
        name=${disk##*/}
        IFS=: read -r major minor <"/sys/class/block/$name/dev"
        disk_bytes=$(($(<"/sys/class/block/$name/size") * 512))
        trace=/tmp/bench-range-trace.txt report=/tmp/bench-range-fio.json
        regions=/tmp/bench-range-regions.txt
        # The fio report is root's, in the sticky /tmp. A fio that fails before writing it must
        # not pass on the last run's.
        sudo rm -f "$trace" "$report" "$regions"
        drop_caches
        # bpftrace attaches, then runs fio (-c: no shell, so no quotes or redirections in it),
        # and exits when fio does, printing the map: bytes read per 1 GiB region of the disk.
        prog="tracepoint:block:block_rq_issue
            /args.dev == $(((major << 20) | minor)) && strncmp(args.rwbs, \"R\", 1) == 0/
            { @r[args.sector >> 21] = sum(args.bytes); }
            END { printf(\"trace-end\\n\"); }"
        # The trace file is this user's, written by this shell (SC2024): only bpftrace needs root.
        # shellcheck disable=SC2024
        if ! sudo timeout 300 bpftrace -B none -e "$prog" \
            -c "$fio --name=range --filename=$dev --size=$span --rw=read --bs=1m --iodepth=8 --ioengine=io_uring --direct=1 --output-format=json --output=$report" \
            >"$trace" 2>&1 || ! grep -qx trace-end "$trace"; then
            tail -n 20 "$trace" >&2
            die "the trace of $dev's read failed"
        fi
        jq -e --argjson want "$want" \
            '.jobs[0].error == 0 and .jobs[0].read.io_bytes >= $want' "$report" >/dev/null 2>&1 ||
            die "the traced read of $dev failed (fio's report: $report)"
        # bpftrace sorts a map by value, and mawk cannot sort: sort -n. %.0f: mawk prints large
        # integers in exponent form.
        awk '/^@r\[[0-9]+\]: [0-9]+$/ { sub(/^@r\[/, ""); sub(/\]:/, ""); printf "%d %.0f\n", $1, $2 }' \
            "$trace" | sort -n >"$regions"
        awk -v disk="$disk_bytes" -v min=$((64 << 20)) '
            { total += $2; if ($2 >= min) { if (first == "") first = $1; last = $1 } }
            END {
                if (first == "") printf "0 0 %.0f %.0f\n", total, disk
                else printf "%d %d %.0f %.0f\n", first, last + 1, total, disk
            }' "$regions"
        ;;
    job)
        dev=$2 off=$3 span=$4 name=$5 rw=$6 bs=$7 qd=$8 runtime=$9 out=${10} jobs=${11:-1}
        block_device "$dev"
        drop_caches
        sudo fio --name="$name" --filename="$dev" --offset="$off" --size="$span" --rw="$rw" --bs="$bs" \
            --iodepth="$qd" --numjobs="$jobs" --time_based --runtime="$runtime" --ramp_time=3 \
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
        block_device "$2"
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
    *) die "usage: bench-reads.sh fill|warm|range|job|verify|disk ..." ;;
esac
