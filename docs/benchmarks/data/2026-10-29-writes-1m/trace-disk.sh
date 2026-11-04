# Traced 1M QD8 O_DSYNC writes: every write and flush to the data disk. wt.sh DEV OFFSET SIZE OUT
dev=$1 off=$2 size=$3 out=$4
V=$(( $(cut -d: -f1 /sys/class/block/vdb/dev) << 20 | $(cut -d: -f2 /sys/class/block/vdb/dev) ))
sudo rm -f "$out" /tmp/wt.json
sudo sh -c "sync; echo 3 > /proc/sys/vm/drop_caches"
sudo timeout 300 bpftrace -B full -e "tracepoint:block:block_rq_issue /args.dev == $V && strncmp(args.rwbs, \"R\", 1) != 0/ { printf(\"I %lld %s %lld %d\n\", nsecs, args.rwbs, args.sector, args.bytes); } tracepoint:block:block_rq_complete /args.dev == $V && strncmp(args.rwbs, \"R\", 1) != 0/ { printf(\"C %lld %s %lld %d\n\", nsecs, args.rwbs, args.sector, args.nr_sector); }" \
  -c "/usr/bin/fio --name=w --filename=$dev --offset=$off --size=$size --rw=write --bs=1m --iodepth=8 --time_based --runtime=60 --sync=dsync --ioengine=io_uring --direct=1 --output-format=json --output=/tmp/wt.json" >"$out" 2>&1
jq -r ".jobs[0].write | \"$dev: \(.bw_bytes/1048576|floor) MiB/s, clat mean \(.clat_ns.mean/1000|floor) us\"" /tmp/wt.json
