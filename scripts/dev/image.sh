#!/usr/bin/env bash
# Builds a container image without a container engine: the given files, as /usr/local/bin/*,
# appended as one layer to a pinned Ubuntu base (which brings a shell, mount, blkid and
# mkfs.ext4). Writes a docker-format tarball for `k3s ctr images import`.
# Usage: image.sh OUT.tar TAG FILE...
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$1 tag=$2
shift 2
base=docker.io/library/ubuntu:resolute-20260912
crane() { go run github.com/google/go-containerregistry/cmd/crane@v0.22.0 "$@"; }

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
# Owned by root, whatever the builder's user; no timestamps from this machine.
python3 - "$dir/layer.tar" "$@" <<'PY'
import os, sys, tarfile
out, files = sys.argv[1], sys.argv[2:]
with tarfile.open(out, "w", format=tarfile.PAX_FORMAT) as t:
    for d in ["usr", "usr/local", "usr/local/bin"]:
        info = tarfile.TarInfo(d)
        info.type, info.mode, info.mtime = tarfile.DIRTYPE, 0o755, 0
        t.addfile(info)
    for f in files:
        info = t.gettarinfo(f, arcname="usr/local/bin/" + os.path.basename(f))
        info.uid = info.gid = 0
        info.uname = info.gname = "root"
        info.mtime, info.mode = 0, 0o755
        with open(f, "rb") as fh:
            t.addfile(info, fh)
PY
# PLATFORM_ARCH is the image's arch. Clear GOARCH for `go run crane` (host binary).
arch=${PLATFORM_ARCH:-${GOARCH:-amd64}}
unset GOARCH GOOS GOARM
crane append --platform "linux/$arch" -b "$base" -f "$dir/layer.tar" -t "$tag" -o "$out" >/dev/null
echo "$out: $tag on $base (linux/$arch)"
