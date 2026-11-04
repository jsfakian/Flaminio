#!/usr/bin/env bash
# Checks lab.sh's deletions and vol-confirm (docs/issues/14, S5-S7): delete_volume, the write
# campaign's start, write_volume and its restore, and vol-confirm, extracted from lab.sh, against
# fake VMs and a fake metadata service that record every call. Run by `make lint`.
if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "lab-deletion-test: needs bash >= 4.4 (found $BASH_VERSION); skipped"
    exit 0
fi
set -uo pipefail
t=$(mktemp -d)
trap 'rm -rf "${t:?}"' EXIT
export LOCAL_STATE_DIR=$t/lab
# shellcheck source=../lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
# The functions under test: lab.sh runs commands when sourced.
extract() {
    awk -v f="$1() {" '
        index($0, f) == 1 { on = 1; print; if ($0 ~ /}$/) on = 0; next }
        on { print; if ($0 == "}") on = 0 }' "$SCRIPTS/lab.sh"
}
for f in valid_id transient cmd_vol data_vms sha256 lost meta_try meta_retry meta_failed detach_all detached_everywhere delete_volume cmd_vol_confirm \
    delete_wbench write_volume write_start write_restore write_reset write_whole; do
    # shellcheck source=/dev/null
    source <(extract "$f")
    declare -F "$f" >/dev/null || {
        echo "FAIL: $f not found in lab.sh"
        exit 1
    }
done
# Above the real time a case takes, below a fake sleep (5 s): two polls, then the time-out.
export TOMBSTONE_WAIT=3 VOL=vol1
failed=0
n=0

# The fake lab, in the case's directory $w:
#   down/VM       VM's node is not running        lost/VM       ssh to VM loses its connection
#   attached/VM   the volume attached on VM       sticky/VM     VM's attachment survives a detach
#   io/VM         a flaminio-io process on VM     detach-fails/VM, clean-down/VM: node.sh fails
#   meta/ID       volume ID's record (get)        volumes       the ids ListVolumes gives, in order
#   polls         the tombstone listings, one a call, the last one repeated: gone, pending NODES...,
#                 fail RC, unknown
# and knobs: delete-err, recreate, start-fails, alive-after, placement, check, check-fails,
# reattach, remote-hash. Every call goes to $w/calls.
rec() { echo "$*" >>"$w/calls"; }

run_on_vm() {
    local vm=$1 script=${2##*/} vol
    shift 2
    vol=$(sed -n 's/.*VOLUME=\([^ ]*\).*/\1/p' <<<"${ENV_VARS:-}")
    rec "run_on_vm $vm $script $*"
    [[ ! -e $w/lost/$vm ]] || return 255
    case "$script $1" in
        "node.sh detach")
            [[ ! -e $w/detach-fails/$vm ]] || return 1
            [[ ! -e $w/down/$vm ]] || {
                echo "not running"
                return 3
            }
            if [[ $(cat "$w/attached/$vm" 2>/dev/null) == "$vol" ]]; then
                [[ -e $w/sticky/$vm ]] || rm "$w/attached/$vm"
                echo detached
            else
                echo "not attached"
            fi
            ;;
        "node.sh delete")
            [[ ! -e $w/down/$vm && ! -e $w/clean-down/$vm ]] || {
                echo "not running"
                return 3
            }
            echo "deleted 1"
            ;;
        "confirm.sh check")
            [[ ! -e $w/check-fails ]] || return 1
            cat "$w/check" 2>/dev/null || echo listed
            ;;
        "confirm.sh delete")
            [[ ! -e $w/reattach ]] || echo "$vol" >"$w/attached/$VM_A"
            echo "deleted 2"
            ;;
        "confirm.sh confirm")
            [[ ! -e $w/confirm-lost ]] || return 255
            echo '{"volume":"v","nodes":["fl-a","fl-c"],"pending":["fl-a"]}' ;;
    esac
}

