#!/usr/bin/env bash
# A phase-1 node in a lab VM: flaminio-io node as the systemd service "flaminio-node".
# Usage: node.sh clock-check | challenge | up | down | status | attach | detach | delete
#   up      NODE, LISTEN, META, TRUST_DOMAIN; CLOCK_CONTRACT selects the operator file;
#           FORMAT=1 reformats the data disk; DEVICE overrides the disk the agent claimed;
#           ZERO_COPY=1 serves local reads
#           without copies (and without checksum verification)
#   attach  VOLUME, NODE, META: obtain a process-bound handoff as this node's agent;
#           at /dev/ublkb1, through its control socket (no restart)
#   detach  VOLUME; prints one line: "detached" or "not attached" (exit 0), "not running" (exit 3)
#   delete  VOLUME; prints one line: "deleted <n>" (exit 0), "not running" (exit 3)
# detach and delete stop with exit 1 on anything else; their log goes to stderr.
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
    [[ -z ${ATTACH:-} && -z ${TOKEN:-} ]] || die "legacy startup ATTACH/TOKEN refused; start unattached, then node.sh attach"
    clock_preflight >/dev/null
    sudo modprobe ublk_drv
    systemctl is-active --quiet "$unit" && down
    local args=(node --device "${DEVICE:-$(data_disk)}" --node "$NODE" --listen "$LISTEN" --meta "$META"
        --pki /etc/flaminio/pki --trust-domain "$TRUST_DOMAIN" --id "$id"
        --clock-contract "${CLOCK_CONTRACT:-/etc/flaminio/clock-contract.json}")
    [[ ${FORMAT:-0} == 1 ]] && args+=(--format --force)
    # Zero copy: local reads unverified, for measuring (docs/performance-plan.md).
    [[ ${ZERO_COPY:-0} == 1 ]] && args+=(--zero-copy)
    [[ ${NO_VERIFY:-0} == 1 ]] && args+=(--no-verify)
    [[ ${QUEUES:-1} != 1 ]] && args+=(--queues "$QUEUES")
    [[ ${POLL_US:-0} != 0 ]] && args+=(--poll-us "$POLL_US")
    [[ -n ${GATHER_US:-} ]] && args+=(--gather-us "$GATHER_US")
    [[ -n ${COMMIT_TARGET_MIB:-} ]] && args+=(--commit-target-mib "$COMMIT_TARGET_MIB")
    [[ -n ${FLUSH_COALESCE:-} ]] && args+=(--flush-coalesce "$FLUSH_COALESCE")
    [[ ${SYNC_COMMITS:-0} == 1 ]] && args+=(--sync-commits)
    [[ ${PIPELINED_COMMITS:-0} == 1 ]] && args+=(--pipelined-commits)
    [[ ${FIRST_FIT:-0} == 1 ]] && args+=(--first-fit)
    [[ ${DEFER_TASKRUN:-0} == 1 ]] && args+=(--defer-taskrun)
    sudo systemd-run --quiet --unit="$unit" --property=Restart=no "$bin" "${args[@]}"
    local ready="serving replicas"
    for _ in $(seq 1 120); do
        journal | grep -E "$ready" >/dev/null && break
        systemctl is-active --quiet "$unit" || { journal | tail -20 >&2; die "flaminio-node exited"; }
        sleep 0.5
    done
    log "$(journal | grep -E 'store ready|target of|serving' | tail -3 | tr '\n' ' ')"
}

attach() {
    volume_id
    [[ -z ${TOKEN:-} && -z ${HANDOFF:-} ]] || die "standalone credentials refused; use NODE/META for an authorized challenge handoff"
    systemctl is-active --quiet "$unit" || die "flaminio-node is not running"
    local path
    path=$(attach_target) || return
    # udev creates the device node just after: until then, a write to the path would create a
    # regular file in /dev (in memory) and every later run would use it instead of the volume.
    local _
    for _ in $(seq 100); do
        [[ -b $path ]] && break
        [[ -e $path ]] && die "$path exists but is not a block device: remove it"
        sleep 0.1
    done
    [[ -b $path ]] || die "$path did not appear within 10 s"
    log "attached $VOLUME: $path"
}

# A node that is not running: says so, and exits 3.
running() {
    systemctl is-active --quiet "$unit" && return
    echo "not running"
    exit 3
}

# VOLUME goes into the control socket's line: ids only (domain.ValidID).
volume_id() {
    : "${VOLUME:?}"
    [[ $VOLUME =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$ ]] || die "'$VOLUME' is not a volume id"
}

detach() {
    volume_id
    running
    local answer
    answer=$(control "detach $VOLUME")
    case $answer in
        ok*) echo detached ;;
        *"not attached"*) echo "not attached" ;;
        *) die "detach $VOLUME: $answer" ;;
    esac
}

# Deletes this node's replicas of VOLUME (already deleted from the metadata service).
delete() {
    volume_id
    running
    local answer
    answer=$(control "delete $VOLUME")
    [[ $answer =~ ^ok\ ([0-9]+)\ replicas\ deleted$ ]] || die "delete $VOLUME: $answer"
    log "$VOLUME: ${answer#ok }"
    echo "deleted ${BASH_REMATCH[1]}"
}

status() {
    echo "  flaminio-node: $(systemctl is-active "$unit" 2>/dev/null)"
    [[ -b $dev ]] && echo "  $dev: $(lsblk -dn -o SIZE "$dev")"
    journal | tail -4 | sed 's/^/  /'
}

case "${1:-status}" in
    clock-check) clock_preflight ;;
    challenge) attach_challenge ;;
    up) up ;;
    down) down ;;
    status) status ;;
    attach) attach ;;
    detach) detach ;;
    delete) delete ;;
    *) die "usage: node.sh clock-check|challenge|up|down|status|attach|detach|delete" ;;
esac
