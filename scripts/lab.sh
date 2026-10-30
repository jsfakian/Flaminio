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
        touch "$LOCAL_STATE_DIR/$vm/created"
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
    local jump
    jump=$(vm_jump "$1")
    ssh -t -o "UserKnownHostsFile=$LOCAL_STATE_DIR/known_hosts" -o StrictHostKeyChecking=accept-new \
        ${SSH_IDENTITY:+-i "$SSH_IDENTITY"} ${jump:+-J "$jump"} "$VM_USER@$(vmvar "$1" IP)"
}

cmd_console() {
    local vm host jump
    vm=$(vms_of "${1:-a}")
    host=$(vmvar "$vm" HOST)
    if on_host "$host"; then
        virsh -c qemu:///system console "$vm"
        return
    fi
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
# overlay port of every VM (UDP), each open to the other server only. When DEMO_CP_SRC_IP is set
# (Mac control-plane demo), the same UDP forwards also accept that source.
forward_rules() {
    local host=$1 vm other rules=()
    if [[ $host == "$HOST_B" ]]; then
        rules+=("tcp/$REPL_PORT/$VM_B_IP/$REPL_PORT/$HOST_A_IP")
    fi
    for vm in $(vms_of overlay); do
        [[ $(vmvar "$vm" HOST) == "$host" ]] || continue
        other=$HOST_A_IP
        [[ $host == "$HOST_A" ]] && other=$HOST_B_IP
        rules+=("udp/$(vmvar "$vm" WG_FWD)/$(vmvar "$vm" IP)/$WG_PORT/$other")
        if [[ -n ${DEMO_CP_SRC_IP:-} ]]; then
            rules+=("udp/$(vmvar "$vm" WG_FWD)/$(vmvar "$vm" IP)/$WG_PORT/$DEMO_CP_SRC_IP")
        fi
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
            for vm in $(vms_of overlay); do
                pub[$vm]=$(run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" key | tail -1)
            done
            for vm in $(vms_of overlay); do
                conf="[Interface]
Address = $(vmvar "$vm" WG)/24
ListenPort = $WG_PORT
PostUp = wg set %i private-key /etc/wireguard/wg0.key
"
                for peer in $(vms_of overlay); do
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
            for vm in $(vms_of overlay); do
                for peer in $(vms_of overlay); do
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
            for vm in $(vms_of overlay); do run_on_vm "$vm" "$SCRIPTS/guest/wg.sh" "$action"; done
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
                # The node's agent (the CSI node plugin, for now): it restarts the targets
                # the metadata service places on its node.
                [[ $vm == "$VM_W" ]] || issue "$vm" agent "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/agent" --client
            done
            # The lab administrator works from fl-a, and from fl-w when fl-a is cut off.
            for vm in $(vms_of "${2:-cluster}"); do
                [[ $vm == "$VM_A" || $vm == "$VM_W" ]] || continue
                issue "$vm" admin "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/admin/lab" --client
            done
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
        # Distinct zones/racks so failure-domain placement can be exercised (docs/followups.md §4).
        # The lab is not physically independent (one switch; fl-w shares a server with fl-a).
        [[ $vm == "$VM_A" ]] && labels+=(topology.kubernetes.io/zone=lab-a "flaminio.io/rack=rack-a")
        [[ $vm == "$VM_B" ]] && labels+=(topology.kubernetes.io/zone=lab-b "flaminio.io/rack=rack-b")
        metactl "$VM_A" "$vm" register "$vm" "$(vmvar "$vm" WG):$REPL_PORT" "${labels[@]}" >/dev/null
        metactl "$VM_A" "$vm" heartbeat "$vm" >/dev/null
    done
    log "nodes: $(metactl "$VM_A" "$VM_A" nodes | jq -c '[.nodes[] | {id, alive, witness: (.labels // {} | has("flaminio.io/witness")), zone: .labels["topology.kubernetes.io/zone"]}]')"
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

# --- Phase 1: nodes (flaminio-io node) and a replicated volume ---

meta_endpoints() {
    local vm parts=()
    for vm in $(vms_of cluster); do parts+=("$(vmvar "$vm" WG):$META_PORT"); done
    (IFS=,; echo "${parts[*]}")
}

node_env() {
    local vm=$1
    echo "NODE=$vm LISTEN=$(vmvar "$vm" WG):$NODE_PORT META=$(meta_endpoints) TRUST_DOMAIN=$TRUST_DOMAIN FORMAT=${FORMAT:-0} ZERO_COPY=${ZERO_COPY:-0} NO_VERIFY=${NO_VERIFY:-0} QUEUES=${QUEUES:-1} POLL_US=${POLL_US:-0} GATHER_US=${GATHER_US:-} COMMIT_TARGET_MIB=${COMMIT_TARGET_MIB:-} FLUSH_COALESCE=${FLUSH_COALESCE:-} SYNC_COMMITS=${SYNC_COMMITS:-0} PIPELINED_COMMITS=${PIPELINED_COMMITS:-0} FIRST_FIT=${FIRST_FIT:-0} DEFER_TASKRUN=${DEFER_TASKRUN:-0}"
}

# node up|down|status [VM selector, default all]: the data VMs' nodes, replicas only.
cmd_node() {
    local action=${1:-status} vm
    for vm in $(vms_of "${2:-all}"); do
        case "$action" in
            up) ENV_VARS=$(node_env "$vm") run_on_vm "$vm" "$SCRIPTS/guest/node.sh" up ;;
            down | status)
                [[ $action == status ]] && log "$vm"
                run_on_vm "$vm" "$SCRIPTS/guest/node.sh" "$action"
                ;;
            *) die "usage: node up|down|status [VM]" ;;
        esac
    done
}

# vol create|attach VM|show: the test volume, as the lab administrator.
cmd_vol() {
    local out vm tok
    case "${1:-show}" in
        create)
            out=$(metactl "$VM_A" "$VM_A" create "$VOL" "$VOL_SIZE" 2 1 "$VOL_CHUNK")
            log "created $VOL: $(jq -r '[.volume.chunks[].replicas | join("+")] | group_by(.) | map("\(length)×\(.[0])") | join(", ")' <<<"$out")"
            ;;
        attach)
            vm=$(vms_of "${2:-a}")
            # The volume moves: the other nodes let go of it first (best effort: a node that
            # is down lets go anyway, and a newer epoch deposes it).
            local other
            for other in $(vms_of all); do
                [[ $other == "$vm" ]] && continue
                ENV_VARS="VOLUME=$VOL" run_on_vm "$other" "$SCRIPTS/guest/node.sh" detach 2>/dev/null || true
            done
            out=$(metactl "$VM_A" "$VM_A" attach "$VOL" "$vm")
            tok=$(jq -r .token <<<"$out")
            log "attached $VOL to $vm at epoch $(jq -r .volume.epoch <<<"$out")"
            ENV_VARS="VOLUME=$VOL TOKEN=$tok" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" attach
            ;;
        detach)
            ENV_VARS="VOLUME=$VOL" run_on_vm "$(vms_of "${2:-a}")" "$SCRIPTS/guest/node.sh" detach
            ;;
        delete)
            metactl "$VM_A" "$VM_A" delete "$VOL" >/dev/null
            log "deleted $VOL from the metadata service"
            ;;
        show)
            metactl "$VM_A" "$VM_A" get "$VOL" | jq -r '"epoch \(.volume.epoch), target \(.volume.target)",
                ([.volume.chunks[] | (.isr | join("+"))] | group_by(.) | map("  ISR \(.[0]): \(length) chunks") | .[])'
            ;;
        *) die "usage: vol create|attach VM|detach VM|show|delete" ;;
    esac
}

# Chunks whose ISR has fewer members than replicas.
vol_degraded() {
    metactl "$VM_A" "$VM_A" get "$VOL" | jq '[.volume.chunks[] | select((.isr | length) < (.replicas | length))] | length'
}

# Verified writes on the target while fl-b's node is stopped and restarted; then fl-b must
# rejoin every ISR, and a failover to fl-b must read back everything (from fl-b's replica).
cmd_node_fault() {
    local load_pid rc=0 start
    ensure_nodes
    cmd_vol attach a
    log "waiting for full ISRs before the load"
    wait_isr_full
    log "write load with verification on $VM_A; $VM_B's node stops after 10 s, restarts 20 s later"
    ENV_VARS="DEV=/dev/ublkb1" run_detached "$VM_A" "$SCRIPTS/guest/write-load.sh" &
    load_pid=$!
    sleep 10
    cmd_node down b
    sleep 5
    log "$VM_B down: $(vol_degraded) chunks degraded"; cmd_vol show
    sleep 15
    cmd_node up b
    wait "$load_pid" || rc=$?
    ((rc == 0)) || die "write load failed (exit $rc): writes must not fail while a replica is down"
    start=$SECONDS
    while [[ $(vol_degraded) != 0 ]]; do
        ((SECONDS - start < 900)) || die "ISR not full after 15 min"
        sleep 5
    done
    log "every ISR full again $((SECONDS - start)) s after the load ended"; cmd_vol show

    log "failover: stop $VM_A's node, attach $VOL to $VM_B, verify everything there"
    cmd_node down a
    cmd_vol attach b
    ENV_VARS="DEV=/dev/ublkb1 VERIFY_ONLY=1" run_detached "$VM_B" "$SCRIPTS/guest/write-load.sh" ||
        die "verification on $VM_B failed"
    cmd_vol show
    log "OK: no failed writes, ISR restored, and $VM_B serves every byte after the failover"
}

# Starts the nodes of the data VMs that are not running (a running node is left alone: a
# restart would cost a reconciliation).
ensure_nodes() {
    local vm
    for vm in $(vms_of all); do
        vssh "$vm" "systemctl is-active --quiet flaminio-node" || cmd_node up "$(vms_of "$vm")"
    done
}

wait_isr_full() {
    local start=$SECONDS
    while [[ $(vol_degraded) != 0 ]]; do
        ((SECONDS - start < ${1:-900})) || die "ISR not full after ${1:-900} s"
        sleep 5
    done
}

probe() { run_on_vm "$1" "$SCRIPTS/guest/probe.sh" "${@:2}"; }

