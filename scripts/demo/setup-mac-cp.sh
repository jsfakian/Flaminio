#!/usr/bin/env bash
# Install the demo control plane: Lima guest + WireGuard peer + k3s server (NoSchedule).
# Prerequisites: brew install lima kubectl jq bash; DEMO_CP_SRC_IP set; lab overlay up.
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

demo_require_tools
demo_require_src_ip

template="$SCRIPTS/demo/lima-flaminio-cp.yaml"
mkdir -p "$DEMO_DIR"

if ! limactl list -q 2>/dev/null | grep -qx "$DEMO_CP_LIMA"; then
    log "creating Lima instance $DEMO_CP_LIMA (first boot installs packages)"
    limactl start --name="$DEMO_CP_LIMA" "$template"
else
    log "Lima instance $DEMO_CP_LIMA already exists"
    limactl start "$DEMO_CP_LIMA" 2>/dev/null || true
fi

# WireGuard key and config inside the guest.
if ! demo_lima sudo test -s /etc/wireguard/wg0.key; then
    demo_lima sudo bash -c 'umask 077; wg genkey | tee /etc/wireguard/wg0.key >/dev/null'
fi
pub=$(demo_lima sudo bash -c 'wg pubkey < /etc/wireguard/wg0.key')
printf '%s\n' "$pub" >"$DEMO_DIR/cp.pubkey"
log "control-plane WireGuard pubkey: $pub"

# Peer list for the VMs is filled by mesh-join-cp.sh; here we only set Interface + listen.
demo_lima sudo tee /etc/wireguard/wg0.conf >/dev/null <<EOF
[Interface]
Address = ${DEMO_CP_WG}/24
ListenPort = ${DEMO_CP_WG_PORT}
PostUp = wg set %i private-key /etc/wireguard/wg0.key
EOF
demo_lima sudo chmod 600 /etc/wireguard/wg0.conf
demo_lima sudo systemctl enable --now wg-quick@wg0 2>/dev/null || \
    demo_lima sudo bash -c 'wg-quick down wg0 2>/dev/null; wg-quick up wg0'
log "wg0 on $DEMO_CP_LIMA: $DEMO_CP_WG (UDP $DEMO_CP_WG_PORT forwarded to the Mac)"

# k3s server: witness label + NoSchedule, same pin as the lab.
demo_lima sudo install -d /etc/rancher/k3s
demo_lima sudo tee /etc/rancher/k3s/config.yaml >/dev/null <<EOF
node-ip: $DEMO_CP_WG
advertise-address: $DEMO_CP_WG
bind-address: $DEMO_CP_WG
tls-san: [$DEMO_CP_WG, 127.0.0.1, host.lima.internal]
flannel-iface: wg0
disable: [traefik, servicelb, local-storage]
write-kubeconfig-mode: "0644"
secrets-encryption: true
node-taint: ["node-role.kubernetes.io/control-plane=true:NoSchedule"]
node-label: ["flaminio.io/witness=true", "flaminio.io/demo-cp=true"]
EOF
if ! demo_lima bash -c "k3s --version 2>/dev/null | awk 'NR==1 {print \$3}' | grep -qx '$K3S_VERSION'" ||
    ! demo_lima systemctl is-active --quiet k3s; then
    log "installing k3s $K3S_VERSION on $DEMO_CP_LIMA"
    demo_lima sudo bash -c "curl -sfL https://raw.githubusercontent.com/k3s-io/k3s/${K3S_VERSION}/install.sh -o /tmp/k3s-install.sh"
    demo_lima sudo env INSTALL_K3S_VERSION="$K3S_VERSION" INSTALL_K3S_EXEC=server \
        sh /tmp/k3s-install.sh >/dev/null
fi

# Wait for Ready.
for _ in $(seq 1 60); do
    demo_lima sudo k3s kubectl get node "$(demo_lima hostname)" 2>/dev/null | grep -qw Ready && break
    sleep 2
done
demo_lima sudo k3s kubectl get node "$(demo_lima hostname)" | grep -qw Ready ||
    die "k3s server on $DEMO_CP_LIMA not Ready"

# Kubeconfig for this Mac: API via localhost port-forward from Lima.
demo_lima sudo cat /etc/rancher/k3s/k3s.yaml |
    sed -e "s#server: https://.*:6443#server: https://127.0.0.1:6443#" \
        -e 's/default/flaminio-demo/g' >"$LOCAL_STATE_DIR/kubeconfig"
chmod 600 "$LOCAL_STATE_DIR/kubeconfig"
printf '%s\n' "$DEMO_CP_SRC_IP" >"$DEMO_DIR/src-ip"
printf '%s\n' "$DEMO_CP_ENDPOINT" >"$DEMO_DIR/endpoint"
log "wrote $LOCAL_STATE_DIR/kubeconfig (API https://127.0.0.1:6443)"
log "next: scripts/demo/mesh-join-cp.sh && scripts/demo/join-workers.sh"
