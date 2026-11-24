#!/usr/bin/env bash
# Checks issue #82 failover policy seams used by lab.sh and the default observe-only path:
# unsupported providers refuse before destructive action; csi_failover does not power off;
# cleanup must not auto-delete foreign marks. Run by `make lint`.
if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "failover-policy-test: needs bash >= 4.4 (found $BASH_VERSION); skipped"
    exit 0
fi
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
failed=0
fail() {
    echo "FAIL: $*"
    failed=1
}
ok() { echo "ok: $*"; }

# csi_failover must die before virsh destroy / mark waits (no unchecked power sample).
if ! grep -q 'csi-failover refused (issue #82)' "$ROOT/scripts/lab.sh"; then
    fail "lab.sh csi_failover missing issue #82 refusal"
else
    ok "lab.sh csi_failover refuses before power/mutation"
fi
if grep -n 'csi_failover' -A80 "$ROOT/scripts/lab.sh" | grep -E 'virsh.*destroy|out-of-service'; then
    # After the function's die line, no destructive body should remain.
    body=$(awk '
        /^csi_failover\(\)/ {on=1; next}
        on && /^[a-zA-Z_][a-zA-Z0-9_]*\(\)/ {exit}
        on {print}
    ' "$ROOT/scripts/lab.sh")
    if grep -E 'virsh.*destroy|until.*out-of-service' <<<"$body"; then
        fail "csi_failover still contains power-off or mark-wait loops"
    else
        ok "csi_failover body has no power-off or mark-wait loops"
    fi
else
    ok "no destructive virsh/mark waits adjacent to csi_failover"
fi

# Default deployment must not pass an authority config (observe-only).
if grep -E -- '--failover-authority-config=' "$ROOT/deploy/csi/driver.yaml" | grep -v '#'; then
    fail "driver.yaml enables --failover-authority-config by default"
else
    ok "flaminio-failover default args stay observe-only"
fi

# RBAC must not grant pod or VolumeAttachment writes (scoped recovery unavailable).
rbac=$ROOT/deploy/csi/rbac.yaml
# Extract the flaminio-failover ClusterRole rules block (skip comments).
role=$(awk '
    $0 == "kind: ClusterRole" {c=1; next}
    c && /name: flaminio-failover/ {on=1; next}
    on && /^kind:/ {exit}
    on && $0 !~ /^[[:space:]]*#/ {print}
' "$rbac")
if grep -E 'resources:.*\[.*(pods|volumeattachments)' <<<"$role"; then
    fail "flaminio-failover RBAC grants pods or VolumeAttachments"
else
    ok "flaminio-failover RBAC has no pod/VolumeAttachment verbs"
fi
if ! grep -q 'resourceNames: \[kube-system\]' <<<"$role" && ! grep -A2 'resources: \[namespaces\]' <<<"$role" | grep -q kube-system; then
    fail "flaminio-failover RBAC missing kube-system namespace get"
else
    ok "flaminio-failover RBAC allows kube-system namespace get"
fi

# Unsupported profile names must be rejected in production code (not only docs).
if ! grep -q 'unsupported failover authority profile' "$ROOT/control/failover/manual_lockout.go"; then
    fail "manual_lockout.go missing unsupported profile refusal"
else
    ok "unsupported authority profiles refuse at config load"
fi

# Foreign/legacy marks must not be auto-cleared by Ready+heartbeat.
if ! grep -q 'Ready + heartbeat never supply a return permit' "$ROOT/control/failover/failover.go"; then
    fail "failover.go missing return-permit cleanup gate comment"
else
    ok "cleanup requires return permit, not Ready+heartbeat"
fi

# Makefile help must not claim automatic VM mark/unmark success for csi-failover.
help=$(grep '^csi-failover:' "$ROOT/Makefile" || true)
if grep -qiE 'mark|rejoins|comes back' <<<"$help"; then
    fail "Makefile csi-failover help still claims automatic mark/rejoin"
else
    ok "Makefile csi-failover help matches refusal"
fi

if ((failed)); then
    echo "failover-policy-test: $failed failure(s)"
    exit 1
fi
echo "failover-policy-test: ok"
