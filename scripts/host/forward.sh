#!/usr/bin/env bash
# Port forwards from this host's uplink to its lab VMs, each for one source address only.
# Usage: forward.sh up|down|status
#   IFACE   the uplink
#   RULES   space-separated "proto/port/vm_ip/vm_port/source_ip" entries
# `up` makes the tagged rules exactly RULES: it removes every tagged rule, then adds them.
set -euo pipefail
export LC_ALL=C  # parsed command output must not be localized
tag=flaminio-lab

remove_all() {
    local table chain line
    for table in nat filter; do
        chain=FORWARD
        [[ $table == nat ]] && chain=PREROUTING
        # `|| true`: no tagged rules is not an error.
        { sudo iptables -t "$table" -S "$chain" | grep -- "--comment $tag" || true; } | while read -r line; do
            # shellcheck disable=SC2086 # the rule is a list of iptables arguments
            sudo iptables -t "$table" ${line/-A /-D }
        done
    done
}

case "${1:-status}" in
    up)
        remove_all
        for r in $RULES; do
            IFS=/ read -r proto port ip vport src <<<"$r"
            sudo iptables -t nat -I PREROUTING 1 -i "$IFACE" -p "$proto" -s "$src" --dport "$port" \
                -m comment --comment "$tag" -j DNAT --to-destination "$ip:$vport"
            # Position 1: ahead of the libvirt, Docker and ufw chains, which would reject
            # inbound connections to the NAT network.
            sudo iptables -I FORWARD 1 -i "$IFACE" -o "$LIBVIRT_BRIDGE" -p "$proto" -s "$src" -d "$ip" \
                --dport "$vport" -m comment --comment "$tag" -j ACCEPT
            echo "  [$(hostname)] $src -> $(hostname):$port/$proto forwarded to $ip:$vport"
        done
        ;;
    down)
        remove_all
        echo "  [$(hostname)] port forwards removed"
        ;;
    status)
        sudo iptables -t nat -S PREROUTING | grep -- "$tag" || echo "  no NAT rules"
        sudo iptables -S FORWARD | grep -- "$tag" || echo "  no FORWARD rules"
        ;;
    *) echo "usage: $0 up|down|status" >&2; exit 2 ;;
esac
