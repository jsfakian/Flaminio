#!/usr/bin/env bash
# The metadata service, beside each etcd member. Usage: meta.sh up | down | status
#   up   LISTEN (overlay address:port), ETCD (comma-separated endpoints); the binary was
#        copied to ~/flaminio-meta by lab.sh
# It runs as an unprivileged, sandboxed systemd service.

unit=/etc/systemd/system/flaminio-meta.service

case "${1:-status}" in
    up)
        : "${LISTEN:?}" "${ETCD:?}"
        sudo install -m 755 ~/flaminio-meta /usr/local/bin/flaminio-meta
        sudo tee "$unit" >/dev/null <<UNIT
[Unit]
Description=Flaminio metadata service
After=network-online.target flaminio-etcd.service
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/flaminio-meta serve --listen $LISTEN --etcd $ETCD
Restart=on-failure
RestartSec=2
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
SystemCallFilter=@system-service

[Install]
WantedBy=multi-user.target
UNIT
        sudo systemctl daemon-reload
        sudo systemctl enable --quiet flaminio-meta
        sudo systemctl restart flaminio-meta
        log "flaminio-meta listening on $LISTEN"
        ;;
    down)
        sudo systemctl stop flaminio-meta 2>/dev/null || true
        log "flaminio-meta stopped"
        ;;
    status) echo "  flaminio-meta: $(systemctl is-active flaminio-meta 2>/dev/null)" ;;
    *) die "usage: meta.sh up|down|status" ;;
esac