vssh() {
    local vm=$1
    shift
    rec "vssh $vm $*"
    [[ ! -e $w/lost/$vm ]] || return 255
    case "$*" in
        *"pgrep -x flaminio-io"*) cat "$w/io/$vm" 2>/dev/null || true ;;
        *sha256sum*) cat "$w/remote-hash" 2>/dev/null || true ;;
        *"is-active --quiet flaminio-node"*) [[ -e $w/c-active ]] || return 3 ;;
        *"install -m 755"*)
            cat >/dev/null
            rec "install $vm"
            ;;
    esac
}

metactl() {
    shift 2
    rec "metactl $*"
    [[ ! -e $w/lost/$VM_A ]] || return 255
    case "$1" in
        delete)
            if [[ -e $w/delete-transient ]]; then
                rm "$w/delete-transient"
                echo "flaminio-meta: rpc error: code = DeadlineExceeded" >&2
                return 1
            fi
            if [[ -e $w/delete-err ]]; then
                cat "$w/delete-err" >&2
                return 1
            fi
            echo "$2" >"$w/deleted"
            [[ -e $w/meta/$2 ]] || {
                echo "flaminio-meta: rpc error: code = NotFound desc = not found: volume \"$2\"" >&2
                return 1
            }
            [[ -e $w/recreate ]] || rm "$w/meta/$2"
            ;;
        get)
            [[ -e $w/meta/$2 ]] || {
                echo "flaminio-meta: rpc error: code = NotFound desc = not found: volume \"$2\"" >&2
                return 1
            }
            cat "$w/meta/$2"
            ;;
        volumes) jq -R . "$w/volumes" | jq -sc '{ids: .}' ;;
        tombstones)
            local a
            a=$(head -1 "$w/polls" 2>/dev/null || true)
            if (($(wc -l <"$w/polls" 2>/dev/null || echo 0) > 1)); then
                tail -n +2 "$w/polls" >"$w/polls.next" && mv "$w/polls.next" "$w/polls"
            fi
            case $a in
                "" | gone) echo "{}" ;;
                "pending "*)
                    # shellcheck disable=SC2086 # the nodes, one argument each
                    jq -cn --arg v "$(cat "$w/deleted")" '{tombstones: [{volume: $v, nodes: ["fl-a", "fl-b", "fl-c"], pending: $ARGS.positional}]}' \
                        --args ${a#pending }
                    ;;
                "fail "*)
                    echo "flaminio-meta: rpc error: code = Unavailable" >&2
                    return "${a#fail }"
                    ;;
                "error "*)
                    echo "flaminio-meta: rpc error: code = ${a#error }" >&2
                    return 1
                    ;;
                unknown)
                    echo 'flaminio-meta: unknown command "tombstones"' >&2
                    return 1
                    ;;
            esac
            ;;
        nodes)
            local polls e
            # $w/nodes-errs: an error code per poll, before any answer.
            e=$(head -1 "$w/nodes-errs" 2>/dev/null || true)
            if [[ -n $e ]]; then
                tail -n +2 "$w/nodes-errs" >"$w/nodes-errs.next" && mv "$w/nodes-errs.next" "$w/nodes-errs"
                [[ $e == bad-json ]] && { echo "not json"; return 0; }
                echo "flaminio-meta: rpc error: code = $e desc = x" >&2
                return 1
            fi
            polls=$(($(cat "$w/nodes-polls" 2>/dev/null || echo 0) + 1))
            echo "$polls" >"$w/nodes-polls"
            if ((polls > $(cat "$w/alive-after" 2>/dev/null || echo 0))); then
                rec "alive"
                echo '{"nodes":[{"id":"fl-a","alive":true},{"id":"fl-b","alive":true}]}'
            else
                echo '{"nodes":[{"id":"fl-a","alive":true},{"id":"fl-b"}]}'
            fi
            ;;
        create) jq -c '{volume: {chunks: map({replicas: .})}}' <<<"$(cat "$w/placement" 2>/dev/null || echo '[["fl-b"]]')" ;;
    esac
}

