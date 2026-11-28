#!/usr/bin/env bash
# The node agent's device side in a lab VM (flaminio-agent, copied to /usr/local/bin by lab.sh).
# Usage: agent.sh disks | claim
#   disks  every block device, with the claim policy's verdict
#   claim  prints the data disk: the Flaminio disk, or the one blank disk whose serial matches
#          SERIAL (default "data"), formatted first
tool="$HOME/flaminio/dataplane/target/release/flaminio-disk"
agent_bin=/usr/local/bin/flaminio-agent
if [[ ${FLAMINIO_SCRIPT_TEST:-0} == 1 ]]; then
    tool=${DISK_TOOL:-$tool}
    agent_bin=${AGENT_BIN:-$agent_bin}
fi
[[ -x $tool ]] || die "flaminio-disk not built; run make deploy"
args=(--serial "${SERIAL:-data}" --disk-tool "$tool")
case "${1:-disks}" in
    disks) sudo "$agent_bin" disks "${args[@]}" ;;
    claim) clock_preflight >/dev/null; sudo "$agent_bin" claim "${args[@]}" --label "$(hostname)" ;;
    *) die "usage: agent.sh disks|claim" ;;
esac
