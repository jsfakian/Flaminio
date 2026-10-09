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
        for v in CPUS MEM_GIB MAC IP DATA_GIB; do
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

# Renders a VM's cloud-init and domain files and uploads them to its host.
upload_vm_files() {
    local vm=$1 host dir f
    host=$(vmvar "$vm" HOST)
    dir="$LOCAL_STATE_DIR/$vm"
    mkdir -p "$dir"
    VM=$vm VM_CPUS=$(vmvar "$vm" CPUS) VM_MEM_GIB=$(vmvar "$vm" MEM_GIB) VM_MAC=$(vmvar "$vm" MAC)
    SSH_PUBKEY=$(<"$SSH_PUBKEY_FILE") INSTANCE_STAMP=$(date +%s)
    export VM VM_CPUS VM_MEM_GIB VM_MAC SSH_PUBKEY INSTANCE_STAMP
    render "$SCRIPTS/templates/user-data.yaml" >"$dir/user-data"
    render "$SCRIPTS/templates/meta-data.yaml" >"$dir/meta-data"
    render "$SCRIPTS/templates/libvirt-domain.xml" >"$dir/domain.xml"
    hsh "$host" "mkdir -p ~/$HOST_STATE_DIR/$vm"
    for f in user-data meta-data domain.xml; do
        hsh "$host" "cat > ~/$HOST_STATE_DIR/$vm/$f" <"$dir/$f"
    done
}

cmd_create() {
    local vm host
    [[ -r $SSH_PUBKEY_FILE ]] || die "SSH public key $SSH_PUBKEY_FILE not found (set SSH_PUBKEY_FILE)"
    for vm in $(vms_of "${1:-all}"); do
        host=$(vmvar "$vm" HOST)
        log "creating $vm on $host"
        upload_vm_files "$vm"
        ENV_VARS=$(host_env "$vm") run_on_host "$host" "$SCRIPTS/host/create-vm.sh"
    done
    for vm in $(vms_of "${1:-all}"); do
        wait_for "SSH on $vm" 600 vssh "$vm" true
        log "$vm: waiting for cloud-init"
        vssh "$vm" 'cloud-init status --wait >/dev/null 2>&1; cloud-init status | tail -1' || true
    done
}

# Applies the current domain template to existing VMs. Takes effect at their next boot.
cmd_redefine() {
    local vm host
    for vm in $(vms_of "${1:-all}"); do
        host=$(vmvar "$vm" HOST)
        upload_vm_files "$vm"
        # Keep the domain's UUID, or libvirt sees a different domain with the same name.
        hsh "$host" "set -e; f=~/$HOST_STATE_DIR/$vm/domain.xml; uuid=\$(virsh -c qemu:///system domuuid $vm);
            sed -i \"s|<name>$vm</name>|<name>$vm</name><uuid>\$uuid</uuid>|\" \$f;
            virsh -c qemu:///system define \$f" >/dev/null
        log "$vm redefined (takes effect at its next power cycle: make vm-restart FORCE=1)"
    done
}

# Reboots VMs: a clean reboot, or with FORCE=1 a hard reset (for a hung guest).
cmd_restart() {
    local vm host
    for vm in $(vms_of "${1:-all}"); do
        host=$(vmvar "$vm" HOST)
        if [[ ${FORCE:-0} == 1 ]]; then
            log "$vm: hard reset"
            hsh "$host" "virsh -c qemu:///system destroy $vm >/dev/null 2>&1; virsh -c qemu:///system start $vm" >/dev/null
        else
            log "$vm: rebooting"
            vssh "$vm" 'sudo systemctl reboot' >/dev/null 2>&1 || true
            sleep 10
        fi
    done
    for vm in $(vms_of "${1:-all}"); do wait_for "SSH on $vm" 300 vssh "$vm" true; done
}

# Shows the end of a VM's console log (kept on its host).
cmd_console_log() {
    local vm
    vm=$(vms_of "${1:-a}")
    hsh "$(vmvar "$vm" HOST)" "sudo tail -n ${LINES:-60} /var/log/libvirt/qemu/$vm-console.log"
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
            bad=""; for c in $(seq 0 $(($(nproc) - 1))); do taskset -c "$c" true 2>/dev/null || bad="$bad $c"; done
            printf "       vCPU health: %s; CET: %s\n" "${bad:+FAILING on vCPU$bad}${bad:-all $(nproc) run processes}" \
                "$(grep -qw user_shstk /proc/cpuinfo && echo on || echo off)"
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
        -J "$(vm_jump "$1")" "$VM_USER@$(vmvar "$1" IP)"
}