cmd_node() {
    rec "cmd_node $1 $2 FORMAT=${FORMAT:-unset}"
    [[ ! -e $w/start-fails ]] || return 1
    case "$1" in
        up) rm -f "$w/down/$2" ;;
        down) touch "$w/down/$2" ;;
    esac
}
ensure_node_c() {
    rec "ensure_node_c FORMAT=${FORMAT:-unset}"
    rm -f "$w/down/$VM_C"
}
attach_wbench() { rec "attach_wbench $1"; }
# ensure_node_c's own (the real one, in its cases), and write_reset's.
cmd_agent() {
    [[ ! -e $w/claim-lost ]] || return 255
    cat "$w/claim" 2>/dev/null || echo /dev/vdb
}
node_env() { echo "NODE=$1"; }
run_on_host() { rec "run_on_host $*"; }
host_drop_caches() { :; }
meta_build() {
    rec "meta_build"
    mkdir -p "$LOCAL_STATE_DIR/bin"
    echo flaminio-meta >"$LOCAL_STATE_DIR/bin/flaminio-meta"
}
sleep() { SECONDS=$((SECONDS + ${1%.*})); }

# run WHAT RC CODE: runs CODE in a new fake lab where fl-c exists, every node runs, and the
# tombstone goes at the first listing; with errexit, as lab.sh runs; it must exit RC.
run() {
    n=$((n + 1))
    w=$t/case.$n
    mkdir -p "$w/down" "$w/lost" "$w/attached" "$w/sticky" "$w/io" "$w/meta" "$w/detach-fails" "$w/clean-down" \
        "$LOCAL_STATE_DIR/$VM_C"
    touch "$LOCAL_STATE_DIR/$VM_C/created" "$w/calls" "$w/volumes"
    what=$1
    (
        set -e
        eval "$3"
    ) >"$w/out" 2>"$w/err"
    rc=$?
    if [[ $rc != "$2" ]]; then
        echo "FAIL: $what: exit $rc, want $2 ($(tail -2 "$w/err" | tr '\n' ' '))"
        failed=1
    fi
}

# called PATTERN [no]: the last case made a call matching PATTERN (or none, with "no").
called() {
    local found=0
    grep -qE -- "$1" "$w/calls" && found=1
    if [[ ${2:-} == no ]] && ((found)); then
        echo "FAIL: $what: a call matches '$1': $(grep -E -- "$1" "$w/calls" | head -1)"
        failed=1
    elif [[ ${2:-} != no ]] && ((!found)); then
        echo "FAIL: $what: no call matches '$1'"
        failed=1
    fi
}

# said PATTERN [no]: the last case's errors match PATTERN (or do not, with "no").
said() {
    local found=0
    grep -qE -- "$1" "$w/err" && found=1
    if [[ ${2:-} == no ]] && ((found)); then
        echo "FAIL: $what: it said '$1'"
        failed=1
    elif [[ ${2:-} != no ]] && ((!found)); then
        echo "FAIL: $what: it did not say '$1' ($(tail -2 "$w/err" | tr '\n' ' '))"
        failed=1
    fi
}

# A volume's record, its chunks' replicas as JSON arrays: record ID '["fl-a","fl-b"]' ...
record() {
    local id=$1
    shift
    printf '%s\n' "$@" | jq -sc '{volume: {chunks: map({replicas: .})}}' >"$w/meta/$id"
}

# --- delete_volume (S5) ---

run "delete" 0 'record vol1 "[\"fl-a\",\"fl-b\"]"; echo fl-a >"$w/attached/fl-b"; echo vol1 >"$w/attached/fl-b"; delete_volume vol1'
called "run_on_vm fl-c node.sh detach"
called "metactl delete vol1"
for vm in fl-a fl-b fl-c; do called "run_on_vm $vm node.sh delete"; done
called "cmd_node|ensure_node_c" no

