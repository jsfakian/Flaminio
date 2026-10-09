#!/usr/bin/env bash
# This VM's keys and certificates, in /etc/flaminio/pki (root only). Usage:
#   pki.sh csr NAME       create NAME.key here if needed (ECDSA P-256), print a CSR for it
#   pki.sh install NAME   install NAME.crt from CERT (base64 PEM)
#   pki.sh file NAME      install a file from DATA (base64), mode MODE (default 0644)
# Private keys are made here and never leave the VM.

dir=/etc/flaminio/pki
sudo install -d -m 755 /etc/flaminio
sudo install -d -m 700 "$dir"

case "${1:-}" in
    csr)
        name=${2:?}
        if ! sudo test -s "$dir/$name.key"; then
            sudo sh -c "umask 077; openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt -out '$dir/$name.key'"
        fi
        sudo openssl req -new -key "$dir/$name.key" -subj "/CN=$name"
        ;;
    install)
        name=${2:?}
        echo "${CERT:?}" | base64 -d | sudo tee "$dir/$name.crt" >/dev/null
        sudo chmod 644 "$dir/$name.crt"
        ;;
    file)
        name=${2:?}
        echo "${DATA:?}" | base64 -d | sudo tee "$dir/$name" >/dev/null
        sudo chmod "${MODE:-644}" "$dir/$name"
        ;;
    *) die "usage: pki.sh csr|install|file NAME" ;;
esac
