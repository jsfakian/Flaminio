#!/usr/bin/env bash
# The node-side steps of `lab.sh vol-confirm` (docs/issues/14, S7), which runs them in order:
# confirming a volume's deletion for a data node without a sweep (fl-c), by hand, as the node's
# agent. Never run them otherwise.
# Usage: confirm.sh check | delete | confirm
#   VOLUME, NODE (this node), META (a metadata service, address:port), TRUST_DOMAIN; META_BIN,
#   AGENT_BIN and DISK_TOOL are the paths of flaminio-meta, flaminio-agent and flaminio-disk
#   check    prints "listed" when the node's tombstones list VOLUME, after checking that
#            nothing on the node can sweep (no CSI plugin, agent or Kubernetes agent), or
#            "not listed": nothing to do
#   delete   checks that the node's data plane is its flaminio-node unit, on the disk the claim
#            policy calls ours; then deletes VOLUME's replicas: prints "deleted <n>"
#   confirm  the same checks, a second delete that must find none, then the confirmation:
#            prints the tombstone as read, less this node
# Any other outcome stops, with exit 1.
: "${VOLUME:?}" "${NODE:?}" "${META:?}" "${TRUST_DOMAIN:?}"
meta_bin=${META_BIN:-/usr/local/bin/flaminio-meta}
agent_bin=${AGENT_BIN:-/usr/local/bin/flaminio-agent}
disk_tool=${DISK_TOOL:-$HOME/flaminio/dataplane/target/release/flaminio-disk}
unit=flaminio-node

# flaminio-meta's client, as this node's agent.
meta() {
    local p=/etc/flaminio/pki
    sudo "$meta_bin" --server "$META" --cert $p/agent.crt --key $p/agent.key --ca $p/ca.crt "$@"
}

check() {
    local out proc u state
    out=$(meta tombstones "$NODE") || die "listing $NODE's tombstones failed"
    if ! jq -e --arg v "$VOLUME" '.tombstones // [] | any(.volume == $v)' <<<"$out" >/dev/null; then
        echo "not listed"
        return
    fi
    # Exactly one confirmer per node: a node with a sweep confirms by itself.
    for proc in flaminio-csi flaminio-agent; do
        ! pgrep -x "$proc" >/dev/null || die "$proc runs on $NODE: its sweep confirms; nothing to do by hand"
    done
    for u in k3s k3s-agent; do
        state=$(systemctl is-active "$u" 2>/dev/null || true)
        [[ $state == inactive ]] || die "$u is ${state:-in an unknown state} on $NODE: a Kubernetes node runs the CSI sweep"
        state=$(systemctl is-enabled "$u" 2>/dev/null || true)
        [[ $state == disabled || $state == not-found ]] ||
            die "$u is ${state:-in an unknown state} (enabled) on $NODE: a Kubernetes node runs the CSI sweep"
    done
    echo listed
}

# The node's data plane is its flaminio-node unit, alone: one flaminio-io process, the unit's,
# so that no stray process holds the control socket; on the one disk the claim policy calls ours.
data_plane() {
    local main pids device ours a b
    systemctl is-active --quiet "$unit" || die "flaminio-node is not running on $NODE: make node-up VM=$NODE, then vol-confirm again"
    main=$(systemctl show -p MainPID --value "$unit")
    pids=$(pgrep -x flaminio-io || true)
    [[ $pids == "$main" && $main != 0 ]] ||
        die "flaminio-io on $NODE: processes ${pids//$'\n'/ } (none if empty), the unit's $main: exactly the unit's must run"
    device=$(systemctl show -p ExecStart --value "$unit" | grep -o -- '--device [^ ;]*' | cut -d' ' -f2 || true)
    [[ -n $device ]] || die "flaminio-node on $NODE runs without --device"
    ours=$(sudo "$agent_bin" disks --disk-tool "$disk_tool" | awk '$2 == "ours" {print $1}')
    [[ -n $ours && $ours != *$'\n'* ]] || die "the claim policy calls ${ours//$'\n'/ and } ours on $NODE (none if empty): want exactly one"
    a=$(readlink -f "$ours")
    b=$(readlink -f "$device")
    [[ -n $a && $a == "$b" ]] || die "flaminio-node on $NODE runs on $device, but the disk the claim policy calls ours is $ours"
}

# Deletes VOLUME's replicas through the control socket; prints how many.
delete_replicas() {
    local answer
    answer=$(control "delete $VOLUME")
    [[ $answer =~ ^ok\ ([0-9]+)\ replicas\ deleted$ ]] || die "delete $VOLUME on $NODE: $answer"
    echo "${BASH_REMATCH[1]}"
}

case "${1:-}" in
    check) check ;;
    delete)
        data_plane
        n=$(delete_replicas)
        echo "deleted $n"
        ;;
    confirm)
        data_plane
        n=$(delete_replicas)
        [[ $n == 0 ]] || die "a second delete of $VOLUME on $NODE found $n replicas: something created them again; not confirmed"
        out=$(meta --trust-domain "$TRUST_DOMAIN" confirm "$VOLUME" "$NODE" --replicas-deleted)
        [[ $(tr -d ' \n' <<<"$out") != "{}" ]] || die "$VOLUME was no longer pending for $NODE: confirmed by another caller"
        echo "$out"
        ;;
    *) die "usage: confirm.sh check|delete|confirm" ;;
esac
