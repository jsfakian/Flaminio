#!/usr/bin/env bash
# Interactive (or DEMO_AUTO=1) investor/colleague demo driver. Scenarios 0–5, 7–8 (6 optional).
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

export KUBECONFIG="$LOCAL_STATE_DIR/kubeconfig"
[[ -f $DEMO_DIR/active ]] || die "demo cluster not active (make demo-up)"

[[ -n ${SCENARIOS:-} ]] || SCENARIOS=0,1,2,3,4,5,7,8
IFS=',' read -r -a WANT <<<"$SCENARIOS"

want() {
    local s=$1 x
    for x in "${WANT[@]}"; do [[ $x == "$s" ]] && return 0; done
    return 1
}

talk() { say "$1"; prompt "Talk this, then continue"; }

scenario_0() {
    talk "Topology: Mac control plane (NoSchedule), Site A = fl-a on rhythm, Site B = fl-b on rhythm-backup."
    demo_kubectl get nodes -o wide
    demo_kubectl get nodes -o custom-columns=NAME:.metadata.name,TAINTS:.spec.taints,LABELS:.metadata.labels
}

scenario_1() {
    talk "Provision: apply the demo StatefulSet with StorageClass flaminio."
    demo_kubectl apply -f "$REPO_ROOT/deploy/demo/workload.yaml"
    demo_kubectl wait -n flaminio-demo --for=condition=Ready pod/demo-0 --timeout=300s
    demo_kubectl get pvc,pod -n flaminio-demo -o wide
}

scenario_2() {
    talk "Durable write: the pod creates a 64 MiB blob + sha256 (or verifies if already present)."
    demo_kubectl logs -n flaminio-demo demo-0 | tail -5
    [[ $(demo_kubectl logs -n flaminio-demo demo-0 | tail -1) =~ ^(created|verified) ]] ||
        die "unexpected demo pod log"
}

# Map VM name → lab.sh selector (a|b).
demo_vm_sel() {
    case "$1" in
        "$VM_A") echo a ;;
        "$VM_B") echo b ;;
        "$VM_C") echo c ;;
        *) die "unknown VM $1" ;;
    esac
}

scenario_3() {
    talk "Kill the replica site: stop flaminio-node on the node that is not hosting the pod; bring it back."
    local pod_node replica
    pod_node=$(demo_kubectl get pod -n flaminio-demo demo-0 -o jsonpath='{.spec.nodeName}')
    if [[ $pod_node == "$VM_A" ]]; then replica=$VM_B; else replica=$VM_A; fi
    log "pod on $pod_node; stopping dataplane on replica $replica"
    "$SCRIPTS/lab.sh" node down "$(demo_vm_sel "$replica")"
    sleep 5
    demo_kubectl get nodes
    demo_kubectl get pod -n flaminio-demo demo-0 -o wide
    [[ $(demo_kubectl get pod -n flaminio-demo demo-0 -o jsonpath='{.status.phase}') == Running ]] ||
        die "demo pod should keep running while the replica dataplane is down"
    prompt "Point at Grafana / exporter: meta still up; replica ISR should shrink"
    log "restarting dataplane on $replica"
    "$SCRIPTS/lab.sh" node up "$(demo_vm_sel "$replica")"
    vssh "$replica" "systemctl is-active --quiet flaminio-node"
    log "replica $replica: flaminio-node active again"
}

scenario_4() {
    talk "Kill the target site: power off the demo pod's VM; CSI failover should land on the other site (~2 min)."
    local lost other start
    lost=$(demo_kubectl get pod -n flaminio-demo demo-0 -o jsonpath='{.spec.nodeName}')
    other=$(vms_of all | tr ' ' '\n' | grep -vx "$lost")
    log "demo pod on $lost; powering it off"
    hsh "$(vmvar "$lost" HOST)" "virsh -c qemu:///system destroy $lost" >/dev/null
    start=$SECONDS
    trap 'hsh "$(vmvar "'"$lost"'" HOST)" "virsh -c qemu:///system start '"$lost"'" >/dev/null 2>&1' EXIT
    until [[ $(demo_kubectl get pod -n flaminio-demo demo-0 -o jsonpath='{.spec.nodeName}' 2>/dev/null) == "$other" ]] &&
        demo_kubectl wait -n flaminio-demo --for=condition=Ready pod/demo-0 --timeout=5s >/dev/null 2>&1; do
        ((SECONDS - start < 900)) || die "failover to $other did not complete in 900s"
        sleep 5
    done
    log "Ready on $other after $((SECONDS - start))s: $(demo_kubectl logs -n flaminio-demo demo-0 | tail -1)"
    [[ $(demo_kubectl logs -n flaminio-demo demo-0 | tail -1) == "verified on $other" ]] ||
        warn "expected verified on $other (may still be starting)"
    log "starting $lost again and bringing its dataplane back"
    hsh "$(vmvar "$lost" HOST)" "virsh -c qemu:///system start $lost" >/dev/null
    trap - EXIT
    wait_for "$lost SSH" 300 vssh "$lost" true
    # k3s-agent and WireGuard come back with the guest; restart the dataplane unit.
    "$SCRIPTS/lab.sh" node up "$(demo_vm_sel "$lost")" || warn "node up on $lost failed; check make node-up"
}

