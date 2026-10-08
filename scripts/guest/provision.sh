#!/usr/bin/env bash
# Prepares a lab VM: waits for cloud-init, makes sure ublk is available, installs Rust.

log "waiting for cloud-init"
cloud-init status --wait >/dev/null 2>&1 || log "cloud-init reported errors; continuing ($(cloud-init status 2>&1 | tail -1))"

missing=()
for pkg in build-essential pkg-config fio jq netcat-openbsd rsync curl; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
done
if ((${#missing[@]})); then
    log "installing ${missing[*]}"
    sudo apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${missing[@]}" >/dev/null
fi

if ! sudo modprobe ublk_drv 2>/dev/null; then
    log "ublk_drv not in the base kernel modules; installing linux-modules-extra"
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "linux-modules-extra-$(uname -r)" >/dev/null
    sudo modprobe ublk_drv
fi

if [[ ! -x $HOME/.cargo/bin/cargo ]]; then
    log "installing Rust (stable, minimal)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
fi

disk=$(data_disk)
cat <<SUMMARY
  kernel     $(uname -r)
  cpus       $(nproc)
  memory     $(free -g | awk '/Mem:/ {print $2}') GiB
  ublk       $(test -e /dev/ublk-control && echo "ready (/dev/ublk-control)" || echo MISSING)
  io_uring   $(sysctl -n kernel.io_uring_disabled 2>/dev/null | sed 's/^0$/enabled/;s/^[12]$/DISABLED/')
  rust       $(cargo --version)
  fio        $(fio --version)
  data disk  $disk $(lsblk -dn -o SIZE "$disk") (rotational=$(lsblk -dn -o ROTA "$disk"))
SUMMARY
