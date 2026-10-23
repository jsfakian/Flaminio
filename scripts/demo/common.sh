# shellcheck shell=bash
# Shared helpers for scripts/demo/*. Source after scripts/lib.sh (or via demo.sh).
: "${DEMO_DIR:=$LOCAL_STATE_DIR/demo}"
mkdir -p "$DEMO_DIR"

demo_require_tools() {
    local missing=()
    command -v limactl >/dev/null || missing+=(lima)
    command -v kubectl >/dev/null || missing+=(kubectl)
    command -v jq >/dev/null || missing+=(jq)
    ((${#missing[@]} == 0)) || die "install on this Mac: brew install ${missing[*]} (see docs/demo.md)"
}

demo_require_src_ip() {
    [[ -n ${DEMO_CP_SRC_IP:-} ]] || die "set DEMO_CP_SRC_IP to this Mac's address as rhythm/rhythm-backup see it (campus or VPN)"
    if [[ -z ${DEMO_CP_ENDPOINT:-} ]]; then
        DEMO_CP_ENDPOINT="${DEMO_CP_SRC_IP}:${DEMO_CP_WG_PORT}"
        export DEMO_CP_ENDPOINT
    fi
}

demo_kubectl() {
    command kubectl --kubeconfig="$LOCAL_STATE_DIR/kubeconfig" "$@"
}

demo_lima() {
    limactl shell "$DEMO_CP_LIMA" -- "$@"
}

say() { printf '\n\033[1;35m##\033[0m %s\n' "$*" >&2; }
prompt() {
    if [[ ${DEMO_AUTO:-0} == 1 ]]; then
        log "(auto) $*"
        return
    fi
    read -r -p "$* [Enter] " _
}
