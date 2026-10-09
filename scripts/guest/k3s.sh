#!/usr/bin/env bash
# k3s in a lab VM, over the overlay: the server (control plane) on the witness, agents on the
# data VMs. Every Kubernetes address is an overlay address, and flannel's VXLAN runs over wg0,
# so pod traffic between the servers is encrypted.
# Usage: k3s.sh server | agent | token | down | uninstall | status
#   server  K3S_VERSION, NODE_IP
#   agent   K3S_VERSION, NODE_IP, SERVER_IP, TOKEN
config=/etc/rancher/k3s/config.yaml

# The install script of the pinned release (not get.k3s.io's latest); it checks the binary
# against the release's sha256 list.
install_k3s() {
    local kind=$1
    if [[ $(k3s --version 2>/dev/null | awk 'NR==1 {print $3}') == "$K3S_VERSION" ]] &&
        systemctl is-active --quiet "$(unit)"; then
        log "k3s $K3S_VERSION already running as $kind"
        return
    fi
    curl -sfL "https://raw.githubusercontent.com/k3s-io/k3s/$K3S_VERSION/install.sh" -o /tmp/k3s-install.sh
    sudo env INSTALL_K3S_VERSION="$K3S_VERSION" INSTALL_K3S_EXEC="$kind" ${K3S_URL:+K3S_URL="$K3S_URL"} \
        ${K3S_TOKEN:+K3S_TOKEN="$K3S_TOKEN"} sh /tmp/k3s-install.sh >/dev/null
    log "k3s $K3S_VERSION installed and started as $kind"
}

unit() { if [[ -e /etc/systemd/system/k3s-agent.service ]]; then echo k3s-agent; else echo k3s; fi; }

server() {
    : "${K3S_VERSION:?}" "${NODE_IP:?}"
    sudo install -d /etc/rancher/k3s
    # The witness runs the control plane and no workloads: it holds no volume data.
    sudo tee "$config" >/dev/null <<EOF
node-ip: $NODE_IP
advertise-address: $NODE_IP
bind-address: $NODE_IP
tls-san: [$NODE_IP, 127.0.0.1]
flannel-iface: wg0
disable: [traefik, servicelb, local-storage]
write-kubeconfig-mode: "0600"
secrets-encryption: true
node-taint: ["node-role.kubernetes.io/control-plane=true:NoSchedule"]
node-label: ["flaminio.io/witness=true"]
EOF
    install_k3s server
    for _ in $(seq 1 60); do
        sudo k3s kubectl get node "$(hostname)" 2>/dev/null | grep -qw Ready && break
        sleep 2
    done
    sudo k3s kubectl get node "$(hostname)" | grep -qw Ready || die "k3s server not Ready"
}

agent() {
    : "${K3S_VERSION:?}" "${NODE_IP:?}" "${SERVER_IP:?}" "${TOKEN:?}"
    sudo install -d /etc/rancher/k3s
    sudo tee "$config" >/dev/null <<EOF
node-ip: $NODE_IP
flannel-iface: wg0
node-label: ["flaminio.io/storage=true"]
EOF
    K3S_URL="https://$SERVER_IP:6443" K3S_TOKEN=$TOKEN install_k3s agent
}

case "${1:-status}" in
    server) server ;;
    agent) agent ;;
    token) sudo cat /var/lib/rancher/k3s/server/node-token ;;
    down)
        if command -v k3s >/dev/null; then
            sudo systemctl stop "$(unit)" 2>/dev/null || true
            # Pods' containers outlive the service; k3s's own script stops them too.
            [[ -x /usr/local/bin/k3s-killall.sh ]] && sudo /usr/local/bin/k3s-killall.sh >/dev/null 2>&1
            log "k3s stopped"
        fi
        ;;
    uninstall)
        for s in k3s-uninstall.sh k3s-agent-uninstall.sh; do
            [[ -x /usr/local/bin/$s ]] && sudo "/usr/local/bin/$s" >/dev/null 2>&1
        done
        sudo rm -rf /etc/rancher/k3s
        log "k3s removed"
        ;;
    status)
        if command -v k3s >/dev/null; then
            echo "  $(unit): $(systemctl is-active "$(unit)") ($(k3s --version | awk 'NR==1 {print $3}'))"
        else
            echo "  k3s: not installed"
        fi
        ;;
    *) die "usage: k3s.sh server|agent|token|down|uninstall|status" ;;
esac
