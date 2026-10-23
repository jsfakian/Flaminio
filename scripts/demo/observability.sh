#!/usr/bin/env bash
# Deploy kube-prometheus-stack (Prometheus + Grafana) and the Flaminio status exporter.
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# shellcheck source=common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

demo_require_tools
[[ -f $DEMO_DIR/active ]] || die "demo cluster not active (make demo-up)"
command -v helm >/dev/null || die "brew install helm"

export KUBECONFIG="$LOCAL_STATE_DIR/kubeconfig"

# Password stays on the workstation only (.lab/ is gitignored). Override with GRAFANA_ADMIN_PASSWORD.
pw_file="$DEMO_DIR/grafana-admin-password"
if [[ -n ${GRAFANA_ADMIN_PASSWORD:-} ]]; then
    printf '%s\n' "$GRAFANA_ADMIN_PASSWORD" >"$pw_file"
elif [[ ! -s $pw_file ]]; then
    openssl rand -base64 24 | tr -d '\n' >"$pw_file"
fi
chmod 600 "$pw_file"
grafana_pw=$(<"$pw_file")

log "adding prometheus-community helm repo"
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts >/dev/null
helm repo update >/dev/null

log "installing kube-prometheus-stack into monitoring (may take a few minutes)"
helm upgrade --install monitoring prometheus-community/kube-prometheus-stack \
    --namespace monitoring --create-namespace \
    --set "grafana.adminPassword=$grafana_pw" \
    --set grafana.service.type=NodePort \
    --set grafana.service.nodePort=30000 \
    --set prometheus.service.type=NodePort \
    --set prometheus.service.nodePort=30090 \
    --set alertmanager.enabled=false \
    --set grafana.sidecar.dashboards.enabled=true \
    --wait --timeout 10m

log "applying Flaminio status exporter + dashboards ConfigMap"
demo_kubectl apply -f "$REPO_ROOT/deploy/demo/status-exporter.yaml"
demo_kubectl apply -f "$REPO_ROOT/deploy/demo/dashboards.yaml"
demo_kubectl apply -f "$REPO_ROOT/deploy/demo/scoreboard.yaml"

# Import dashboard ConfigMaps into Grafana's sidecar label.
demo_kubectl -n monitoring label configmap flaminio-demo-dashboards grafana_dashboard=1 --overwrite >/dev/null 2>&1 || true

log "Grafana: kubectl -n monitoring port-forward svc/monitoring-grafana 3000:80  (user admin; password in $pw_file)"
log "Prometheus: kubectl -n monitoring port-forward svc/monitoring-kube-prometheus-prometheus 9090:9090"
log "Scoreboard: kubectl -n flaminio-demo port-forward svc/scoreboard 8080:80"
