#!/usr/bin/env bash
# Exercise the real guest functions with fake external commands; no VM or storage access.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
t=$(mktemp -d /tmp/fl83-scripts.XXXXXX)
trap 'rm -rf "${t:?}"' EXIT
export STATE="$t" FLAMINIO_SCRIPT_TEST=1 NODE=fl-a VOLUME=v META=127.0.0.1:1 LISTEN=127.0.0.1:2 TRUST_DOMAIN=lab FORMAT=1 DEVICE=/never-open-a-disk
export IO_BIN="$t/io" META_BIN="$t/meta" CLOCK_CONTRACT=/operator/clock-contract.json
cat >"$IO_BIN" <<'STUB'
#!/usr/bin/env bash
printf 'clock %s\n' "$*" >>"$STATE/events"
exit "${PREFLIGHT_RC:-0}"
STUB
cat >"$META_BIN" <<'STUB'
#!/usr/bin/env python3
import base64,hashlib,json,os,sys
with open(os.environ['STATE']+'/meta.log','a') as f: f.write(json.dumps(sys.argv[1:])+'\n')
a=sys.argv[1:];nonce=bytes.fromhex(a[a.index('--target-nonce')+1]);token=b'\xfb\xff\x01'
handoff=b'FLCH\x01\x01'+nonce+hashlib.sha256(token).digest()+bytes(64)
print(json.dumps({'token':base64.b64encode(token).decode(),'targetHandoff':base64.b64encode(handoff).decode()}))
STUB
chmod +x "$IO_BIN" "$META_BIN"
# Source exactly the production function definitions, omitting only their command dispatcher.
cat "$root/scripts/guest/common.sh" >"$t/functions"
sed '/^case "${1:-status}" in/,$d' "$root/scripts/guest/node.sh" >>"$t/functions"
cat >>"$t/functions" <<'STUB'
bin=$IO_BIN
sudo() { "$@"; }
modprobe() { echo modprobe >>"$STATE/events"; }
systemctl() { echo "systemctl $*" >>"$STATE/events"; return 1; }
systemd-run() { echo "start $*" >>"$STATE/events"; }
journal() { echo "serving replicas"; }
data_disk() { echo unexpected-discovery >>"$STATE/events"; return 1; }
control() {
    printf '%s\n' "$1" >>"$STATE/socket.log"
    case "$1" in
        attach-challenge) cat "$STATE/challenge" ;;
        attach\ *)
            local count=0
            [[ ! -f $STATE/attempt ]] || read -r count <"$STATE/attempt"
            echo $((count + 1)) >"$STATE/attempt"
            if ((count == 0)); then echo "error: v has an existing device lifetime; retry this handoff only after that device stops"; else echo "ok /dev/ublkb83"; fi
            ;;
        *) echo "error: forbidden status/detach/fallback" ;;
    esac
}
STUB
fresh() { : >"$t/events"; : >"$t/socket.log"; : >"$t/meta.log"; rm -f "$t/attempt"; printf 'ok %064d\n' 1 >"$t/challenge"; }
run() { cat "$t/functions" >"$t/run"; printf '%s\n' "$1" >>"$t/run"; bash "$t/run" >"$t/out" 2>"$t/err"; }
fresh
if PREFLIGHT_RC=1 run up; then echo "failed preflight admitted node startup" >&2; exit 1; fi
[[ $(wc -l <"$t/events") == 1 ]]
rg -q '^clock clock-check --clock-contract /operator/clock-contract.json --json$' "$t/events"
fresh
run up
[[ $(head -1 "$t/events") == 'clock clock-check --clock-contract /operator/clock-contract.json --json' ]]
rg -q '^start .*--format --force' "$t/events"
fresh
if ATTACH=v TOKEN=old run up; then echo "legacy startup admitted" >&2; exit 1; fi
[[ ! -s $t/events ]]
fresh
run attach_target
[[ $(cat "$t/out") == /dev/ublkb83 ]]
python3 - "$t" <<'PY'
import base64,json,pathlib,sys
p=pathlib.Path(sys.argv[1]);calls=[json.loads(x) for x in (p/'meta.log').read_text().splitlines()]
assert len(calls)==1,calls
a=calls[0];assert a[-4:]==['attach','v','fl-a','only-if-target'];assert a.index('--target-nonce')<a.index('attach');assert '/etc/flaminio/pki/agent.crt' in a
lines=(p/'socket.log').read_text().splitlines();assert lines[0]=='attach-challenge';assert len(lines)==3
one,two=(x.split() for x in lines[1:]);assert one[:4]==two[:4];assert len(one)==5;assert one[2]==base64.b64encode(b'\xfb\xff\x01').decode();assert len(one[3])==179 and '=' not in one[3] and '+' not in one[3] and '/' not in one[3]
assert all(not x.startswith(('status','detach')) for x in lines)
PY
for challenge in 'ok none' "ok $(printf '%064d' 0)" 'ok ABCDEF'; do
    fresh
    printf '%s\n' "$challenge" >"$t/challenge"
    if run attach_target; then echo "invalid challenge admitted" >&2; exit 1; fi
    [[ ! -s $t/meta.log ]]
done
for script in replica ublk; do
    fresh
    cat "$root/scripts/guest/common.sh" "$root/scripts/guest/$script.sh" >"$t/mvp"
    if bash "$t/mvp" up >"$t/out" 2>"$t/err"; then echo "legacy MVP admitted" >&2; exit 1; fi
    [[ ! -s $t/events ]]
done
# Exercise the actual destructive agent dispatcher, with guarded binary overrides only.
export DISK_TOOL="$t/disk-tool" AGENT_BIN="$t/agent"
printf '#!/usr/bin/env bash\nexit 0\n' >"$DISK_TOOL"
cat >"$AGENT_BIN" <<'STUB'
#!/usr/bin/env bash
printf 'agent %s\n' "$*" >>"$STATE/events"
STUB
chmod +x "$DISK_TOOL" "$AGENT_BIN"
cat "$root/scripts/guest/common.sh" >"$t/agent-functions"
printf 'sudo() { "$@"; }\n' >>"$t/agent-functions"
cat "$root/scripts/guest/agent.sh" >>"$t/agent-functions"
fresh
if PREFLIGHT_RC=1 bash "$t/agent-functions" claim >"$t/out" 2>"$t/err"; then
    echo 'failed preflight admitted disk claim' >&2; exit 1
fi
[[ $(wc -l <"$t/events") == 1 ]]
rg -q '^clock clock-check ' "$t/events"
fresh
bash "$t/agent-functions" claim >"$t/out" 2>"$t/err"
[[ $(wc -l <"$t/events") == 2 ]]
[[ $(head -1 "$t/events") == 'clock clock-check --clock-contract /operator/clock-contract.json --json' ]]
rg -q '^agent claim --serial data --disk-tool .* --label ' "$t/events"
fresh
PREFLIGHT_RC=1 bash "$t/agent-functions" disks >"$t/out" 2>"$t/err"
[[ $(wc -l <"$t/events") == 1 ]]
rg -q '^agent disks ' "$t/events"
echo 'clock-handoff-test: preflight, legacy refusal, own-agent handshake and retained canonical credentials pass'