cmd_console() {
    local vm host jump
    vm=$(vms_of "${1:-a}")
    host=$(vmvar "$vm" HOST)
    jump=$(host_jump "$host")
    ssh -t ${jump:+-J "$jump"} "$host" "virsh -c qemu:///system console $vm"
}

cmd_exec() {
    local vm
    vm=$(vms_of "$1")
    shift
    vssh "$vm" "$@"
}

cmd_provision() {
    local vm out
    for vm in $(vms_of "${1:-all}"); do
        log "provisioning $vm"
        out=$(ENV_VARS="GUEST_KERNEL_ARGS=$(printf %q "$GUEST_KERNEL_ARGS")" run_on_vm "$vm" "$SCRIPTS/guest/provision.sh" | tee /dev/stderr)
        if [[ $out == *REBOOT_REQUIRED* ]]; then
            cmd_restart "$vm"
            ENV_VARS="GUEST_KERNEL_ARGS=$(printf %q "$GUEST_KERNEL_ARGS")" run_on_vm "$vm" "$SCRIPTS/guest/provision.sh" |
                grep -E "kernel args|cet" >&2
        fi
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
# clippy on Linux in a VM: covers code that only compiles there (io_uring, ublk).
cmd_lint() {
    local vm
    vm=$(vms_of "${1:-a}")
    log "clippy on $vm (Linux-only code included)"
    vssh "$vm" '$HOME/.cargo/bin/rustup component add clippy >/dev/null 2>&1; cd $HOME/flaminio/dataplane &&
        $HOME/.cargo/bin/cargo clippy --release --all-targets --message-format=short -- -D warnings 2>&1 |
        grep -E "^(crates|error|warning)" | grep -v "could not compile" | sort -u; exit ${PIPESTATUS[0]}'
}

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
        out=$(run_detached "$vm" "$SCRIPTS/guest/fio-raw.sh" "${RUNTIME:-30}" "${JOBS:-read}" | tee /dev/stderr | tail -1)
        mkdir -p "$LOCAL_STATE_DIR/results/$vm"
        vssh "$vm" "tar -czf - -C $(dirname "$out") $(basename "$out")" | tar -xzf - -C "$LOCAL_STATE_DIR/results/$vm"
        log "results saved in .lab/results/$vm/$(basename "$out")"
    done
}

cmd_bench_store() {
    local vm out
    for vm in $(vms_of "${1:-all}"); do
        log "replica-store benchmark on $vm (${RUNTIME:-30}s per job)"
        out=$(ENV_VARS="FORMAT=${FORMAT:-1} JOURNAL=${JOURNAL:-}" run_detached "$vm" "$SCRIPTS/guest/store-bench.sh" "${RUNTIME:-30}" "${JOBS:-read}" |
            tee /dev/stderr | tail -1)
        mkdir -p "$LOCAL_STATE_DIR/results/$vm"
        vssh "$vm" "tar -czf - -C $(dirname "$out") $(basename "$out")" | tar -xzf - -C "$LOCAL_STATE_DIR/results/$vm"
        log "results saved in .lab/results/$vm/$(basename "$out")"
    done
}

# ublk frontend on a VM (default a): up|down|status|smoke|fio
cmd_ublk() {
    local action=${1:-status} vm out
    vm=$(vms_of "${2:-a}")
    case "$action" in
        fio)
            out=$(ENV_VARS="FILL=${FILL:-1}" run_detached "$vm" "$SCRIPTS/guest/ublk.sh" fio "${RUNTIME:-30}" | tee /dev/stderr | tail -1)
            mkdir -p "$LOCAL_STATE_DIR/results/$vm"
            vssh "$vm" "tar -czf - -C $(dirname "$out") $(basename "$out")" | tar -xzf - -C "$LOCAL_STATE_DIR/results/$vm"
            log "results saved in .lab/results/$vm/$(basename "$out")"
            ;;
        smoke) run_detached "$vm" "$SCRIPTS/guest/ublk.sh" smoke ;;
        *) ENV_VARS="FORMAT=${FORMAT:-0} VOLUME_SIZE=${VOLUME_SIZE:-64G} REPLICA=${REPLICA:-}" \
            run_on_vm "$vm" "$SCRIPTS/guest/ublk.sh" "$action" ;;
    esac
}

