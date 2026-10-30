#!/usr/bin/env bash
# Checks the guest scripts that delete replicas and confirm deletions (docs/issues/14): node.sh's
# detach and delete, and confirm.sh's steps. Each runs as run_on_vm runs it, common.sh first, with
# sudo, systemctl and pgrep stubbed, fakes of flaminio-meta and flaminio-agent, and the control
# socket answered by a fake server. Run by `make lint`.
if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "node-scripts-test: needs bash >= 4.4 (found $BASH_VERSION); skipped"
    exit 0
fi
set -uo pipefail
guest=$(cd "$(dirname "${BASH_SOURCE[0]}")/../guest" && pwd)
# A short path: a Unix socket's path is limited to 104 bytes on macOS.
t=$(mktemp -d /tmp/fl14.XXXXXX)
server=""
trap '[[ -z $server ]] || kill "$server" 2>/dev/null; rm -rf "${t:?}"' EXIT
mkdir -p "$t/bin" "$t/dev"
failed=0
n=0

# Stubs, answering from the case's state directory, STATE. sudo runs its command.
cat >"$t/bin/sudo" <<'STUB'
#!/usr/bin/env bash
exec "$@"
STUB
cat >"$t/bin/systemctl" <<'STUB'
#!/usr/bin/env bash
case "$*" in
    "is-active --quiet flaminio-node") [[ $(cat "$STATE/active") == active ]] ;;
    "show -p MainPID --value flaminio-node") cat "$STATE/mainpid" ;;
    "show -p ExecStart --value flaminio-node") cat "$STATE/execstart" ;;
    "is-active "*) cat "$STATE/$2.active"; [[ $(cat "$STATE/$2.active") == active ]] ;;
    "is-enabled "*) cat "$STATE/$2.enabled"; [[ $(cat "$STATE/$2.enabled") == enabled ]] ;;
    *) echo "systemctl stub: $*" >&2; exit 1 ;;
esac
STUB
cat >"$t/bin/pgrep" <<'STUB'
#!/usr/bin/env bash
[[ $1 == -x ]] || exit 2
[[ -s $STATE/pgrep.$2 ]] && cat "$STATE/pgrep.$2"
STUB
# flaminio-meta and flaminio-agent, at the paths confirm.sh is given: they record their arguments.
cat >"$t/bin/fake-meta" <<'STUB'
#!/usr/bin/env bash
echo "$*" >>"$STATE/meta.log"
case " $* " in
    *" tombstones "*) cat "$STATE/tombstones"; exit "$(cat "$STATE/tombstones.rc")" ;;
    *" confirm "*) cat "$STATE/confirmed" ;;