# Step 5: the target's node (fl-a) is cut off the overlay while a reader and a writer run on
# its device; fl-b is made the target from the majority side. The old target must neither
# return a read it started after its lease ran out, nor acknowledge a write it started after
# the cut; fl-b must see every write fl-a acknowledged. Then the overlay heals: fl-a learns it
# was deposed, and rejoins every ISR as a replica. Checks use one clock at a time.
cmd_partition() {
    local dev=/dev/ublkb1 x=$((1 << 30)) y=$((2 << 30)) lease=5 hold=${HOLD:-30}
    local t_cut t_heal out tok line v first acked

    vssh "$VM_W" "sudo test -f /etc/flaminio/pki/admin.crt" || cmd_pki issue w
    ensure_nodes
    cmd_vol attach a
    log "waiting for full ISRs before the cut"
    wait_isr_full
    out=$(probe "$VM_A" write "$dev" "$x" gen-1)
    [[ $out == *" ok gen-1" ]] || die "write on $VM_A failed: $out"
    probe "$VM_A" start reader reader "$dev" "$x"
    probe "$VM_A" start writer writer "$dev" "$y"
    sleep 5

    # Whatever happens from here, fl-a ends up connected and the probes stopped.
    trap 'warn "healing $VM_A after a failure"; run_on_vm "$VM_A" "$SCRIPTS/guest/partition.sh" off >/dev/null 2>&1;
          probe "$VM_A" stop reader; probe "$VM_A" stop writer' EXIT
    t_cut=$(run_on_vm "$VM_A" "$SCRIPTS/guest/partition.sh" on)
    log "$VM_A cut off the overlay at $t_cut; making $VM_B the target from $VM_W"
    sleep 2
    out=$(metactl "$VM_W" "$VM_W" attach "$VOL" "$VM_B")
    tok=$(jq -r .token <<<"$out")
    log "$VOL attached to $VM_B at epoch $(jq -r .volume.epoch <<<"$out")"
    ENV_VARS="VOLUME=$VOL TOKEN=$tok" run_on_vm "$VM_B" "$SCRIPTS/guest/node.sh" attach

    first=$(probe "$VM_B" read "$dev" "$x")
    log "$VM_B first read: $first"
    [[ $first == *" ok gen-1" ]] || die "$VM_B read $first at offset $x, expected gen-1"
    log "$VM_B served its first read $(awk -v s="${first%% *}" -v t="$t_cut" 'BEGIN {printf "%.1f", s - t}') s after the cut (clocks of two VMs)"
    line=$(probe "$VM_B" read "$dev" "$y")
    [[ $line == *" ok "* ]] || die "$VM_B read $line at offset $y"
    v=${line##* }
    out=$(probe "$VM_B" write "$dev" "$x" gen-2)
    [[ $out == *" ok gen-2" ]] || die "write on $VM_B failed: $out"
    [[ $(probe "$VM_B" read "$dev" "$x") == *" ok gen-2" ]] || die "$VM_B does not read back gen-2"
    log "$VM_B wrote gen-2; holding the partition for ${hold} s"
    sleep "$hold"

    t_heal=$(run_on_vm "$VM_A" "$SCRIPTS/guest/partition.sh" off)
    trap - EXIT
    log "overlay healed at $t_heal"
    local start=$SECONDS
    until vssh "$VM_A" "sudo journalctl -u flaminio-node --since=-10min -o cat | grep -q 'deposed as the target of $VOL'"; do
        ((SECONDS - start < 120)) || die "$VM_A was not told it was deposed within 120 s of the heal"
        sleep 2
    done
    log "$VM_A deposed $((SECONDS - start)) s after the heal"
    sleep 3
    probe "$VM_A" stop reader
    probe "$VM_A" stop writer
    probe "$VM_A" log reader >"$LOCAL_STATE_DIR/partition-reader.log"
    probe "$VM_A" log writer >"$LOCAL_STATE_DIR/partition-writer.log"
    ENV_VARS="VOLUME=$VOL" run_on_vm "$VM_A" "$SCRIPTS/guest/node.sh" detach || true

    # Reads on fl-a: none started after the lease ran out (fl-a's clock) may succeed.
    awk -v t="$t_cut" -v l="$lease" '
        $1 < t && $3 == "ok" {before++}
        $1 >= t && $1 < t + l && $3 == "ok" {within++}
        $1 >= t + l && $3 == "ok" {bad++; print "  VIOLATION: read started " $1 - t " s after the cut returned " $4}
        $3 == "err" {err++}
        END {printf "  reader on fl-a: %d reads before the cut, %d after it within the lease, %d failed, %d after the lease (must be 0)\n", before, within, err, bad; exit bad > 0}
    ' "$LOCAL_STATE_DIR/partition-reader.log" || die "the old target served a read after its lease expired"
    # Writes on fl-a: none started after the cut may be acknowledged; fl-b must hold the last
    # acknowledged one (or the one in flight at the cut).
    acked=$(awk -v t="$t_cut" '
        $3 == "ok" && $1 < t {split($4, p, "-"); last = p[2]}
        $3 == "ok" && $1 >= t {bad++; print "  VIOLATION: write started " $1 - t " s after the cut was acknowledged" > "/dev/stderr"}
        END {print last + 0; exit bad > 0}
    ' "$LOCAL_STATE_DIR/partition-writer.log") || die "the old target acknowledged a write after the cut"
    log "  writer on fl-a: last acknowledged writer-$acked; $VM_B reads $v"
    [[ $v =~ ^writer-([0-9]+)$ ]] || die "$VM_B holds '$v' at offset $y"
    ((BASH_REMATCH[1] == acked || BASH_REMATCH[1] == acked + 1)) ||
        die "$VM_B holds $v, but $VM_A acknowledged writer-$acked: an acknowledged write was lost"

    log "waiting for $VM_A to rejoin every ISR as a replica"
    start=$SECONDS
    wait_isr_full 900
    log "every ISR full again $((SECONDS - start)) s later"
    cmd_vol show
    log "OK: the cut-off target stopped reading at its lease and never acknowledged a write; $VM_B took over with every acknowledged write"
}

# --- Phase 1 step 7: Kubernetes (k3s) over the overlay ---

kubectl_w() { vssh "$VM_W" "sudo k3s kubectl $(printf '%q ' "$@")"; }

# Demo mode (make demo-up): API on the Mac Lima CP; kubeconfig at .lab/kubeconfig.
demo_active() { [[ -f $LOCAL_STATE_DIR/demo/active ]]; }

kubectl_lab() {
    if demo_active; then
        command kubectl --kubeconfig="$LOCAL_STATE_DIR/kubeconfig" "$@"
    else
        kubectl_w "$@"
    fi
}

# Apply a manifest from stdin on the active cluster.
kubectl_apply() {
    if demo_active; then
        kubectl_lab apply -f -
    else
        vssh "$VM_W" "sudo k3s kubectl apply -f -"
    fi
}

# Create a generic secret from files already on this workstation (demo) or on fl-w (lab).
kubectl_create_tls_secret() {
    local name=$1 ns=$2 crt=$3 key=$4 ca=$5
    if demo_active; then
        kubectl_lab create secret generic -n "$ns" "$name" \
            --from-file=tls.crt="$crt" --from-file=tls.key="$key" --from-file=ca.crt="$ca"
    else
        vssh "$VM_W" "sudo k3s kubectl create secret generic -n $ns $name \
            --from-file=tls.crt=$crt --from-file=tls.key=$key --from-file=ca.crt=$ca"
    fi
}

# k8s up|down|status|uninstall|smoke|kubeconfig|tunnel: the k3s server (control plane) on the
# witness, agents on the data VMs.
cmd_k8s() {
    local action=${1:-status} vm token
    case "$action" in
        up)
            vssh "$VM_A" "ping -c 1 -W 2 -q $VM_W_WG >/dev/null" || die "no overlay (make mesh-up)"
            ENV_VARS="K3S_VERSION=$K3S_VERSION NODE_IP=$VM_W_WG" run_on_vm "$VM_W" "$SCRIPTS/guest/k3s.sh" server
            token=$(run_on_vm "$VM_W" "$SCRIPTS/guest/k3s.sh" token)
            for vm in $(vms_of all); do
                ENV_VARS="K3S_VERSION=$K3S_VERSION NODE_IP=$(vmvar "$vm" WG) SERVER_IP=$VM_W_WG TOKEN=$token" \
                    run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" agent
            done
            local start=$SECONDS
            until [[ $(kubectl_w get nodes --no-headers 2>/dev/null | awk '$2 == "Ready"' | wc -l) -eq 3 ]]; do
                ((SECONDS - start < 300)) || die "not every node Ready after 300 s: $(kubectl_w get nodes 2>&1)"
                sleep 5
            done
            cmd_k8s kubeconfig
            cmd_k8s status
            ;;
        down | uninstall)
            [[ $action == uninstall && ${CONFIRM:-} != yes ]] && die "this removes k3s and its state; rerun with CONFIRM=yes"
            for vm in $(vms_of all) "$VM_W"; do run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" "$action"; done
            ;;
        status)
            for vm in $(vms_of cluster); do log "$vm"; run_on_vm "$vm" "$SCRIPTS/guest/k3s.sh" status; done
            kubectl_w get nodes -o wide 2>/dev/null || warn "the API server does not answer"
            kubectl_w get pods -A 2>/dev/null || true
            ;;
        smoke)
            kubectl_w delete namespace flaminio-smoke --ignore-not-found --wait=true >/dev/null
            vssh "$VM_W" "sudo k3s kubectl apply -f -" <"$REPO_ROOT/deploy/lab/k8s-smoke.yaml" >/dev/null
            log "waiting for the client on $VM_B to reach the server on $VM_A"
            if ! kubectl_w wait -n flaminio-smoke --for=jsonpath='{.status.phase}'=Succeeded pod/client --timeout=180s >/dev/null; then
                kubectl_w get pods -n flaminio-smoke -o wide || true
                kubectl_w logs -n flaminio-smoke client || true
                die "the client pod did not succeed"
            fi
            log "client: $(kubectl_w logs -n flaminio-smoke client | tail -1)"
            if vssh "$VM_W" "sudo k3s kubectl run -n flaminio-smoke privileged --image=busybox:1.37 --restart=Never \
                    --overrides='{\"spec\":{\"containers\":[{\"name\":\"p\",\"image\":\"busybox:1.37\",\"securityContext\":{\"privileged\":true}}]}}'" >/dev/null 2>&1; then
                die "a privileged pod was admitted in a restricted namespace"
            fi
            log "a privileged pod in the restricted namespace: refused"
            [[ $(kubectl_w get node "$VM_W" -o jsonpath='{.spec.taints[*].effect}') == *NoSchedule* ]] ||
                die "the witness $VM_W takes workloads"
            log "the witness $VM_W takes no workloads"
            kubectl_w delete namespace flaminio-smoke --wait=false >/dev/null
            log "OK: pods on two servers talk through a service over the overlay"
            ;;
        kubeconfig)
            # For kubectl on this workstation, through `make k8s-tunnel`.
            vssh "$VM_W" "sudo cat /etc/rancher/k3s/k3s.yaml" |
                sed "s#server: https://.*:6443#server: https://127.0.0.1:$K8S_TUNNEL_PORT#" >"$LOCAL_STATE_DIR/kubeconfig"
            chmod 600 "$LOCAL_STATE_DIR/kubeconfig"
            log "wrote $LOCAL_STATE_DIR/kubeconfig (API at 127.0.0.1:$K8S_TUNNEL_PORT while make k8s-tunnel runs)"
            ;;
        tunnel)
            log "API server of $VM_W at 127.0.0.1:$K8S_TUNNEL_PORT; export KUBECONFIG=$LOCAL_STATE_DIR/kubeconfig. Ctrl-C ends it."
            ssh "${VM_SSH_OPTS[@]}" -N -J "$(vm_jump "$VM_W")" -L "$K8S_TUNNEL_PORT:$VM_W_WG:6443" "$VM_USER@$VM_W_IP"
            ;;
        *) die "usage: k8s up|down|status|uninstall|smoke|kubeconfig|tunnel" ;;
    esac
}