# Replica service on a VM (default b): up (FORMAT=1 reformats) | down | status
cmd_replica() {
    ENV_VARS="FORMAT=${FORMAT:-0} REPL_PORT=$REPL_PORT" run_on_vm "$(vms_of "${2:-b}")" "$SCRIPTS/guest/replica.sh" "${1:-status}"
}

# RF=2: replica service on VM b, volume target on VM a mirroring to it through HOST_B.
cmd_repl_up() {
    FORMAT=${FORMAT:-0} cmd_replica up b
    FORMAT=${FORMAT:-0} REPLICA="$HOST_B_IP:$REPL_PORT" cmd_ublk up a
    log "fl-a: /dev/ublkb0 mirrors to fl-b (watch: make ublk-status)"
}

cmd_repl_down() {
    cmd_ublk down a
    cmd_replica down b
}

# Stops both sides and compares per-chunk checksums of the two replicas.
cmd_repl_verify() {
    cmd_repl_down
    local a b
    a=$(ENV_VARS="VOLUME_SIZE=${VOLUME_SIZE:-64G}" run_on_vm "$VM_A" "$SCRIPTS/guest/digest.sh")
    b=$(ENV_VARS="VOLUME_SIZE=${VOLUME_SIZE:-64G}" run_on_vm "$VM_B" "$SCRIPTS/guest/digest.sh")
    log "fl-a: $(tail -1 <<<"$a")   fl-b: $(tail -1 <<<"$b")"
    if [[ $a == "$b" ]]; then
        log "OK: both replicas hold identical data"
    else
        diff <(echo "$a") <(echo "$b") | head -20 >&2
        die "replicas differ"
    fi
}

# Writes with verification on VM a while the replica on VM b is stopped and restarted, then
# checks that the volume came back in sync and that both replicas are identical.
cmd_repl_fault() {
    local load_pid rc=0
    log "write load with verification on fl-a; fl-b's replica stops after 10 s, restarts 15 s later"
    run_detached "$VM_A" "$SCRIPTS/guest/write-load.sh" &
    load_pid=$!
    sleep 10
    cmd_replica down b
    sleep 15
    cmd_replica up b
    wait "$load_pid" || rc=$?
    ((rc == 0)) || die "write load failed (exit $rc): writes must not fail while the replica is down"
    log "write load finished and verified; waiting for the replica to come back in sync"
    local start=$SECONDS state
    while :; do
        state=$(vssh "$VM_A" "sudo journalctl -u flaminio-io -o cat | grep -E 'replica .*: (InSync|Resyncing|Down)' | tail -1")
        [[ $state == *InSync* ]] && break
        ((SECONDS - start < 900)) || die "not in sync after 15 min: $state"
        sleep 10
    done
    log "in sync after $((SECONDS - start)) s"
    cmd_repl_verify
}

# The port forwards each host needs, as forward.sh rules: replication to VM b (TCP), and the
# overlay port of every VM (UDP), each open to the other server only.
forward_rules() {
    local host=$1 vm other rules=()
    if [[ $host == "$HOST_B" ]]; then
        rules+=("tcp/$REPL_PORT/$VM_B_IP/$REPL_PORT/$HOST_A_IP")
    fi
    for vm in $(vms_of cluster); do
        [[ $(vmvar "$vm" HOST) == "$host" ]] || continue
        other=$HOST_A_IP
        [[ $host == "$HOST_A" ]] && other=$HOST_B_IP
        rules+=("udp/$(vmvar "$vm" WG_FWD)/$(vmvar "$vm" IP)/$WG_PORT/$other")
    done
    echo "${rules[*]}"
}