run "a detach that fails: no metadata delete" 1 'record vol1 "[\"fl-a\"]"; touch "$w/detach-fails/fl-b"; delete_volume vol1'
called "metactl delete" no
run "a lost connection at the detach: 255" 255 'record vol1 "[\"fl-a\"]"; touch "$w/lost/fl-c"; delete_volume vol1'
called "metactl delete" no
said "fl-c: its VM may be off"
run "a volume the metadata service does not know: NotFound goes on" 0 'delete_volume vol1'
called "run_on_vm fl-a node.sh delete"
run "the metadata delete fails" 1 'record vol1 "[\"fl-a\"]"; echo "flaminio-meta: rpc error: code = Unavailable" >"$w/delete-err"; delete_volume vol1'
called "node.sh delete" no
said "code = Unavailable"

run "the campaign's node stopped with a raw disk (delete_wbench): formatted, cleaned" 0 \
    'record wbench "[\"fl-b\"]"; touch "$w/down/fl-b"; delete_wbench fl-b'
called "cmd_node up fl-b FORMAT=1"
called "run_on_vm fl-b node.sh delete"
run "the campaign's node stopped (the \$VOL removal): formatted, cleaned, the campaign goes on" 0 \
    'record vol1 "[\"fl-b\",\"fl-a\"]"; echo vol1 >"$w/volumes"; touch "$w/down/fl-b"; write_start fl-b'
called "cmd_node up fl-b FORMAT=1"
called "run_on_vm fl-b node.sh delete"
run "fl-c as the campaign's node, stopped: formatted, by the serial disk" 0 \
    'record wbench "[\"fl-c\"]"; touch "$w/down/fl-c"; delete_wbench fl-c'
called "cmd_node up fl-c FORMAT=1"
called "ensure_node_c" no
run "another node stopped during a campaign: started as it is" 0 \
    'record wbench "[\"fl-b\"]"; touch "$w/down/fl-a"; delete_wbench fl-b'
called "cmd_node up fl-a FORMAT=0"
called "FORMAT=1" no
run "fl-c stopped during vol delete: started as it is" 0 'record vol1 "[\"fl-c\"]"; touch "$w/down/fl-c"; delete_volume vol1'
called "ensure_node_c FORMAT=0"
called "FORMAT=1" no
run "a failed start" 1 'record vol1 "[\"fl-a\"]"; touch "$w/down/fl-a" "$w/start-fails"; RERUN="make vol-delete" delete_volume vol1'
called "node.sh delete" no
said "starting fl-a's node failed \(exit 1\): fix it, then run again: make vol-delete"
run "fl-c's start, a lost connection: 255" 255 \
    'source <(extract ensure_node_c); touch "$w/lost/fl-c"; ensure_node_c'
called "node.sh up" no
run "fl-c stopped, lost during its claim: 255" 255 \
    'source <(extract ensure_node_c); record vol1 "[\"fl-c\"]"; touch "$w/down/fl-c" "$w/claim-lost"; delete_volume vol1'
said "lost the connection to fl-c"
called "node.sh up|node.sh delete" no
run "fl-c's start, no disk claimed" 1 'source <(extract ensure_node_c); echo "error" >"$w/claim"; ensure_node_c'
said "claimed no disk"
called "node.sh up" no
run "fl-c's start" 0 'source <(extract ensure_node_c); ensure_node_c'
called "run_on_vm fl-c node.sh up"
run "fl-c's node running" 0 'source <(extract ensure_node_c); touch "$w/c-active"; ensure_node_c'
called "node.sh up" no
run "the volume found again before the clean" 1 'record vol1 "[\"fl-a\"]"; touch "$w/recreate"; delete_volume vol1'
called "node.sh delete" no
said "exists again"
run "a node not running at the clean" 1 'record vol1 "[\"fl-a\"]"; touch "$w/clean-down/fl-a"; delete_volume vol1'
said "fl-a: not running"
called "metactl tombstones" no
run "pending for fl-c: stops at once, with the procedure" 1 \
    'record wbench "[\"fl-c\"]"; printf "pending fl-c fl-a\ngone\n" >"$w/polls"; RERUN="RESUME=/x make bench-writes" delete_wbench fl-b'