# --- Phase 1 step 7: the CSI driver ---

# csi deploy|status|logs|test|failover|undeploy
cmd_csi() {
    local dir="$LOCAL_STATE_DIR/pki" vm tag
    case "${1:-status}" in
        deploy)
            kubectl_lab get nodes >/dev/null 2>&1 || die "no Kubernetes (make k8s-up or make demo-up)"
            # Identities: each data node's agent (node plugin), and the controller's, made on
            # fl-w and kept in a Secret (Kubernetes encrypts Secrets at rest).
            for vm in $(vms_of all); do
                vssh "$vm" "sudo test -f /etc/flaminio/pki/agent.crt" ||
                    issue "$vm" agent "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/node/$vm/agent" --client
            done
            kubectl_apply <"$REPO_ROOT/deploy/csi/rbac.yaml" >/dev/null
            if ! kubectl_lab get secret -n flaminio-system flaminio-csi-controller-tls >/dev/null 2>&1; then
                issue "$VM_W" csi-controller "$dir/flaminio" "spiffe://$TRUST_DOMAIN/flaminio/admin/csi-controller" --client
                if demo_active; then
                    mkdir -p "$LOCAL_STATE_DIR/demo/csi-tls"
                    vssh "$VM_W" "sudo cat /etc/flaminio/pki/csi-controller.crt" >"$LOCAL_STATE_DIR/demo/csi-tls/tls.crt"
                    vssh "$VM_W" "sudo cat /etc/flaminio/pki/csi-controller.key" >"$LOCAL_STATE_DIR/demo/csi-tls/tls.key"
                    vssh "$VM_W" "sudo cat /etc/flaminio/pki/ca.crt" >"$LOCAL_STATE_DIR/demo/csi-tls/ca.crt"
                    kubectl_create_tls_secret flaminio-csi-controller-tls flaminio-system \
                        "$LOCAL_STATE_DIR/demo/csi-tls/tls.crt" \
                        "$LOCAL_STATE_DIR/demo/csi-tls/tls.key" \
                        "$LOCAL_STATE_DIR/demo/csi-tls/ca.crt" >/dev/null
                else
                    kubectl_create_tls_secret flaminio-csi-controller-tls flaminio-system \
                        /etc/flaminio/pki/csi-controller.crt \
                        /etc/flaminio/pki/csi-controller.key \
                        /etc/flaminio/pki/ca.crt >/dev/null
                fi
                log "controller identity in Secret flaminio-system/flaminio-csi-controller-tls"
            fi
            # The image goes straight into each node's containerd: no registry. Controllers run
            # on the witness (fl-w or the Mac demo CP); the node plugin on the data VMs.
            # Lab VMs are amd64; the Lima demo CP is arm64 — build both when demo is active.
            tag="docker.io/flaminio/csi:dev-$(date +%Y%m%d%H%M%S)"
            "$SCRIPTS/dev/csi-image.sh" "$LOCAL_STATE_DIR/csi-image.tar" "$tag"
            if demo_active; then
                for vm in $(vms_of all); do
                    vssh "$vm" "sudo k3s ctr -n k8s.io images import -" <"$LOCAL_STATE_DIR/csi-image.tar" >/dev/null
                    log "$vm: imported $tag (amd64)"
                done
                GOARCH=arm64 "$SCRIPTS/dev/csi-image.sh" "$LOCAL_STATE_DIR/csi-image-arm64.tar" "$tag"
                limactl shell "$DEMO_CP_LIMA" -- sudo k3s ctr -n k8s.io images import - \
                    <"$LOCAL_STATE_DIR/csi-image-arm64.tar" >/dev/null
                log "$DEMO_CP_LIMA: imported $tag (arm64)"
            else
                for vm in $(vms_of cluster); do
                    vssh "$vm" "sudo k3s ctr -n k8s.io images import -" <"$LOCAL_STATE_DIR/csi-image.tar" >/dev/null
                    log "$vm: imported $tag"
                done
            fi
            CSI_IMAGE=$tag META_ENDPOINTS=$(meta_endpoints) render "$REPO_ROOT/deploy/csi/driver.yaml" |
                kubectl_apply
            kubectl_lab rollout status -n flaminio-system deploy/flaminio-csi-controller --timeout=180s
            kubectl_lab rollout status -n flaminio-system deploy/flaminio-failover --timeout=180s
            kubectl_lab rollout status -n flaminio-system ds/flaminio-csi-node --timeout=180s
            ;;
        status)
            kubectl_lab get pods -n flaminio-system -o wide
            kubectl_lab get storageclass,pv,volumeattachments
            kubectl_lab get pvc,pods -n flaminio-test -o wide 2>/dev/null || true
            ;;
        logs)
            kubectl_lab logs -n flaminio-system deploy/flaminio-csi-controller -c flaminio-csi --tail=30
            log "failover controller"
            kubectl_lab logs -n flaminio-system deploy/flaminio-failover --tail=10
            for vm in $(vms_of all); do
                log "node plugin on $vm"
                kubectl_lab logs -n flaminio-system "$(kubectl_lab get pods -n flaminio-system -l app=flaminio-csi-node \
                    --field-selector "spec.nodeName=$vm" -o name)" -c flaminio-csi --tail=30 || true
            done
            ;;
        test) csi_test ;;
        failover) csi_failover ;;
        undeploy)
            kubectl_lab delete namespace flaminio-test --ignore-not-found --wait=true
            if demo_active; then
                render_csi_placeholder | kubectl_lab delete --ignore-not-found -f -
            else
                render_csi_placeholder | vssh "$VM_W" "sudo k3s kubectl delete --ignore-not-found -f -"
            fi
            ;;
        *) die "usage: csi deploy|status|logs|test|failover|undeploy" ;;
    esac
}

# The driver manifest with dummy values, for deleting what it created.
render_csi_placeholder() {
    CSI_IMAGE=none META_ENDPOINTS=none render "$REPO_ROOT/deploy/csi/driver.yaml"
}

# The data VMs' nodes hold one attached volume at a time: the lab's test volume lets go.
release_lab_volume() {
    local vm
    for vm in $(vms_of all); do
        ENV_VARS="VOLUME=$VOL" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" detach 2>/dev/null || true
    done
}

# The test pod's node, and its log's last line.
data_pod_node() { kubectl_lab get pod -n flaminio-test data-0 -o jsonpath='{.spec.nodeName}'; }
data_pod_says() { kubectl_lab logs -n flaminio-test data-0 | tail -1; }

wait_data_pod() {
    kubectl_lab wait -n flaminio-test --for=condition=Ready pod/data-0 --timeout="${1:-300}s" >/dev/null || {
        kubectl_lab describe pod -n flaminio-test data-0 | tail -15
        die "the test pod did not become Ready"
    }
}

# A pod writes 256 MiB with its checksum to a Flaminio volume, then moves to the other node
# (its node cordoned, the pod deleted) and must find the same data there.
csi_test() {
    local first second start
    release_lab_volume
    kubectl_lab delete namespace flaminio-test --ignore-not-found --wait=true >/dev/null
    kubectl_apply <"$REPO_ROOT/deploy/lab/csi-test.yaml" >/dev/null
    start=$SECONDS
    wait_data_pod 300
    first=$(data_pod_node)
    log "pod Ready on $first after $((SECONDS - start)) s: $(data_pod_says)"
    [[ $(data_pod_says) == "created on $first" ]] || die "unexpected first start: $(data_pod_says)"
    log "volume $(kubectl_lab get pvc -n flaminio-test data-data-0 -o jsonpath='{.spec.volumeName}'): $(
        metactl "$VM_A" "$VM_A" get "$(kubectl_lab get pvc -n flaminio-test data-data-0 -o jsonpath='{.spec.volumeName}')" |
            jq -r '"epoch \(.volume.epoch), target \(.volume.target), replicas \([.volume.chunks[].replicas | join("+")] | unique | join(" "))"')"

    log "moving the pod off $first"
    kubectl_lab cordon "$first" >/dev/null
    trap 'kubectl_lab uncordon "'"$first"'" >/dev/null 2>&1' EXIT
    kubectl_lab delete pod -n flaminio-test data-0 --wait=true >/dev/null
    start=$SECONDS
    wait_data_pod 300
    second=$(data_pod_node)
    kubectl_lab uncordon "$first" >/dev/null
    trap - EXIT
    log "pod Ready on $second after $((SECONDS - start)) s: $(data_pod_says)"
    [[ $second != "$first" ]] || die "the pod did not move"
    [[ $(data_pod_says) == "verified on $second" ]] || die "after the move: $(data_pod_says)"
    log "OK: provisioned, attached, formatted, written on $first; moved to $second with its data intact"
    [[ ${KEEP:-0} == 1 ]] || {
        kubectl_lab delete namespace flaminio-test --wait=true >/dev/null
        log "test namespace and volume deleted"
    }
}

