# Issue #12 — report

## Review rounds

### Plan review round 1 — 6f70e98

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-S1 — High:** #12 contradicted #74's approved plan on the unit scripts' device deletion, the socket's directory and the split itself, and never named #74. Evidence: #74 design "The lab node" and "The I/O process under `--privsep`"; #12 design.md:109–116, :166–199, :337–345.
- **R1-S2 — High:** Pod Security was configured through `pod-security-admission-config-file`, a key k3s does not have. Evidence: design.md:202–204; docs.k3s.io Hardening Guide ("Pod Security") and CLI `k3s server`.
- **R1-C1 — Medium:** #12 and #74 set contradictory rules for deleting ublk devices in `node.sh`/`ublk.sh`; merged in the wrong order, #74's `ublk.sh up` deletes a live pod device. Evidence: design.md:166–197; 74/design.md:209–214; decisions.md:58–61.
- **R1-C2 — Medium:** the plan was written against a base behind `main`, where `node.sh detach`/`delete` exit 3 when the unit is inactive and `lab.sh` acts on it; no script tests named. Evidence: main `node.sh` `running()`; main `lab.sh` `detach_all`, `detached_everywhere`, `delete_volume`; intent.md:19–20; design.md:158.
- **R1-C3 — Medium:** `make csi-undeploy` checked VolumeAttachments, not the sockets, dropped the driver's deletion, and its lab test could not fail. Evidence: spec.md:75–79; design.md:251–252, :350–354, :465, :472; lab.sh:1051–1057.
- **R1-C4 — Medium:** template values went unquoted into shell strings; `VOLUME` unvalidated in a root path; #43's ServiceAccount location unmet. Evidence: design.md:34–49, :161; lib.sh:199–208; 43/decisions.md:75–78.
- **R1-C5 — Medium:** single-file PKI mounts keep the old inode, breaking #16's rotation by rename. Evidence: design.md:59; decisions.md:167; #16's ADR, Options 1.
- **R1-C6 — Medium:** lab 5's expected steps did not follow the upgrade order (RF 1, `minIsr`, ISR not full). Evidence: design.md:263–264, :455–459.
- **R1-S3 — Medium:** `status --none` counted an unanswering data plane as "nothing attached". Evidence: design.md:240–241, :370–371; decisions.md:89–90.
- **R1-S4 — Medium:** attaches were still accepted after SIGTERM. Evidence: design.md:122–125, :232–237; ublk.rs:755–760.
- **R1-C7 — Low:** the init script did not stop on a failed check. Evidence: design.md:34–38.
- **R1-C8 — Low:** the euid check could not be written with safe `std`. Evidence: design.md:109–116; dataplane/Cargo.toml:40.
- **R1-C9 — Low:** attaches accepted after SIGTERM (same root cause as R1-S4). Evidence: ublk.rs:755–761; spec.md:71–72.
- **R1-C10 — Low:** who `served_isr_full` and `k8s-down` ask was unspecified. Evidence: design.md:243–249, :270–272; main `data_vms`.
- **R1-C11 — Low:** the raw-disk benchmarks were not guarded. Evidence: `fio-raw.sh`, `store-bench.sh`; design.md:182–191.
- **R1-C12 — Low:** the base image digest must be the multi-arch index digest. Evidence: design.md:73–74; lab.sh:1018.
- **R1-C13 — Low:** no plan owned adopting #74's helper in the pod; ADR named no successor. Evidence: 74/decisions.md:93–94, :115; 74/intent.md:29–32; intent.md:124–125.
- **R1-C14 — Low:** ADR number 0007 was taken four times. Evidence: audit 0007–0011, #16's and #17's 0007.
- **R1-C15 — Low:** the Makefile and docs were not aligned with the new commands. Evidence: design.md:355; Makefile:366.
- **R1-C16 — Low:** S3 understated the protocol change. Evidence: spec.md:44; control.rs:95.
- **R1-C17 — Low:** `detach` waited for the wrong state. Evidence: design.md:137; control.rs:205–210.
- **R1-C18 — Low:** token files outlived a crashed unit. Evidence: spec.md:52; design.md:162–163.
- **R1-C19 — Low:** untested branches (startup retry thread; `ublk.sh up`'s refusal). Evidence: design.md:134–136, :195–196.
- **R1-S5 — Low:** ids reached a shell and root file paths unvalidated (same root cause as R1-C4). Evidence: design.md:34–38, :161; node.sh:68; flaminio-agent main.go:34.
- **R1-S6 — Low:** S7 claimed the server token was gone from the data nodes, but their logs keep it. Evidence: spec.md:117–118; design.md:317–318, :417–421.
- **R1-S7 — Low:** the decision that only root-equivalents read `/proc/*/cmdline` was wrong. Evidence: decisions.md:178–179; `scripts/templates/user-data.yaml` (no `hidepid`).
- **R1-S8 — Low:** new consumers of the data plane's answers did not bound or validate them. Evidence: design.md:170–177; node.sh:85–90; dataplane.go:76.
- **R1-S9 — Low:** the `crictl` guard matched pods from any namespace. Evidence: design.md:185.
- **N1 (code) — Nit:** the exit "sends `Input::Stop`", which does nothing. Evidence: design.md:235; node.rs:361–366.
- **N2 (code) — Nit:** the deploy "reads its digest", which nothing uses. Evidence: design.md:340.
- **N3 (code) — Nit:** the new `unsafe` module contradicts ARCHITECTURE §15.5 and `flaminio-uring/src/lib.rs:10–11`. Evidence: design.md:214.
- **Security Nit — Nit:** spec and design disagreed on when token files go (same root cause as R1-C18). Evidence: spec.md:52; design.md:161–163.

Changes/resolution (the plan was approved on 2026-11-13; these are changes made after approval, recorded in decisions.md, "Plan review round 1"):
- **R1-S1, R1-C1, R1-C13:** new design section "#74, the privileged helper" and intent Why 9. The pod stays in direct mode, as #74's intent foresees; adopting the helper in the pod is #12's follow-up (follow-ups §9), with #74's list of needs (intent, Out of scope; ADR 0012, which now names #74's ADR as its successor). R1-S1's option (b): `make csi-deploy` keeps `DATAPLANE=unit`, so #74's evidence runs. The unit scripts delete a ublk device only at their own id and only once the pod-node guard passes; #74's leftover deletion and "`up` runs `down` first" kept; `node.sh down` detaches its unit's volume through the socket before the stop; `ublk.sh up`'s refusal cut. S4's `down` rule restated (it could not hold with #74's leftover deletion). The socket directory rules are one function, `control::bind`, which #74's helper calls once both have landed.
- **R1-C2:** scripts described as on `origin/main` (8aaa7a3); `running()` counts a pod's data plane, `detach` included; exit 3 means no data plane; `vol-delete`, `detach_all`, `detached_everywhere` on pod nodes stated; script-test cases named in `node-scripts-test.sh`, a new `lab-k8s-test.sh`, and `lab-deletion-test.sh`.
- **R1-C3:** `csi-undeploy`: namespace, bounded wait for its VolumeAttachments, the ssh socket check (`pod_nodes_idle`), then DaemonSet, driver, labels. Lab 6 holds the attach outside `flaminio-test`.
- **R1-C4, R1-S5:** values checked against a grammar in `lab.sh`, passed only as `env`; constant `sh -ec` scripts quote them; `ATTACH`/`VOLUME` checked before any path; ServiceAccount in `deploy/csi/rbac.yaml`; manifest and script tests.
- **R1-C5:** one read-only `Directory` hostPath, `/etc/flaminio/io-pki`, holding only the data plane's four files, used by the unit too; manifest test.
- **R1-C6:** RF 1 and `minIsr` cases served from a controller-owned PVC pod on fl-a; ISR case kills fl-b's container while the load pod is Terminating and writing (30 s `preStop`).
- **R1-S2:** `kube-apiserver-arg: ["admission-control-config-file=..."]` with an `AdmissionConfiguration`, per docs.k3s.io (Hardening Guide; CLI `k3s server`, no admission option), valid for the pinned `v1.36.5+k3s1`; `k3s.sh server` checks it with a server-side dry run in `default`.
- **R1-S3:** `status --none` exits 0 only on `ok none`, a missing path, `ENOENT` or `ECONNREFUSED`, with a 5 s bound; tests for a silent socket and an error answer.
- **R1-S4, R1-C9:** `begin_attach` refuses with `the node is shutting down` once SIGTERM is requested; unit test.
- **R1-C7:** both scripts run with `sh -ec`. **R1-C8:** safe `euid()` in `flaminio-uring`'s new `process` module; `control::bind` takes the owner. **R1-C10:** `served_isr_full` asks `data_vms` and refuses on an unreachable VM; `pod_nodes_idle` asks fl-a and fl-b, skipping an active unit. **R1-C11:** guard in `fio-raw.sh` and `store-bench.sh`. **R1-C12:** index digest. **R1-C14:** ADR moved to `docs/adr/0012-privileged-data-plane-pod.md` (README updated; every reference in docs/issues/12/). **R1-C15:** Makefile help, `node-upgrade` target and `usage()`, `docs/lab.md`, `docs/followups.md` §9. **R1-C16:** S3 lists every protocol change. **R1-C17:** `detach V` waits until the state no longer names V. **R1-C18, security Nit:** `down` deletes token files once no data plane runs, a crash included; S3 aligned. **R1-C19:** `startup_next` pure function with a unit test; the `ublk.sh up` refusal is cut, its replacement has script tests.
- **R1-S6:** S7, the design and ADR 0012 say the old copies stay in the logs until rotation; lab 1 searches the whole journal and `/var/log`. **R1-S7:** accepted risk corrected to "any local user". **R1-S8:** 4096 printable bytes and a grammar for the agent, `node.sh socket` and the CSI parse; the CSI bound is #74's, added by whichever lands first. **R1-S9:** `crictl` filters on namespace and label; decoy tests.
- **N1, N2, N3:** `Input::Stop` and the digest read removed; `lib.rs` and ARCHITECTURE §15.5 updates named.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 74979ae

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-C1 — High:** the new `/etc/flaminio/io-pki` (root 0700, `io.key` 0600) contradicted #74's PKI modes, so the unit's unprivileged I/O process could not read it and #74's evidence could not run; and #16's approved `pki.sh` keeps `io` in `/etc/flaminio/pki`, so its renewal would not reach `io-pki`. Evidence: design.md:109–119, :18–37; 74/design.md:200–207, :369; 16/design.md:87–89, :141–143.
- **R2-C2 — Medium:** the guard added to `fio-raw.sh` (R1-C11) broke `make bench-pod`'s read-only raw comparison on pods, and the plan did not say so. Evidence: main lab.sh:1342–1380; design.md:323–327; spec S8; intent.md:166.
- **R2-C3 — Low:** `DATAPLANE` was read per command: #74's evidence would silently test pods, and `csi_failover` chose its mode by the variable. Evidence: design.md:25–27, :512–515, :525–530.
- **R2-C4 — Low:** nothing created `/etc/flaminio/io-pki` on an existing lab, so `node-up` and the DaemonSet would fail. Evidence: design.md:115–117; main lab.sh:998–1000.
- **R2-B1 — Low:** an attach between preStop's `ok none` and kubelet's SIGTERM was still taken. Evidence: design.md:100, :187–191, :386–395.
- **R2-S1 — Low:** `node.sh attach` kept main's unvalidated `VOLUME` in a socket line, and `control()` read unbounded answers printed raw by `die`. Evidence: main node.sh:76–81; main common.sh:24–29; design.md:224–233, :258–259.
- **R2-C5 — Low:** several new rules had no test, and spec Tests overclaimed S7 script coverage. Evidence: design.md:269–273, :341–344, :625–628, :667–668; spec.md:161–162.
- **R2-C6 — Low:** the script tests for `node.sh up` and token files could not run as specified (real `/run/flaminio`, unstubbed `modprobe`, `systemd-run`, `journalctl`, `lsblk`). Evidence: design.md:327–330, :625–633; main node-scripts-test.sh.
- **R2-N1 — Nit:** ADR 0012's wording read as if the wait kept the crash rule. Evidence: ADR 0012:96–98.
- **R2-N2 — Nit:** intent said `node.sh up` deletes nothing first. Evidence: intent.md:25; main node.sh:28–29, :44.
- **R2-N3 — Nit:** `down`'s `*.token` glob would expand as the VM user. Evidence: design.md:262.
- **R2-N4 — Nit:** `socket: error refused` was not tied to a case. Evidence: design.md:298–300.

Changes/resolution (the plan was approved on 2026-11-13; these are changes made after approval, recorded in decisions.md, "Plan review round 2"):
- **R2-C1:** a PKI bullet under "#74, the privileged helper": once #74 has landed, `io-pki` is `root:flaminio-io` 0750 with `io.key` 0640 (#74's modes move there; `/etc/flaminio/pki` can stay 0700); the pod reads it as root, so S2 holds. Whichever of #12 and #16 lands second puts `io` in `io-pki` in #16's per-name `pki.sh` map. `pki.sh install io` removes the old `pki/io.*`. Script cases for the routing, the removal, `file ca.crt|token.pub` to both directories, and `node.sh up`'s `--pki`.
- **R2-C2:** `fio-raw.sh` skips the guard when every selected job is a read and the disk is already preconditioned (it writes nothing); S8 and the intent say so; lab 12 (`make bench-pod` on pods) and a script case added.
- **R2-C3:** `csi_failover` reads the mode from the cluster (the DaemonSet's existence); #74's evidence becomes `DATAPLANE=unit make csi-deploy`, written into #74's lab plan and `docs/lab.md` by whichever lands second. Lab 11 and a script case extended.
- **R2-C4:** `cmd_csi deploy`, `cmd_node up`, `cmd_data_node_up` and `ensure_node_c` issue `io` into `io-pki` (with `ca.crt` and `token.pub`) when it is missing, as for `agent.crt`; `docs/lab.md` says so; lab 13 and a script case.
- **R2-B1:** preStop runs `kill -TERM 1` before its wait; `flaminio-io` is PID 1 (its script `exec`s it, `hostPID` false, `shareProcessNamespace` unset), and a blocked SIGTERM reaches a namespace's init. S4 states it. Manifest case and lab 6 case added. Nothing new accepted.
- **R2-S1:** `attach` validates `VOLUME`; `control()` reads at most 4096 printable bytes and returns `malformed` outside each command's grammar; `attach` waits only for a `/dev/ublkbN` path; script cases.
- **R2-C5:** script cases for `down`'s refusal while attaching, the `--pki` argument, `pki.sh`'s routing, and `k3s.sh server` stopping when its dry run is admitted; the spec's script-test sentence narrowed (other S7 rules stay lab tests).
- **R2-C6:** test overrides `RUN_DIR`, `PKI_ROOT`, `K3S_CONFIG_DIR` and the stubs (`modprobe`, `systemd-run`, `journalctl`, `lsblk`, `findmnt`, `mountpoint`, `k3s`, `curl`, `fio`) named, honoured only under `FLAMINIO_SCRIPT_TEST=1`.
- **R2-N1:** ADR 0012 says "must be reconciled with". **R2-N2:** intent says `node.sh up` runs `down` first when the unit is active. **R2-N3:** `sudo find /run/flaminio -maxdepth 1 -name '*.token' -type f -delete`. **R2-N4:** `refused` names a well-formed `error ...` answer; `ECONNREFUSED` stays `absent`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 0165a65

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R3-C1 — Medium:** `down`'s `sudo find /run/flaminio ...` fails when `/run/flaminio` does not exist (tmpfs after a reboot, a fresh fl-c), ending `down` under `set -euo pipefail`; with #74's "`up` runs `down` first", `make data-node-up` would fail on its first run. Evidence: design.md:294–297, :309; main driver.yaml:167, node.sh:61, lab.sh:1222–1236.
- **R3-C2 — Low:** the `pki.sh` script case expected `io.key` 0640 and group `flaminio-io` from `pki.sh`, which under #74's split only `node.sh up` sets. Evidence: design.md:31–34, :700–703; 74/design.md:206, :216; main pki.sh:15–16.
- **R3-C3 — Low:** lab 6's probe expected "the node is shutting down" from an attach while attached, but main's `serve` answers "is attached here" first, and no unit case pinned the order. Evidence: control.rs:85–96; design.md:213–219, :662–664, :810–814.
- **R3-C4 — Low:** "every path that runs `node.sh up`" missed `scripts/checks/reconcile.sh` and `pvc-loop.sh`, and the tests covered two of the four paths named. Evidence: reconcile.sh:38; pvc-loop.sh:21; design.md:142–147, :731–733.
- **R3-B1 — Low:** no negative cases for `file`'s routing (a key could reach `io-pki`, against S2), the overrides' gating, `socket: error refused`, or a stale socket reading `absent`. Evidence: design.md:333–335, :367, :677, :702; spec.md:36; main node-scripts-test.sh:238–246.
- **R3-N1 — Nit:** "`pki.sh install io` removes the old `/etc/flaminio/pki/io.*`" would expand as the VM user. Evidence: design.md:133–134.
- **R3-N2 — Nit:** the PKI bullet under #74 had no "whichever lands second" clause. Evidence: design.md:33; 74/design.md:206.
- **R3-N3 — Nit:** the hook's signal ends PID 1, so kubelet records a `FailedPreStopHook` event the plan did not mention. Evidence: design.md:108–110.
- **R3-N4 — Nit:** `cmd_bench_pod`'s warning and lab 12 did not say when the raw disk can be preconditioned. Evidence: main lab.sh:1375; design.md:854–855.

Changes/resolution (the plan was approved on 2026-11-13; these are changes made after approval, recorded in decisions.md, "Plan review round 3"):
- **R3-C1:** the deletion is `[[ ! -d /run/flaminio ]] || sudo find ...`; script case with `RUN_DIR` missing: exit 0, the leftover `del` still runs.
- **R3-C2:** the `pki.sh` case checks only the modes `pki.sh` sets (`io.key` 0600; `io-pki` 0700, or 0750 once #74 has landed); the 0640 and the group are #74's `node.sh up` case.
- **R3-C3:** in `serve`, `begin_attach`'s checks replace `serve`'s own attached-state check and come first (shutdown, then state, then token and limit); unit case: attached to `v`, flag set, `attach w` answers `the node is shutting down`.
- **R3-C4:** the reviewer's first option, the simplest safe one: `node.sh up` dies naming `make pki-issue` when `io-pki/io.crt` is missing, covering every caller (`reconcile.sh`, `pvc-loop.sh` included) with one script case; `cmd_data_node_up` and `ensure_node_c` join the lab-k8s-test case; `pvc-loop.sh` runs with `DATAPLANE=unit`.
- **R3-B1:** cases added: `file token.key` and `file etcd-ca.crt` stay out of `io-pki`, which holds exactly its four files after `cmd_pki issue`'s sequence; each of `RUN_DIR`, `PKI_ROOT`, `K3S_CONFIG_DIR` is ignored without `FLAMINIO_SCRIPT_TEST=1` (the `sudo` stub refuses the real paths); `socket: error refused` on `error x`; `socket: absent` on a stale socket.
- **R3-N1:** `sudo rm -f /etc/flaminio/pki/io.crt /etc/flaminio/pki/io.key`. **R3-N2:** whichever of #12 and #74 lands second changes #74's lab-node line and `pki.sh:10` to `io-pki`. **R3-N3:** the expected `FailedPreStopHook` event is stated beside the preStop and in lab 6. **R3-N4:** `cmd_bench_pod`'s warning names the step (before the first `make csi-deploy`, or after `make csi-undeploy`, then `FORMAT=1 make node-up`); lab 12's setup follows it.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 3008b06

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R4-L1 — Medium:** lab 12's setup was not runnable after lab 11: `make bench-raw VM=b` would precondition beneath fl-b's live unit (the guard passes with the unit active), and the final `make csi-deploy` would refuse on fl-a's still-active unit. Evidence: design.md:896–905, :846–847, :~373.
- **R4-L2 — Low:** the "overrides without the switch" case could not fail for `node.sh down`: with `RUN_DIR` ignored and the real `/run/flaminio` absent, `down` reaches no `sudo` and exits 0. Evidence: design.md:~745.
- **R4-L3 — Low:** where `node.sh up`'s `io.crt` check sits was unstated; after `systemctl is-active && down`, a refusal would stop a running unit. Evidence: design.md:155–160; node.sh:36–41.
- **R4-M1 — Low:** step 2's refusal (`already attached`) no longer named the volume, and no unit case pinned state-before-token. Evidence: design.md:236; spec.md:46.
- **R4-N1 — Nit (the report's R4-S1):** the `sudo` stub's path match was by argument, missing `sudo sh -c "..."` and `sudo env` forms, and the `systemd-run` exemption was unexplained. Evidence: design.md:399–402; pki.sh:16.

Changes/resolution (the plan was approved on 2026-11-13; these are changes made after approval, recorded in decisions.md, "Plan review round 4"):
- **R4-L1:** lab 12's setup, after `make csi-undeploy`: `make node-down` (fl-a and fl-b), `make bench-raw VM=b`, `FORMAT=1 make node-up VM=b`, `make node-down VM=b`, `make csi-deploy`. It says volumes made before it lose their fl-b replica, so it runs after the labs that use them, or on a fresh lab.
- **R4-L2:** for `node.sh down`, the case asserts that the token planted in the temporary `RUN_DIR` survives and that the `sudo` stub records no command naming it; `pki.sh csr io` and `k3s.sh server` keep the stub's refusal as their check.
- **R4-L3:** the `io.crt` check comes first in `up`, with the pod-node guard, before `systemctl is-active && down`; the script case checks that a running stubbed unit stays active (no `systemctl stop`, no `del`).
- **R4-M1:** the refusals are `error: already attaching V` and `error: already attached to V` (design and S3); unit case: attached plus a non-base64 token answers `already attached to v`.
- **R4-N1:** the `sudo` stub matches a substring of any argument; the `systemd-run` exemption is dropped, since under the overrides its arguments name only temporary paths.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 1d200c96b367192ade6e3d819ee7ba1b070c444f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
