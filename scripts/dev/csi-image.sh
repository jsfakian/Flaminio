#!/usr/bin/env bash
# Builds the CSI driver's container image without a container engine: flaminio-csi, built for
# linux/amd64, appended as one layer to a pinned Ubuntu base (it brings mount, blkid and
# mkfs.ext4). Writes a docker-format tarball for `k3s ctr images import`.
# Usage: csi-image.sh OUT.tar TAG
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$1 tag=$2
base=docker.io/library/ubuntu:resolute-20260912
crane() { go run github.com/google/go-containerregistry/cmd/crane@v0.22.0 "$@"; }

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
mkdir -p "$dir/root/usr/local/bin"
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath \
    -ldflags "-X github.com/flaminio/flaminio/control/csi.Version=${tag##*:}" \
    -o "$dir/root/usr/local/bin/flaminio-csi" ./control/cmd/flaminio-csi
# Owned by root, whatever the builder's user; no timestamps from this machine.
python3 - "$dir/root" "$dir/layer.tar" <<'PY'
import os, sys, tarfile
root, out = sys.argv[1], sys.argv[2]
with tarfile.open(out, "w", format=tarfile.PAX_FORMAT) as t:
    for path in ["usr", "usr/local", "usr/local/bin", "usr/local/bin/flaminio-csi"]:
        info = t.gettarinfo(os.path.join(root, path), arcname=path)
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        info.mtime = 0
        info.mode = 0o755
        if info.isfile():
            with open(os.path.join(root, path), "rb") as f:
                t.addfile(info, f)
        else:
            t.addfile(info)
PY
crane append --platform linux/amd64 -b "$base" -f "$dir/layer.tar" -t "$tag" -o "$out" >/dev/null
echo "$out: $tag on $base"
