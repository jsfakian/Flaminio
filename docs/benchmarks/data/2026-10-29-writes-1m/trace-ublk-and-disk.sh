# Traced 1M QD8 O_DSYNC writes on /dev/ublkb1: the ublk device's requests (U) and the data disk's
# (D), issue and completion; and the queue thread's CPU mid-run. wt2.sh OUT
out=$1
V=$(( $(cut -d: -f1 /sys/class/block/vdb/dev) << 20 | $(cut -d: -f2 /sys/class/block/vdb/dev) ))
U=$(( $(cut -d: -f1 /sys/class/block/ublkb1/dev) << 20 | $(cut -d: -f2 /sys/class/block/ublkb1/dev) ))
sudo rm -f "$out" /tmp/wt.json
sudo sh -c "sync; echo 3 > /proc/sys/vm/drop_caches"
( sleep 30; pid=$(pgrep -f 'flaminio-io node' | head -1); top -H -b -n 2 -d 5 -p "$pid" | awk '/^ *PID/ {n++} n == 2 && $1 ~ /^[0-9]+$/ && NF > 10 {print "top:", $9, $12}' | head -4 > /tmp/wt-top.txt ) &
sudo timeout 300 bpftrace -B full -e "tracepoint:block:block_rq_issue /(args.dev == $V || args.dev == $U) && strncmp(args.rwbs, \"R\", 1) != 0/ { printf(\"I %lld %s %s %lld %d\n\", nsecs, args.dev == $U ? \"U\" : \"D\", args.rwbs, args.sector, args.bytes); } tracepoint:block:block_rq_complete /(args.dev == $V || args.dev == $U) && strncmp(args.rwbs, \"R\", 1) != 0/ { printf(\"C %lld %s %s %lld %d\n\", nsecs, args.dev == $U ? \"U\" : \"D\", args.rwbs, args.sector, args.nr_sector); }" \
  -c "/usr/bin/fio --name=w --filename=/dev/ublkb1 --size=16G --rw=write --bs=1m --iodepth=8 --time_based --runtime=60 --sync=dsync --ioengine=io_uring --direct=1 --output-format=json --output=/tmp/wt.json" >"$out" 2>&1
wait
jq -r ".jobs[0].write | \"\(.bw_bytes/1048576|floor) MiB/s, clat mean \(.clat_ns.mean/1000|floor) us\"" /tmp/wt.json
cat /tmp/wt-top.txt
