# The hand measurements of docs/benchmarks/2026-10-29-reads-qd1-queues.md, as run on fl-b (root
# shell, volb attached as /dev/ublkb1, the data disk /dev/vdb). Not a script to run as a whole:
# each block is one experiment. Before each fio job: sync; echo 3 > /proc/sys/vm/drop_caches.
# dev_t, as block tracepoints give it: MAJOR << 20 | MINOR (/sys/class/block/<name>/dev).
U=271581184 # ublkb1
V=265289744 # vdb

# QD1 phases (qd1-phases.bt), with the ublk device's scheduler set to none and wbt off by hand:
echo none >/sys/block/ublkb1/queue/scheduler
echo 0 >/sys/block/ublkb1/queue/wbt_lat_usec
bpftrace qd1-phases.bt $U $V &
fio --name=qd1 --filename=/dev/ublkb1 --rw=randread --bs=4k --iodepth=1 --direct=1 \
    --ioengine=libaio --size=16G --runtime=20 --time_based
kill -INT %1
# ...with the node started with --queues 1, then --queues 4.

# Device latency by 8 GiB region (dev-regions.bt), and by issuing CPU (dev-cpu.bt):
bpftrace dev-regions.bt $V &
fio --name=q --filename=/dev/vdb --offset=32G --size=16G --rw=randread --bs=4k --iodepth=1 \
    --direct=1 --ioengine=io_uring --runtime=15 --time_based
kill -INT %1
# ...and with /dev/ublkb1 (offset 0), /dev/vdb at 0 and 64G.

# Where volb's data is: bytes read per 1 GiB region while reading all of it.
bpftrace -e "tracepoint:block:block_rq_issue /args.dev == $V && args.rwbs == \"R\"/
    { @gib[args.sector / 2097152] = sum(args.bytes); }" &
fio --name=s --filename=/dev/ublkb1 --size=16G --rw=read --bs=1m --iodepth=8 --direct=1 \
    --ioengine=io_uring
kill -INT %1

# Raw spans: 0+8G, 0+16G, 0+21G, 0+24G.
fio --name=q --filename=/dev/vdb --offset=0 --size=21G --rw=randread --bs=4k --iodepth=1 \
    --direct=1 --ioengine=io_uring --invalidate=1 --runtime=30 --ramp_time=3 --time_based

# Raw with a gap between reads, spinning (2, 4, 6, 8 us) and sleeping (10, 30 us), with
# dev-cpu.bt running for the device latency:
fio --name=q --filename=/dev/vdb --offset=0 --size=16G --rw=randread --bs=4k --iodepth=1 \
    --direct=1 --ioengine=io_uring --runtime=12 --time_based \
    --thinktime=2 --thinktime_spin=2 --thinktime_blocks=1

# QD32 and 1M, raw and Flaminio, with the node at --queues 1 and --queues 4, 2 runs each:
fio --name=q --filename=/dev/ublkb1 --offset=0 --size=16G --rw=randread --bs=4k --iodepth=32 \
    --direct=1 --ioengine=io_uring --invalidate=1 --runtime=20 --ramp_time=3 --time_based
fio --name=q --filename=/dev/ublkb1 --offset=0 --size=16G --rw=read --bs=1m --iodepth=8 \
    --direct=1 --ioengine=io_uring --invalidate=1 --runtime=20 --ramp_time=3 --time_based

# Profile of the CPU that serves a pinned read stream (--queues 4: queue thread on CPU 2):
perf record -e cpu-clock -C 2 -g -o perf-cpu2.data -- sleep 12 &
taskset -c 2 fio --name=q --filename=/dev/ublkb1 --size=16G --rw=randread --bs=4k \
    --iodepth=1 --direct=1 --ioengine=io_uring --runtime=15 --time_based
perf report -i perf-cpu2.data --no-children --sort comm,dso --stdio -g none
# The queue thread alone, with DWARF call graphs:
perf record -e cpu-clock -F 20000 --call-graph dwarf,16384 -t "$QUEUE_THREAD" -- sleep 8

# The queue thread's system calls and sleeps per read:
bpftrace -e "tracepoint:raw_syscalls:sys_enter /tid == $QUEUE_THREAD/ { @sys[args.id] = count(); }
    tracepoint:sched:sched_switch /args.prev_pid == $QUEUE_THREAD/ { @out[args.prev_state] = count(); }
    tracepoint:block:block_rq_issue /args.dev == $U/ { @reads = count(); }"

# The prototype (prototype-poll-inflight.diff): the node started with FLAMINIO_POLL_INFLIGHT=1
# in its environment (systemctl set-environment), --queues 4, zero copy; then 3 interleaved
# rounds of raw 0+21G and Flaminio, as for the spans above.
