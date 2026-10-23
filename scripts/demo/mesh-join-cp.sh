#!/usr/bin/env bash
# Add the Mac Lima control plane as a WireGuard peer on every lab overlay VM, and add those
# VMs as peers on the control plane. Refresh host UDP forwards so DEMO_CP_SRC_IP can reach them.
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

demo_require_tools
demo_require_src_ip
[[ -f $DEMO_DIR/cp.pubkey ]] || die "run setup-mac-cp.sh first (missing $DEMO_DIR/cp.pubkey)"
cp_pub=$(<"$DEMO_DIR/cp.pubkey")

log "refreshing host port forwards to allow DEMO_CP_SRC_IP=$DEMO_CP_SRC_IP"
"$SCRIPTS/lab.sh" net-setup

# Address at which VM $1 reaches VM $2's WireGuard port (same rule as lab.sh).
wg_endpoint() {
    local from=$1 to=$2
    if [[ $(vmvar "$from" HOST) == "$(vmvar "$to" HOST)" ]]; then
        echo "$(vmvar "$to" IP):$WG_PORT"
    else
        echo "$(host_ip "$(vmvar "$to" HOST)"):$(vmvar "$to" WG_FWD)"
    fi
}

# Collect VM public keys (same as mesh up).
declare -A pub
for vm in $(vms_of overlay); do
    pub[$vm]=$(run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" key | tail -1)
done

# Rebuild each VM's wg0.conf with the existing peers plus the control plane.
for vm in $(vms_of overlay); do
    conf="[Interface]
Address = $(vmvar "$vm" WG)/24
ListenPort = $WG_PORT
PostUp = wg set %i private-key /etc/wireguard/wg0.key
"
    for peer in $(vms_of overlay); do
        [[ $peer == "$vm" ]] && continue
        conf+="
[Peer]
# $peer
PublicKey = ${pub[$peer]}
AllowedIPs = $(vmvar "$peer" WG)/32
Endpoint = $(wg_endpoint "$vm" "$peer")
PersistentKeepalive = 25
"
    done
    conf+="
[Peer]
# demo control plane ($DEMO_CP_LIMA)
PublicKey = $cp_pub
AllowedIPs = ${DEMO_CP_WG}/32
Endpoint = $DEMO_CP_ENDPOINT
PersistentKeepalive = 25
"
    ENV_VARS="WG_CONF=$(printf '%s' "$conf" | base64 | tr -d '\n')" run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" up
    log "$vm: peer $DEMO_CP_WG via $DEMO_CP_ENDPOINT"
done

# Control-plane wg0: Interface + every VM peer.
cp_conf="[Interface]
Address = ${DEMO_CP_WG}/24
ListenPort = ${DEMO_CP_WG_PORT}
PostUp = wg set %i private-key /etc/wireguard/wg0.key
"
for vm in $(vms_of overlay); do
    # From the Mac, reach each VM the same way the other server does: host uplink + WG_FWD.
    endpoint="$(host_ip "$(vmvar "$vm" HOST)"):$(vmvar "$vm" WG_FWD)"
    cp_conf+="
[Peer]
# $vm
PublicKey = ${pub[$vm]}
AllowedIPs = $(vmvar "$vm" WG)/32
Endpoint = $endpoint
PersistentKeepalive = 25
"
done
demo_lima sudo tee /etc/wireguard/wg0.conf >/dev/null <<<"$cp_conf"
demo_lima sudo chmod 600 /etc/wireguard/wg0.conf
demo_lima sudo bash -c 'wg-quick down wg0 2>/dev/null || true; wg-quick up wg0'

log "checking overlay reachability to the control plane"
bad=0
for vm in $(vms_of overlay); do
    if vssh "$vm" "ping -c 3 -i 0.3 -W 2 -q $DEMO_CP_WG >/dev/null"; then
        log "$vm -> cp ($DEMO_CP_WG): ok"
    else
        warn "$vm -> cp ($DEMO_CP_WG): no reply (check DEMO_CP_ENDPOINT / DEMO_CP_SRC_IP / net-setup)"
        bad=1
    fi
done
if demo_lima ping -c 3 -i 0.3 -W 2 -q "$VM_A_WG" >/dev/null; then
    log "cp -> $VM_A ($VM_A_WG): ok"
else
    warn "cp -> $VM_A: no reply"
    bad=1
fi
((bad == 0)) || die "control-plane overlay incomplete"
log "OK: Mac control plane is on the WireGuard mesh"
