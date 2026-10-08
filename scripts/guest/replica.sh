#!/usr/bin/env bash
# The replica service in a lab VM (MVP step 5). Usage: replica.sh up|down|status
#   up      start "flaminio-io replica" as the systemd service "flaminio-replica" on the data disk,
#           listening on REPL_PORT (FORMAT=1 reformats the disk first)
bin="$HOME/flaminio/dataplane/target/release/flaminio-io"
port=${REPL_PORT:-7410}

case "${1:-status}" in
    up)
        [[ -x $bin ]] || die "flaminio-io not built; run make deploy"
        if systemctl is-active --quiet flaminio-replica; then
            log "flaminio-replica already running"
            exit 0
        fi
        format=()
        [[ ${FORMAT:-0} == 1 ]] && format=(--format --force)
        sudo systemctl reset-failed flaminio-replica 2>/dev/null || true
        sudo systemd-run --quiet --unit=flaminio-replica --property=Restart=no \
            "$bin" replica --device "$(data_disk)" --listen "0.0.0.0:$port" "${format[@]}"
        for _ in $(seq 1 30); do
            ss -ltn | grep -q ":$port " && break
            systemctl is-active --quiet flaminio-replica || { sudo journalctl -u flaminio-replica --no-pager | tail -20 >&2; die "flaminio-replica exited"; }
            sleep 1
        done
        ss -ltn | grep -q ":$port " || die "replica not listening on $port"
        log "replica listening on :$port"
        ;;
    down)
        systemctl is-active --quiet flaminio-replica && sudo systemctl stop flaminio-replica
        log "flaminio-replica stopped"
        ;;
    status)
        systemctl is-active flaminio-replica || true
        sudo journalctl -u flaminio-replica --no-pager -o cat | tail -5
        ;;
    *) die "usage: replica.sh up|down|status" ;;
esac
