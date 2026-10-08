#!/usr/bin/env bash
# Flaminio test lab: two servers, one libvirt/KVM VM on each. See docs/mvp-plan.md.
# Usually driven through the Makefile (`make help`). Usage: scripts/lab.sh COMMAND [ARGS...]
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# Variables the host scripts need, as `env` arguments.
host_env() {
    local vm=${1:-} pairs=() v
    for v in LIBVIRT_POOL LIBVIRT_POOL_PATH LIBVIRT_NET LIBVIRT_BRIDGE HOST_STATE_DIR UBUNTU_CODENAME \
        IMAGE_URL VM_ROOT_GIB VM_DATA_GIB HOST_A_IP HOST_B_IFACE REPL_PORT; do
        pairs+=("$v=$(printf %q "${!v}")")
    done
    if [[ -n $vm ]]; then
        pairs+=("VM=$vm")
        for v in CPUS MEM_GIB MAC IP; do
            pairs+=("VM_$v=$(printf %q "$(vmvar "$vm" "$v")")")
        done
    fi
    echo "${pairs[*]}"
}

vm_state() { hsh "$(vmvar "$1" HOST)" "LC_ALL=C virsh -c qemu:///system domstate $1 2>/dev/null || echo absent"; }
vm_is_off() { [[ $(vm_state "$1") == "shut off" ]]; }

cmd_check() {
    local host
    for host in $(hosts_of "${1:-all}"); do
        log "$host"
        hsh "$host" 'printf "  %-10s %s\n" os "$(. /etc/os-release; echo $PRETTY_NAME)" kernel "$(uname -r)" \
            cpus "$(nproc)" memory "$(free -g | awk "/Mem:/ {print \$2}") GiB" \
            kvm "$(test -e /dev/kvm && echo yes || echo NO)" \
            sudo "$(sudo -n true 2>/dev/null && echo passwordless || echo "needs password")" \
            libvirt "$(command -v virsh >/dev/null && virsh -c qemu:///system version --daemon 2>/dev/null | awk "/daemon/ {print \$NF}" || echo "not installed")" \
            "free disk" "$(df -h /var/lib/libvirt 2>/dev/null | awk "NR==2 {print \$4}" | grep . || df -h / | awk "NR==2 {print \$4}")"' ||
            warn "cannot reach $host"
    done
}

cmd_host_prep() {
    local host
    for host in $(hosts_of "${1:-all}"); do
        log "preparing $host"
        ENV_VARS=$(host_env) run_on_host "$host" "$SCRIPTS/host/prep.sh"
    done
}

cmd_create() {
    local vm host dir
    [[ -r $SSH_PUBKEY_FILE ]] || die "SSH public key $SSH_PUBKEY_FILE not found (set SSH_PUBKEY_FILE)"
    for vm in $(vms_of "${1:-all}"); do
        host=$(vmvar "$vm" HOST)
        log "creating $vm on $host"
        dir="$LOCAL_STATE_DIR/$vm"
        mkdir -p "$dir"
        # Template variables.
        VM=$vm VM_CPUS=$(vmvar "$vm" CPUS) VM_MEM_GIB=$(vmvar "$vm" MEM_GIB) VM_MAC=$(vmvar "$vm" MAC)
        SSH_PUBKEY=$(<"$SSH_PUBKEY_FILE") INSTANCE_STAMP=$(date +%s)
        export VM VM_CPUS VM_MEM_GIB VM_MAC SSH_PUBKEY INSTANCE_STAMP
        render "$SCRIPTS/templates/user-data.yaml" >"$dir/user-data"
        render "$SCRIPTS/templates/meta-data.yaml" >"$dir/meta-data"
        render "$SCRIPTS/templates/libvirt-domain.xml" >"$dir/domain.xml"
        hsh "$host" "mkdir -p ~/$HOST_STATE_DIR/$vm"
        local f
        for f in user-data meta-data domain.xml; do
            hsh "$host" "cat > ~/$HOST_STATE_DIR/$vm/$f" <"$dir/$f"
        done
        ENV_VARS=$(host_env "$vm") run_on_host "$host" "$SCRIPTS/host/create-vm.sh"
    done
    for vm in $(vms_of "${1:-all}"); do
        wait_for "SSH on $vm" 600 vssh "$vm" true
        log "$vm: waiting for cloud-init"
        vssh "$vm" 'cloud-init status --wait >/dev/null 2>&1; cloud-init status | tail -1' || true
    done
}

cmd_start() {
    local vm
    for vm in $(vms_of "${1:-all}"); do
        if [[ $(vm_state "$vm") == running ]]; then
            log "$vm already running"
        else
            hsh "$(vmvar "$vm" HOST)" "virsh -c qemu:///system start $vm" >/dev/null
            log "$vm started"
        fi
    done
    for vm in $(vms_of "${1:-all}"); do wait_for "SSH on $vm" 300 vssh "$vm" true; done
}