net_forward() {
    local host iface
    for host in $HOST_A $HOST_B; do
        iface=$HOST_B_IFACE
        [[ $host == "$HOST_A" ]] && iface=$HOST_A_IFACE
        ENV_VARS="IFACE=$iface LIBVIRT_BRIDGE=$LIBVIRT_BRIDGE RULES=$(printf %q "$(forward_rules "$host")")" \
            run_on_host "$host" "$SCRIPTS/host/forward.sh" "$1"
    done
}
cmd_net_setup() { net_forward up; }
cmd_net_teardown() { net_forward down; }

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

# --- Phase 1: overlay, etcd and the metadata service on all three VMs ---

# The address at which VM $1 reaches VM $2's WireGuard port.
wg_endpoint() {
    local from=$1 to=$2
    if [[ $(vmvar "$from" HOST) == "$(vmvar "$to" HOST)" ]]; then
        echo "$(vmvar "$to" IP):$WG_PORT"
    else
        echo "$(host_ip "$(vmvar "$to" HOST)"):$(vmvar "$to" WG_FWD)"
    fi
}

# mesh up|down|status|check
cmd_mesh() {
    local action=${1:-status} vm peer conf
    declare -A pub
    case "$action" in
        up)
            for vm in $(vms_of cluster); do
                pub[$vm]=$(run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" key | tail -1)
            done
            for vm in $(vms_of cluster); do
                conf="[Interface]
Address = $(vmvar "$vm" WG)/24
ListenPort = $WG_PORT
PostUp = wg set %i private-key /etc/wireguard/wg0.key
"
                for peer in $(vms_of cluster); do
                    [[ $peer == "$vm" ]] && continue
                    conf+="
[Peer]
# $peer
PublicKey = ${pub[$peer]}
AllowedIPs = $(vmvar "$peer" WG)/32
Endpoint = $(wg_endpoint "$vm" "$peer")
PersistentKeepalive = 25
"
                done
                ENV_VARS="WG_CONF=$(printf '%s' "$conf" | base64 | tr -d '\n')" run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" up
            done
            cmd_mesh check
            ;;
        check)
            local bad=0
            for vm in $(vms_of cluster); do
                for peer in $(vms_of cluster); do
                    [[ $peer == "$vm" ]] && continue
                    if vssh "$vm" "ping -c 3 -i 0.3 -W 2 -q $(vmvar "$peer" WG) >/dev/null"; then
                        log "$vm -> $peer ($(vmvar "$peer" WG)): ok"
                    else
                        warn "$vm -> $peer ($(vmvar "$peer" WG)): no reply"
                        bad=1
                    fi
                done
            done
            ((bad == 0)) || die "overlay incomplete (make net-setup? make mesh-up?)"
            ;;
        down | status)
            for vm in $(vms_of cluster); do run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" "$action"; done
            ;;
        *) die "usage: mesh up|down|status|check" ;;
    esac
}

etcd_cluster() {
    local vm parts=()
    for vm in $(vms_of cluster); do parts+=("$vm=https://$(vmvar "$vm" WG):2380"); done
    (IFS=,; echo "${parts[*]}")
}

etcd_endpoints() {
    local vm parts=()
    for vm in $(vms_of cluster); do parts+=("https://$(vmvar "$vm" WG):2379"); done
    (IFS=,; echo "${parts[*]}")
}

# etcd up|down|status|wipe [VM selector, default cluster]
cmd_etcd() {
    local action=${1:-status} vm
    for vm in $(vms_of "${2:-cluster}"); do
        case "$action" in
            up)
                ENV_VARS="ETCD_VERSION=$ETCD_VERSION" run_on_vm "$vm" "$SCRIPTS/guest/etcd.sh" install
                ENV_VARS="NAME=$vm IP=$(vmvar "$vm" WG) CLUSTER=$(etcd_cluster)" run_on_vm "$vm" "$SCRIPTS/guest/etcd.sh" up
                ;;
            wipe)
                [[ ${CONFIRM:-} == yes ]] || die "this deletes the metadata; rerun with CONFIRM=yes"
                run_on_vm "$vm" "$SCRIPTS/guest/etcd.sh" wipe
                ;;
            down | status)
                [[ $action == status ]] && log "$vm"
                run_on_vm "$vm" "$SCRIPTS/guest/etcd.sh" "$action"
                ;;
            *) die "usage: etcd up|down|status|wipe [VM]" ;;
        esac
    done
}

