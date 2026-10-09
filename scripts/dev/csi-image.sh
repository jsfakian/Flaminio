#!/usr/bin/env bash
# Builds the CSI driver's container image: flaminio-csi for linux/amd64, on the pinned base of
# image.sh. Usage: csi-image.sh OUT.tar TAG
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$1 tag=$2
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath \
    -ldflags "-X github.com/flaminio/flaminio/control/csi.Version=${tag##*:}" \
    -o "$dir/flaminio-csi" ./control/cmd/flaminio-csi
scripts/dev/image.sh "$out" "$tag" "$dir/flaminio-csi"
