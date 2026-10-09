#!/usr/bin/env bash
# Single-block probes on a block device, for the partition test: O_DIRECT 4 KiB reads and
# writes, each logged with its start and end time (this VM's clock, seconds since the epoch).
# Usage: probe.sh write DEV OFFSET VALUE | read DEV OFFSET
#        probe.sh start NAME reader|writer DEV OFFSET   (a loop, as the transient unit flaminio-probe-NAME)
#        probe.sh stop NAME | log NAME
# Log lines: START END ok VALUE | START END err ERRNO. A writer writes "NAME-1", "NAME-2", ...
lib=/usr/local/lib/flaminio/probe.py

install_probe() {
    sudo install -d /usr/local/lib/flaminio /var/lib/flaminio-probe
    sudo tee "$lib" >/dev/null <<'PY'
import errno, mmap, os, sys, time

BS = 4096

def open_dev(dev):
    return os.open(dev, os.O_RDWR | os.O_DIRECT | os.O_DSYNC)

def read(fd, off):
    buf = mmap.mmap(-1, BS)
    if os.preadv(fd, [buf], off) != BS:
        raise OSError(errno.EIO, "short read")
    return bytes(buf).split(b"\0", 1)[0].decode(errors="replace") or "-"

def write(fd, off, value):
    buf = mmap.mmap(-1, BS)
    buf.write(value.encode())
    if os.pwritev(fd, [buf], off) != BS:
        raise OSError(errno.EIO, "short write")

def timed(f, *args):
    start = time.time()
    try:
        r = f(*args)
        return "%.3f %.3f ok %s" % (start, time.time(), r if r is not None else args[-1])
    except OSError as e:
        return "%.3f %.3f err %s" % (start, time.time(), errno.errorcode.get(e.errno, e.errno))

cmd, dev, off = sys.argv[1], sys.argv[2], int(sys.argv[3])
fd = open_dev(dev)
if cmd == "read":
    print(timed(read, fd, off))
elif cmd == "write":
    print(timed(write, fd, off, sys.argv[4]))
else:
    # reader|writer NAME LOG: one operation at a time, until stopped.
    name, log, seq = sys.argv[4], open(sys.argv[5], "a", buffering=1), 0
    while True:
        seq += 1
        line = timed(read, fd, off) if cmd == "reader" else timed(write, fd, off, "%s-%d" % (name, seq))
        log.write(line + "\n")
        time.sleep(0.05)
PY
}

case "${1:-}" in
    read | write)
        install_probe
        sudo python3 "$lib" "$1" "$2" "$3" "${4:-}"
        ;;
    start)
        name=$2 kind=$3 dev=$4 off=$5
        install_probe
        sudo systemctl stop "flaminio-probe-$name" 2>/dev/null || true
        sudo systemctl reset-failed "flaminio-probe-$name" 2>/dev/null || true
        sudo rm -f "/var/lib/flaminio-probe/$name.log"
        sudo systemd-run --quiet --unit="flaminio-probe-$name" \
            python3 "$lib" "$kind" "$dev" "$off" "$name" "/var/lib/flaminio-probe/$name.log"
        ;;
    stop)
        # A probe blocked in the kernel on the device cannot be killed until its I/O ends:
        # do not wait for it.
        sudo systemctl kill --signal=KILL "flaminio-probe-$2" 2>/dev/null || true
        ;;
    log) sudo cat "/var/lib/flaminio-probe/$2.log" 2>/dev/null || true ;;
    *) die "usage: probe.sh write DEV OFF VALUE | read DEV OFF | start NAME reader|writer DEV OFF | stop NAME | log NAME" ;;
esac
