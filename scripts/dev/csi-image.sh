#!/usr/bin/env bash
# Builds the CSI driver's container image: flaminio-csi for linux/$GOARCH (default amd64), on
# the pinned base of image.sh. Usage: csi-image.sh OUT.tar TAG
# The Mac demo CP is arm64 (Lima); lab VMs are amd64 — set GOARCH=arm64 for the CP import.
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$1 tag=$2
arch=${GOARCH:-amd64}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
GOOS=linux GOARCH=$arch CGO_ENABLED=0 go build -trimpath \
    -ldflags "-X github.com/flaminio/flaminio/control/csi.Version=${tag##*:}" \
    -o "$dir/flaminio-csi" ./control/cmd/flaminio-csi
PLATFORM_ARCH=$arch scripts/dev/image.sh "$out" "$tag" "$dir/flaminio-csi"
