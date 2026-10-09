#!/usr/bin/env bash
# WireGuard overlay between the lab VMs. Usage: wg.sh key | up | down | status
#   key   install wireguard-tools, create this VM's key if needed, print its public key
#   up    install WG_CONF (base64 wg-quick config, without the private key) and start wg0
# The private key never leaves the VM: wg-quick loads it from /etc/wireguard/wg0.key.

case "${1:-status}" in
    key)
        if ! command -v wg >/dev/null; then
            sudo apt-get update -qq
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq wireguard-tools >/dev/null
        fi
        if ! sudo test -s /etc/wireguard/wg0.key; then
            (umask 077 && wg genkey | sudo tee /etc/wireguard/wg0.key >/dev/null)
        fi
        sudo cat /etc/wireguard/wg0.key | wg pubkey
        ;;
    up)
        [[ -n ${WG_CONF:-} ]] || die "WG_CONF not set"
        echo "$WG_CONF" | base64 -d | sudo tee /etc/wireguard/wg0.conf >/dev/null
        sudo chmod 600 /etc/wireguard/wg0.conf
        sudo systemctl enable --quiet wg-quick@wg0
        sudo systemctl restart wg-quick@wg0
        log "wg0 up: $(ip -4 -o addr show wg0 | awk '{print $4}')"
        ;;
    down)
        sudo systemctl disable --now --quiet wg-quick@wg0 2>/dev/null || true
        log "wg0 down"
        ;;
    status) sudo wg show wg0 2>/dev/null || echo "wg0 not up" ;;
    *) die "usage: wg.sh key|up|down|status" ;;
esac
