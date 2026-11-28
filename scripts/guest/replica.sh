#!/usr/bin/env bash
# The replica service in a lab VM (MVP step 5). Usage: replica.sh up|down|status
#   up      refuses the unsupported MVP replica path before preparation

case "${1:-status}" in
    up)
        die "legacy MVP replica is unsupported; use node.sh up with a qualified physical clock contract"
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
