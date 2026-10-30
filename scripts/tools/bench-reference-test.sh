#!/usr/bin/env bash
# Checks bench_reference (scripts/lib.sh): from the volume's range on the node's disk, the span
# the raw jobs of bench-reads read (docs/issues/9). Run by `make lint`.
if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "bench-reference-test: needs bash >= 4.4 (found $BASH_VERSION); skipped"
    exit 0
fi
set -euo pipefail
# shellcheck source=../lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

G=$((1 << 30))
DISK=$((100 * G))
failed=0

# want EXPECTED ARGS...: bench_reference ARGS prints EXPECTED, or stops if EXPECTED is "refused".
want() {
    local expected=$1 got
    shift
    got=$( (bench_reference "$@") 2>/dev/null) || got=refused
    if [[ $got != "$expected" ]]; then
        echo "FAIL: bench_reference $*: got '$got', want '$expected'"
        failed=1
    fi
}

# fl-b: volb's data at GiB 0-21, all of it read from the disk.
want "0 16" 16G 0 21 $((16 * G)) $DISK
# A range at the end of the disk: the span moves back inside it.
want "84 16" 16G 85 100 $((15 * G)) $DISK
# The range at most 1.5 times the volume, plus the region a volume not aligned to 1 GiB takes.
want "0 16" 16G 0 25 $((16 * G)) $DISK
want refused 16G 0 26 $((16 * G)) $DISK
want "3 1" 1G 3 5 $G $DISK
want refused 1G 3 6 $G $DISK
# From 90% to 110% of the volume read from the disk (16 GiB: 15461882265.6 and 18897856102.4).
want "0 16" 16G 0 21 15461882266 $DISK
want refused 16G 0 21 15461882265 $DISK
want "0 16" 16G 0 21 18897856102 $DISK
want refused 16G 0 21 18897856103 $DISK
# No region reached the threshold, or nothing was read at all.
want refused 16G 5 5 $((16 * G)) $DISK
want refused 16G 6 5 $((16 * G)) $DISK
want refused 16G 0 0 0 $DISK
# A disk smaller than the volume.
want refused 16G 0 10 $((16 * G)) $((10 * G))
# Malformed arguments: leading zeros (octal to bash), signs, other units, a missing argument.
want refused 16G 08 21 $((16 * G)) $DISK
want refused 16G 010 21 $((16 * G)) $DISK
want refused 016G 0 21 $((16 * G)) $DISK
want refused 0G 0 0 0 $DISK
want refused 16 0 21 $((16 * G)) $DISK
want refused 16M 0 21 $((16 * G)) $DISK
want refused 16G -1 21 $((16 * G)) $DISK
want refused 16G 0 21 $((16 * G))

((failed == 0)) || exit 1
echo "bench-reference-test: ok"