# Seconds since the test started, for the timeline.
since() { echo "$((SECONDS - ${1:?}))s"; }

# The test pod's data VM is powered off. The failover controller must mark it out of service,
# and the pod come back on the other data VM with its data. Then the VM is started again, its
# node rejoins, and the mark comes off.
csi_failover() {
    local lost other start vol taint
    if ! kubectl_lab get pod -n flaminio-test data-0 >/dev/null 2>&1; then
        release_lab_volume
        kubectl_apply <"$REPO_ROOT/deploy/lab/csi-test.yaml" >/dev/null
    fi
    wait_data_pod 300
    lost=$(data_pod_node)
    other=$(vms_of all | tr ' ' '\n' | grep -vx "$lost")
    vol=$(kubectl_lab get pvc -n flaminio-test data-data-0 -o jsonpath='{.spec.volumeName}')
    log "pod on $lost ($(data_pod_says)); volume $vol"

    log "powering $lost off (virsh destroy: no shutdown, no flush)"
    hsh "$(vmvar "$lost" HOST)" "virsh -c qemu:///system destroy $lost" >/dev/null
    start=$SECONDS
    # Restore the lab whatever happens.
    trap 'warn "starting $lost again after a failure"; hsh "$(vmvar "'"$lost"'" HOST)" "virsh -c qemu:///system start '"$lost"'" >/dev/null 2>&1' EXIT

    until [[ $(kubectl_lab get node "$lost" -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}') != True ]]; do
        ((SECONDS - start < 300)) || die "$lost still Ready after 300 s"
        sleep 2
    done
    log "$(since "$start"): Kubernetes sees $lost not Ready"
    until taint=$(kubectl_lab get node "$lost" -o jsonpath='{.spec.taints[*].key}') && [[ $taint == *out-of-service* ]]; do
        ((SECONDS - start < 400)) || die "$lost not marked out of service after 400 s"
        sleep 2
    done
    log "$(since "$start"): the failover controller marked $lost out of service"
    until [[ $(data_pod_node 2>/dev/null) == "$other" ]] &&
        kubectl_lab wait -n flaminio-test --for=condition=Ready pod/data-0 --timeout=5s >/dev/null 2>&1; do
        ((SECONDS - start < 900)) || {
            kubectl_lab describe pod -n flaminio-test data-0 | tail -15
            die "the pod is not Ready on $other after 900 s"
        }
        sleep 3
    done
    log "$(since "$start"): pod Ready on $other: $(data_pod_says)"
    [[ $(data_pod_says) == "verified on $other" ]] || die "after the failover: $(data_pod_says)"
    log "volume $vol: $(metactl "$VM_W" "$VM_W" get "$vol" | jq -r '"epoch \(.volume.epoch), target \(.volume.target), ISR \([.volume.chunks[].isr | join("+")] | unique | join(" "))"')"

    log "starting $lost again"
    hsh "$(vmvar "$lost" HOST)" "virsh -c qemu:///system start $lost" >/dev/null
    trap - EXIT
    wait_for "SSH on $lost" 300 vssh "$lost" true
    cmd_node up "$(vms_of "$lost")"
    start=$SECONDS
    until [[ $(kubectl_lab get node "$lost" -o jsonpath='{.spec.taints[*].key}') != *out-of-service* ]]; do
        ((SECONDS - start < 300)) || die "the out-of-service mark is still on $lost after 300 s"
        sleep 3
    done
    log "$(since "$start"): $lost is back, and the mark is off"
    until metactl "$VM_W" "$VM_W" get "$vol" |
        jq -e '[.volume.chunks[] | select((.isr | length) < (.replicas | length))] | length == 0' >/dev/null; do
        ((SECONDS - start < 900)) || die "$vol's ISRs are not full after 900 s"
        sleep 5
    done
    log "$(since "$start"): $lost's replicas of $vol are back in every ISR"
    log "OK: $lost lost without warning; the pod came back on $other with its data; $lost rejoined"
    [[ ${KEEP:-0} == 1 ]] || {
        kubectl_lab delete namespace flaminio-test --wait=true >/dev/null
        log "test namespace and volume deleted"
    }
}

# --- Phase 1 step 6: the node agent's disks, and drains ---

# agent disks|claim VM: builds flaminio-agent for Linux, installs it, and runs it in the VM.
cmd_agent() {
    local action=${1:-disks} vm bin="$LOCAL_STATE_DIR/bin/flaminio-agent"
    vm=$(vms_of "${2:-c}")
    (cd "$REPO_ROOT" && GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -o "$bin" ./control/cmd/flaminio-agent)
    vssh "$vm" 'f=$(mktemp) && cat >"$f" && sudo install -m 755 "$f" /usr/local/bin/flaminio-agent; rc=$?; rm -f "$f"; exit $rc' <"$bin"
    run_on_vm "$vm" "$SCRIPTS/guest/agent.sh" "$action"
}

# fl-c from nothing to a running data node: VM, overlay, identities, build, its disk claimed by
# the node agent, and its node (replicas only).
cmd_data_node_up() {
    local dev
    cmd_create c
    cmd_provision c
    cmd_net_setup
    cmd_mesh up
    cmd_pki issue c
    cmd_deploy c
    log "$VM_C's devices, as the node agent sees them:"
    cmd_agent disks c
    dev=$(cmd_agent claim c | tail -1)
    [[ $dev == /dev/* ]] || die "the node agent claimed no disk on $VM_C"
    log "$VM_C's data disk: $dev"
    vssh "$VM_C" "sudo modprobe ublk_drv"
    ENV_VARS="$(node_env "$VM_C") DEVICE=$dev" run_on_vm "$VM_C" "$SCRIPTS/guest/node.sh" up
}

# Replica nodes on fl-c too, if it exists (it is started like the others, never reformatted).
ensure_node_c() {
    [[ -f $LOCAL_STATE_DIR/$VM_C/created ]] || die "no $VM_C: make data-node-up"
    if ! vssh "$VM_C" "systemctl is-active --quiet flaminio-node"; then
        local dev
        dev=$(cmd_agent claim c | tail -1)
        ENV_VARS="$(node_env "$VM_C") DEVICE=$dev" run_on_vm "$VM_C" "$SCRIPTS/guest/node.sh" up
    fi
}

# Chunks of the test volume with a replica on node $1.
chunks_on() {
    metactl "$VM_A" "$VM_A" get "$VOL" | jq --arg n "$1" '[.volume.chunks[] | select(.replicas | index($n))] | length'
}

# Step 6: fio writes and verifies on the target (fl-a) while every replica on fl-b moves to
# fl-c. Then fl-c alone must serve every byte: fl-a's and fl-b's nodes stop, the volume fails
# over to fl-c, and fio verifies there. Finally the replicas move back (fl-c drained to fl-b),
# which restores the usual layout and drains a second time.
cmd_drain_test() {
    local load_pid rc=0 start
    ensure_nodes
    ensure_node_c
    cmd_vol attach a
    log "waiting for full ISRs before the drain"
    wait_isr_full
    log "$VOL: $(chunks_on "$VM_B") chunks on $VM_B, $(chunks_on "$VM_C") on $VM_C"

    log "verified write load on $VM_A; draining $VM_B into $VM_C meanwhile"
    ENV_VARS="DEV=/dev/ublkb1" run_detached "$VM_A" "$SCRIPTS/guest/write-load.sh" &
    load_pid=$!
    sleep 5
    start=$SECONDS
    metactl "$VM_A" "$VM_A" drain "$VM_B" >/dev/null
    log "$VM_B drained in $((SECONDS - start)) s"
    wait "$load_pid" || rc=$?
    ((rc == 0)) || die "write load failed (exit $rc): writes must not fail during a drain"
    [[ $(chunks_on "$VM_B") == 0 ]] || die "$VM_B still holds replicas of $VOL"
    wait_isr_full 300
    cmd_vol show

    log "nothing may depend on $VM_B now: stopping it and $VM_A, failing over to $VM_C"
    cmd_node down b
    cmd_node down a
    cmd_vol attach c
    ENV_VARS="DEV=/dev/ublkb1 VERIFY_ONLY=1" run_detached "$VM_C" "$SCRIPTS/guest/write-load.sh" ||
        die "verification on $VM_C failed"
    log "$VM_C served every byte from its own replicas"

    log "restoring: nodes back, target on $VM_A, replicas moved from $VM_C back to $VM_B"
    ENV_VARS="VOLUME=$VOL" run_on_vm "$VM_C" "$SCRIPTS/guest/node.sh" detach || true
    cmd_node up a
    cmd_node up b
    metactl "$VM_A" "$VM_A" undrain "$VM_B" >/dev/null
    cmd_vol attach a
    wait_isr_full
    start=$SECONDS
    metactl "$VM_A" "$VM_A" drain "$VM_C" >/dev/null
    log "$VM_C drained in $((SECONDS - start)) s"
    metactl "$VM_A" "$VM_A" undrain "$VM_C" >/dev/null
    wait_isr_full 300
    cmd_vol show
    log "OK: drained under a verified write load, $VM_C alone served every byte, and the replicas moved back"
}

# --- Phase 1 step 8: benchmark and chaos ---

# The fio image: fio built statically in fl-a, on the pinned base, imported on every node.
fio_image() {
    local bin tag="docker.io/flaminio/fio:$FIO_VERSION"
    if ! vssh "$VM_A" "sudo k3s ctr -n k8s.io images ls -q | grep -qx $tag" 2>/dev/null; then
        bin=$(ENV_VARS="FIO_VERSION=$FIO_VERSION" run_on_vm "$VM_A" "$SCRIPTS/guest/fio-static.sh" | tail -1)
        vssh "$VM_A" "cat $bin" >"$LOCAL_STATE_DIR/bin/fio"
        "$SCRIPTS/dev/image.sh" "$LOCAL_STATE_DIR/fio-image.tar" "$tag" "$LOCAL_STATE_DIR/bin/fio" >&2
        local vm
        for vm in $(vms_of all); do
            vssh "$vm" "sudo k3s ctr -n k8s.io images import -" <"$LOCAL_STATE_DIR/fio-image.tar" >/dev/null
        done
    fi
    echo "$tag"
}

# Splits the pod's log into one fio JSON file per job (BEGIN name ... END; anything fio prints
# before its JSON is dropped), and prints a row per read job: job, IOPS, MiB/s, p50 and p99
# completion latency (us).
fio_table() {
    awk '/^BEGIN /{n=$2; f="'"$LOCAL_STATE_DIR"'/bench/" n ".json"; printf "" > f; json=0; next}
         /^END /{close(f); f=""; next} f && /^\{/ {json=1} f && json {print >> f}' "$1"
    local j
    for j in randread-4k-qd1 randread-4k-qd32 read-1m-qd8; do
        jq -r --arg n "$j" '.jobs[0].read | [$n, (.iops|floor), ((.bw_bytes/1048576)*10|floor/10),
            ((.clat_ns.percentile["50.000000"] // 0)/1000|floor), ((.clat_ns.percentile["99.000000"] // 0)/1000|floor)] | @tsv' \
            "$LOCAL_STATE_DIR/bench/$j.json"
    done
}

# Reads from a pod with a Flaminio volume on NODE (default b), against the raw disk of that node
# the same day. RUNTIME seconds per job.
cmd_bench_pod() {
    local node=${NODE:-$VM_B} image runtime=${RUNTIME:-30} log="$LOCAL_STATE_DIR/bench/pod.log" start
    mkdir -p "$LOCAL_STATE_DIR/bench"
    image=$(fio_image)
    release_lab_volume
    kubectl_lab delete namespace flaminio-bench --ignore-not-found --wait=true >/dev/null
    NODE=$node FIO_IMAGE=$image SIZE=16Gi RUNTIME=$runtime render "$REPO_ROOT/deploy/lab/bench-pod.yaml" |
        kubectl_apply >/dev/null
    log "fio pod on $node: fills 16 GiB once, then reads ${runtime}s per job"
    start=$SECONDS
    until [[ $(kubectl_lab get pod -n flaminio-bench fio -o jsonpath='{.status.phase}' 2>/dev/null) =~ ^(Succeeded|Failed)$ ]]; do
        ((SECONDS - start < 1800)) || die "the benchmark pod did not finish in 30 min"
        sleep 10
    done
    kubectl_lab logs -n flaminio-bench fio >"$log"
    [[ $(kubectl_lab get pod -n flaminio-bench fio -o jsonpath='{.status.phase}') == Succeeded ]] ||
        { tail -20 "$log"; die "the benchmark pod failed"; }
    local vol
    vol=$(kubectl_lab get pvc -n flaminio-bench bench -o jsonpath='{.spec.volumeName}')
    log "volume $vol: $(metactl "$VM_A" "$VM_A" get "$vol" | jq -r --arg n "$node" \
        '"target \(.volume.target), \([.volume.chunks[] | select(.replicas | index($n))] | length) of \(.volume.chunks | length) chunks with a replica on \($n)"')"
    fio_table "$log" >"$LOCAL_STATE_DIR/bench/pod.tsv"
    jq -r '.jobs[0].write | "fill: \(.io_bytes/1073741824|floor) GiB at \(.bw_bytes/1048576|floor) MiB/s (writes: indicative only)"' \
        "$LOCAL_STATE_DIR/bench/fill.json"

    # The raw disk, read only. Never precondition it here: that would overwrite the store.
    if vssh "$node" "test -f ~/.flaminio-preconditioned-\$(lsblk -dn -o NAME,SERIAL | awk '\$2 ~ /data/ {print \$1; exit}')"; then
        log "raw data disk of $node, same jobs"
        run_on_vm "$node" "$SCRIPTS/guest/fio-raw.sh" "$runtime" '^(randread-4k-qd1|randread-4k-qd32|read-1m-qd8)$' |
            awk 'NF == 5 && $1 ~ /^(randread|read)/ {print $1 "\t" $2 "\t" $3 "\t" $4 "\t" $5}' >"$LOCAL_STATE_DIR/bench/raw.tsv"
    else
        warn "$node's raw disk was never preconditioned: no raw comparison (make bench-raw first, before node-up)"
        : >"$LOCAL_STATE_DIR/bench/raw.tsv"
    fi
    printf '%-18s %10s %10s %10s %10s %10s %8s\n' job IOPS MiB/s "p50(us)" "p99(us)" "raw IOPS" share
    join -t $'\t' -a 1 "$LOCAL_STATE_DIR/bench/pod.tsv" "$LOCAL_STATE_DIR/bench/raw.tsv" |
        awk -F'\t' '{share = ($6 > 0) ? sprintf("%d%%", 100 * $2 / $6) : "-";
            printf "%-18s %10s %10s %10s %10s %10s %8s\n", $1, $2, $3, $4, $5, ($6 ? $6 : "-"), share}'
    [[ ${KEEP:-0} == 1 ]] || kubectl_lab delete namespace flaminio-bench --wait=false >/dev/null
}

# One fault on the replica node $1 (not the target), for about 15–25 s; prints what it was.
chaos_fault() {
    local other=$1 kind=$((RANDOM % 4))
    case $kind in
        0)
            echo "restart of $other's node"
            cmd_node down "$(vms_of "$other")" >/dev/null 2>&1
            sleep 10
            cmd_node up "$(vms_of "$other")" >/dev/null 2>&1
            ;;
        1)
            echo "$other cut off the overlay for 15 s"
            run_on_vm "$other" "$SCRIPTS/guest/partition.sh" on >/dev/null
            sleep 15
            run_on_vm "$other" "$SCRIPTS/guest/partition.sh" off >/dev/null
            ;;
        2)
            echo "$VM_W's etcd member and metadata service down for 20 s"
            cmd_meta down w >/dev/null 2>&1
            cmd_etcd down w >/dev/null 2>&1
            sleep 20
            cmd_etcd up w >/dev/null 2>&1
            cmd_meta up w >/dev/null 2>&1
            ;;
        3)
            echo "100 ms on every packet to and from $other for 20 s"
            run_on_vm "$other" "$SCRIPTS/guest/chaos.sh" slow on 100ms
            sleep 20
            run_on_vm "$other" "$SCRIPTS/guest/chaos.sh" slow off
            ;;
    esac
}

# Step 8: ROUNDS rounds (default 4). Each: a verified fio write load on the target while random
# faults hit the rest (replica restarts, partitions, a metadata member down, slow links); the
# load must complete with no errors. Then the target's node is killed (SIGKILL), the volume
# fails over to the other replica, and fio verifies every byte there. The new target starts
# the next round.
cmd_chaos() {
    local rounds=${ROUNDS:-4} target=$VM_A other=$VM_B r load_pid rc start faults t0 summary=()
    ensure_nodes
    trap 'warn "chaos run interrupted: healing"; for v in $(vms_of all); do
            run_on_vm "$v" "$SCRIPTS/guest/partition.sh" off >/dev/null 2>&1
            run_on_vm "$v" "$SCRIPTS/guest/chaos.sh" slow off >/dev/null 2>&1; done' EXIT
    # Attached once: after each failover the new target is already serving, and another attach
    # would depose it.
    cmd_vol attach "${target#fl-}" >/dev/null
    for r in $(seq 1 "$rounds"); do
        log "round $r/$rounds: target $target, replica $other"
        wait_isr_full
        ENV_VARS="DEV=/dev/ublkb1" run_detached "$target" "$SCRIPTS/guest/write-load.sh" >"$LOCAL_STATE_DIR/chaos-load.log" 2>&1 &
        load_pid=$!
        faults=0
        sleep 5
        while kill -0 "$load_pid" 2>/dev/null; do
            log "  fault: $(chaos_fault "$other")"
            faults=$((faults + 1))
            sleep 5
        done
        rc=0
        wait "$load_pid" || rc=$?
        ((rc == 0)) || { cat "$LOCAL_STATE_DIR/chaos-load.log"; die "round $r: the write load failed under faults"; }
        log "  load: $(grep -o 'wrote .*' "$LOCAL_STATE_DIR/chaos-load.log")"
        # A failover needs an in-sync copy elsewhere: a replica still resyncing has none.
        t0=$SECONDS
        wait_isr_full
        log "  every ISR full $((SECONDS - t0)) s after the load"

        log "  killing $target's node (SIGKILL); failing over to $other"
        t0=$SECONDS
        run_on_vm "$target" "$SCRIPTS/guest/chaos.sh" kill-node
        cmd_vol attach "${other#fl-}" >/dev/null
        start=$((SECONDS - t0))
        ENV_VARS="DEV=/dev/ublkb1 VERIFY_ONLY=1" run_detached "$other" "$SCRIPTS/guest/write-load.sh" \
            >"$LOCAL_STATE_DIR/chaos-verify.log" 2>&1 ||
            { cat "$LOCAL_STATE_DIR/chaos-verify.log"; die "round $r: verification on $other failed"; }
        log "  attached on $other ${start} s after the kill; $(grep -o 'verified .*' "$LOCAL_STATE_DIR/chaos-verify.log")"
        summary+=("round $r: $faults faults during the load, failover $target -> $other in ${start} s, every byte verified")
        cmd_node up "$(vms_of "$target")" >/dev/null 2>&1
        local tmp=$target
        target=$other
        other=$tmp
    done
    trap - EXIT
    # Back to the usual layout, without deposing a target that is already there.
    [[ $target == "$VM_A" ]] || cmd_vol attach a >/dev/null
    wait_isr_full
    printf '  %s\n' "${summary[@]}"
    log "OK: $rounds rounds of faults under a verified write load, each ending in a crash and a verified failover"
}

# --- Reads against the raw disk, by the rules of docs/performance-plan.md ---

host_drop_caches() { hsh "$(vmvar "$1" HOST)" 'sync; echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null'; }

# The host's data drive temperature (°C), where smartctl is there; "-" otherwise.
drive_temp() {
    hsh "$(vmvar "$1" HOST)" 'd=$(ls /dev/nvme0 2>/dev/null); [ -n "$d" ] && command -v smartctl >/dev/null &&
        sudo -n smartctl -A "$d" 2>/dev/null | awk "/^Temperature:/ {print \$2; exit}"' 2>/dev/null | grep . || echo -
}

# MODE=zero-copy|verify, NODE (fl-b), REPS (3), RUNTIME (30): the volume VOL (vol1) on NODE
# against NODE's raw data disk where the volume's data is: a span of the volume's size, from the
# start of the range a traced read of the volume found (docs/issues/9). FILL=1 writes the volume
# again first. The node runs with QUEUES ublk queues (1) and POLL_US of polling (0); fio runs
# NUMJOBS jobs (1) on both sides.
cmd_bench_reads() {
    local node mode=${MODE:-zero-copy} reps=${REPS:-3} runtime=${RUNTIME:-30} span size
    local dir raw zc=0 r spec name rw bs qd side dev off jspan row t0 t1 since
    local meta range first end total disk_bytes reference roff rlen where ncpu hwq cpus
    node=$(vms_of "${NODE:-b}")
    local nv=0 flags want queues=${QUEUES:-1} poll=${POLL_US:-0} jobs=${NUMJOBS:-1} defer=${DEFER_TASKRUN:-0}
    case $mode in
        zero-copy) zc=1 ;;
        verify) ;;
        no-verify) nv=1 ;; # an experiment: the copy path without checksum checks (R1)
        *) die "MODE is zero-copy, verify or no-verify" ;;
    esac
    want=""
    ((zc)) && want+="--zero-copy "
    ((nv)) && want+="--no-verify "
    ((queues != 1)) && want+="--queues $queues "
    ((poll != 0)) && want+="--poll-us $poll "
    ((defer)) && want+="--defer-taskrun "
    dir="$LOCAL_STATE_DIR/bench/reads-$mode-q$queues-poll$poll-jobs$jobs-defer$defer-$(date +%Y%m%dT%H%M%S)"
    mkdir -p "$dir"
    release_lab_volume
    # The node runs in the mode asked for: restart it if not.
    flags=$(vssh "$node" "systemctl show -p ExecStart --value flaminio-node | grep -o -- '--zero-copy\|--no-verify\|--queues [0-9]*\|--poll-us [0-9]*\|--defer-taskrun' | tr '\n' ' '" || true)
    if [[ $flags != "$want" ]] || ! vssh "$node" "systemctl is-active --quiet flaminio-node"; then
        log "$node: node in $mode mode"
        ZERO_COPY=$zc NO_VERIFY=$nv QUEUES=$queues POLL_US=$poll DEFER_TASKRUN=$defer cmd_node up "$(vms_of "$node")" >/dev/null
    fi
    ensure_nodes
    cmd_vol attach "${node#fl-}" >/dev/null
    # Local reads only: every chunk must have a replica on the target node. After a write
    # campaign the restore can leave the ISR elsewhere; measuring then reads over the wire.
    local n_chunks local_n
    meta=$(metactl "$VM_A" "$VM_A" get "$VOL")
    n_chunks=$(jq '.volume.chunks | length' <<<"$meta")
    size=$(jq -r '.volume.sizeBytes' <<<"$meta")
    if ! [[ $size =~ ^[1-9][0-9]*$ ]] || ((size % (1 << 30))); then
        die "$VOL: its size ($size bytes) is not a whole number of GiB"
    fi
    span=$((size >> 30))G
    local_n=$(chunks_on "$node")
    [[ $local_n == "$n_chunks" ]] ||
        die "$VOL: only $local_n/$n_chunks chunks have a replica on $node (need local ISR for read benches)"
    since=$(vssh "$node" "date '+%Y-%m-%d %H:%M:%S'")
    raw=$(run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" disk)
    if [[ ${FILL:-0} == 1 ]] || ! vssh "$node" "test -f ~/.flaminio-bench-filled-$VOL"; then
        log "filling $VOL ($span, fio checksums in every block): once"
        run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" fill /dev/ublkb1 "$span"
        vssh "$node" "touch ~/.flaminio-bench-filled-$VOL"
    fi
    log "warm-up read of $VOL (waits for reconciliation after a restart)"
    run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" warm /dev/ublkb1 "$span"
    # Raw reads where the volume's data is: a traced read of all of it finds the range.
    log "where $VOL's data is on $raw: a read of all of it, traced"
    range=$(run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" range /dev/ublkb1 "$span" "$raw")
    vssh "$node" "cat /tmp/bench-range-regions.txt" >"$dir/regions.txt"
    read -r first end total disk_bytes <<<"$range"
    reference=$(bench_reference "$span" "$first" "$end" "$total" "$disk_bytes")
    read -r roff rlen <<<"$reference"
    where="$VOL: data at GiB $first-$end of $raw ($((total >> 20)) MiB read there, regions.txt); raw reads GiB $roff-$((roff + rlen))"
    ncpu=$(vssh "$node" nproc)
    hwq=$(vssh "$node" "ls /sys/block/${raw##*/}/mq | wc -l")
    cpus="$ncpu vCPUs, $queues ublk queue(s), $raw with $hwq hardware queue(s)"
    log "$where"
    log "$cpus"

    local filter=${JOBS:-.}
    log "$node, $mode, $queues queue(s), ${poll} us polling, $jobs fio job(s), defer=$defer, filter=$filter, $ncpu vCPUs, $reps repetitions, ${runtime}s per job, caches dropped before each"
    printf 'rep\tside\tjob\tIOPS\tMiB/s\tp50\tp99\tfio_cpu\tqueue_cpu\ttemp\n' >"$dir/runs.tsv"
    for r in $(seq 1 "$reps"); do
        for spec in "randread-4k-qd1 randread 4k 1" "randread-4k-qd32 randread 4k 32" "read-1m-qd8 read 1m 8"; do
            read -r name rw bs qd <<<"$spec"
            [[ $name =~ $filter ]] || continue
            for side in raw flaminio; do
                if [[ $side == raw ]]; then
                    dev=$raw off=${roff}G jspan=${rlen}G
                else
                    dev=/dev/ublkb1 off=0 jspan=$span
                fi
                host_drop_caches "$node"
                t0=$(drive_temp "$node")
                row=$(run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" job "$dev" "$off" "$jspan" "$name" "$rw" "$bs" "$qd" \
                    "$runtime" "/tmp/bench-$side-$name-$r.json" "$jobs")
                t1=$(drive_temp "$node")
                vssh "$node" "cat /tmp/bench-$side-$name-$r.json" >"$dir/$side-$name-$r.json"
                printf '%s\t%s\t%s\t%s\n' "$r" "$side" "$row" "$t0/$t1" >>"$dir/runs.tsv"
                log "  rep $r $side: $(cut -f1-5,7 <<<"$row" | tr '\t' ' ')"
            done
        done
    done
    bench_summary "$dir/runs.tsv" | tee "$dir/summary.txt"
    printf '  %s\n' "$where" "$cpus" | tee -a "$dir/summary.txt"

    log "integrity: fio reads all of $VOL and checks every block's checksum ($mode path)"
    run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" verify /dev/ublkb1 "$span" | tee -a "$dir/summary.txt"
    if [[ $mode == verify ]]; then
        local bad
        bad=$(vssh "$node" "sudo journalctl -u flaminio-node --since '$since' -o cat | grep -c -i -E 'local read failed|checksum' || true")
        echo "  Flaminio checksum failures during the run: $bad" | tee -a "$dir/summary.txt"
        [[ $bad == 0 ]] || die "checksum failures: see the node's journal"
    fi
    log "results in $dir"
}

# Median and spread ((max - min) / median) per side and job, and Flaminio's share of raw.
bench_summary() {
    awk -F'\t' 'NR > 1 {
            k = $3 "\t" $2; n[k]++; v[k, n[k]] = $4; bw[k, n[k]] = $5; p99[k, n[k]] = $7; q[k] = q[k] " " $9
            jobs[$3] = 1
        }
        function med(a, k, c,   i, j, t, x) {
            for (i = 1; i <= c; i++) x[i] = a[k, i]
            for (i = 1; i <= c; i++) for (j = i + 1; j <= c; j++) if (x[j] < x[i]) { t = x[i]; x[i] = x[j]; x[j] = t }
            return (c % 2) ? x[(c + 1) / 2] : (x[c / 2] + x[c / 2 + 1]) / 2
        }
        function spread(a, k, c,   i, lo, hi) {
            lo = hi = a[k, 1]
            for (i = 2; i <= c; i++) { if (a[k, i] < lo) lo = a[k, i]; if (a[k, i] > hi) hi = a[k, i] }
            return lo ? 100 * (hi - lo) / med(a, k, c) : 0
        }
        END {
            printf "%-17s %10s %7s %10s %7s %7s %9s %s\n", "job", "Flaminio", "spread", "raw", "spread", "share", "p99 (us)", "queue thread CPU %"
            split("randread-4k-qd1 randread-4k-qd32 read-1m-qd8", order, " ")
            for (o = 1; o <= 3; o++) {
                j = order[o]; f = j "\tflaminio"; r = j "\traw"
                if (!(j in jobs)) continue
                metric = (j ~ /1m/) ? "bw" : "v"
                if (metric == "bw") { fm = med(bw, f, n[f]); rm = med(bw, r, n[r]); fs = spread(bw, f, n[f]); rs = spread(bw, r, n[r]); unit = " MiB/s" }
                else { fm = med(v, f, n[f]); rm = med(v, r, n[r]); fs = spread(v, f, n[f]); rs = spread(v, r, n[r]); unit = "" }
                flag = (fs > 10 || rs > 10) ? "  UNSTABLE (spread > 10%)" : ""
                printf "%-17s %10d %6.1f%% %10d %6.1f%% %6.0f%% %9d %s%s%s\n", j, fm, fs, rm, rs, 100 * fm / rm, med(p99, f, n[f]), q[f], unit, flag
            }
        }' "$1"
}

# --- Writes, by the rules of docs/performance-plan.md (W1–W3) ---

# W1: the empty state, before every write run. The data disk is discarded in the guest (QEMU
# punches holes in the image), the host trims its filesystem and preallocates the image again,
# then everything rests for IDLE seconds and the caches are dropped. For Flaminio, the store is
# then formatted and a fresh 16 GiB volume created (RF replicas, one on NODE) and attached.
write_reset() {
    local node=$1 side=$2 rf=$3 host
    host=$(vmvar "$node" HOST)
    # A connection lost mid-run can leave fio running in the VM.
    vssh "$node" "sudo pkill -x fio; while pgrep -x fio >/dev/null; do sleep 1; done" || true
    ENV_VARS="VOLUME=wbench" run_on_vm "$node" "$SCRIPTS/guest/node.sh" detach >/dev/null 2>&1 || true
    delete_wbench
    cmd_node down "$(vms_of "$node")" >/dev/null
    run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" discard 2>/dev/null
    ENV_VARS="IMAGE=$LIBVIRT_POOL_PATH/$node-data.img" run_on_host "$host" "$SCRIPTS/host/reset-disk.sh" reset
    sleep "${IDLE:-300}"
    host_drop_caches "$node"
    [[ $side == flaminio ]] && write_volume "$node" "$rf"
    return 0
}

# Detaches wbench wherever it is attached, deletes it from the metadata service, then its
# replicas from every data node: a volume's replicas outlive its metadata, and a new wbench
# (epoch 1 again) would be deposed by old replicas fenced at a later epoch.
delete_wbench() {
    local vm
    for vm in $(vms_of all) "$VM_C"; do
        ENV_VARS="VOLUME=wbench" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" detach >/dev/null 2>&1 || true
    done
    metactl "$VM_A" "$VM_A" delete wbench >/dev/null 2>&1 || true
    for vm in $(vms_of all) "$VM_C"; do
        ENV_VARS="VOLUME=wbench" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" delete >/dev/null
    done
}

# A freshly formatted store on NODE, and a new 16 GiB volume wbench (RF replicas, one on NODE)
# attached there.
write_volume() {
    local node=$1 rf=$2
    FORMAT=1 cmd_node up "$(vms_of "$node")" >/dev/null
    metactl "$VM_A" "$VM_A" create wbench 16G "$rf" 1 256M "$node" >/dev/null
    attach_wbench "$node"
}

attach_wbench() {
    local node=$1 out tok
    out=$(metactl "$VM_A" "$VM_A" attach wbench "$node")
    tok=$(jq -r .token <<<"$out")
    ENV_VARS="VOLUME=wbench TOKEN=$tok" run_on_vm "$node" "$SCRIPTS/guest/node.sh" attach >/dev/null
}

# The integrity check after each Flaminio run (the plan's second run, for writes): fio writes
# the volume with a checksum in every block and reads it back; then the node restarts, which
# replays the store's journal, on the verify path (no zero copy: every read is checked against
# the store's checksums too), and fio checks the random writes again. Any error stops the
# campaign.
#
# Returns 0 if the data checked out, 1 if fio or the checksums found an error, and 2 if the check
# did not complete (a lost connection, a node that did not come back). It runs where `set -e` is
# ignored (its caller tests it), so every step checks its own status.
write_integrity() {
    local node=$1 since bad status
    since=$(vssh "$node" "date '+%Y-%m-%d %H:%M:%S'") || return 2
    run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" integrity-write /dev/ublkb1 16G || {
        status=$?
        ((status == 1)) && return 1 # fio's check failed (bench-writes.sh exits 1 only then)
        return 2
    }
    ENV_VARS="VOLUME=wbench" run_on_vm "$node" "$SCRIPTS/guest/node.sh" detach >/dev/null 2>&1 || true
    cmd_node down "$(vms_of "$node")" >/dev/null || return 2
    ZERO_COPY=0 cmd_node up "$(vms_of "$node")" >/dev/null || return 2
    attach_wbench "$node" || return 2
    run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" integrity-read /dev/ublkb1 16G || {
        status=$?
        ((status == 1)) && return 1
        return 2
    }
    bad=$(vssh "$node" "sudo journalctl -u flaminio-node --since '$since' -o cat | grep -c -i -E 'local read failed|checksum' || true") ||
        return 2
    [[ $bad =~ ^[0-9]+$ ]] || return 2
    echo "  Flaminio checksum failures: $bad"
    ((bad == 0)) || return 1
}

# bench-writes with RETRIES: a campaign cut off (by a lost connection) is resumed, at most
# RETRIES times. A failed integrity check is never retried.
cmd_bench_writes_retrying() {
    local dir try
    dir=${RESUME:-$LOCAL_STATE_DIR/bench/writes-rf${RF:-1}-$(date +%Y%m%dT%H%M%S)}
    for try in $(seq 0 "$RETRIES"); do
        ((try > 0)) && { warn "campaign cut off; resuming in 2 minutes (retry $try of $RETRIES)"; sleep 120; }
        local status=0
        RESUME=$dir RETRIES=0 "$0" bench-writes || status=$?
        ((status == 0)) && return 0
        [[ -e $dir/INTEGRITY_FAILED ]] && die "integrity check failed: see $dir/integrity.txt"
        # 255: ssh lost its connection. Anything else is a real failure.
        ((status == 255)) || die "campaign failed (exit $status): resume with RESUME=$dir"
    done
    die "campaign still cut off after $RETRIES retries: resume with RESUME=$dir"
}

# After the fresh round, on both sides: the whole disk written once, then (Flaminio) a store
# formatted again on it. The lab's disks are files: after the reset, the host's first write to
# each block costs 3-4x more (its filesystem converts unwritten extents; measured 145 against
# 460 MB/s for 1M writes at QD1). The raw side rewrites one span, while the store allocates
# over the whole disk, so without this its steady state would keep paying for first writes.
write_whole() {
    local node=$1 side=$2 rf=$3
    if [[ $side == flaminio ]]; then
        ENV_VARS="VOLUME=wbench" run_on_vm "$node" "$SCRIPTS/guest/node.sh" detach >/dev/null 2>&1 || true
        delete_wbench
        cmd_node down "$(vms_of "$node")" >/dev/null
    fi
    run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" whole
    [[ $side == flaminio ]] && write_volume "$node" "$rf"
    return 0
}

# W3 B: the network between NODE and the other data nodes, measured in the same campaign, at
# its start and end: TCP throughput both ways (iperf3, 10 s each) and the round-trip time, over
# the overlay the replicas use.
write_network() {
    local node=$1 when=$2 peer vm ip up down rtt
    for peer in $(vms_of all) "$VM_C"; do
        [[ $peer == "$node" ]] && continue
        ip=$(vmvar "$peer" WG)
        for vm in "$node" "$peer"; do
            vssh "$vm" 'command -v iperf3 >/dev/null || sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq iperf3 >/dev/null 2>&1'
        done
        vssh "$peer" "iperf3 -s -1 -D -B $ip" && sleep 1
        up=$(vssh "$node" "iperf3 -c $ip -t 10 -J" | jq '.end.sum_received.bits_per_second / 8e6 | floor')
        vssh "$peer" "iperf3 -s -1 -D -B $ip" && sleep 1
        down=$(vssh "$node" "iperf3 -c $ip -t 10 -R -J" | jq '.end.sum_received.bits_per_second / 8e6 | floor')
        rtt=$(vssh "$node" "LC_ALL=C ping -c 50 -i 0.2 -q $ip" | awk -F/ '/^rtt/ {sub(/ ms$/, "", $7); print $5 " ms avg, " $7 " ms mdev"}')
        printf '%s %s -> %s: %s MB/s out, %s MB/s in, RTT %s\n' "$when" "$node" "$peer" "$up" "$down" "$rtt"
    done
}

# A write job's measure, from its TSV row: MiB/s for large writes, otherwise IOPS (reads
# included, for the mixed job).
write_metric() {
    if [[ $1 == 1m ]]; then cut -f3 <<<"$2"; else awk -F'\t' '{print $2 + $6}' <<<"$2"; fi
}

# Steady state (SNIA PTS-style): the last 5 rounds within ±10% of their mean, and the best-fit
# line's change across them within 10% of the mean. Prints the mean if steady, nothing if not.
steady_mean() {
    tail -5 | awk '{v[NR] = $1; s += $1} END {
        if (NR < 5) exit
        m = s / NR; lo = hi = v[1]
        for (i = 1; i <= NR; i++) { if (v[i] < lo) lo = v[i]; if (v[i] > hi) hi = v[i]; sx += i; sxx += i * i; sxy += i * v[i] }
        slope = (NR * sxy - sx * s) / (NR * sxx - sx * sx)
        if (hi - m <= 0.1 * m && m - lo <= 0.1 * m && (slope * (NR - 1) < 0 ? -slope * (NR - 1) : slope * (NR - 1)) <= 0.1 * m) printf "%d", m
    }'
}

