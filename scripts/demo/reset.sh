#!/usr/bin/env bash
# Tear down demo namespaces and optionally restore lab k3s on fl-w (RESTORE_LAB_K8S=1).
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

export KUBECONFIG="$LOCAL_STATE_DIR/kubeconfig"

if [[ -f $DEMO_DIR/active ]] && command -v kubectl >/dev/null && [[ -f $LOCAL_STATE_DIR/kubeconfig ]]; then
    log "deleting demo namespaces"
    demo_kubectl delete namespace flaminio-demo flaminio-test --ignore-not-found --wait=false || true
    if [[ ${KEEP_MONITORING:-0} != 1 ]] && command -v helm >/dev/null; then
        helm uninstall monitoring -n monitoring 2>/dev/null || true
        demo_kubectl delete namespace monitoring --ignore-not-found --wait=false || true
    fi
fi

rm -f "$DEMO_DIR/active"

if [[ ${RESTORE_LAB_K8S:-0} == 1 ]]; then
    log "restoring lab k3s (server on fl-w, agents on fl-a/fl-b)"
    for vm in $(vms_of all); do
        run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" uninstall || true
    done
    # Leave Lima CP running unless DESTROY_LIMA=1; lab k8s-up brings fl-w back.
    "$SCRIPTS/lab.sh" k8s up
fi

if [[ ${DESTROY_LIMA:-0} == 1 ]]; then
    log "deleting Lima instance $DEMO_CP_LIMA"
    limactl delete -f "$DEMO_CP_LIMA" 2>/dev/null || true
fi

# Clear DEMO_CP_SRC_IP forwards by re-running net-setup without it if unset in the environment
# the operator used; document that operators should `unset DEMO_CP_SRC_IP && make net-setup`.
log "demo marker cleared. To drop Mac UDP forwards: unset DEMO_CP_SRC_IP && make net-setup"
log "To restore lab Kubernetes: RESTORE_LAB_K8S=1 $0"
