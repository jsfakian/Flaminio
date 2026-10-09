#!/usr/bin/env bash
# The metadata service, beside each etcd member. Usage: meta.sh up | down | status
#   up   LISTEN (overlay address:port), ETCD (comma-separated endpoints), TRUST_DOMAIN; the
#        binary was copied to ~/flaminio-meta by lab.sh
# It runs as an unprivileged, sandboxed systemd service. Its keys and certificates stay
# root-only on disk (/etc/flaminio/pki) and reach it as systemd credentials.

unit=/etc/systemd/system/flaminio-meta.service

case "${1:-status}" in
    up)
        : "${LISTEN:?}" "${ETCD:?}" "${TRUST_DOMAIN:?}"
        sudo install -m 755 ~/flaminio-meta /usr/local/bin/flaminio-meta
        sudo tee "$unit" >/dev/null <<UNIT
[Unit]
Description=Flaminio metadata service
After=network-online.target flaminio-etcd.service
Wants=network-online.target

[Service]
LoadCredential=meta.crt:/etc/flaminio/pki/meta.crt
LoadCredential=meta.key:/etc/flaminio/pki/meta.key
LoadCredential=ca.crt:/etc/flaminio/pki/ca.crt
LoadCredential=meta-etcd.crt:/etc/flaminio/pki/meta-etcd.crt
LoadCredential=meta-etcd.key:/etc/flaminio/pki/meta-etcd.key
LoadCredential=etcd-ca.crt:/etc/flaminio/pki/etcd-ca.crt
LoadCredential=token.key:/etc/flaminio/pki/token.key
ExecStart=/usr/local/bin/flaminio-meta serve --listen $LISTEN --etcd $ETCD --trust-domain $TRUST_DOMAIN \\
  --token-key \${CREDENTIALS_DIRECTORY}/token.key \\
  --tls-cert \${CREDENTIALS_DIRECTORY}/meta.crt --tls-key \${CREDENTIALS_DIRECTORY}/meta.key \\
  --tls-ca \${CREDENTIALS_DIRECTORY}/ca.crt \\
  --etcd-cert \${CREDENTIALS_DIRECTORY}/meta-etcd.crt --etcd-key \${CREDENTIALS_DIRECTORY}/meta-etcd.key \\
  --etcd-ca \${CREDENTIALS_DIRECTORY}/etcd-ca.crt
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
        sudo systemctl reset-failed flaminio-meta 2>/dev/null || true # after a crash loop
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
