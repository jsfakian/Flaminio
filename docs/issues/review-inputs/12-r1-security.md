# Issue #12 plan review — round 1, focus: design-security

Scope: docs/issues/12/{intent,spec,design,decisions}.md and docs/adr/0007 at HEAD, checked against
the code in /home/jsfakian/Documents/src/flaminio-12, plus #74's approved plan
(/tmp/flaminio-plans-2026-11-12/74/docs/issues/74/).

### R1-S1 — High
**#12 contradicts #74's approved plan on the same scripts and the same socket code, and undoes #74's split on fl-a and fl-b. #12 never mentions #74.**
- **Device deletion.** #74 (design, "The lab node"): `node.sh up` always runs `down` first. `down`, "in both modes", stops the unit and then `del`s any remaining `/dev/ublkc$id`. `ublk.sh up` deletes a leftover `/dev/ublkc0` when its unit is inactive and `$mnt` is not mounted. #12 (design.md:166–168, 193–199) deletes only while the unit's own unit is active, and otherwise refuses and prints `del --id 0`. Its reasons are that the pod's kernel-chosen id can be 0 or 1, and that `findmnt` misses a pod's raw-block bind. If they are merged without reconciling, whichever lands second deletes a pod's live device under a tenant volume. That breaks #12's S4 rule "never deletes a ublk device its own unit does not serve" (spec.md:80–81).
- **The socket directory.** #74's helper binds `--control` "as `control.rs:46–52` does" (`create_dir_all`, so 0755; remove; bind; chmod 0600). Under `--privsep`, `control::spawn` only takes the bound listener (#74 design.md:33, 163–164). #12 puts its directory rules (0700, `O_NOFOLLOW`, an owner check) inside `spawn` (design.md:109–116). #74 makes `PRIVSEP=1` the default, so S3's "directory is root's, 0700" (spec.md:51) no longer holds on unit nodes.
- **Undoing the split.** #74's S8 evidence runs `make node-up` (under the helper), then `make csi-deploy` and `make csi-test` (#74 design.md:299–302). #12's `csi-deploy` stops the unit and starts root, direct-mode pods (design.md:337–345). The Kubernetes path then runs as root again whenever the driver is deployed, and #74's provisioning, attach and I/O evidence cannot be produced. ADR 0007 still says "There is no such helper yet" (0007:9–10).
- **Adopting the helper later conflicts with #12's choices.** #74's Risks list what #12's pod needs: a fixed `--id` with leftover deletion, and an init process, because the helper handles no SIGTERM as PID 1. #12 instead chose kernel-chosen ids with no cleanup (decisions.md:58–61), and puts SIGTERM handling in `node::main` with `flaminio-io` as PID 1 (design.md:44–45, 220).

Fix:
- Add a "#74" subsection to the design and decisions:
  - In `node.sh` and `ublk.sh`, the pod-node guard and the check that the unit is the script's own active unit run before any `del`, whichever plan lands second. #74's leftover deletion then applies only when the unit is inactive, the socket is absent and no data-plane sandbox is ready.
  - The directory check becomes one shared function, used by both `spawn` and the helper's bind.
- Decide one of two:
  - (a) Once #74 lands, the pod runs `flaminio-helper --id N -- flaminio-io node --privsep`. It stays privileged for the device cgroup, and gets an init that forwards SIGTERM to the child.
  - (b) `csi-deploy` keeps a unit mode for #74's S8.
- Update ADR 0007 to name #74's ADR as its successor and to list these constraints.

### R1-S2 — High
**Pod Security is configured through a key that k3s does not have.** `pod-security-admission-config-file` (design.md:204) is an RKE2 server option. In k3s, the keys of `config.yaml` are its CLI flags. k3s documents Pod Security admission as `kube-apiserver-arg: ["admission-control-config-file=<file>"]`, where the file is an `AdmissionConfiguration` that embeds a `PodSecurityConfiguration`. The design's `psa.yaml` is only the inner part, `defaults` and `exemptions` (design.md:202–203). From memory of k3s's config parser, an unknown key is logged and skipped. If so, no default applies. Privileged and hostPath pods are then admitted in every unlabelled namespace, and S3's "Pod Security refuses such pods outside kube-system and flaminio-system" (spec.md:47) does not hold. Lab test 1 would catch this, but the mechanism as written is wrong. I could not check this offline; confirm it with `k3s server --help` at the pinned `K3S_VERSION`.

Fix:
- Write `psa.yaml` as an `AdmissionConfiguration` (`plugins: [{name: PodSecurity, configuration: {apiVersion: pod-security.admission.config.k8s.io/v1, kind: PodSecurityConfiguration, defaults: …, exemptions: …}}]`).
- Load it with `kube-apiserver-arg: ["admission-control-config-file=/etc/rancher/k3s/psa.yaml"]`.
- Make `k3s.sh server` fail unless a server-side dry run of a privileged pod in a fresh unlabelled namespace is refused.

### R1-S3 — Medium
**`status --none` counts a data plane that does not answer as "nothing attached".** The preStop rule says it "exits 0 when no socket answers" (design.md:240–241; decisions.md:89–90). The tests define only `ok none`, "no socket" and the two attach forms (design.md:370–371). A hung or slow data plane, an `error …` answer, a time-out, and connect errors other than `ENOENT` or `ECONNREFUSED` are either unspecified or literally "no answer", so they exit 0. That check is also the upgrade's only step-4 and step-5 guard for attaches made outside Kubernetes, which have no VolumeAttachment (design.md:283–289). The pod can then be deleted under an attached volume. This contradicts the rule the plan sets for `node.sh socket`: "A hung data plane gives this, never absent" (design.md:176–177).

