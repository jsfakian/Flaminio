#!/usr/bin/env bash
# Cuts this VM off the overlay (wg0), both ways, or heals it. Traffic to its own overlay
# address stays local (lo), so its own services keep talking to each other; SSH comes in over
# the libvirt network and is not affected.
# Usage: partition.sh on | off | status. `on` prints the time the cut took effect.
tag=flaminio-partition

rules() { sudo iptables -S | grep -c -- "--comment $tag" || true; }

case "${1:-status}" in
    on)
        if [[ $(rules) == 0 ]]; then
            sudo iptables -I INPUT -i wg0 -m comment --comment "$tag" -j DROP
            sudo iptables -I OUTPUT -o wg0 -m comment --comment "$tag" -j DROP
        fi
        date +%s.%N
        ;;
    off)
        while sudo iptables -D INPUT -i wg0 -m comment --comment "$tag" -j DROP 2>/dev/null; do :; done
        while sudo iptables -D OUTPUT -o wg0 -m comment --comment "$tag" -j DROP 2>/dev/null; do :; done
        date +%s.%N
        ;;
    status) if [[ $(rules) == 0 ]]; then echo connected; else echo partitioned; fi ;;
    *) die "usage: partition.sh on|off|status" ;;
esac
