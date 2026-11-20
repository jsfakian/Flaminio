# Issue #70, plan review round 4 (incremental), design-code

Reviewed: git diff 82b7bec..91caea0 (without report.md).

## Round-3 findings
- R3-1: fixed. design "Engine refresh" runs #80's check on the raw `pb::Volume`, before `rt.chunk_meta` (engine.rs:1555), `node_id` (:386-395) and the ListNodes call (:1567-1574); `a_malformed_refresh_is_refused` asserts the node table unchanged and no ListNodes call.
- R3-2: fixed. `(KEEP=1 csi_failover)` in a subshell keeps the EXIT trap (:1158, cleared :1184) in the subshell and keeps the namespace (:1200-1203); the exec for the added file and 6Gi runs before cleanup. (See F2 for a detail.)
- R3-S1..S4 (security) and the two nits: fixed in the text (S9 names the flag, the sim sentence says replica/tests.rs, wait ends when a replica accepts). Citations node.go:108, 135-136, mount.go:37-40, tombstones.go:13, target.rs:829-836, main.go:84, auth.go:20, meta.sh:28 match the code. Approval lines and decisions.md agree; no file still lists the node-wide wait as accepted.

## Findings

### F1 Medium: how VOLUME_EXPANSION reaches meta.sh, and which `meta up` callers keep it, is not stated
design.md Flag (l.75-79) and Lab (l.196-198). `meta.sh` takes its inputs only from `ENV_VARS`, which `cmd_meta` builds from fixed values (lab.sh:514-519); `make meta-up` is `$(LAB) meta up` (Makefile:332). The plan says "as #75's `--qos`" and "`meta.sh` writes it only with `VOLUME_EXPANSION=1`", but not that `cmd_meta` appends `VOLUME_EXPANSION` to `ENV_VARS` or that the Makefile passes it to `lab.sh`. It also does not say what other callers do: `meta.sh up` rewrites the whole unit on every call, so `cluster-up` (Makefile:308), `chaos`' `cmd_meta up w` (lab.sh:1407) and `make meta-up QOS=1` (#75, which has no knowledge of this flag) each regenerate the unit without `--volume-expansion`. The flag then silently disappears on w after a chaos run, or after the #75 lab check, and `csi_expand` refuses or ExpandVolume answers refused. "Default 0, else refused" is also unclear (refused: any value other than 0/1?).
Fix: name the passing path (Makefile exports the variable; `cmd_meta` adds `VOLUME_EXPANSION=${VOLUME_EXPANSION:-0}` to `ENV_VARS`; `meta.sh` adds the flag line only for 1); say the rule for other callers (every `meta up` that should keep the flag passes it, including chaos at lab.sh:1407, or the flag is read back from the existing unit), and that `QOS=1` and `VOLUME_EXPANSION=1` compose, agreed with #75's plan. Add a script test in `scripts/tools/` (the unit text with and without the variable, and with both), since `make lint` runs those.

### F2 Low: how csi_expand checks "every unit has the flag", and that the subshell failure fails the check
design.md Lab. The refusal is named but not the mechanism (e.g. `systemctl cat flaminio-meta | grep -- --volume-expansion` on every VM of `vms_of cluster`, through `run_on_vm`/`vssh`; the VM selector must be the one `meta up` used). Also `lab.sh` functions are called under whatever `set -e` the script has; `(KEEP=1 csi_failover)` in a subshell inside an `||` or `if` list ignores `-e`, so say it is `(KEEP=1 csi_failover) || die ...` (the doc says "its failure fails the check", but the form decides it). Fix: name both.

### F3 Low: no test pins the refusal or the flag rollout script paths
Tests table row S9 is "the lab check" only; the new refusal in `csi_expand` and the `meta.sh` flag are untested outside the lab (see F1 for a script test). Fix: list the script test as S9's test.

## Nits
- design.md Risks, Release: "the target's reconnect (`target.rs:829-836`) is accepted" reads as a decision; it means a replica accepts the reopened session. Say "succeeds once a replica accepts the session".
