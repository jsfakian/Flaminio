#!/usr/bin/env bash
# Replica-store benchmark on the data disk, with the same jobs as fio-raw.sh where they make
# sense. Reformats the data disk unless FORMAT=0.
# Usage: store-bench.sh RUNTIME_SECONDS [JOB_REGEX]
runtime=${1:-30}
filter=${2:-.}
disk=$(data_disk)
bench="$HOME/flaminio/dataplane/target/release/flaminio-bench"
[[ -x $bench ]] || die "flaminio-bench not built; run make deploy"
out="$HOME/results/store-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"

# 32 chunks of 1 GiB: a 32 GiB working set.
common=(--device "$disk" --chunks 32 --chunk-size 1G)
if [[ ${FORMAT:-1} == 1 ]]; then
    log "formatting $disk and prefilling the 32 GiB working set"
    sudo "$bench" "${common[@]}" --format --force --prefill --workload read --bs 1M --batch 8 --runtime 1 >/dev/null
fi

# name workload bs batch
jobs=(
    "read-1m-b8        read      1M 8"
    "randread-4k-b1    randread  4K 1"
    "randread-4k-b32   randread  4K 32"
    "randwrite-4k-b1   randwrite 4K 1"
    "randwrite-4k-b32  randwrite 4K 32"
    "write-1m-b8       write     1M 8"
)

log "store benchmark on $disk, ${runtime}s per job, results in $out"
printf '%-18s %10s %10s %10s %10s\n' job IOPS MiB/s "p50(us)" "p99(us)"
for spec in "${jobs[@]}"; do
    read -r name workload bs batch <<<"$spec"
    [[ $name =~ $filter ]] || continue
    sudo "$bench" "${common[@]}" --workload "$workload" --bs "$bs" --batch "$batch" --runtime "$runtime" \
        --name "$name" --json "$out/$name.json" 2>/dev/null
done
sudo chown -R "$(id -u):$(id -g)" "$out"
echo "$out"
