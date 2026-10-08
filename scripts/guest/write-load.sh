#!/usr/bin/env bash
# Writes 4 GiB of random 64 KiB blocks to /dev/ublkb0 with fio's own checksums, then reads it all
# back and verifies (fio --verify). For correctness under faults, not for performance numbers.
sudo fio --name=write-verify --filename=/dev/ublkb0 --rw=randwrite --bs=64k --size=4G \
    --ioengine=io_uring --direct=1 --iodepth=16 --verify=crc32c --do_verify=1 --verify_fatal=1 \
    --output-format=json --output=/tmp/write-verify.json >/dev/null
rc=$?
jq -r '.jobs[0] | "  wrote \(.write.io_bytes/1048576|floor) MiB, verified \(.read.io_bytes/1048576|floor) MiB, errors \(.error)"' /tmp/write-verify.json
exit "$rc"