said "pending for fl-c, which has no sweep"
said "scripts/lab.sh vol-confirm wbench fl-c"
said "RESUME=/x make bench-writes"
[[ $(grep -c "metactl tombstones" "$w/calls") == 1 ]] || {
    echo "FAIL: $what: it listed the tombstones more than once"
    failed=1
}
run "pending for another node without a sweep: stops, no procedure" 1 \
    'record vol1 "[\"fl-a\"]"; echo "pending fl-x" >"$w/polls"; delete_volume vol1'
said "pending for fl-x"
said "vol-confirm" no
run "a failed listing, then gone" 0 'record vol1 "[\"fl-a\"]"; printf "fail 1\ngone\n" >"$w/polls"; delete_volume vol1'
run "a lost connection at a listing, then gone" 0 'record vol1 "[\"fl-a\"]"; printf "fail 255\ngone\n" >"$w/polls"; delete_volume vol1'
run "the sweeps never confirm: time-out" 1 'record vol1 "[\"fl-a\"]"; echo "pending fl-a" >"$w/polls"; delete_volume vol1'
said "pending for fl-a after 3 s"
said "make csi-deploy"
for code in DeadlineExceeded Canceled; do
    run "a listing that fails with $code, then gone" 0 'record vol1 "[\"fl-a\"]"; printf "error '"$code"'\ngone\n" >"$w/polls"; delete_volume vol1'
    run "an alive poll that fails with $code, then alive" 0 'echo '"$code"' >"$w/nodes-errs"; write_volume fl-b 1'
    called "attach_wbench fl-b"
done
run "an alive poll that fails with Internal: stops" 1 'echo Internal >"$w/nodes-errs"; write_volume fl-b 1'
said "code = Internal"
called "metactl create" no
run "a listing that fails otherwise: stops at once" 1 'record vol1 "[\"fl-a\"]"; printf "error Internal\ngone\n" >"$w/polls"; delete_volume vol1'
said "code = Internal"
[[ $(grep -c "metactl tombstones" "$w/calls") == 1 ]] || {
    echo "FAIL: $what: it listed again after the error"
    failed=1
}
run "time-out after a failed listing: its error" 1 'record vol1 "[\"fl-a\"]"; printf "pending fl-a\nfail 1\n" >"$w/polls"; delete_volume vol1'
said "the last listing of the tombstones failed: .*code = Unavailable"
run "time-out after a lost connection: 255" 255 'record vol1 "[\"fl-a\"]"; printf "pending fl-a\nfail 255\n" >"$w/polls"; delete_volume vol1'
run "an older flaminio-meta" 1 'record vol1 "[\"fl-a\"]"; echo unknown >"$w/polls"; delete_volume vol1'
said "make meta-up"
# The harness never confirms.
for c in $(seq 1 "$n"); do
    ! grep -qE "confirm" "$t/case.$c/calls" || {
        echo "FAIL: case $c confirmed: $(grep -E confirm "$t/case.$c/calls" | head -1)"
        failed=1
    }
done

# --- The campaign's start, the restore, write_volume (S6) ---

run "another volume on the campaign's node: stops before anything" 1 \
    'record vol1 "[\"fl-b\"]"; record x "[\"fl-a\"]" "[\"fl-b\"]"; record y "[\"fl-a\"]"
     jq -c ".volume.chunks[0].movingTo = \"fl-b\"" "$w/meta/y" >"$w/y" && mv "$w/y" "$w/meta/y"
     record z "[\"fl-a\"]"; record wbench "[\"fl-b\"]"
     printf "vol1\nx\ny\nz\nwbench\n" >"$w/volumes"; touch "$w/down/fl-b"; write_start fl-b'
