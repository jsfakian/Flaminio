# shellcheck shell=bash
# Shared helpers for the lab scripts. Sourced, not executed.

if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "error: bash >= 4.4 required (found $BASH_VERSION). On macOS: brew install bash" >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS="$REPO_ROOT/scripts"
# shellcheck source=lab.conf
source "$SCRIPTS/lab.conf"
mkdir -p "$LOCAL_STATE_DIR"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() {
    printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
    exit 1
}

SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15)
# Lab VMs get fresh host keys whenever they are recreated, so they use their own known_hosts,
# from which vm-destroy removes them.
VM_SSH_OPTS=("${SSH_OPTS[@]}" -o "UserKnownHostsFile=$LOCAL_STATE_DIR/known_hosts" -o StrictHostKeyChecking=accept-new)

# Expands "a", "b" or "all" into VM names.
vms_of() {
    case "${1:-all}" in
        a | "$VM_A") echo "$VM_A" ;;
        b | "$VM_B") echo "$VM_B" ;;
        all | "") echo "$VM_A $VM_B" ;;
        *) die "unknown VM '$1' (use a, b or all)" ;;
    esac
}

# Expands "a", "b" or "all" into host names.
hosts_of() {
    case "${1:-all}" in
        a | "$VM_A" | "$HOST_A") echo "$HOST_A" ;;
        b | "$VM_B" | "$HOST_B") echo "$HOST_B" ;;
        all | "") echo "$HOST_A $HOST_B" ;;
        *) die "unknown host '$1' (use a, b or all)" ;;
    esac
}

# Per-VM setting: vmvar VM FIELD, e.g. vmvar fl-a CPUS -> $VM_A_CPUS.
vmvar() {
    local prefix
    if [[ $1 == "$VM_A" ]]; then prefix=VM_A; else prefix=VM_B; fi
    eval "echo \"\${${prefix}_$2}\""
}

# Runs a command on a host: hsh HOST CMD...
hsh() {
    local host=$1
    shift
    ssh "${SSH_OPTS[@]}" "$host" "$@"
}

# Runs a local script file on a host or VM with arguments. The script is copied to a temporary
# file first, so commands inside it cannot swallow the rest of the script from stdin.
#   run_on_host HOST SCRIPT [ARGS...]     (environment: pass VAR=value pairs in ENV_VARS)
#   run_on_vm VM SCRIPT [ARGS...]         (scripts/guest/common.sh is prepended)
REMOTE_RUNNER='f=$(mktemp); cat >"$f"; bash "$f" "$@"; rc=$?; rm -f "$f"; exit $rc'

run_on_host() {
    local host=$1 script=$2
    shift 2
    local args=""
    (($#)) && args=$(printf ' %q' "$@")
    ssh "${SSH_OPTS[@]}" "$host" "env ${ENV_VARS:-} bash -c $(printf %q "$REMOTE_RUNNER") _$args" <"$script"
}

vssh() {
    local vm=$1
    shift
    ssh "${VM_SSH_OPTS[@]}" -J "$(vmvar "$vm" HOST)" "$VM_USER@$(vmvar "$vm" IP)" "$@"
}

run_on_vm() {
    local vm=$1 script=$2
    shift 2
    local args=""
    (($#)) && args=$(printf ' %q' "$@")
    cat "$SCRIPTS/guest/common.sh" "$script" | vssh "$vm" "env ${ENV_VARS:-} bash -c $(printf %q "$REMOTE_RUNNER") _$args"
}

# Renders a template, replacing {{NAME}} with the value of $NAME.
render() {
    local text name isset
    text=$(<"$1")
    for name in $(grep -o '{{[A-Z_][A-Z0-9_]*}}' "$1" | sort -u | tr -d '{}'); do
        eval "isset=\${$name+x}"
        [[ -n $isset ]] || die "template $1 uses unset variable $name"
        text=${text//"{{$name}}"/$(eval "printf '%s' \"\$$name\"")}
    done
    printf '%s\n' "$text"
}

# Waits until a command succeeds: wait_for DESCRIPTION TIMEOUT_SECONDS CMD...
wait_for() {
    local what=$1 timeout=$2 start=$SECONDS
    shift 2
    log "waiting for $what"
    until "$@" >/dev/null 2>&1; do
        ((SECONDS - start < timeout)) || die "timed out after ${timeout}s waiting for $what"
        sleep 5
    done
}
