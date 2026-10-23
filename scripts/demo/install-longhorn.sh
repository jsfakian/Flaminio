#!/usr/bin/env bash
# Optional: install Longhorn on the demo cluster for Layer B comparison. Needs free disk on
# fl-a/fl-b (Longhorn uses host filesystem paths — disclose that in the talk).
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

[[ -f $DEMO_DIR/active ]] || die "demo cluster not active"
export KUBECONFIG="$LOCAL_STATE_DIR/kubeconfig"
command -v helm >/dev/null || die "brew install helm"

# Longhorn pods must run on storage nodes; tolerate nothing special beyond that.
helm repo add longhorn https://charts.longhorn.io >/dev/null
helm repo update >/dev/null
log "installing Longhorn $LONGHORN_VERSION"
helm upgrade --install longhorn longhorn/longhorn \
    --namespace longhorn-system --create-namespace \
    --version "${LONGHORN_VERSION#v}" \
    --set defaultSettings.defaultReplicaCount=2 \
    --set persistence.defaultClass=false \
    --wait --timeout 15m || warn "helm wait timed out; check kubectl -n longhorn-system get pods"

demo_kubectl apply -f "$REPO_ROOT/deploy/demo/longhorn-workload.yaml"
log "Longhorn installed. StorageClass: longhorn. Demo twin workload in longhorn-demo namespace."