# meta up|down|status [VM selector, default cluster]: builds flaminio-meta for Linux here, and
# runs it on each VM against the whole etcd cluster.
cmd_meta() {
    local action=${1:-status} vm bin="$LOCAL_STATE_DIR/bin/flaminio-meta"
    if [[ $action == up ]]; then
        log "building flaminio-meta for linux/amd64"
        (cd "$REPO_ROOT" && GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -o "$bin" ./meta/cmd/flaminio-meta)
    fi
    for vm in $(vms_of "${2:-cluster}"); do
        case "$action" in
            up)
                vssh "$vm" "cat > ~/flaminio-meta" <"$bin"
                ENV_VARS="LISTEN=$(vmvar "$vm" WG):$META_PORT ETCD=$(etcd_endpoints) TRUST_DOMAIN=$TRUST_DOMAIN" \
                    run_on_vm "$vm" "$SCRIPTS/guest/meta.sh" up
                ;;
            down | status) run_on_vm "$vm" "$SCRIPTS/guest/meta.sh" "$action" ;;
            *) die "usage: meta up|down|status [VM]" ;;
        esac
    done
}

# Runs flaminio-meta's client in VM $1 against the metadata service on VM $2, as the identity
# AS (default: the lab administrator, whose key is on fl-a).
metactl() {
    local vm=$1 server=$2 as=${AS:-admin}
    shift 2
    local p=/etc/flaminio/pki
    vssh "$vm" "sudo /usr/local/bin/flaminio-meta --server $(vmvar "$server" WG):$META_PORT \
        --cert $p/$as.crt --key $p/$as.key --ca $p/ca.crt $(printf '%q ' "$@")"
}

ca_tool() {
    local bin="$LOCAL_STATE_DIR/bin/flaminio-ca"
    if [[ ! -x $bin || -n $(find "$REPO_ROOT/control/cmd/flaminio-ca" "$REPO_ROOT/pkg/pki" -newer "$bin" -name '*.go' 2>/dev/null) ]]; then
        (cd "$REPO_ROOT" && go build -o "$bin" ./control/cmd/flaminio-ca)
    fi
    "$bin" "$@"
}

# Issues identity NAME on VM: the VM makes the key and a CSR, CA_DIR signs it.
#   issue VM NAME CA_DIR SPIFFE_ID FLAGS...   (FLAGS: --server, --client, --ip ...)
issue() {
    local vm=$1 name=$2 ca=$3 id=$4 csr cert
    shift 4
    csr=$(run_on_vm "$vm" "$SCRIPTS/guest/pki.sh" csr "$name")
    cert=$(ca_tool sign "$ca" --id "$id" --days "$CERT_DAYS" "$@" <<<"$csr")
    ENV_VARS="CERT=$(base64 <<<"$cert" | tr -d '\n')" run_on_vm "$vm" "$SCRIPTS/guest/pki.sh" install "$name"
    log "$vm: $name.crt for $id"
}

put_file() {
    local vm=$1 name=$2 file=$3 mode=${4:-644}
    ENV_VARS="MODE=$mode DATA=$(base64 <"$file" | tr -d '\n')" run_on_vm "$vm" "$SCRIPTS/guest/pki.sh" file "$name"
}

# pki init|issue: the lab's two CAs and token key (on this workstation), and every identity of
# every VM. The workload CA signs identities (meta, io, admin); the etcd CA signs only etcd
# members and the metadata service's etcd clients.
cmd_pki() {
    local dir="$LOCAL_STATE_DIR/pki" vm ip
    case "${1:-}" in
        init)
            [[ -f $dir/flaminio/ca.crt ]] || ca_tool init "$dir/flaminio" "Flaminio lab workload CA"
            [[ -f $dir/etcd/ca.crt ]] || ca_tool init "$dir/etcd" "Flaminio lab etcd CA"
            [[ -f $dir/token.key ]] || ca_tool token-key "$dir/token.key"
            log "CAs and token key in $dir"
            ;;
        issue)
            [[ -f $dir/flaminio/ca.crt ]] || cmd_pki init
            for vm in $(vms_of "${2:-cluster}"); do
                ip=$(vmvar "$vm" WG)
                put_file "$vm" ca.crt "$dir/flaminio/ca.crt"
                put_file "$vm" etcd-ca.crt "$dir/etcd/ca.crt"
                put_file "$vm" token.key "$dir/token.key" 600
                put_file "$vm" token.pub "$dir/token.key.pub"
                issue "$vm" etcd "$dir/etcd" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/etcd" --server --client --ip "$ip"
                issue "$vm" meta-etcd "$dir/etcd" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/meta" --client
                issue "$vm" meta "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/meta" --server --ip "$ip"
                issue "$vm" io "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/io" --server --client --ip "$ip"
            done
            if [[ " $(vms_of "${2:-cluster}") " == *" $VM_A "* ]]; then
                issue "$VM_A" admin "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/admin/lab" --client
            fi
            ;;
        *) die "usage: pki init|issue [VM]" ;;
    esac
}

