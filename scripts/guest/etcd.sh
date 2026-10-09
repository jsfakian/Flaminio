#!/usr/bin/env bash
# The metadata service's dedicated etcd (ADR 0002), one member per lab VM, on the overlay.
# Usage: etcd.sh install | up | down | status | wipe
#   install  ETCD_VERSION: official release, checked against its published SHA-256 sums
#   up       NAME, IP (overlay address), CLUSTER (name=http://ip:2380,...): start the member
# TLS 1.3 everywhere, with client certificates required from peers and clients alike. The
# certificates come from the lab's etcd CA (lab.sh pki issue), which signs only etcd members
# and metadata-service instances: nothing else can talk to etcd.

unit=/etc/systemd/system/flaminio-etcd.service
data=/var/lib/flaminio-etcd
pki=/etc/flaminio/pki
tls=(--cacert "$pki/etcd-ca.crt" --cert "$pki/etcd.crt" --key "$pki/etcd.key")

case "${1:-status}" in
    install)
        v=${ETCD_VERSION:?}
        if [[ $(etcd --version 2>/dev/null | awk '/etcd Version/ {print $3}') == "$v" ]]; then
            log "etcd $v already installed"
            exit 0
        fi
        tmp=$(mktemp -d)
        base=https://github.com/etcd-io/etcd/releases/download/v$v
        tarball=etcd-v$v-linux-amd64.tar.gz
        log "downloading etcd $v"
        curl -fsSL --retry 3 -o "$tmp/$tarball" "$base/$tarball"
        curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
        (cd "$tmp" && grep " $tarball\$" SHA256SUMS | sha256sum -c --quiet) || die "checksum mismatch for $tarball"
        tar -xzf "$tmp/$tarball" -C "$tmp"
        sudo install -m 755 "$tmp/etcd-v$v-linux-amd64/"{etcd,etcdctl,etcdutl} /usr/local/bin/
        rm -rf "$tmp"
        log "installed $(etcd --version | head -1)"
        ;;
    up)
        : "${NAME:?}" "${IP:?}" "${CLUSTER:?}"
        sudo mkdir -p "$data"
        sudo chmod 700 "$data"
        sudo tee "$unit" >/dev/null <<UNIT
[Unit]
Description=Flaminio metadata etcd member $NAME
After=network-online.target wg-quick@wg0.service
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/etcd --name $NAME --data-dir $data \\
  --listen-peer-urls https://$IP:2380 --initial-advertise-peer-urls https://$IP:2380 \\
  --listen-client-urls https://$IP:2379 --advertise-client-urls https://$IP:2379 \\
  --initial-cluster $CLUSTER --initial-cluster-state new --initial-cluster-token flaminio-lab \\
  --cert-file $pki/etcd.crt --key-file $pki/etcd.key --trusted-ca-file $pki/etcd-ca.crt --client-cert-auth \\
  --peer-cert-file $pki/etcd.crt --peer-key-file $pki/etcd.key --peer-trusted-ca-file $pki/etcd-ca.crt \\
  --peer-client-cert-auth --tls-min-version TLS1.3 \\
  --log-level warn
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
UNIT
        sudo systemctl daemon-reload
        sudo systemctl enable --quiet flaminio-etcd
        sudo systemctl restart flaminio-etcd
        log "etcd member $NAME started on $IP"
        ;;
    down)
        sudo systemctl stop flaminio-etcd 2>/dev/null || true
        log "etcd member stopped"
        ;;
    status)
        if systemctl is-active --quiet flaminio-etcd; then
            ip=$(ip -4 -o addr show wg0 | awk '{print $4}' | cut -d/ -f1)
            sudo etcdctl "${tls[@]}" --endpoints="https://$ip:2379" --command-timeout=3s endpoint status -w table 2>&1 | sed 's/^/  /' || true
        else
            echo "  flaminio-etcd: $(systemctl is-active flaminio-etcd 2>/dev/null)"
        fi
        ;;
    wipe)
        sudo systemctl disable --now --quiet flaminio-etcd 2>/dev/null || true
        sudo rm -rf "$data"
        log "etcd data removed"
        ;;
    *) die "usage: etcd.sh install|up|down|status|wipe" ;;
esac