said "fl-b holds replicas of x y,"
called "FORMAT=1" no
called "metactl delete" no
called "node.sh" no
run "only \$VOL on the campaign's node: removed" 0 'record vol1 "[\"fl-b\"]"; echo vol1 >"$w/volumes"; write_start fl-b'
called "metactl delete vol1"
run "\$VOL elsewhere: removed too" 0 'record vol1 "[\"fl-a\"]"; echo vol1 >"$w/volumes"; write_start fl-b'
called "metactl delete vol1"
run "a volume gone since the listing: skipped" 0 'record vol1 "[\"fl-a\"]"; printf "ghost\nvol1\n" >"$w/volumes"; write_start fl-b'
called "metactl delete vol1"
run "no \$VOL: nothing removed" 0 'record z "[\"fl-a\"]"; echo z >"$w/volumes"; write_start fl-b'
called "metactl delete" no
run "the restore" 0 'record wbench "[\"fl-b\"]"; write_restore fl-b'
called "drain" no
called "cmd_node up fl-b FORMAT=1"
run "wbench created once the campaign's node is alive" 0 'echo 2 >"$w/alive-after"; write_volume fl-b 1'
[[ $(grep -A1 "^alive$" "$w/calls" | tail -1) == "metactl create wbench 16G 1 1 256M fl-b" ]] || {
    echo "FAIL: $what: create before the node was alive: $(tr '\n' ';' <"$w/calls")"
    failed=1
}
called "attach_wbench fl-b"
run "a lost connection at the alive poll: 255" 255 'touch "$w/lost/fl-a"; write_volume fl-b 1'
called "metactl create" no
run "an Unavailable alive poll, then alive" 0 'printf "Unavailable\nUnavailable\n" >"$w/nodes-errs"; write_volume fl-b 1'
called "attach_wbench fl-b"
run "another error at the alive poll: stops at once" 1 'echo PermissionDenied >"$w/nodes-errs"; write_volume fl-b 1'
said "code = PermissionDenied"
[[ $(grep -c "metactl nodes" "$w/calls") == 1 ]] || {
    echo "FAIL: $what: polled again after the error"
    failed=1
}
run "an unreadable alive poll: stops" 1 'echo bad-json >"$w/nodes-errs"; write_volume fl-b 1'
said "reading the nodes failed"
called "metactl create" no
run "the campaign's node never alive" 1 'echo 1000 >"$w/alive-after"; write_volume fl-b 1'
called "metactl create" no
run "a chunk without a replica on the campaign's node" 1 \
    'echo "[[\"fl-b\",\"fl-a\"],[\"fl-a\",\"fl-c\"]]" >"$w/placement"; write_volume fl-b 2'
called "attach_wbench" no

# --- vol-confirm (S7) ---

for node in c fl-c; do
    run "vol-confirm v $node" 0 'cmd_vol_confirm v '"$node"
    [[ $(grep -E "^(meta_build|install |run_on_vm .*(confirm.sh|node.sh detach))" "$w/calls" | sed 's/^run_on_vm //' | tr '\n' ';') == \
        "meta_build;install fl-c;fl-c confirm.sh check;fl-a node.sh detach;fl-b node.sh detach;fl-c node.sh detach;fl-a node.sh detach;fl-b node.sh detach;fl-c node.sh detach;fl-c confirm.sh delete;fl-a node.sh detach;fl-b node.sh detach;fl-c node.sh detach;fl-c confirm.sh confirm;" ]] || {
        echo "FAIL: $what: steps $(tr '\n' ';' <"$w/calls")"
        failed=1
    }
done
for node in a fl-a all cluster w fl-w nowhere; do
    run "vol-confirm v $node: refused" 1 'cmd_vol_confirm v '"$node"
    called "meta_build|install|confirm.sh|node.sh" no
done
run "vol-confirm, the same flaminio-meta there" 0 'echo flaminio-meta | sha256 >"$w/remote-hash"; cmd_vol_confirm v c'
called "install" no
run "vol-confirm, not listed" 0 'echo "not listed" >"$w/check"; cmd_vol_confirm v c'
called "node.sh|confirm.sh delete|confirm.sh confirm" no
run "vol-confirm, a sweep on the node" 1 'touch "$w/check-fails"; cmd_vol_confirm v c'
called "node.sh|confirm.sh delete|confirm.sh confirm" no
run "vol-confirm, still attached" 1 'echo v >"$w/attached/fl-b"; touch "$w/sticky/fl-b"; cmd_vol_confirm v c'
called "confirm.sh delete|confirm.sh confirm" no
run "vol-confirm, a stopped node's flaminio-io still runs" 1 'touch "$w/down/fl-a"; echo 4242 >"$w/io/fl-a"; cmd_vol_confirm v c'
called "confirm.sh delete|confirm.sh confirm" no
run "vol-confirm, a stopped node without flaminio-io" 0 'touch "$w/down/fl-a"; cmd_vol_confirm v c'
run "vol-confirm, attached again after the delete" 1 'touch "$w/reattach"; cmd_vol_confirm v c'
called "confirm.sh delete"
called "confirm.sh confirm" no
run "vol-confirm, a lost connection" 255 'touch "$w/lost/fl-b"; cmd_vol_confirm v c'
called "confirm.sh delete|confirm.sh confirm" no