cmd_stop() {
    local vm
    for vm in $(vms_of "${1:-all}"); do
        [[ $(vm_state "$vm") == running ]] || { log "$vm not running"; continue; }
        hsh "$(vmvar "$vm" HOST)" "virsh -c qemu:///system shutdown $vm" >/dev/null
        log "$vm: shutting down"
    done
    for vm in $(vms_of "${1:-all}"); do
        if ! (wait_for "$vm to power off" 120 vm_is_off "$vm"); then
            warn "$vm did not shut down cleanly; forcing it off"
            hsh "$(vmvar "$vm" HOST)" "virsh -c qemu:///system destroy $vm" >/dev/null || true
        fi
    done
}

cmd_destroy() {
    [[ ${CONFIRM:-} == yes ]] || die "this deletes the VMs and their disks; rerun with CONFIRM=yes"
    local vm
    for vm in $(vms_of "${1:-all}"); do
        log "destroying $vm"
        ENV_VARS=$(host_env "$vm") run_on_host "$(vmvar "$vm" HOST)" "$SCRIPTS/host/destroy-vm.sh"
        ssh-keygen -R "$(vmvar "$vm" IP)" -f "$LOCAL_STATE_DIR/known_hosts" >/dev/null 2>&1 || true
        rm -rf "${LOCAL_STATE_DIR:?}/$vm"
    done
}

cmd_status() {
    local vm state
    for vm in $(vms_of "${1:-all}"); do
        state=$(vm_state "$vm")
        printf '\033[1m%-6s\033[0m on %-14s %-10s %s\n' "$vm" "$(vmvar "$vm" HOST)" "$state" "$(vmvar "$vm" IP)"
        [[ $state == running ]] || continue
        vssh "$vm" 'disk=$(lsblk -dn -o PATH,SERIAL | awk "\$2 ~ /data/ {print \$1; exit}");
            printf "       kernel %s, %s vCPU, %s GiB, ublk %s, data %s %s, rust %s\n" "$(uname -r)" "$(nproc)" \
                "$(free -g | awk "/Mem:/ {print \$2}")" "$(test -e /dev/ublk-control && echo ready || echo missing)" \
                "${disk:-none}" "$(lsblk -dn -o SIZE "$disk" 2>/dev/null)" \
                "$($HOME/.cargo/bin/cargo --version 2>/dev/null | cut -d" " -f2 || echo none)"' 2>/dev/null ||
            echo "       (not reachable over SSH yet)"
    done
}

cmd_ssh() { vssh_tty "$(vms_of "${1:-a}")"; }
vssh_tty() {
    ssh -t -o "UserKnownHostsFile=$LOCAL_STATE_DIR/known_hosts" -o StrictHostKeyChecking=accept-new \
        -J "$(vmvar "$1" HOST)" "$VM_USER@$(vmvar "$1" IP)"
}

cmd_console() { ssh -t "$(vmvar "$(vms_of "${1:-a}")" HOST)" "virsh -c qemu:///system console $(vms_of "${1:-a}")"; }

cmd_exec() {
    local vm
    vm=$(vms_of "$1")
    shift
    vssh "$vm" "$@"
}

cmd_provision() {
    local vm
    for vm in $(vms_of "${1:-all}"); do
        log "provisioning $vm"
        run_on_vm "$vm" "$SCRIPTS/guest/provision.sh"
    done
}

# Copies the working tree (tracked and untracked, minus ignored files) into ~/flaminio, keeping
# the VM's build cache, then builds.
cmd_deploy() {
    local vm
    for vm in $(vms_of "${1:-all}"); do
        log "deploying working tree to $vm"
        (cd "$REPO_ROOT" && git ls-files -co --exclude-standard -z |
            COPYFILE_DISABLE=1 tar --null -T - -czf -) |
            vssh "$vm" 'set -e; mkdir -p ~/flaminio/dataplane
                find ~/flaminio -mindepth 1 -maxdepth 1 ! -name dataplane -exec rm -rf {} +
                find ~/flaminio/dataplane -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} +
                tar -xzf - -C ~/flaminio 2>/dev/null'
        run_on_vm "$vm" "$SCRIPTS/guest/build.sh"
    done
}

# test [VM] [CARGO_ARGS...]
cmd_test() {
    local which=${1:-a} vm
    shift || true
    for vm in $(vms_of "$which"); do
        log "testing on $vm"
        ENV_VARS="PROPTEST_CASES=${PROPTEST_CASES:-256}" run_on_vm "$vm" "$SCRIPTS/guest/test.sh" "$@"
    done
}

