#!/usr/bin/env bash
# Point fl-a and fl-b k3s agents at the Mac Lima API; stop k3s on fl-w (etcd/meta stay).
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

demo_require_tools
[[ -f $LOCAL_STATE_DIR/kubeconfig ]] || die "missing kubeconfig (run setup-mac-cp.sh)"
demo_kubectl get nodes >/dev/null || die "API not reachable at $LOCAL_STATE_DIR/kubeconfig"

# Overlay must reach the control plane.
vssh "$VM_A" "ping -c 1 -W 2 -q $DEMO_CP_WG >/dev/null" || die "no overlay to CP (run mesh-join-cp.sh)"

token=$(demo_lima sudo cat /var/lib/rancher/k3s/server/node-token)

# Retire the old control plane on fl-w (keeps etcd + flaminio-meta).
log "stopping k3s on $VM_W (witness stays for etcd/meta only)"
run_on_vm "$VM_W" "$SCRIPTS/guest/k3s.sh" uninstall || true

# Reinstall agents against the Mac CP.
for vm in $(vms_of all); do
    log "joining $vm as agent to $DEMO_CP_WG"
    run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" uninstall || true
    ENV_VARS="K3S_VERSION=$K3S_VERSION NODE_IP=$(vmvar "$vm" WG) SERVER_IP=$DEMO_CP_WG TOKEN=$token" \
        run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" agent
done

start=$SECONDS
until [[ $(demo_kubectl get nodes --no-headers 2>/dev/null | awk '$2 == "Ready"' | wc -l | tr -d ' ') -ge 3 ]]; do
    ((SECONDS - start < 300)) || die "not every node Ready: $(demo_kubectl get nodes 2>&1)"
    sleep 5
done

# Ensure labels/taints match the demo story.
cp_node=$(demo_kubectl get nodes -l flaminio.io/demo-cp=true -o jsonpath='{.items[0].metadata.name}')
[[ -n $cp_node ]] || cp_node=$(demo_kubectl get nodes -l node-role.kubernetes.io/control-plane -o jsonpath='{.items[0].metadata.name}')
demo_kubectl label node "$cp_node" flaminio.io/witness=true --overwrite >/dev/null
demo_kubectl taint node "$cp_node" node-role.kubernetes.io/control-plane=true:NoSchedule --overwrite >/dev/null 2>&1 || true

for vm in $(vms_of all); do
    demo_kubectl label node "$vm" flaminio.io/storage=true --overwrite >/dev/null
done

touch "$DEMO_DIR/active"
demo_kubectl get nodes -o wide
log "OK: demo cluster active (marker $DEMO_DIR/active). CSI: make csi-deploy"
