#!/usr/bin/env bash
# Forwards REPL_PORT on this host's uplink to the replica VM, for traffic from HOST_A_IP only.
# Usage: net-forward.sh up|down|status
set -euo pipefail
export LC_ALL=C  # parsed command output must not be localized
# Note: `grep ... >/dev/null`, never `grep -q`, after a pipe: -q exits early and, with pipefail,
# the SIGPIPE in the writer makes a successful match look like a failure.
tag=flaminio-lab
nat=(PREROUTING -i "$HOST_B_IFACE" -p tcp -s "$HOST_A_IP" --dport "$REPL_PORT"
    -m comment --comment "$tag" -j DNAT --to-destination "$VM_IP:$REPL_PORT")
fwd=(FORWARD -i "$HOST_B_IFACE" -o "$LIBVIRT_BRIDGE" -p tcp -s "$HOST_A_IP" -d "$VM_IP" --dport "$REPL_PORT"
    -m comment --comment "$tag" -j ACCEPT)

case "${1:-status}" in
    up)
        sudo iptables -t nat -C "${nat[@]}" 2>/dev/null || sudo iptables -t nat -I "${nat[0]}" 1 "${nat[@]:1}"
        # Position 1: ahead of the libvirt, Docker and ufw chains, which would reject inbound
        # connections to the NAT network.
        sudo iptables -C "${fwd[@]}" 2>/dev/null || sudo iptables -I "${fwd[0]}" 1 "${fwd[@]:1}"
        echo "  [$(hostname)] $HOST_A_IP -> $(hostname):$REPL_PORT forwarded to $VM_IP:$REPL_PORT"
        ;;
    down)
        while sudo iptables -t nat -D "${nat[@]}" 2>/dev/null; do :; done
        while sudo iptables -D "${fwd[@]}" 2>/dev/null; do :; done
        echo "  [$(hostname)] port forward removed"
        ;;
    status)
        sudo iptables -t nat -S PREROUTING | grep -- "$tag" || echo "  no NAT rule"
        sudo iptables -S FORWARD | grep -- "$tag" || echo "  no FORWARD rule"
        ;;
    *) echo "usage: $0 up|down|status" >&2; exit 2 ;;
esac