# End-to-end check of the metadata service: nodes, a volume, attach and ISR, then the loss of
# one and of two etcd members.
cmd_meta_smoke() {
    local vm vol out
    vol="smoke-$(date +%s)"
    for vm in $(vms_of cluster); do
        local labels=()
        [[ $vm == "$VM_W" ]] && labels=(flaminio.io/witness=)
        metactl "$VM_A" "$vm" register "$vm" "$(vmvar "$vm" WG):$REPL_PORT" "${labels[@]}" >/dev/null
        metactl "$VM_A" "$vm" heartbeat "$vm" >/dev/null
    done
    log "nodes: $(metactl "$VM_A" "$VM_A" nodes | jq -c '[.nodes[] | {id, alive, witness: (.labels // {} | has("flaminio.io/witness"))}]')"
    out=$(metactl "$VM_A" "$VM_A" create "$vol" 1G 2 1 256M)
    log "created $vol: $(jq -c '[.volume.chunks[].replicas]' <<<"$out")"
    if jq -e --arg w "$VM_W" '[.volume.chunks[].replicas[]] | index($w)' <<<"$out" >/dev/null; then
        die "a replica was placed on the witness $VM_W"
    fi
    out=$(metactl "$VM_A" "$VM_B" attach "$vol" "$VM_B")
    log "attached through $VM_B's service: epoch $(jq -r .volume.epoch <<<"$out"), target $(jq -r .volume.target <<<"$out")"

    log "stopping etcd on $VM_W: 2 of 3 members left, quorum holds"
    cmd_etcd down w
    out=$(metactl "$VM_A" "$VM_A" attach "$vol" "$VM_A")
    log "attach with 2 members: epoch $(jq -r .volume.epoch <<<"$out")"

    log "stopping etcd on $VM_B too: 1 of 3, no quorum"
    cmd_etcd down b
    if metactl "$VM_A" "$VM_A" get "$vol" >/dev/null 2>&1; then
        warn "read succeeded without quorum (expected a refusal)"
    else
        log "without quorum the service refuses, as it must"
    fi

    log "restarting both members"
    cmd_etcd up b
    cmd_etcd up w
    wait_for "the metadata service to answer" 60 metactl "$VM_A" "$VM_W" get "$vol"
    out=$(metactl "$VM_A" "$VM_W" get "$vol")
    [[ $(jq -r .volume.epoch <<<"$out") == 2 ]] || die "unexpected state after recovery: $out"
    metactl "$VM_A" "$VM_A" delete "$vol" >/dev/null
    log "OK: metadata survived the loss of one member, refused without quorum, recovered (epoch 2)"

    log "refusals:"
    if vssh "$VM_A" "/usr/local/bin/flaminio-meta --server $VM_A_WG:$META_PORT nodes" >/dev/null 2>&1; then
        die "a plain-text client was served"
    fi
    log "  plain-text client: refused"
    if AS=io metactl "$VM_A" "$VM_A" create "$vol-x" 1G 2 1 >/dev/null 2>&1; then
        die "a node identity created a volume"
    fi
    log "  node identity creating a volume: refused"
    if vssh "$VM_A" "sudo curl -sf --cacert /etc/flaminio/pki/etcd-ca.crt https://$VM_B_WG:2379/health" >/dev/null 2>&1; then
        die "etcd answered a client without a certificate"
    fi
    log "  etcd without a client certificate: refused"
    if vssh "$VM_A" "sudo curl -sf --cacert /etc/flaminio/pki/etcd-ca.crt --cert /etc/flaminio/pki/io.crt \
            --key /etc/flaminio/pki/io.key https://$VM_B_WG:2379/health" >/dev/null 2>&1; then
        die "etcd answered a node's workload certificate"
    fi
    log "  etcd with a node's workload certificate: refused"
}

