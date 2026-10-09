#!/usr/bin/env bash
# Fault injection in a lab VM, for the chaos run. Usage:
#   chaos.sh slow on DELAY | slow off    every packet on the overlay (wg0) delayed by DELAY
#   chaos.sh kill-node                   SIGKILL flaminio-node, like a crash (its device stays)
case "${1:-}" in
    slow)
        sudo tc qdisc del dev wg0 root 2>/dev/null || true
        if [[ ${2:-off} == on ]]; then
            sudo tc qdisc add dev wg0 root netem delay "${3:?delay}"
        fi
        ;;
    kill-node)
        sudo systemctl kill --signal=KILL flaminio-node 2>/dev/null || true
        sleep 1
        sudo systemctl reset-failed flaminio-node 2>/dev/null || true
        ;;
    *) die "usage: chaos.sh slow on DELAY | slow off | kill-node" ;;
esac
