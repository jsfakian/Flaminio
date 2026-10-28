# shellcheck shell=bash
# Shared helpers for the lab scripts. Sourced, not executed.

if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "error: bash >= 4.4 required (found $BASH_VERSION). On macOS: brew install bash" >&2
    exit 1
fi

# Numbers with a decimal point, whatever the workstation's locale: awk and printf read
# timestamps such as 1791544816.62 as text in locales with a decimal comma.
export LC_ALL=C

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

# Long campaigns run over the internet through a jump host: tolerate short drops (2 minutes
# without an answer, 3 attempts to connect).
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10 -o ConnectionAttempts=3 -o ServerAliveInterval=15
    -o ServerAliveCountMax=8)
# Lab VMs get fresh host keys whenever they are recreated, so they use their own known_hosts,
# from which vm-destroy removes them.
VM_SSH_OPTS=("${SSH_OPTS[@]}" -o "UserKnownHostsFile=$LOCAL_STATE_DIR/known_hosts" -o StrictHostKeyChecking=accept-new)

# Expands a VM selector into VM names: a, b, w, c, all (the data VMs a and b), cluster (a, b and
# the witness w: the etcd and metadata members) or overlay (every VM on the mesh: the cluster,
# and fl-c once it exists).
vms_of() {
    case "${1:-all}" in
        a | "$VM_A") echo "$VM_A" ;;
        b | "$VM_B") echo "$VM_B" ;;
        w | "$VM_W") echo "$VM_W" ;;
        c | "$VM_C") echo "$VM_C" ;;
        all | "") echo "$VM_A $VM_B" ;;
        cluster) echo "$VM_A $VM_B $VM_W" ;;
        overlay)
            if [[ -f $LOCAL_STATE_DIR/$VM_C/created ]]; then
                echo "$VM_A $VM_B $VM_W $VM_C"
            else
                echo "$VM_A $VM_B $VM_W"
            fi
            ;;
        *) die "unknown VM '$1' (use a, b, w, c, all, cluster or overlay)" ;;
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
    case "$1" in
        "$VM_A") prefix=VM_A ;;
        "$VM_B") prefix=VM_B ;;
        "$VM_W") prefix=VM_W ;;
        "$VM_C") prefix=VM_C ;;
        *) die "unknown VM '$1'" ;;
    esac
    eval "echo \"\${${prefix}_$2}\""
}

# The address of a host on its uplink.
host_ip() { if [[ $1 == "$HOST_A" ]]; then echo "$HOST_A_IP"; else echo "$HOST_B_IP"; fi; }

# The relay (-J) for reaching a host: HOST_A_JUMP for HOST_A when set, otherwise none.
host_jump() {
    if [[ $1 == "$HOST_A" && -n $HOST_A_JUMP ]]; then
        echo "$HOST_A_JUMP"
    fi
}

# True when this machine is HOST (its uplink IP is assigned locally). Lab commands often run on
# HOST_B itself; jumping through HOST_B to a local VM then hangs (SSH to the public IP of self).
on_host() {
    local hip
    hip=$(host_ip "$1")
    ip -4 -o addr show 2>/dev/null | grep -q "inet ${hip}/"
}

# The -J value for reaching a VM: its host, preceded by the host's relay if any.
# Empty when the VM's host is this machine (SSH straight to the VM IP).
vm_jump() {
    local host jump
    host=$(vmvar "$1" HOST)
    on_host "$host" && return 0
    jump=$(host_jump "$host")
    echo "${jump:+$jump,}$host"
}

# Runs a command on a host: hsh HOST CMD...
hsh() {
    local host=$1 jump
    shift
    if on_host "$host"; then
        # Already on this host: do not SSH to the public IP (or localhost — sshd may not
        # listen on 127.0.0.1).
        bash -c "$*"
        return
    fi
    jump=$(host_jump "$host")
    ssh "${SSH_OPTS[@]}" ${jump:+-J "$jump"} "$host" "$@"
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
    if on_host "$host"; then
        # ENV_VARS holds VAR=value words: split on purpose. The arguments go as they are; $args
        # is quoted for the remote shell, which no local shell would undo.
        # shellcheck disable=SC2086
        env ${ENV_VARS:-} bash -c "$REMOTE_RUNNER" _ "$@" <"$script"
        return
    fi
    local jump
    jump=$(host_jump "$host")
    ssh "${SSH_OPTS[@]}" ${jump:+-J "$jump"} "$host" "env ${ENV_VARS:-} bash -c $(printf %q "$REMOTE_RUNNER") _$args" <"$script"
}

vssh() {
    local vm=$1 jump
    shift
    jump=$(vm_jump "$vm")
    ssh "${VM_SSH_OPTS[@]}" ${jump:+-J "$jump"} "$VM_USER@$(vmvar "$vm" IP)" "$@"
}

run_on_vm() {
    local vm=$1 script=$2
    shift 2
    local args=""
    (($#)) && args=$(printf ' %q' "$@")
    cat "$SCRIPTS/guest/common.sh" "$script" | vssh "$vm" "env ${ENV_VARS:-} bash -c $(printf %q "$REMOTE_RUNNER") _$args"
}

# Runs a guest script detached from SSH (as a transient systemd unit in the VM), so long jobs
# survive a dropped connection. Streams its output while polling, retrying through network
# errors, and returns its exit status.   run_detached VM SCRIPT [ARGS...]
run_detached() {
    local vm=$1 script=$2
    shift 2
    local unit args rc="" shown=0 out
    unit="flaminio-$(basename "$script" .sh)-$(date +%s)"
    args=""
    (($#)) && args=$(printf ' %q' "$@")
    cat "$SCRIPTS/guest/common.sh" "$script" | vssh "$vm" "mkdir -p ~/.flaminio-lab && cat > ~/.flaminio-lab/$unit.sh"
    vssh "$vm" "sudo systemd-run --quiet --unit=$unit --uid=$VM_USER --setenv=HOME=/home/$VM_USER \
        bash -c 'cd ~; env ${ENV_VARS:-} bash ~/.flaminio-lab/$unit.sh$args > ~/.flaminio-lab/$unit.log 2>&1;
                 echo \$? > ~/.flaminio-lab/$unit.rc'"
    log "$vm: running $(basename "$script") as unit $unit (survives SSH drops)"
    # shellcheck disable=SC2088 # expanded by the remote shell
    local logf="~/.flaminio-lab/$unit.log" state
    while true; do
        sleep 10
        # Only complete lines are printed while the job runs; the exit code is read in the same
        # round trip, and once it exists the rest of the log is printed.
        if ! state=$(vssh "$vm" "rc=\$(cat ~/.flaminio-lab/$unit.rc 2>/dev/null); n=\$(wc -l < $logf);
                if [ -n \"\$rc\" ]; then echo \"RC=\$rc\"; tail -n +$((shown + 1)) $logf;
                else echo \"N=\$n\"; tail -n +$((shown + 1)) $logf | head -n \$((n - $shown)); fi" 2>/dev/null); then
            warn "$vm unreachable; still waiting for $unit"
            continue
        fi
        out=${state#*$'\n'}
        [[ $out == "$state" ]] && out=""
        [[ -n $out ]] && printf '%s\n' "$out"
        case "${state%%$'\n'*}" in
            RC=*) rc=${state%%$'\n'*}; rc=${rc#RC=}; break ;;
            N=*) shown=${state%%$'\n'*}; shown=${shown#N=} ;;
        esac
    done
    return "$rc"
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
