#!/usr/bin/env bash
# A phase-1 node in a lab VM: flaminio-io node as the systemd service "flaminio-node".
# Usage: node.sh up | down | status | attach | detach
#   up      NODE, LISTEN, META, TRUST_DOMAIN; FORMAT=1 reformats the data disk; DEVICE overrides
#           the data disk (as the node agent claimed it); ZERO_COPY=1 serves local reads
#           without copies (and without checksum verification)
#   attach  VOLUME, TOKEN (base64 capability token): the running node becomes VOLUME's target,
#           at /dev/ublkb1, through its control socket (no restart)
#   detach  VOLUME
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
    # The kernel does not delete a block device that is mounted: deleting it would wait forever.
    local mounts
    mounts=$(findmnt -rn -S "$dev" -o TARGET 2>/dev/null || true)
    [[ -z $mounts ]] || die "$dev is mounted (${mounts//$'\n'/, }): unmount it first, or delete the pod that uses it (make csi-undeploy)"
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
    local args=(node --device "${DEVICE:-$(data_disk)}" --node "$NODE" --listen "$LISTEN" --meta "$META"
        --pki /etc/flaminio/pki --trust-domain "$TRUST_DOMAIN" --id "$id")
    [[ ${FORMAT:-0} == 1 ]] && args+=(--format --force)
    # Zero copy: local reads unverified, for measuring (docs/performance-plan.md).
    [[ ${ZERO_COPY:-0} == 1 ]] && args+=(--zero-copy)
    [[ ${NO_VERIFY:-0} == 1 ]] && args+=(--no-verify)
    [[ ${QUEUES:-1} != 1 ]] && args+=(--queues "$QUEUES")
    [[ ${POLL_US:-0} != 0 ]] && args+=(--poll-us "$POLL_US")
    [[ -n ${GATHER_US:-} ]] && args+=(--gather-us "$GATHER_US")
    [[ ${SYNC_COMMITS:-0} == 1 ]] && args+=(--sync-commits)
    [[ ${PIPELINED_COMMITS:-0} == 1 ]] && args+=(--pipelined-commits)
    [[ ${FIRST_FIT:-0} == 1 ]] && args+=(--first-fit)
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

# Sends one command to the node's control socket and prints the answer.
control() {
    sudo python3 -c 'import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(180)
s.connect("/run/flaminio/node.sock")
s.sendall((sys.argv[1] + "\n").encode())
print(s.makefile().readline().strip())' "$1"
}

attach() {
    : "${VOLUME:?}" "${TOKEN:?}"
    systemctl is-active --quiet "$unit" || die "flaminio-node is not running"
    local answer
    answer=$(control "attach $VOLUME $TOKEN")
    [[ $answer == ok* ]] || die "attach $VOLUME: $answer"
    # udev creates the device node just after: until then, a write to the path would create a
    # regular file in /dev (in memory) and every later run would use it instead of the volume.
    local path=${answer#ok } _
    for _ in $(seq 100); do
        [[ -b $path ]] && break
        [[ -e $path ]] && die "$path exists but is not a block device: remove it"
        sleep 0.1
    done
    [[ -b $path ]] || die "$path did not appear within 10 s"
    log "attached $VOLUME: $path"
}

detach() {
    : "${VOLUME:?}"
    local answer
    answer=$(control "detach $VOLUME")
    [[ $answer == ok* || $answer == *"not attached"* ]] || die "detach $VOLUME: $answer"
}

# Deletes this node's replicas of VOLUME (already deleted from the metadata service).
delete() {
    : "${VOLUME:?}"
    systemctl is-active --quiet "$unit" || return 0
    local answer
    answer=$(control "delete $VOLUME")
    [[ $answer == ok* ]] || die "delete $VOLUME: $answer"
    log "$VOLUME: ${answer#ok }"
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
    attach) attach ;;
    detach) detach ;;
    delete) delete ;;
    *) die "usage: node.sh up|down|status|attach|detach|delete" ;;
esac