Fix:
- Exit 0 only on `ok none`, a missing path, `ENOENT` or `ECONNREFUSED`. Exit non-zero on a time-out (its own bound, say 5 s), on an `error` answer, or on anything else.
- Tests: a socket that accepts and never answers, and a socket that answers `error x`. Both must exit non-zero.

### R1-S4 — Medium
**After SIGTERM the data plane still accepts new attaches.** `begin_attach` refuses only on the attach state (design.md:122–125). `should_exit` is checked once per main-loop turn, after `drive_until`, which runs up to 50 ms (design.md:232–237; ublk.rs:755–760). Two cases go wrong:
- An attach that arrives right after kubelet's SIGTERM is taken.
- An attach that arrives in the window after the detach that a remembered SIGTERM waits for is also taken.

In both cases the terminating pod takes the new volume, keeps running, and is SIGKILLed when the grace period ends, under that volume. This breaks "exits after the next detach" (spec.md:71–72) and "Pod deletions then end in seconds, and never stop a served volume early" (decisions.md:69–71). The case is realistic when the node is not cordoned, for example after the label is removed or the DaemonSet is deleted, while kubelet keeps retrying NodeStage.

Fix:
- `begin_attach` answers `error: the node is stopping` once `sigterm::requested()` is set.
- Unit test with a fake engine: SIGTERM while attached, then detach, then a new attach; the attach is refused.
- `should_exit` test: the process exits after that detach.

### R1-S5 — Low
**Ids reach a shell and root file paths without validation or quoting** (AGENTS.md: "Ids reaching shell commands or socket lines are validated and quoted").
- `{{SERIAL}}` is a regular expression by `flaminio-agent`'s contract (`control/cmd/flaminio-agent/main.go:34`). `render` substitutes it literally (`scripts/lib.sh:199–208`) and unquoted into the init container's `sh` script (design.md:34–38), so `|`, `$`, `(` or `*` in it become shell syntax.
- The new `node.sh detach` removes `/run/flaminio/<VOLUME>.token` as root (design.md:161). `up` already writes `/run/flaminio/$ATTACH.token` (`node.sh:68`). Neither validates VOLUME or ATTACH, so a value containing `../` reaches other root-owned paths.

Fix:
- Pass the serial as an env var, quoted as `"$SERIAL"` in the script, and validate it in `lab.sh`.
- In `node.sh`, check VOLUME and ATTACH against `^[A-Za-z0-9._-]+$` before building any path.
- Add script tests for both.

### R1-S6 — Low
**S7 claims the server token is gone from the data nodes, but it is still in their logs.** S7 says "The server's token is no longer kept on the data nodes" (spec.md:117–118). The design itself says sudo logged `K3S_TOKEN` (design.md:317–318), so earlier installs left the server token in fl-a's and fl-b's journal and in `/var/log/auth.log`. The token is not rotated, so it is still valid. The lab test searches only "the journal since the restart" and never `/var/log` (design.md:417–421), so it cannot find those copies.

Fix (one of):
- State in S7 and ADR 0007 that the old token stays in the logs until it is rotated, and widen the test to the whole journal and `/var/log`, expecting the hit.
- Purge those logs as part of the change.

### R1-S7 — Low
**A settled decision about who can read capability tokens is wrong.** decisions.md:178–179 says the tokens on `/proc/*/cmdline` are read only by root-equivalents. But the VMs mount `/proc` without `hidepid` (`scripts/templates/user-data.yaml` sets none), so every local user can read those command lines. That includes system daemons, and after #74 the non-root `flaminio-io` user. The same holds for the agent token on ssh's argv (design.md:322–323).

Fix: correct the decision to "any local user". Alternatively, mount `/proc` with `hidepid=invisible` in `user-data.yaml`, or send the token over ssh's stdin and Python's stdin instead of on command lines.

### R1-S8 — Low
**The new consumers of the data plane's answers do not bound or validate them.** `flaminio-agent status`, `node.sh socket` and the CSI parse of `ok attaching VOLUME` are new or changed root consumers of the data plane's answers. `node.sh socket` prints `socket: answer <the answer>` as received, through ssh to the operator's terminal (design.md:170–177). Today, Python's `readline` and Go's `ReadString` are unbounded (`node.sh:85–90`, `dataplane.go:76`). #74's S6 treats the I/O process as untrusted and bounds answers to 4096 printable bytes.

Fix:
- Read at most 4096 printable bytes.
- Accept only `ok none`, `ok attaching <id>` and `ok <id> /dev/ublkb<N>`, with ids checked against a pattern. Anything else is `socket: error malformed` or a non-zero exit.
- Test each refusal.

### R1-S9 — Low
**The crictl guard matches pods from any namespace.** It filters on `--name '^flaminio-dataplane-'` only (design.md:185). A tenant pod named, for example, `flaminio-dataplane-x` makes `node.sh up`, `ublk.sh up` and `replica.sh up` refuse on that node. This fails safe, but a tenant can block lab commands.

Fix: add `--namespace flaminio-system` and `--label app=flaminio-dataplane`, and test a decoy pod in another namespace.

### Nits
- spec.md:52 says token files "go when their volume is detached or the unit stops". The design deletes them only on `node.sh detach`, and on a `down` that stopped an active unit (design.md:161–163). A detach by the CSI plugin or by deposition, or a unit that crashes, leaves the file. Align the spec with the design.

Reviewed commit: 6f70e984153f9e68bb815b28969a8ad7b6fcf20a
