#!/usr/bin/env bash
# Layer B: same-lab wall-clock provision + checksum verify for Flaminio vs Longhorn.
# Full fio campaigns stay in make bench-writes / docs/benchmarks — this script is demo-sized.
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

[[ -f $DEMO_DIR/active ]] || die "demo cluster not active"
export KUBECONFIG="$LOCAL_STATE_DIR/kubeconfig"
out="$LOCAL_STATE_DIR/demo/bench-compare-$(date +%Y%m%d-%H%M%S).md"
mkdir -p "$DEMO_DIR"

time_ready() {
    local ns=$1 name=$2 start=$SECONDS
    demo_kubectl wait -n "$ns" --for=condition=Ready "pod/$name" --timeout=600s >/dev/null
    echo $((SECONDS - start))
}

{
    echo "# Demo compare $(date -u +%Y-%m-%dT%H:%MZ)"
    echo
    echo "Hardware: fl-a on $HOST_A, fl-b on $HOST_B. Longhorn $LONGHORN_VERSION."
    echo "Fairness: same VMs, RF≈2, 64 MiB checksum blob. Longhorn uses host filesystem; Flaminio raw devices."
    echo
} >"$out"

log "ensuring Flaminio demo workload"
demo_kubectl apply -f "$REPO_ROOT/deploy/demo/workload.yaml" >/dev/null
t_f=$(time_ready flaminio-demo demo-0)
log_line=$(demo_kubectl logs -n flaminio-demo demo-0 | tail -1)
echo "| Flaminio | ${t_f}s to Ready | $log_line |" >>"$out"
log "Flaminio Ready in ${t_f}s ($log_line)"

if demo_kubectl get sc longhorn >/dev/null 2>&1; then
    log "ensuring Longhorn demo workload"
    demo_kubectl apply -f "$REPO_ROOT/deploy/demo/longhorn-workload.yaml" >/dev/null
    t_l=$(time_ready longhorn-demo demo-0)
    log_line=$(demo_kubectl logs -n longhorn-demo demo-0 | tail -1)
    echo "| Longhorn | ${t_l}s to Ready | $log_line |" >>"$out"
    log "Longhorn Ready in ${t_l}s ($log_line)"
else
    echo "| Longhorn | not installed | run make demo-longhorn |" >>"$out"
    warn "Longhorn StorageClass missing"
fi

cat >>"$out" <<EOF

## Notes
- Provision Ready time is not peak IOPS; use docs/benchmarks for fio.
- Disclose filesystem vs raw-block when showing this table.
EOF

log "wrote $out"
cat "$out"