cmd_bench_raw() {
    local vm out
    for vm in $(vms_of "${1:-all}"); do
        log "raw-disk fio baseline on $vm (${RUNTIME:-30}s per job)"
        out=$(run_on_vm "$vm" "$SCRIPTS/guest/fio-raw.sh" "${RUNTIME:-30}" "${JOBS:-.}" | tee /dev/stderr | tail -1)
        mkdir -p "$LOCAL_STATE_DIR/results/$vm"
        vssh "$vm" "tar -czf - -C $(dirname "$out") $(basename "$out")" | tar -xzf - -C "$LOCAL_STATE_DIR/results/$vm"
        log "results saved in .lab/results/$vm/$(basename "$out")"
    done
}

cmd_bench_store() {
    local vm out
    for vm in $(vms_of "${1:-all}"); do
        log "replica-store benchmark on $vm (${RUNTIME:-30}s per job)"
        out=$(ENV_VARS="FORMAT=${FORMAT:-1}" run_on_vm "$vm" "$SCRIPTS/guest/store-bench.sh" "${RUNTIME:-30}" "${JOBS:-.}" |
            tee /dev/stderr | tail -1)
        mkdir -p "$LOCAL_STATE_DIR/results/$vm"
        vssh "$vm" "tar -czf - -C $(dirname "$out") $(basename "$out")" | tar -xzf - -C "$LOCAL_STATE_DIR/results/$vm"
        log "results saved in .lab/results/$vm/$(basename "$out")"
    done
}

cmd_net_setup() { ENV_VARS="$(host_env "$VM_B")" run_on_host "$HOST_B" "$SCRIPTS/host/net-forward.sh" up; }
cmd_net_teardown() { ENV_VARS="$(host_env "$VM_B")" run_on_host "$HOST_B" "$SCRIPTS/host/net-forward.sh" down; }

# Sends a message from VM a to HOST_B:REPL_PORT and checks that VM b receives it.
cmd_net_check() {
    local msg got rc
    msg="flaminio-$(date +%s)"
    log "$VM_A -> $HOST_B_IP:$REPL_PORT -> $VM_B:$REPL_PORT"
    got=$(mktemp)
    run_on_vm "$VM_B" "$SCRIPTS/guest/net-listen.sh" "$REPL_PORT" 20 >"$got" &
    sleep 3
    rc=0
    run_on_vm "$VM_A" "$SCRIPTS/guest/net-send.sh" "$HOST_B_IP" "$REPL_PORT" "$msg" || rc=$?
    wait || true
    if [[ $rc -eq 0 && $(<"$got") == "$msg" ]]; then
        log "OK: $VM_B received the message"
    else
        rm -f "$got"
        die "message did not arrive (send exit $rc). Check: make net-setup, ufw on $HOST_B, VM networking"
    fi
    rm -f "$got"
}

usage() {
    cat <<USAGE
usage: scripts/lab.sh COMMAND [a|b|all]

  check        host reachability and prerequisites
  host-prep    install KVM/libvirt, storage pool, network, ufw rules (idempotent)
  create       create and boot VMs (idempotent)
  start|stop   power VMs on or off
  status       VM state, kernel, ublk, data disk
  destroy      delete VMs and disks (CONFIRM=yes)
  provision    toolchain and kernel modules in the VMs
  deploy       copy the working tree to VMs and build (release)
  test         cargo test --release in a VM (PROPTEST_CASES=N)
  bench-raw    fio baseline on the raw data disk (RUNTIME=s, JOBS=regex); destroys data on it
  bench-store  replica-store benchmark on the data disk (RUNTIME, JOBS, FORMAT=0 to reuse)
  net-setup    forward $HOST_B_IP:$REPL_PORT to $VM_B (from $HOST_A_IP only)
  net-teardown remove that forward
  net-check    end-to-end check: $VM_A -> $HOST_B -> $VM_B
  ssh|console  interactive shell or serial console (default: a)
  exec VM CMD  run a command in a VM
USAGE
}

cmd=${1:-help}
shift || true
case "$cmd" in
    check) cmd_check "$@" ;;
    host-prep) cmd_host_prep "$@" ;;
    create) cmd_create "$@" ;;
    start) cmd_start "$@" ;;
    stop) cmd_stop "$@" ;;
    status) cmd_status "$@" ;;
    destroy) cmd_destroy "$@" ;;
    provision) cmd_provision "$@" ;;
    deploy) cmd_deploy "$@" ;;
    test) cmd_test "$@" ;;
    bench-raw) cmd_bench_raw "$@" ;;
    bench-store) cmd_bench_store "$@" ;;
    net-setup) cmd_net_setup ;;
    net-teardown) cmd_net_teardown ;;
    net-check) cmd_net_check ;;
    ssh) cmd_ssh "$@" ;;
    console) cmd_console "$@" ;;
    exec) cmd_exec "$@" ;;
    help | -h | --help) usage ;;
    *) usage >&2; exit 2 ;;
esac