usage() {
    cat <<USAGE
usage: scripts/lab.sh COMMAND [a|b|w|all|cluster]   (all: the data VMs a and b; cluster: a, b, w)

  check        host reachability and prerequisites
  host-prep    install KVM/libvirt, storage pool, network, ufw rules (idempotent)
  create       create and boot VMs (idempotent)
  start|stop   power VMs on or off
  restart      reboot VMs (FORCE=1: hard reset of a hung guest)
  redefine     apply the current domain template to existing VMs (next boot)
  console-log  end of a VM's console log, kept on its host (default: a)
  status       VM state, kernel, ublk, data disk
  destroy      delete VMs and disks (CONFIRM=yes)
  provision    toolchain and kernel modules in the VMs
  deploy       copy the working tree to VMs and build (release)
  test         cargo test --release in a VM (PROPTEST_CASES=N)
  lint         clippy (warnings are errors) in a VM, including Linux-only code
  bench-raw    fio baseline on the raw data disk (RUNTIME=s, JOBS=regex, default reads only)
  bench-store  replica-store benchmark on the data disk (RUNTIME, JOBS, FORMAT=0 to reuse)
  ublk ACTION [VM]  ublk frontend: up (FORMAT=1 reformats) | down | status | smoke | fio
  replica ACTION [VM]  replica service: up (FORMAT=1 reformats) | down | status (default VM b)
  repl-up      RF=2: replica on b, target on a mirroring to it (FORMAT=1 reformats both)
  repl-down    stop both sides
  repl-verify  stop both sides and compare the replicas' checksums
  repl-fault   verified writes while the replica is stopped and restarted, then repl-verify
  net-setup    port forwards on both hosts: replication to $VM_B, overlay (UDP) to every VM
  net-teardown remove them
  net-check    end-to-end check: $VM_A -> $HOST_B -> $VM_B
  mesh ACTION  WireGuard overlay between a, b and w: up | down | status | check
  etcd ACTION [VM]  metadata etcd (default: all three): up | down | status | wipe (CONFIRM=yes)
  meta ACTION [VM]  metadata service (default: all three): up | down | status
  meta-smoke   end-to-end metadata check: etcd member loss, and refusal of unauthenticated callers
  pki ACTION [VM]  identities: init (CAs and token key, here) | issue (keys made in the VMs)
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
    restart) cmd_restart "$@" ;;
    redefine) cmd_redefine "$@" ;;
    console-log) cmd_console_log "$@" ;;
    stop) cmd_stop "$@" ;;
    status) cmd_status "$@" ;;
    destroy) cmd_destroy "$@" ;;
    provision) cmd_provision "$@" ;;
    deploy) cmd_deploy "$@" ;;
    test) cmd_test "$@" ;;
    lint) cmd_lint "$@" ;;
    bench-raw) cmd_bench_raw "$@" ;;
    bench-store) cmd_bench_store "$@" ;;
    ublk) cmd_ublk "$@" ;;
    replica) cmd_replica "$@" ;;
    repl-up) cmd_repl_up ;;
    repl-down) cmd_repl_down ;;
    repl-verify) cmd_repl_verify ;;
    repl-fault) cmd_repl_fault ;;
    net-setup) cmd_net_setup ;;
    net-teardown) cmd_net_teardown ;;
    net-check) cmd_net_check ;;
    mesh) cmd_mesh "$@" ;;
    etcd) cmd_etcd "$@" ;;
    meta) cmd_meta "$@" ;;
    meta-smoke) cmd_meta_smoke ;;
    pki) cmd_pki "$@" ;;
    ssh) cmd_ssh "$@" ;;
    console) cmd_console "$@" ;;
    exec) cmd_exec "$@" ;;
    help | -h | --help) usage ;;
    *) usage >&2; exit 2 ;;
esac