# W2–W3: NODE (fl-b), RF (1: the volume on NODE's own disk, against raw O_DSYNC; 2: a second
# replica elsewhere), REPS (3), IDLE (300 s), ROUND (60 s), MAX_ROUNDS (12), JOBS (a regex over
# the job names), QUEUES (ublk queues, 1), NUMJOBS (fio jobs on both sides, 1). NODE's replicas
# of other volumes move away first and come back after.
cmd_bench_writes() {
    local node rf=${RF:-1} reps=${REPS:-3} round=${ROUND:-60} max=${MAX_ROUNDS:-12} filter=${JOBS:-.}
    local queues=${QUEUES:-1} jobs=${NUMJOBS:-1}
    local dir raw spec name rw bs qd mix r side dev k row mean fresh w0 w1 c0 c1 bytes
    node=$(vms_of "${NODE:-b}")
    dir=${RESUME:-$LOCAL_STATE_DIR/bench/writes-rf$rf-q$queues-jobs$jobs-$(date +%Y%m%dT%H%M%S)}
    mkdir -p "$dir"
    raw=$(run_on_vm "$node" "$SCRIPTS/guest/bench-reads.sh" disk)
    # The write campaign reformats NODE's disk: drop $VOL rather than draining (a drain can
    # stall when the only other replicas are remote and resync does not finish).
    if metactl "$VM_A" "$VM_A" get "$VOL" >/dev/null 2>&1; then
        log "removing $VOL before the write campaign (NODE's disk will be reset)"
        ENV_VARS="VOLUME=$VOL" run_on_vm "$node" "$SCRIPTS/guest/node.sh" detach >/dev/null 2>&1 || true
        metactl "$VM_A" "$VM_A" delete "$VOL" >/dev/null 2>&1 || true
    fi
    local vm
    for vm in $(vms_of all) "$VM_C"; do
        ENV_VARS="VOLUME=$VOL" run_on_vm "$vm" "$SCRIPTS/guest/node.sh" delete >/dev/null 2>&1 || true
    done
    metactl "$VM_A" "$VM_A" undrain "$node" >/dev/null 2>&1 || true
    # QUEUES reaches write_volume → node_env for every Flaminio reset in this campaign.
    export QUEUES=$queues
    log "$node, RF $rf, $queues queue(s), $jobs fio job(s), $reps repetitions, $round s rounds (steady: 5 within ±10%, at most $max), idle ${IDLE:-300} s after each reset"
    ((rf > 1)) && write_network "$node" before | tee -a "$dir/network.txt"
    [[ -e $dir/INTEGRITY_FAILED ]] && die "$dir: an integrity check failed; not resuming"
    if [[ -n ${RESUME:-} && -f $dir/runs.tsv ]]; then
        # Runs already recorded are skipped; the rounds of the run that was cut off are dropped.
        log "resuming $dir: $(($(wc -l <"$dir/runs.tsv") - 1)) runs done"
        awk -F'\t' 'NR == FNR { done[$1 FS $2 FS $3] = 1; next } FNR == 1 || done[$1 FS $2 FS $3]' \
            "$dir/runs.tsv" "$dir/rounds.tsv" >"$dir/.rounds" && mv "$dir/.rounds" "$dir/rounds.tsv"
    else
        printf 'rep\tside\tjob\tphase\tround\tIOPS\tMiB/s\tp50\tp99\tread_IOPS\n' >"$dir/rounds.tsv"
        printf 'rep\tside\tjob\tfresh\tsteady\trounds\tclient_bytes\tdisk_bytes\tdrive_bytes\ttemp\n' >"$dir/runs.tsv"
    fi
    for spec in "randwrite-4k-qd1 randwrite 4k 1 -" "randwrite-4k-qd32 randwrite 4k 32 -" \
        "write-1m-qd8 write 1m 8 -" "randrw-70-30-4k-qd32 randrw 4k 32 70"; do
        read -r name rw bs qd mix <<<"$spec"
        [[ $name =~ $filter ]] || continue
        [[ $mix == - ]] && mix=""
        for r in $(seq 1 "$reps"); do
            for side in raw flaminio; do
                if awk -F'\t' -v k="$r\t$side\t$name" '$1 FS $2 FS $3 == k { found = 1 } END { exit !found }' "$dir/runs.tsv"; then
                    continue
                fi
                rm -f "$dir/.disk"
                write_reset "$node" "$side" "$rf"
                if [[ $side == raw ]]; then dev=$raw; else dev=/dev/ublkb1; fi
                row=$(run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" job "$dev" 16G "$name" "$rw" "$bs" "$qd" "$round" /tmp/bench-w.json "$mix" "$jobs")
                fresh=$(write_metric "$bs" "$row")
                printf '%s\t%s\t%s\tfresh\t0\t%s\n' "$r" "$side" "$name" "$row" | cut -f1-4,5,7- >>"$dir/rounds.tsv"
                write_whole "$node" "$side" "$rf"
                run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" precondition "$dev" 16G
                # The drive works off the whole-disk write and the preconditioning in the
                # background, and does so only when idle: under load, it stays slow for more than
                # 20 minutes (4K QD1: 275 IOPS against 460), while 5 minutes idle restore it.
                sleep "${SETTLE:-300}"
                read -r c0 _ <<<"$(ENV_VARS="IMAGE=$LIBVIRT_POOL_PATH/$node-data.img" run_on_host "$(vmvar "$node" HOST)" "$SCRIPTS/host/reset-disk.sh" counters)"
                : >"$dir/.window"
                bytes=0 mean=""
                for k in $(seq 1 "$max"); do
                    w0=$(run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" written)
                    row=$(run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" job "$dev" 16G "$name" "$rw" "$bs" "$qd" "$round" /tmp/bench-w.json "$mix" "$jobs")
                    w1=$(run_on_vm "$node" "$SCRIPTS/guest/bench-writes.sh" written)
                    write_metric "$bs" "$row" >>"$dir/.window"
                    bytes=$((bytes + $(vssh "$node" "jq '.jobs[0].write.io_bytes' /tmp/bench-w.json")))
                    echo "$(((w1 - w0) * 512))" >>"$dir/.disk"
                    printf '%s\t%s\t%s\tround\t%s\t%s\n' "$r" "$side" "$name" "$k" "$row" | cut -f1-5,7- >>"$dir/rounds.tsv"
                    mean=$(steady_mean <"$dir/.window")
                    [[ -n $mean ]] && break
                done
                local temp disk
                read -r c1 temp <<<"$(ENV_VARS="IMAGE=$LIBVIRT_POOL_PATH/$node-data.img" run_on_host "$(vmvar "$node" HOST)" "$SCRIPTS/host/reset-disk.sh" counters)"
                disk=$(awk '{s += $1} END {print s + 0}' "$dir/.disk")
                rm -f "$dir/.disk"
                # Recorded after the integrity check: a run cut off before it is run again.
                local result
                result=$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' "$r" "$side" "$name" "$fresh" "${mean:-unsteady}" "$k" \
                    "$bytes" "$disk" "$(((c1 - c0) * 512000))" "$temp")
                log "  $name rep $r $side: fresh $fresh, steady ${mean:-not reached} after $k rounds (${temp} °C)"
                if [[ $side == flaminio ]]; then
                    local checked=0
                    { echo "$name rep $r"; write_integrity "$node"; } | tee -a "$dir/integrity.txt" || checked=$?
                    if ((checked == 1)); then
                        touch "$dir/INTEGRITY_FAILED"
                        die "integrity check of $name rep $r failed: see $dir/integrity.txt"
                    elif ((checked != 0)); then
                        echo "  (did not complete: run again)" >>"$dir/integrity.txt"
                        warn "integrity check of $name rep $r did not complete"
                        exit 255 # as a lost connection: the run is done again on resume
                    fi
                fi
                echo "$result" >>"$dir/runs.tsv"
            done
        done
    done
    ((rf > 1)) && write_network "$node" after | tee -a "$dir/network.txt"
    write_summary "$dir/runs.tsv" | tee "$dir/summary.txt"
    if [[ ${RESTORE:-1} == 0 ]]; then
        log "results in $dir ($node left as it is: RESTORE=0)"
        return
    fi

    log "restoring $node: a clean store, and $VOL's replicas moved back"
    delete_wbench
    cmd_node down "$(vms_of "$node")" >/dev/null
    FORMAT=1 cmd_node up "$(vms_of "$node")" >/dev/null
    local other
    for other in $(vms_of all) "$VM_C"; do
        [[ $other == "$node" || $(chunks_on "$other") == 0 ]] && continue
        [[ $other == "$VM_A" ]] && continue # the target's node keeps its replicas
        metactl "$VM_A" "$VM_A" drain "$other" >/dev/null && metactl "$VM_A" "$VM_A" undrain "$other" >/dev/null
    done
    log "results in $dir"
}

# Per job: medians over the repetitions of the fresh and steady values, Flaminio's share of raw
# at steady state, and write amplification (bytes the guest wrote to the disk, and bytes the
# drive was given by its host, per byte the client wrote; the drive's count includes the host).
write_summary() {
    awk -F'\t' 'NR > 1 {
            k = $3 "\t" $2; n[k]++; f[k, n[k]] = $4; st[k, n[k]] = ($5 == "unsteady") ? -1 : $5
            cb[k] += $7; db[k] += $8; vb[k] += $9; jobs[$3] = 1
        }
        function med(a, k, c,   i, j, t, x) {
            for (i = 1; i <= c; i++) x[i] = a[k, i]
            for (i = 1; i <= c; i++) for (j = i + 1; j <= c; j++) if (x[j] < x[i]) { t = x[i]; x[i] = x[j]; x[j] = t }
            return (c % 2) ? x[(c + 1) / 2] : (x[c / 2] + x[c / 2 + 1]) / 2
        }
        END {
            printf "%-21s %-9s %10s %10s %7s %9s %9s\n", "job", "side", "fresh", "steady", "share", "disk WA", "drive WA"
            for (j in jobs) for (s = 0; s < 2; s++) {
                side = s ? "flaminio" : "raw"; k = j "\t" side
                if (!n[k]) continue
                sm = med(st, k, n[k]); rm = med(st, j "\traw", n[j "\traw"])
                share = (side == "flaminio" && rm > 0 && sm > 0) ? sprintf("%d%%", 100 * sm / rm) : "-"
                printf "%-21s %-9s %10d %10s %7s %9.2f %9.2f\n", j, side, med(f, k, n[k]), (sm < 0 ? "unsteady" : sm), share,
                    cb[k] ? db[k] / cb[k] : 0, db[k] ? vb[k] / db[k] : 0
            }
        }' "$1"
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
  node ACTION [VM]  phase-1 nodes (replicas only): up (FORMAT=1 reformats) | down | status
  vol ACTION   the test volume \$VOL: create | attach VM (that node becomes its target) | show | delete
  node-fault   verified writes while $VM_B's node restarts; ISR restored; failover to $VM_B verified
  k8s ACTION   k3s: up (server on $VM_W, agents on a and b) | down | status | smoke | kubeconfig | tunnel | uninstall
  kubectl ARGS kubectl on $VM_W
  csi ACTION   CSI driver: deploy (build, import, apply) | status | logs | test | undeploy
  agent ACTION [VM]  node agent: disks (the claim policy's verdict on each device) | claim (default VM c)
  data-node-up fl-c, a third data node: VM, overlay, identities, build, disk claimed by the agent, node
  drain-test   verified writes while $VM_B is drained into $VM_C; $VM_C alone then serves every byte
  chaos        ROUNDS (4) of random faults under a verified write load, each ending with a crashed target
  bench-writes writes against raw O_DSYNC from an empty state each run: NODE, RF, REPS, IDLE, ROUND, JOBS, QUEUES, NUMJOBS
  bench-reads  VOL against the raw disk where its data is: MODE=zero-copy|verify, NODE, REPS, RUNTIME, QUEUES, NUMJOBS
  bench-pod    reads from a pod with a Flaminio volume (NODE=fl-b, RUNTIME=30), against the raw disk
  partition    $VM_A (the target) cut off the overlay; $VM_B takes over; no stale reads, no lost writes
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
    node) cmd_node "$@" ;;
    vol) cmd_vol "$@" ;;
    node-fault) cmd_node_fault ;;
    partition) cmd_partition ;;
    bench-pod) cmd_bench_pod ;;
    bench-reads) cmd_bench_reads ;;
    bench-writes) if ((${RETRIES:-0} > 0)); then cmd_bench_writes_retrying; else cmd_bench_writes; fi ;;
    chaos) cmd_chaos ;;
    agent) cmd_agent "$@" ;;
    data-node-up) cmd_data_node_up ;;
    drain-test) cmd_drain_test ;;
    k8s) cmd_k8s "$@" ;;
    kubectl) kubectl_lab "$@" ;;
    csi) cmd_csi "$@" ;;
    ssh) cmd_ssh "$@" ;;
    console) cmd_console "$@" ;;
    exec) cmd_exec "$@" ;;
    help | -h | --help) usage ;;
    *) usage >&2; exit 2 ;;
esac