esac
STUB
cat >"$t/bin/fake-agent" <<'STUB'
#!/usr/bin/env bash
echo "$*" >>"$STATE/agent.log"
cat "$STATE/disks"
STUB
chmod +x "$t/bin"/*

# The control socket: each connection gets the first of the case's answers left, the last one
# repeated, and is logged.
python3 - "$t/sock" "$t/current" <<'PY' &
import socket, sys
path, current = sys.argv[1], sys.argv[2]
srv = socket.socket(socket.AF_UNIX)
srv.bind(path)
srv.listen(8)
while True:
    conn, _ = srv.accept()
    state = open(current).read().strip()
    with conn, conn.makefile("rw") as f:
        line = f.readline().strip()
        with open(state + "/socket.log", "a") as log:
            log.write(line + "\n")
        with open(state + "/answers") as a:
            answers = a.read().splitlines()
        if len(answers) > 1:
            with open(state + "/answers", "w") as a:
                a.write("\n".join(answers[1:]) + "\n")
        f.write((answers[0] if answers else "error: no answer") + "\n")
        f.flush()
PY
server=$!
for _ in $(seq 50); do
    [[ -S $t/sock ]] && break
    sleep 0.1
done

# A new case: a running node on $t/dev/vdb, the disk the claim policy calls ours, nothing that
# sweeps, and a tombstone pending for the node.
fresh() {
    n=$((n + 1))
    s=$t/state.$n
    mkdir "$s"
    echo "$s" >"$t/current"
    echo active >"$s/active"
    echo 4242 >"$s/mainpid"
    echo 4242 >"$s/pgrep.flaminio-io"
    : >"$t/dev/vdb"
    : >"$t/dev/vdc"
    ln -sf "$t/dev/vdb" "$t/dev/by-id"
    echo "{ path=/x/flaminio-io ; argv[]=/x/flaminio-io node --device $t/dev/vdb --node fl-c --id 1 ; ignore_errors=no }" >"$s/execstart"
    printf '%s ours       100.0 GiB  serial "flaminio-data" \n/dev/vda refused 20.0 GiB  serial "" mounted\n' "$t/dev/vdb" >"$s/disks"
    for u in k3s k3s-agent; do
        echo inactive >"$s/$u.active"
        echo not-found >"$s/$u.enabled"
    done
    echo '{"tombstones":[{"volume":"v","nodes":["fl-a","fl-c"],"pending":["fl-a","fl-c"]}]}' >"$s/tombstones"
    echo 0 >"$s/tombstones.rc"
    echo '{"volume":"v","nodes":["fl-a","fl-c"],"pending":["fl-a"]}' >"$s/confirmed"
    echo "ok 2 replicas deleted" >"$s/answers"
}

# guest SCRIPT ARGS...: runs a guest script as run_on_vm does; its exit status in rc, its
# output in out.
guest() {
    local script=$1
    shift
    cat "$guest/common.sh" "$guest/$script" >"$t/script"
    rc=0
    out=$(env PATH="$t/bin:$PATH" STATE="$s" CONTROL_SOCKET="$t/sock" VOLUME=v NODE=fl-c META=10.77.0.1:7400 \
        TRUST_DOMAIN=lab META_BIN="$t/bin/fake-meta" AGENT_BIN="$t/bin/fake-agent" DISK_TOOL=/x/flaminio-disk \
        bash "$t/script" "$@" 2>"$t/err") || rc=$?
}

# want WHAT RC OUT: the last guest run exited RC and printed OUT.
want() {
    if [[ $rc != "$2" || $out != "$3" ]]; then
        echo "FAIL: $1: exit $rc, printed '$out'; want exit $2, '$3' ($(tail -1 "$t/err"))"
        failed=1
    fi
}

# logged WHAT FILE PATTERN [no]: the case's FILE has a line matching PATTERN (or none, with "no").
logged() {
    local found=0
    grep -qE -- "$3" "$s/$2" 2>/dev/null && found=1
    if [[ ${4:-} == no ]] && ((found)); then
        echo "FAIL: $1: $2 has '$3'"
        failed=1
    elif [[ ${4:-} != no ]] && ((!found)); then
        echo "FAIL: $1: $2 has no '$3'"
        failed=1
    fi
}

meta_flags='^--server 10\.77\.0\.1:7400 --cert /etc/flaminio/pki/agent\.crt --key /etc/flaminio/pki/agent\.key --ca /etc/flaminio/pki/ca\.crt'

# node.sh detach and delete: one line on stdout, distinct exits.
fresh && echo "ok detached v" >"$s/answers" && guest node.sh detach && want "detach" 0 detached
logged "detach" socket.log "^detach v$"
fresh && echo "error: v is not attached here" >"$s/answers" && guest node.sh detach && want "detach, not attached" 0 "not attached"
fresh && echo inactive >"$s/active" && guest node.sh detach && want "detach, unit inactive" 3 "not running"
logged "detach, unit inactive" socket.log . no
fresh && echo "error: the node is shutting down" >"$s/answers" && guest node.sh detach && want "detach, an error" 1 ""
fresh && echo "ok 3 replicas deleted" >"$s/answers" && guest node.sh delete && want "delete" 0 "deleted 3"
logged "delete" socket.log "^delete v$"
fresh && echo inactive >"$s/active" && guest node.sh delete && want "delete, unit inactive" 3 "not running"
logged "delete, unit inactive" socket.log . no
fresh && echo "error: no answer within 30 s" >"$s/answers" && guest node.sh delete && want "delete, an error" 1 ""
fresh && echo "ok" >"$s/answers" && guest node.sh delete && want "delete, an answer without a count" 1 ""

# confirm.sh check: listed, then nothing that sweeps.
fresh && guest confirm.sh check && want "check" 0 listed
logged "check" meta.log "$meta_flags tombstones fl-c$"
logged "check" socket.log . no
fresh && echo disabled >"$s/k3s.enabled" && guest confirm.sh check && want "check, k3s disabled" 0 listed
fresh && echo '{}' >"$s/tombstones" && echo 4343 >"$s/pgrep.flaminio-csi" && guest confirm.sh check &&
    want "check, not listed (before the sweep checks)" 0 "not listed"
fresh && echo 1 >"$s/tombstones.rc" && guest confirm.sh check && want "check, the listing fails" 1 ""
fresh && echo 4343 >"$s/pgrep.flaminio-csi" && guest confirm.sh check && want "check, a CSI plugin" 1 ""
fresh && echo 4343 >"$s/pgrep.flaminio-agent" && guest confirm.sh check && want "check, an agent" 1 ""
fresh && echo active >"$s/k3s.active" && guest confirm.sh check && want "check, k3s active" 1 ""
fresh && echo failed >"$s/k3s-agent.active" && guest confirm.sh check && want "check, k3s-agent failed" 1 ""
fresh && echo enabled >"$s/k3s-agent.enabled" && guest confirm.sh check && want "check, k3s-agent enabled" 1 ""

# confirm.sh delete: the unit's data plane alone, on the disk that is ours; then the delete.
fresh && guest confirm.sh delete && want "delete" 0 "deleted 2"
logged "delete" agent.log "^disks --disk-tool /x/flaminio-disk$"
logged "delete" socket.log "^delete v$"
fresh && echo inactive >"$s/active" && guest confirm.sh delete && want "delete, unit inactive" 1 ""
logged "delete, unit inactive" socket.log . no
fresh && : >"$s/pgrep.flaminio-io" && guest confirm.sh delete && want "delete, no flaminio-io" 1 ""
logged "delete, no flaminio-io" socket.log . no
fresh && printf '4242\n5151\n' >"$s/pgrep.flaminio-io" && guest confirm.sh delete && want "delete, two flaminio-io" 1 ""
logged "delete, two flaminio-io" socket.log . no
fresh && echo 5151 >"$s/pgrep.flaminio-io" && guest confirm.sh delete && want "delete, flaminio-io not the unit's" 1 ""
logged "delete, flaminio-io not the unit's" socket.log . no
fresh && echo "/dev/vda refused 20.0 GiB" >"$s/disks" && guest confirm.sh delete && want "delete, no disk ours" 1 ""
logged "delete, no disk ours" socket.log . no
fresh && printf '%s ours\n%s ours\n' "$t/dev/vdb" "$t/dev/vdc" >"$s/disks" && guest confirm.sh delete &&
    want "delete, two disks ours" 1 ""
logged "delete, two disks ours" socket.log . no
fresh && echo "$t/dev/vdc ours" >"$s/disks" && guest confirm.sh delete && want "delete, the wrong disk ours" 1 ""
logged "delete, the wrong disk ours" socket.log . no
fresh && echo "$t/dev/by-id ours" >"$s/disks" && guest confirm.sh delete && want "delete, a symlink to the disk" 0 "deleted 2"
fresh && echo "error: the volume is attached here" >"$s/answers" && guest confirm.sh delete && want "delete, refused" 1 ""

# confirm.sh confirm: the checks again, a second delete that finds none, then the confirmation.
fresh && echo "ok 0 replicas deleted" >"$s/answers" && guest confirm.sh confirm &&
    want "confirm" 0 '{"volume":"v","nodes":["fl-a","fl-c"],"pending":["fl-a"]}'
logged "confirm" meta.log "$meta_flags --trust-domain lab confirm v fl-c --replicas-deleted$"
fresh && guest confirm.sh confirm && want "confirm, the second delete finds 2" 1 ""
logged "confirm, the second delete finds 2" meta.log confirm no
fresh && echo "ok 1 replicas deleted" >"$s/answers" && guest confirm.sh confirm && want "confirm, the second delete finds 1" 1 ""
logged "confirm, the second delete finds 1" meta.log confirm no
fresh && echo inactive >"$s/active" && guest confirm.sh confirm && want "confirm, unit inactive" 1 ""
logged "confirm, unit inactive" meta.log confirm no
fresh && echo "$t/dev/vdc ours" >"$s/disks" && guest confirm.sh confirm && want "confirm, the wrong disk ours" 1 ""
logged "confirm, the wrong disk ours" meta.log confirm no
fresh && echo "ok 0 replicas deleted" >"$s/answers" && echo '{}' >"$s/confirmed" && guest confirm.sh confirm &&
    want "confirm, confirmed by another caller" 1 ""
fresh && guest confirm.sh && want "no step" 1 ""

((failed == 0)) || exit 1
echo "node-scripts-test: ok"
