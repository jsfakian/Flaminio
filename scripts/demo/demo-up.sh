#!/usr/bin/env bash
# Bring up the Mac control-plane demo end to end (does not deploy CSI or observability).
# Required: DEMO_CP_SRC_IP. Optional: DEMO_CP_ENDPOINT (defaults to SRC_IP:DEMO_CP_WG_PORT).
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

demo_require_tools
demo_require_src_ip

vssh "$VM_A" "ping -c 1 -W 2 -q $VM_B_WG >/dev/null" || die "lab overlay down (make mesh-up)"

"$SCRIPTS/demo/setup-mac-cp.sh"
"$SCRIPTS/demo/mesh-join-cp.sh"
"$SCRIPTS/demo/join-workers.sh"

log "demo-up complete."
log "  export KUBECONFIG=$LOCAL_STATE_DIR/kubeconfig"
log "  make csi-deploy          # Flaminio CSI on this cluster"
log "  make demo-obs            # Prometheus + Grafana + scoreboard"
log "  make demo                # run the talk track"