run "a record jq cannot read: stops" 1 'echo "not json" >"$w/meta/x"; echo x >"$w/volumes"; write_start fl-b'
said "reading x's record failed"
# The resets delete wbench as the campaign's: a stopped campaign node starts formatted.
for step in "write_reset fl-b raw 1" "write_whole fl-b flaminio 1" "write_restore fl-b"; do
    run "$step: the campaign's node, stopped" 0 'record wbench "[\"fl-b\"]"; touch "$w/down/fl-b"; '"$step"
    [[ $(grep -m1 "cmd_node up fl-b" "$w/calls") == "cmd_node up fl-b FORMAT=1" ]] || {
        echo "FAIL: $what: the first start of fl-b is not formatted: $(grep -m1 "cmd_node up fl-b" "$w/calls")"
        failed=1
    }
    # wbench is deleted before the campaign's node stops.
    (($(grep -n "metactl delete wbench" "$w/calls" | cut -d: -f1) < $(grep -n "cmd_node down fl-b" "$w/calls" | cut -d: -f1))) || {
        echo "FAIL: $what: the node stopped before wbench was deleted"
        failed=1
    }
done
run "a metadata delete that fails with DeadlineExceeded once" 0 'record vol1 "[\"fl-a\"]"; touch "$w/delete-transient"; delete_volume vol1'
[[ $(grep -c "metactl delete vol1" "$w/calls") == 2 ]] || {
    echo "FAIL: $what: not retried"
    failed=1
}
run "vol-confirm, lost during the confirmation" 255 'touch "$w/confirm-lost"; cmd_vol_confirm v c'
said "may or may not have confirmed v: run vol-confirm again"
# Ids that are not ids go nowhere, in any deletion.
for vol in "v x" "v;id" "v
id" ""; do
    run "delete_volume '$vol': refused" 1 'delete_volume "'"$vol"'"'
    run "vol delete '$vol': refused" 1 'VOL="'"$vol"'" cmd_vol delete'
    [[ ! -s $w/calls ]] || {
        echo "FAIL: $what: $(head -1 "$w/calls")"
        failed=1
    }
done
# vol detach says what it found.
run "vol detach" 0 'echo vol1 >"$w/attached/fl-b"; cmd_vol detach b'
run "vol detach, not attached" 0 'cmd_vol detach b'
run "vol detach, node not running" 1 'touch "$w/down/fl-b"; cmd_vol detach b'
said "fl-b: not running"
run "vol detach, an error" 1 'touch "$w/detach-fails/fl-b"; cmd_vol detach b'
said "fl-b: exit 1"
run "vol detach, lost" 255 'touch "$w/lost/fl-b"; cmd_vol detach b'

# Ids that are not ids go nowhere.
for vol in "v;id" "v x" "" "-v" "$(printf "v%.0s" $(seq 129))"; do
    run "vol-confirm '${vol:0:12}': refused" 1 'cmd_vol_confirm "'"$vol"'" c'
    called "meta_build|install|confirm.sh|node.sh" no
done
run "vol-confirm, fl-c not created" 1 'rm "$LOCAL_STATE_DIR/fl-c/created"; cmd_vol_confirm v c'
said "not a data node"
called "meta_build|install|confirm.sh|node.sh" no

((failed == 0)) || exit 1
echo "lab-deletion-test: ok"
