#!/usr/bin/env bash
# A phase-1 node in a lab VM: flaminio-io node as the systemd service "flaminio-node".
# Usage: node.sh up | down | status
#   up   NODE, LISTEN, META, TRUST_DOMAIN; FORMAT=1 reformats the data disk. With ATTACH=VOLUME
#        and TOKEN (base64 capability token), the node is also VOLUME's target, at /dev/ublkb1.
bin="$HOME/flaminio/dataplane/target/release/flaminio-io"
id=1
dev=/dev/ublkb$id
unit=flaminio-node

# This run's log only: the journal keeps earlier runs of the unit too.
journal() {
    sudo journalctl -u "$unit" --no-pager -o cat \
        _SYSTEMD_INVOCATION_ID="$(systemctl show -p InvocationID --value "$unit")" 2>/dev/null
}

down() {
    if [[ -e /dev/ublkc$id ]]; then
        sudo "$bin" del --id "$id" >/dev/null 2>&1 || true
    fi
    for _ in $(seq 1 20); do
        systemctl is-active --quiet "$unit" || break
        sleep 0.5
    done
    systemctl is-active --quiet "$unit" && sudo systemctl stop "$unit"
    sudo systemctl reset-failed "$unit" 2>/dev/null || true
    log "flaminio-node stopped"
}

up() {
    : "${NODE:?}" "${LISTEN:?}" "${META:?}" "${TRUST_DOMAIN:?}"
    [[ -x $bin ]] || die "flaminio-io not built; run make deploy"
    sudo modprobe ublk_drv
    systemctl is-active --quiet "$unit" && down
    local args=(node --device "$(data_disk)" --node "$NODE" --listen "$LISTEN" --meta "$META"
        --pki /etc/flaminio/pki --trust-domain "$TRUST_DOMAIN" --id "$id")
    [[ ${FORMAT:-0} == 1 ]] && args+=(--format --force)
    if [[ -n ${ATTACH:-} ]]; then
        sudo install -d -m 700 /run/flaminio
        echo "${TOKEN:?}" | base64 -d | sudo tee "/run/flaminio/$ATTACH.token" >/dev/null
        args+=(--attach "$ATTACH" --token "/run/flaminio/$ATTACH.token")
    fi
    sudo systemd-run --quiet --unit="$unit" --property=Restart=no "$bin" "${args[@]}"
    local ready="serving replicas"
    [[ -n ${ATTACH:-} ]] && ready="serving .* at $dev"
    for _ in $(seq 1 120); do
        journal | grep -E "$ready" >/dev/null && break
        systemctl is-active --quiet "$unit" || { journal | tail -20 >&2; die "flaminio-node exited"; }
        sleep 0.5
    done
    log "$(journal | grep -E 'store ready|target of|serving' | tail -3 | tr '\n' ' ')"
}

status() {
    echo "  flaminio-node: $(systemctl is-active "$unit" 2>/dev/null)"
    [[ -b $dev ]] && echo "  $dev: $(lsblk -dn -o SIZE "$dev")"
    journal | tail -4 | sed 's/^/  /'
}

case "${1:-status}" in
    up) up ;;
    down) down ;;
    status) status ;;
    *) die "usage: node.sh up|down|status" ;;
esac
