#!/usr/bin/env bash
# Lab check for follow-ups §8: CYCLES PVCs (RF 2, 1 GiB, StorageClass flaminio), each used by a
# pod that writes 16 MiB through its filesystem on fl-a or fl-b in turn, then deleted. Fails if
# the stores of fl-a and fl-b do not end with the chunk replicas and clusters they started with.
# The data plane reports its store only when it starts, so both nodes restart before and after:
# nothing may be attached on them.
#   CYCLES=100 scripts/checks/pvc-loop.sh
set -euo pipefail
# shellcheck source=../lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
# Helpers of lab.sh, which runs commands when sourced.
# shellcheck source=/dev/null
source <(sed -n '/^kubectl_w()/,/^}/p;/^demo_active()/,/^}/p;/^kubectl_lab()/,/^}/p;/^kubectl_apply()/,/^}/p;/^etcd_endpoints()/,/^}/p;/^meta_endpoints()/,/^}/p;/^node_env()/,/^}/p' "$SCRIPTS/lab.sh")
CYCLES=${CYCLES:-100}
NS=flaminio-pvc-loop

store() { # "<replicas> <clusters>" for each data VM, read as its data plane restarts
    local vm
    for vm in $(vms_of all); do
        run_on_vm "$vm" "$SCRIPTS/guest/node.sh" down >/dev/null
        ENV_VARS=$(node_env "$vm") run_on_vm "$vm" "$SCRIPTS/guest/node.sh" up 2>&1 |
            sed -n 's/.*store ready, \([0-9]*\) chunk replicas, \([0-9]*\) of .*/'"$vm"' \1 \2/p'
    done
}

manifest() { # name node
    cat <<EOF
apiVersion: v1
kind: PersistentVolumeClaim
metadata: {name: $1, namespace: $NS}
spec:
  accessModes: [ReadWriteOncePod]
  storageClassName: flaminio
  resources: {requests: {storage: 1Gi}}
---
apiVersion: v1
kind: Pod
metadata: {name: $1, namespace: $NS}
spec:
  restartPolicy: Never
  nodeSelector: {kubernetes.io/hostname: $2}
  securityContext: {runAsNonRoot: true, runAsUser: 65534, fsGroup: 65534, seccompProfile: {type: RuntimeDefault}}
  containers:
    - name: w
      image: busybox:1.37
      command: [sh, -c, "head -c 16777216 /dev/urandom > /data/blob && sync"]
      securityContext: {allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, capabilities: {drop: [ALL]}}
      volumeMounts: [{name: data, mountPath: /data}]
  volumes: [{name: data, persistentVolumeClaim: {claimName: $1}}]
EOF
}

before=$(store)
log "before: $(tr '\n' ';' <<<"$before")"
printf 'apiVersion: v1\nkind: Namespace\nmetadata:\n  name: %s\n  labels: {pod-security.kubernetes.io/enforce: restricted}\n' "$NS" |
    kubectl_apply >/dev/null
trap 'kubectl_lab delete namespace "$NS" --wait=false >/dev/null 2>&1 || true' EXIT
start=$SECONDS
for i in $(seq 1 "$CYCLES"); do
    node=$VM_A
    ((i % 2 == 0)) && node=$VM_B
    manifest "c$i" "$node" | kubectl_apply >/dev/null
    kubectl_lab wait -n "$NS" --for=jsonpath='{.status.phase}'=Succeeded "pod/c$i" --timeout=300s >/dev/null ||
        die "cycle $i: the pod did not succeed: $(kubectl_lab describe pod -n "$NS" "c$i" | tail -8)"
    pv=$(kubectl_lab get pvc -n "$NS" "c$i" -o jsonpath='{.spec.volumeName}')
    kubectl_lab delete pod -n "$NS" "c$i" --wait=true >/dev/null
    kubectl_lab delete pvc -n "$NS" "c$i" --wait=true >/dev/null
    kubectl_lab wait --for=delete "pv/$pv" --timeout=300s >/dev/null 2>&1 || true
    ((i % 10 == 0)) && log "$i cycles in $((SECONDS - start)) s"
done
# The node plugins sweep deleted volumes every 5 s.
sleep 30
after=$(store)
log "after:  $(tr '\n' ';' <<<"$after")"
[[ $after == "$before" ]] || die "the stores did not return to where they started"
log "OK: $CYCLES PVC cycles left the stores where they started"
