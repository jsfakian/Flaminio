#!/usr/bin/env bash
# The node agent's device side in a lab VM (flaminio-agent, copied to /usr/local/bin by lab.sh).
# Usage: agent.sh disks | claim
#   disks  every block device, with the claim policy's verdict
#   claim  prints the data disk: the Flaminio disk, or the one blank disk whose serial matches
#          SERIAL (default "data"), formatted first
tool="$HOME/flaminio/dataplane/target/release/flaminio-disk"
[[ -x $tool ]] || die "flaminio-disk not built; run make deploy"
args=(--serial "${SERIAL:-data}" --disk-tool "$tool")
case "${1:-disks}" in
    disks) sudo /usr/local/bin/flaminio-agent disks "${args[@]}" ;;
    claim) sudo /usr/local/bin/flaminio-agent claim "${args[@]}" --label "$(hostname)" ;;
    *) die "usage: agent.sh disks|claim" ;;
esac