scenario_5() {
    talk "Network partition: lease fencing on lab volume vol1. CSI volumes must let go first (MaxVolumesPerNode=1)."
    prompt "Delete the demo PVC, then run partition-test?"
    # Partition drives ublk on vol1; a node serves one attached volume at a time.
    # Delete only the StatefulSet/PVC — keep namespace (scoreboard, exporter).
    demo_kubectl delete statefulset -n flaminio-demo demo --ignore-not-found --wait=true >/dev/null 2>&1 || true
    demo_kubectl delete pvc -n flaminio-demo --all --ignore-not-found --wait=true >/dev/null 2>&1 || true
    local start=$SECONDS
    until [[ -z $(demo_kubectl get volumeattachment -o jsonpath='{.items[?(@.spec.attacher=="csi.flaminio.io")].metadata.name}' 2>/dev/null) ]]; do
        ((SECONDS - start < 180)) || die "CSI VolumeAttachments still present after 180s"
        sleep 3
    done
    "$SCRIPTS/lab.sh" vol create 2>/dev/null || true
    HOLD=${HOLD:-30} "$SCRIPTS/lab.sh" partition
}

scenario_6() {
    talk "Optional drain (needs fl-c and lab vol1). Opt-in: DRAIN=1 (can take 15+ min; skip in the default talk)."
    if [[ ${DRAIN:-0} != 1 ]]; then
        log "skip (set DRAIN=1 to run make drain-test)"
        return
    fi
    if [[ -f $LOCAL_STATE_DIR/$VM_C/created ]]; then
        "$SCRIPTS/lab.sh" vol create 2>/dev/null || true
        "$SCRIPTS/lab.sh" drain-test
    else
        log "fl-c not present; skip"
    fi
}

scenario_7() {
    talk "Performance honesty: open the scoreboard (canned vs raw). Do not claim 1M write at 80%+."
    demo_kubectl apply -f "$REPO_ROOT/deploy/demo/status-exporter.yaml" >/dev/null 2>&1 || true
    demo_kubectl apply -f "$REPO_ROOT/deploy/demo/scoreboard.yaml" >/dev/null
    demo_kubectl -n flaminio-demo rollout status deploy/scoreboard --timeout=60s >/dev/null || true
    log "port-forward: kubectl -n flaminio-demo port-forward svc/scoreboard 8080:80"
    log "Grafana: kubectl -n monitoring port-forward svc/monitoring-grafana 3000:80  (admin; password in .lab/demo/grafana-admin-password)"
    cat <<'EOF'
Canned highlights (this lab):
  reads (ZC)     ~95–100% of raw
  4K write QD32  competitive (see W4.3)
  1M write QD8   ~42–70% of raw — open gap, say so
EOF
}

scenario_8() {
    talk "Competitors: Longhorn is the peer; Mayastor later; Ceph wrong weight for 2-node edge."
    if [[ ${LONGHORN:-0} == 1 ]]; then
        log "LONGHORN=1: running bench-compare.sh"
        "$SCRIPTS/demo/bench-compare.sh"
    else
        log "Scoreboard Layer A only. For live Longhorn: LONGHORN=1 make demo-longhorn && LONGHORN=1 make demo"
        log "See docs/demo-competitors.md"
    fi
}

say "Flaminio demo — scenarios: ${SCENARIOS}"
prompt "Audience ready?"

want 0 && scenario_0
want 1 && scenario_1
want 2 && scenario_2
want 3 && scenario_3
want 4 && scenario_4
want 5 && scenario_5
want 6 && scenario_6
want 7 && scenario_7
want 8 && scenario_8

say "Demo complete. Reset with make demo-down when finished."
